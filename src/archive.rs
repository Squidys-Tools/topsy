//! Compress to .zip: a shelf file or folder, zipped beside the original.
//!
//! Reads and writes every byte of it, so run it on a worker thread, never the UI thread.

use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::model::archive::{entry_name, zip_paths};

/// Zip `source` into a new file beside it (`report.zip`, or `report (2).zip` if that is taken)
/// and return its path. A folder is zipped whole, under its own name, as Explorer does. On any
/// failure the new file is deleted again, so nothing half-written is left behind.
pub fn compress(source: &Path) -> io::Result<PathBuf> {
    let is_dir = fs::metadata(source)?.is_dir();
    let (base, candidates) = source
        .parent()
        .zip(zip_paths(source, is_dir))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no folder to zip it into"))?;
    let (path, file) = reserve(candidates)?;
    let written = write(base, source, is_dir, file);
    if written.is_err() {
        let _ = fs::remove_file(&path);
    }
    written.map(|()| path)
}

/// Create the first free name. `create_new` claims it atomically, so a file that appears there
/// meanwhile is never overwritten.
fn reserve(candidates: impl Iterator<Item = PathBuf>) -> io::Result<(PathBuf, File)> {
    for path in candidates {
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free name for the zip",
    ))
}

fn write(base: &Path, source: &Path, is_dir: bool, file: File) -> io::Result<()> {
    let mut zip = ZipWriter::new(BufWriter::new(file));
    if is_dir {
        add_dir(&mut zip, base, source)?;
    } else {
        add_file(&mut zip, base, source)?;
    }
    zip.finish()?.flush()
}

type Zip = ZipWriter<BufWriter<File>>;

fn add_dir(zip: &mut Zip, base: &Path, dir: &Path) -> io::Result<()> {
    zip.add_directory(name(base, dir)?, options(&fs::metadata(dir)?))?;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        // Links and junctions are left out: following them can loop, or reach far outside.
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            add_dir(zip, base, &entry.path())?;
        } else {
            add_file(zip, base, &entry.path())?;
        }
    }
    Ok(())
}

fn add_file(zip: &mut Zip, base: &Path, path: &Path) -> io::Result<()> {
    let mut source = File::open(path)?;
    let meta = source.metadata()?;
    let options = options(&meta).large_file(meta.len() >= u64::from(u32::MAX));
    zip.start_file(name(base, path)?, options)?;
    io::copy(&mut source, zip)?;
    Ok(())
}

fn name(base: &Path, path: &Path) -> io::Result<String> {
    path.strip_prefix(base)
        .ok()
        .and_then(entry_name)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path outside the zip"))
}

fn options(meta: &Metadata) -> SimpleFileOptions {
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    match modified(meta) {
        Some(time) => options.last_modified_time(time),
        None => options,
    }
}

/// When the file was last written, in local time: a zip's timestamps have no time zone, and
/// Explorer reads them as local.
fn modified(meta: &Metadata) -> Option<zip::DateTime> {
    let written = meta.last_write_time();
    let utc = FILETIME {
        dwLowDateTime: written as u32,
        dwHighDateTime: (written >> 32) as u32,
    };
    let (mut universal, mut local) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    unsafe {
        FileTimeToSystemTime(&utc, &mut universal).ok()?;
        SystemTimeToTzSpecificLocalTime(None, &universal, &mut local).ok()?;
    }
    let byte = |v: u16| u8::try_from(v).ok();
    zip::DateTime::from_date_and_time(
        local.wYear,
        byte(local.wMonth)?,
        byte(local.wDay)?,
        byte(local.wHour)?,
        byte(local.wMinute)?,
        byte(local.wSecond)?,
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// A scratch folder of its own under the temp dir, removed when the test ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("topsy-zip-{}-{tag}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn entries(zip: &Path) -> Vec<(String, Vec<u8>)> {
        let mut archive = zip::ZipArchive::new(File::open(zip).unwrap()).unwrap();
        let mut out = Vec::new();
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).unwrap();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            out.push((entry.name().unwrap().into_owned(), data));
        }
        out.sort();
        out
    }

    #[test]
    fn a_file_is_zipped_beside_itself_and_never_over_another() {
        let scratch = Scratch::new("file");
        let source = scratch.0.join("report.txt");
        fs::write(&source, b"hello").unwrap();
        fs::write(scratch.0.join("report.zip"), b"not ours").unwrap();

        let zip = compress(&source).unwrap();
        assert_eq!(zip, scratch.0.join("report (2).zip"));
        assert_eq!(entries(&zip), [("report.txt".into(), b"hello".to_vec())]);
        assert_eq!(fs::read(scratch.0.join("report.zip")).unwrap(), b"not ours");
    }

    #[test]
    fn a_folder_is_zipped_whole_under_its_own_name() {
        let scratch = Scratch::new("dir");
        let source = scratch.0.join("photos");
        fs::create_dir_all(source.join("2024")).unwrap();
        fs::write(source.join("a.jpg"), b"a").unwrap();
        fs::write(source.join("2024").join("b.jpg"), b"b").unwrap();

        let zip = compress(&source).unwrap();
        assert_eq!(zip, scratch.0.join("photos.zip"));
        assert_eq!(
            entries(&zip),
            [
                ("photos/".into(), Vec::new()),
                ("photos/2024/".into(), Vec::new()),
                ("photos/2024/b.jpg".into(), b"b".to_vec()),
                ("photos/a.jpg".into(), b"a".to_vec()),
            ]
        );
    }

    #[test]
    fn a_failed_zip_leaves_nothing_behind() {
        let scratch = Scratch::new("fail");
        let source = scratch.0.join("gone.txt");
        assert!(compress(&source).is_err());

        // A file another process holds open without sharing reads fails mid-zip.
        let locked = scratch.0.join("photos");
        fs::create_dir_all(&locked).unwrap();
        let held = locked.join("held.bin");
        fs::write(&held, b"x").unwrap();
        let _hold = {
            use std::os::windows::fs::OpenOptionsExt;
            OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&held)
                .unwrap()
        };
        assert!(compress(&locked).is_err());
        let left: Vec<_> = fs::read_dir(&scratch.0)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, ["photos"]);
    }
}
