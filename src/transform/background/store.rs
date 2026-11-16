//! Where the background model lives and how it gets there: one https download of a pinned file,
//! checked against a pinned SHA-256, resumable, then never the network again.

use std::cell::RefCell;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use sha2::{Digest, Sha256};

use crate::model::download::{self, ModelStatus, Reply, Start, Ticker};
use crate::win32::http::{self, Streamed};

/// BiRefNet-lite's own weights (MIT), at a fixed revision of ZhengPeng7/BiRefNet_lite.
const HOST: &str = "huggingface.co";
const PATH: &str =
    "/ZhengPeng7/BiRefNet_lite/resolve/aa62cd87eafb9cc43056d08ef3615a14628b831d/model.safetensors";
pub const SIZE: u64 = 177_634_392;
const SHA256: &str = "4417d89795250e698c3cb0ae8df15743810065f646f48a694fdfa7ca052d0815";
const NAME: &str = "birefnet-lite-aa62cd8.safetensors";
/// Hugging Face answers with a redirect to its CDN, which may itself redirect once more.
const MAX_REDIRECTS: usize = 4;

static DOWNLOADING: AtomicBool = AtomicBool::new(false);

pub fn dir() -> io::Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "There's no local app data folder")
    })?;
    Ok(PathBuf::from(base).join(r"topsy\models"))
}

pub fn path() -> io::Result<PathBuf> {
    Ok(dir()?.join(NAME))
}

fn part_path() -> io::Result<PathBuf> {
    Ok(dir()?.join(format!("{NAME}.part")))
}

pub fn status() -> ModelStatus {
    let len = path()
        .ok()
        .and_then(|p| fs::metadata(p).ok())
        .map(|m| m.len());
    ModelStatus::of(DOWNLOADING.load(Ordering::Acquire), len, SIZE)
}

/// Clears the downloading flag however the download ends.
struct Busy;

impl Drop for Busy {
    fn drop(&mut self) {
        DOWNLOADING.store(false, Ordering::Release);
    }
}

pub fn download(progress: &dyn Fn(f32), cancel: &AtomicBool) -> io::Result<()> {
    if status() == ModelStatus::Ready {
        return Ok(());
    }
    if DOWNLOADING.swap(true, Ordering::AcqRel) {
        return Err(io::Error::new(
            io::ErrorKind::ResourceBusy,
            "The background model is already downloading",
        ));
    }
    let _busy = Busy;
    fs::create_dir_all(dir()?)?;
    let part = part_path()?;
    let mut ticker = Ticker::default();
    let mut report = |done: u64| {
        if let Some(f) = ticker.tick(done, SIZE) {
            progress(f);
        }
    };
    let have = fs::metadata(&part).ok().map(|m| m.len());
    let from = match download::start(have, SIZE) {
        Start::Fresh => 0,
        Start::Resume(n) => n,
        Start::Verify => SIZE,
    };
    if from < SIZE {
        fetch(&part, from, &mut report, cancel)?;
    }
    report(SIZE);
    let digest = hash(&part, cancel)?;
    if download::hex(&digest) != SHA256 {
        let _ = fs::remove_file(&part);
        crate::log(&format!(
            "background model: sha-256 {} != {SHA256}",
            download::hex(&digest)
        ));
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The background model didn't download correctly; try again",
        ));
    }
    fs::rename(&part, path()?)
}

fn fetch(
    part: &std::path::Path,
    from: u64,
    report: &mut dyn FnMut(u64),
    cancel: &AtomicBool,
) -> io::Result<()> {
    let (mut host, mut path) = (HOST.to_string(), PATH.to_string());
    for _ in 0..=MAX_REDIRECTS {
        // `head` opens the file, `body` writes to it: both reach it through the cell.
        let sink: RefCell<Option<(File, u64)>> = RefCell::new(None);
        let mut head = |h: &http::Head| {
            let start = h.range.as_deref().and_then(download::range_start);
            *sink.borrow_mut() = Some(match download::reply(from, h.status, start) {
                Reply::Append => (OpenOptions::new().append(true).open(part)?, from),
                Reply::Restart => (File::create(part)?, 0),
                Reply::Fail => {
                    crate::log(&format!("background model: http {} from {host}", h.status));
                    return Err(io::Error::other(
                        "The model server didn't hand over the file; try again later",
                    ));
                }
            });
            Ok(())
        };
        let mut body = |chunk: &[u8]| {
            if cancel.load(Ordering::Relaxed) {
                return Err(io::ErrorKind::Interrupted.into());
            }
            let mut sink = sink.borrow_mut();
            let (file, done) = sink.as_mut().expect("head opens the file");
            file.write_all(chunk)?;
            *done += chunk.len() as u64;
            if *done > SIZE {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "The model server sent more than the model",
                ));
            }
            report(*done);
            Ok(())
        };
        match http::stream(&host, &path, from, &mut head, &mut body)? {
            Streamed::Done => return Ok(()),
            Streamed::Redirect(to) => {
                let (h, p) = download::split_url(&to, &host).ok_or_else(|| {
                    io::Error::other("The model server sent the download somewhere unsafe")
                })?;
                (host, path) = (h.to_string(), p.to_string());
            }
        }
    }
    Err(io::Error::other("The model server kept redirecting"))
}

fn hash(path: &std::path::Path, cancel: &AtomicBool) -> io::Result<[u8; 32]> {
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
