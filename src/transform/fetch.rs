//! Getting a pinned file once: one https download, checked against a pinned SHA-256, resumable,
//! then never the network again. The background model and the downloaded tools come this way.

use std::cell::RefCell;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use sha2::{Digest, Sha256};

use crate::log::{LogErr, MissingOk};
pub use crate::model::download::ModelStatus as Status;
use crate::model::download::{self, Reply, Start, Ticker};
use crate::win32::http::{self, Streamed};

/// One file at a fixed address, with its size and hash, kept under `%LOCALAPPDATA%\topsy`.
pub struct Pinned {
    /// What the user is getting, for messages: "the background model", "FFmpeg".
    pub label: &'static str,
    pub host: &'static str,
    pub path: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    /// Where it is kept, relative to `%LOCALAPPDATA%\topsy`.
    pub file: &'static str,
}

/// A host may redirect to its CDN, which may itself redirect once more.
const MAX_REDIRECTS: usize = 4;

/// The files downloading now, by `file`, so a second ask waits its turn instead of racing.
static DOWNLOADING: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

pub fn root() -> io::Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "There's no local app data folder")
    })?;
    Ok(PathBuf::from(base).join("topsy"))
}

impl Pinned {
    pub fn path(&self) -> io::Result<PathBuf> {
        Ok(root()?.join(self.file))
    }

    fn part(&self) -> io::Result<PathBuf> {
        Ok(root()?.join(format!("{}.part", self.file)))
    }

    pub fn status(&self) -> Status {
        let len = (self.path().ok())
            .and_then(|p| fs::metadata(p).ok())
            .map(|m| m.len());
        let busy = DOWNLOADING.lock().is_ok_and(|d| d.contains(&self.file));
        Status::of(busy, len, self.size)
    }

    /// Download it unless it is already here. Blocking: run it on a worker thread.
    pub fn download(&self, progress: &dyn Fn(f32), cancel: &AtomicBool) -> io::Result<PathBuf> {
        if self.status() == Status::Ready {
            return self.path();
        }
        let _busy = Busy::claim(self)?;
        let path = self.path()?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let part = self.part()?;
        let mut ticker = Ticker::default();
        let mut report = |done: u64| {
            if let Some(f) = ticker.tick(done, self.size) {
                progress(f);
            }
        };
        let have = fs::metadata(&part).ok().map(|m| m.len());
        let from = match download::start(have, self.size) {
            Start::Fresh => 0,
            Start::Resume(n) => n,
            Start::Verify => self.size,
        };
        if from < self.size {
            self.fetch(&part, from, &mut report, cancel)?;
        }
        report(self.size);
        let digest = download::hex(&hash(&part, cancel)?);
        if digest != self.sha256 {
            fs::remove_file(&part).missing_ok().log_err();
            crate::log(&format!(
                "{}: sha-256 {digest} != {}",
                self.label, self.sha256
            ));
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{} didn't download correctly; try again",
                    capital(self.label)
                ),
            ));
        }
        fs::rename(&part, &path)?;
        Ok(path)
    }

    fn fetch(
        &self,
        part: &Path,
        from: u64,
        report: &mut dyn FnMut(u64),
        cancel: &AtomicBool,
    ) -> io::Result<()> {
        let (mut host, mut path) = (self.host.to_string(), self.path.to_string());
        for _ in 0..=MAX_REDIRECTS {
            // `head` opens the file, `body` writes to it: both reach it through the cell.
            let sink: RefCell<Option<(File, u64)>> = RefCell::new(None);
            let mut head = |h: &http::Head| {
                let start = h.range.as_deref().and_then(download::range_start);
                *sink.borrow_mut() = Some(match download::reply(from, h.status, start) {
                    Reply::Append => (OpenOptions::new().append(true).open(part)?, from),
                    Reply::Restart => (File::create(part)?, 0),
                    Reply::Fail => {
                        crate::log(&format!("{}: http {} from {host}", self.label, h.status));
                        return Err(io::Error::other(format!(
                            "Couldn't download {}; try again later",
                            self.label
                        )));
                    }
                });
                Ok(())
            };
            let mut body = |chunk: &[u8]| {
                if cancel.load(Ordering::Relaxed) {
                    return Err(io::ErrorKind::Interrupted.into());
                }
                let mut sink = sink.borrow_mut();
                let (file, done) = sink
                    .as_mut()
                    .ok_or_else(|| io::Error::other("a body before its head"))?;
                file.write_all(chunk)?;
                *done += chunk.len() as u64;
                if *done > self.size {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("The server sent more than {}", self.label),
                    ));
                }
                report(*done);
                Ok(())
            };
            match http::stream(&host, &path, from, &mut head, &mut body)? {
                Streamed::Done => return Ok(()),
                Streamed::Redirect(to) => {
                    let (h, p) = download::split_url(&to, &host).ok_or_else(|| {
                        io::Error::other("The server sent the download somewhere unsafe")
                    })?;
                    (host, path) = (h.to_string(), p.to_string());
                }
            }
        }
        Err(io::Error::other("The server kept redirecting"))
    }
}

/// Marks a file as downloading for as long as it lives.
struct Busy(&'static str);

impl Busy {
    fn claim(pinned: &Pinned) -> io::Result<Self> {
        let mut downloading = DOWNLOADING
            .lock()
            .map_err(|_| io::Error::other("poisoned"))?;
        if downloading.contains(&pinned.file) {
            return Err(io::Error::new(
                io::ErrorKind::ResourceBusy,
                format!("{} is already downloading", capital(pinned.label)),
            ));
        }
        downloading.push(pinned.file);
        Ok(Self(pinned.file))
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        if let Ok(mut downloading) = DOWNLOADING.lock() {
            downloading.retain(|f| *f != self.0);
        }
    }
}

fn capital(label: &str) -> String {
    let mut chars = label.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn hash(path: &Path, cancel: &AtomicBool) -> io::Result<[u8; 32]> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            return Ok(hasher.finalize().into());
        }
        hasher.update(&buffer[..n]);
        if cancel.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::Interrupted.into());
        }
    }
}
