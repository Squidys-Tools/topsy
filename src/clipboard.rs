//! Clipboard history on disk. One thread owns the history and its folder
//! (`%LOCALAPPDATA%\topsy\clipboard`): each entry is `<id>.json`, an image's pixels
//! `<id>.png` beside it, a link's site icon `icons\<host>.png`. Copies arrive from the
//! clipboard watcher and changes from the strip; the thread wakes otherwise only when the
//! oldest entry is due to expire. It publishes each new state for the strip to read. The rules
//! are in `model::clipboard`.

mod card;
mod favicon;
mod pictures;
mod preview;
pub mod strip;

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::autostart::RegString;
use crate::log;
use crate::log::{LogErr, MissingOk};
use crate::win32::clipboard::{self as watcher, Copy};
use crate::win32::paste::Data;
use model::clipboard::{Added, Content, Entry, History, leftovers};
use model::dib::{self, Rgba};

const ENABLED: RegString<'static> = RegString::new(r"Software\topsy", "Clipboard");
const KEEP_DAYS: RegString<'static> = RegString::new(r"Software\topsy", "ClipboardDays");
/// How long history is kept when the user hasn't chosen.
pub const DEFAULT_DAYS: u32 = 7;
/// The choices the settings window steps through; 0 is forever.
pub const DAY_CHOICES: [u32; 4] = [1, 7, 30, 0];

pub enum Command {
    Copied(Copy),
    Pin(u64, bool),
    Remove(u64),
    /// Everything but the pinned entries.
    Clear,
    /// The retention changed: prune again.
    Wake,
}

/// What the strip hears from the clipboard's threads.
pub enum Event {
    /// The hotkey, with the window that was in front.
    Hotkey(usize),
    /// The history changed; read [`entries`].
    Changed,
}

struct Store {
    dir: PathBuf,
    commands: mpsc::Sender<Command>,
}

static STORE: OnceLock<Store> = OnceLock::new();
static ENTRIES: Mutex<Option<Arc<[Entry]>>> = Mutex::new(None);

/// Clipboard history is on unless switched off (`Clipboard` = `off`).
pub fn is_enabled() -> bool {
    ENABLED.value().as_deref() != Some("off")
}

pub fn set_enabled(on: bool) -> windows::core::Result<()> {
    if on {
        ENABLED.remove()?;
    } else {
        ENABLED.set("off")?;
    }
    watcher::WATCHING.store(on, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// Days history is kept; 0 is forever.
pub fn keep_days() -> u32 {
    KEEP_DAYS
        .value()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_DAYS)
}

pub fn set_keep_days(days: u32) -> windows::core::Result<()> {
    KEEP_DAYS.set(&days.to_string())?;
    send(Command::Wake);
    Ok(())
}

fn keep_secs(days: u32) -> Option<u64> {
    (days > 0).then_some(days as u64 * 86_400)
}

/// The history as last published, newest first.
pub fn entries() -> Arc<[Entry]> {
    let published = ENTRIES.lock().unwrap_or_else(|e| e.into_inner());
    published.clone().unwrap_or_else(|| Arc::new([]))
}

pub fn send(command: Command) {
    if let Some(store) = STORE.get() {
        store.commands.send(command).log_err();
    }
}

/// Where an image entry's pixels are kept.
pub fn image_path(id: u64) -> Option<PathBuf> {
    Some(STORE.get()?.dir.join(format!("{id}.png")))
}

/// Where a site's icon is kept.
pub fn icon_path(host: &str) -> Option<PathBuf> {
    Some(STORE.get()?.dir.join("icons").join(format!("{host}.png")))
}

/// What to put back on the clipboard for `entry`. Reads an image's file, so not on the UI
/// thread.
pub fn data(entry: &Entry) -> Option<Data> {
    Some(match &entry.content {
        Content::Text(text) => Data::Text(text.clone()),
        Content::Files(files) => Data::Files(files.clone()),
        Content::Image { .. } => {
            let png = std::fs::read(image_path(entry.id)?).ok()?;
            let image = image::load_from_memory(&png).ok()?.into_rgba8();
            let (width, height) = image.dimensions();
            Data::Dib(dib::encode(&Rgba {
                width,
                height,
                pixels: image.into_raw(),
            }))
        }
    })
}

/// Start watching the clipboard and keeping its history in `dir` (`None`: history is off for
/// this run, as in scripted runs). The strip listens on what comes back.
pub fn start(dir: Option<PathBuf>) -> Option<async_channel::Receiver<Event>> {
    let dir = dir?;
    if !watcher::claim(&dir) {
        eprintln!("topsy: another topsy keeps this clipboard history; this one keeps none");
        return None;
    }
    watcher::WATCHING.store(is_enabled(), std::sync::atomic::Ordering::Relaxed);
    let (tx, rx) = mpsc::channel();
    let (events, heard) = async_channel::unbounded();
    let changed = events.clone();
    let spawned = std::thread::Builder::new()
        .name("topsy-clipboard-store".into())
        .spawn({
            let dir = dir.clone();
            move || run(&dir, &rx, &changed)
        });
    if let Err(err) = spawned {
        eprintln!("topsy: no clipboard history: {err}");
        return None;
    }
    let copies = tx.clone();
    if STORE.set(Store { dir, commands: tx }).is_err() {
        eprintln!("topsy: clipboard history started twice");
    }
    watcher::spawn(
        move |copy| {
            copies.send(Command::Copied(copy)).log_err();
        },
        move |previous| {
            events.try_send(Event::Hotkey(previous)).log_err();
        },
    );
    Some(heard)
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn run(dir: &Path, rx: &mpsc::Receiver<Command>, changed: &async_channel::Sender<Event>) {
    sweep(dir);
    let mut history = History::restore(load(dir));
    log(&format!("clipboard restored {}", history.entries().len()));
    let mut dirty = true;
    loop {
        let keep = keep_secs(keep_days());
        for id in history.prune(now(), keep) {
            delete(dir, id);
            dirty = true;
        }
        if std::mem::take(&mut dirty) {
            *ENTRIES.lock().unwrap_or_else(|e| e.into_inner()) = Some(history.entries().into());
            changed.try_send(Event::Changed).log_err();
        }
        let wait = history
            .next_expiry(keep)
            .map(|at| Duration::from_secs(at.saturating_sub(now())));
        let command = match wait {
            Some(wait) => match rx.recv_timeout(wait) {
                Ok(command) => command,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            },
            None => match rx.recv() {
                Ok(command) => command,
                Err(_) => return,
            },
        };
        dirty = apply(dir, &mut history, command);
    }
}

/// Carry out one command on the history and its files; whether anything changed.
fn apply(dir: &Path, history: &mut History, command: Command) -> bool {
    match command {
        Command::Copied(copy) => {
            let kind = copy.content.kind();
            match history.add(copy.content, copy.app, now()) {
                Added::New(id) => {
                    if let Some(pixels) = copy.pixels {
                        save_png(dir, id, pixels);
                    }
                    save(dir, history.get(id));
                    log(&format!("clipboard new {id} {kind:?}"));
                }
                Added::Bumped(id) => {
                    save(dir, history.get(id));
                    log(&format!("clipboard bumped {id}"));
                }
                Added::Skipped => return false,
            }
        }
        Command::Pin(id, on) => {
            if !history.set_pinned(id, on) {
                return false;
            }
            save(dir, history.get(id));
        }
        Command::Remove(id) => {
            if !history.remove(id) {
                return false;
            }
            delete(dir, id);
        }
        Command::Clear => history.clear().into_iter().for_each(|id| delete(dir, id)),
        Command::Wake => return false,
    }
    true
}

/// Every entry saved in `dir`. A file that can't be read or parsed is skipped, not fatal.
fn load(dir: &Path) -> Vec<Entry> {
    let Ok(files) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    files
        .filter_map(Result::ok)
        .map(|f| f.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter_map(|p| serde_json::from_slice(&std::fs::read(p).ok()?).ok())
        .collect()
}

/// Remove what a crash left half-written (see [`leftovers`]).
fn sweep(dir: &Path) {
    let Ok(files) = std::fs::read_dir(dir) else {
        return;
    };
    let names: Vec<String> = files
        .filter_map(Result::ok)
        .filter_map(|f| f.file_name().into_string().ok())
        .collect();
    for name in leftovers(&names) {
        log(&format!("clipboard leftover {name}"));
        std::fs::remove_file(dir.join(name)).missing_ok().log_err();
    }
    let icons = std::fs::read_dir(dir.join("icons")).into_iter().flatten();
    for icon in icons.filter_map(Result::ok) {
        if icon.path().extension().is_some_and(|e| e == "tmp") {
            std::fs::remove_file(icon.path()).missing_ok().log_err();
        }
    }
}

fn save(dir: &Path, entry: Option<&Entry>) {
    let Some(entry) = entry else {
        return;
    };
    let path = dir.join(format!("{}.json", entry.id));
    let written = std::fs::create_dir_all(dir)
        .and_then(|()| serde_json::to_vec(entry).map_err(std::io::Error::other))
        .and_then(|json| write(&path, &json));
    if let Err(err) = written {
        eprintln!("topsy: could not save a clipboard entry: {err}");
    }
}

fn save_png(dir: &Path, id: u64, image: Rgba) {
    let Some(buffer) = image::RgbaImage::from_raw(image.width, image.height, image.pixels) else {
        return;
    };
    let mut png = Vec::new();
    let encoded = buffer.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
    let written = encoded
        .map_err(std::io::Error::other)
        .and_then(|()| std::fs::create_dir_all(dir))
        .and_then(|()| write(&dir.join(format!("{id}.png")), &png));
    if let Err(err) = written {
        eprintln!("topsy: could not save a copied image: {err}");
    }
}

/// Through a temp file renamed over the real one, so a kill never leaves half an entry.
fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

fn delete(dir: &Path, id: u64) {
    for ext in ["json", "png"] {
        std::fs::remove_file(dir.join(format!("{id}.{ext}")))
            .missing_ok()
            .log_err();
    }
}
