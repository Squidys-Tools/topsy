//! The shelf's state file: where it lives, and the writer that keeps it up to date.
//!
//! The format (one absolute path per line) belongs to `shelf`; this module only owns
//! the file. Writes go to a temp sibling renamed over the real file, and happen on a
//! thread of their own so a slow disk never stalls the UI thread.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::shelf::{self, Shelf};

/// Everything persistence needs, ready at startup: the shelf as it was saved, and the
/// writer that keeps the file in step. `writer` is `None` when persistence is off.
pub struct Persistence {
    pub shelf: Shelf,
    pub writer: Option<Writer>,
}

/// Resolve the state file, read the shelf back from it, and start its writer.
pub fn start() -> Persistence {
    let path = path();
    Persistence {
        shelf: path.as_deref().map(load).unwrap_or_default(),
        writer: path.map(Writer::spawn),
    }
}

/// Where the shelf lives between runs: `TOPSY_STATE` when set (an empty value turns
/// saving off, for scripted runs), else `%LOCALAPPDATA%\topsy\shelf.txt`.
pub fn path() -> Option<PathBuf> {
    resolve(
        std::env::var_os("TOPSY_STATE").as_deref(),
        std::env::var_os("LOCALAPPDATA").as_deref(),
    )
}

fn resolve(topsy_state: Option<&OsStr>, local_app_data: Option<&OsStr>) -> Option<PathBuf> {
    if let Some(path) = topsy_state {
        return (!path.is_empty()).then_some(PathBuf::from(path));
    }
    let dir = local_app_data?;
    Some(PathBuf::from(dir).join("topsy").join("shelf.txt"))
}

/// Read the state file back into a shelf. No file, an unreadable file or pure noise are
/// all just an empty shelf. The listed files are not looked at: asking the disk can stall
/// on a dead share, so chips come back `Unknown` and the island settles them once it is
/// up (`Island::settle_restored`).
pub fn load(path: &Path) -> Shelf {
    let mut shelf = Shelf::default();
    match std::fs::read_to_string(path) {
        Ok(text) => {
            shelf.add(shelf::parse(&text));
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => eprintln!("topsy: could not read the shelf state: {err}"),
    }
    shelf
}

/// The writer half: `send` a serialized shelf and it lands on disk, in send order.
/// Each send replaces the whole file, so the last one always wins.
pub struct Writer {
    tx: Option<std::sync::mpsc::Sender<String>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Writer {
    /// Start the writer thread for `path`.
    pub fn spawn(path: PathBuf) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let thread = std::thread::Builder::new()
            .name("topsy-state".into())
            .spawn(move || {
                while let Ok(text) = rx.recv() {
                    if let Err(err) = write_file(&path, &text) {
                        eprintln!("topsy: could not save the shelf: {err}");
                    }
                }
            });
        match thread {
            Ok(thread) => Self {
                tx: Some(tx),
                thread: Some(thread),
            },
            Err(err) => {
                eprintln!("topsy: could not start the shelf saver: {err}");
                Self {
                    tx: None,
                    thread: None,
                }
            }
        }
    }

    /// Queue the shelf's text to replace the state file. Cheap: the disk hit is the
    /// writer thread's problem, never the caller's.
    pub fn send(&self, text: String) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(text);
        }
    }

    /// Flush whatever was queued and stop the thread. For shutdown: the receiver drains
    /// every send before it reports the channel dead, so nothing in flight is lost.
    pub fn drain(mut self) {
        drop(self.tx.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Replace `path` with `text` through a temp file renamed over it: a kill mid-write can
/// strand the `.tmp`, never leave half a state file.
fn write_file(path: &Path, text: &str) -> std::io::Result<()> {
    // A bare filename's parent is the empty path, which needs no directory made.
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory under %TEMP%, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("topsy-test-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }

        fn file(&self, name: &str) -> PathBuf {
            let path = self.path(name);
            std::fs::write(&path, "topsy test").unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn round_trip_keeps_order() {
        let scratch = Scratch::new("roundtrip");
        let a = scratch.file("a.txt");
        let b = scratch.file("b.txt");
        let c = scratch.file("c.txt");

        let mut shelf = Shelf::default();
        shelf.add([a, c, b]);
        let file = scratch.path("nested\\shelf.txt");
        write_file(&file, &shelf::serialize(shelf.items())).unwrap();

        let back = load(&file);
        assert_eq!(back.items(), shelf.items());
    }

    #[test]
    fn load_does_not_look_at_the_files_it_lists() {
        // Startup must not wait on the disk: gone files are pruned later, off the UI thread.
        let scratch = Scratch::new("untouched");
        let file = scratch.path("shelf.txt");
        let gone = PathBuf::from(format!(r"C:\definitely\not\there-{}", std::process::id()));
        std::fs::write(&file, format!("{}\n", gone.display())).unwrap();

        let back = load(&file);
        assert_eq!(back.len(), 1);
        assert_eq!(back.items()[0].path, gone);
        assert_eq!(back.items()[0].kind, shelf::Kind::Unknown);
    }

    #[test]
    fn load_returns_an_empty_shelf_for_missing_or_corrupt_files() {
        let scratch = Scratch::new("corrupt");
        assert!(load(&scratch.path("shelf.txt")).is_empty());

        let file = scratch.path("shelf.txt");
        std::fs::write(&file, "%%%@\n\tgarbage\t\nrelative\\path\n").unwrap();
        assert!(load(&file).is_empty());
    }

    #[test]
    fn write_file_creates_its_directory_and_overwrites() {
        let scratch = Scratch::new("write");
        let file = scratch.path("deep\\deeper\\shelf.txt");
        write_file(&file, "C:\\a.txt\n").unwrap();
        write_file(&file, "C:\\b.txt\n").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "C:\\b.txt\n");
        assert!(!file.with_extension("tmp").exists());
    }

    #[test]
    fn writer_saves_in_order_and_drain_flushes() {
        let scratch = Scratch::new("writer");
        let file = scratch.path("shelf.txt");
        let writer = Writer::spawn(file.clone());
        writer.send("C:\\a.txt\n".into());
        writer.send("C:\\b.txt\n".into());
        // No sleep: drain joins the thread, which only exits after the queue is empty.
        writer.drain();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "C:\\b.txt\n");
    }

    #[test]
    fn resolve_prefers_the_override_and_honours_empty() {
        let local = OsStr::new(r"C:\Users\me\AppData\Local");
        let override_ = OsStr::new(r"D:\scratch\state.txt");
        assert_eq!(
            resolve(Some(override_), Some(local)),
            Some(PathBuf::from(r"D:\scratch\state.txt"))
        );
        assert_eq!(resolve(Some(OsStr::new("")), Some(local)), None);
        assert_eq!(
            resolve(None, Some(local)),
            Some(PathBuf::from(r"C:\Users\me\AppData\Local\topsy\shelf.txt"))
        );
        assert_eq!(resolve(None, None), None);
    }
}
