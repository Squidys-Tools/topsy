//! Compress to .zip and extract a .zip: shelf files zipped beside the originals, a zip unpacked
//! into a folder beside it.
//!
//! Reads and writes every byte of it, so run it on a worker thread, never the UI thread.

use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, BufReader, BufWriter, Write};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::log::{LogErr, MissingOk};
use model::archive::{entry_name, entry_path, extract_paths, zip_paths};

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
        fs::remove_file(&path).missing_ok().log_err();
    }
    written.map(|()| path)
}

/// Zip `sources` into one new file beside the first, named after it as [`compress`] names a
/// zip of it alone. One source is [`compress`].
pub fn compress_all(sources: &[PathBuf]) -> io::Result<PathBuf> {
    let [first, rest @ ..] = sources else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "nothing to zip",
        ));
    };
    if rest.is_empty() {
        return compress(first);
    }
    let candidates = zip_paths(first, fs::metadata(first)?.is_dir())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no folder to zip it into"))?;
    let (path, file) = reserve(candidates)?;
    drop(file);
    let written = compress_files(sources, &path, &AtomicBool::new(false));
    if written.is_err() {
        fs::remove_file(&path).missing_ok().log_err();
    }
    written.map(|()| path)
}

/// Extract the zip at `zip` into a new folder beside it, named after it (`photos`, or
/// `photos (2)` if that is taken), and return the folder. Nothing is overwritten, and an entry
/// that would land outside the folder fails the whole extraction; on any failure the folder is
/// deleted again. Links inside the zip are left out, as [`compress`] leaves them out.
pub fn extract(zip: &Path) -> io::Result<PathBuf> {
    let mut archive = ZipArchive::new(BufReader::new(File::open(zip)?))?;
    let candidates = extract_paths(zip)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no folder to extract into"))?;
    let dir = reserve_dir(candidates)?;
    let unpacked = unpack(&mut archive, &dir);
    if unpacked.is_err() {
        fs::remove_dir_all(&dir).missing_ok().log_err();
    }
    unpacked.map(|()| dir)
}

fn unpack(archive: &mut ZipArchive<BufReader<File>>, dir: &Path) -> io::Result<()> {
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_symlink() {
            continue;
        }
        let name = entry.name()?.into_owned();
        let target = (entry_path(&name))
            .map(|relative| dir.join(relative))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, format!("unsafe entry {name:?}"))
            })?;
        if entry.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        io::copy(&mut entry, &mut out)?;
    }
    Ok(())
}

/// Create the first free folder of `candidates`; like [`reserve`], never one already there.
pub(crate) fn reserve_dir(candidates: impl Iterator<Item = PathBuf>) -> io::Result<PathBuf> {
    for path in candidates {
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free name for the folder",
    ))
}

/// Zip several files or folders into `out` (already created), each under its own name at the
/// top of the zip. Two sources with the same name keep both: the later is numbered, `a (2).jpg`.
pub fn compress_files(sources: &[PathBuf], out: &Path, cancel: &AtomicBool) -> io::Result<()> {
    let file = OpenOptions::new().write(true).truncate(true).open(out)?;
    let mut zip = ZipWriter::new(BufWriter::new(file));
    let mut taken: Vec<String> = Vec::new();
    for source in sources {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "Cancelled"));
        }
        let base = source
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no folder"))?;
        let own = name(base, source)?;
        let renamed = unique_name(&own, &taken);
        taken.push(renamed.clone());
        if fs::metadata(source)?.is_dir() {
            if renamed != own {
                return Err(io::Error::other(format!("Two folders are called {own}")));
            }
            add_dir(&mut zip, base, source)?;
        } else {
            let mut reader = File::open(source)?;
            let meta = reader.metadata()?;
            let options = options(&meta).large_file(meta.len() >= u64::from(u32::MAX));
            zip.start_file(renamed, options)?;
            io::copy(&mut reader, &mut zip)?;
        }
    }
    zip.finish()?.flush()
}

/// `name`, or the first of `name (2)`, `name (3)` .. not in `taken`, keeping the extension.
fn unique_name(name: &str, taken: &[String]) -> String {
    let free = |n: &String| !taken.iter().any(|t| t.eq_ignore_ascii_case(n));
    if free(&name.to_owned()) {
        return name.to_owned();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], &name[dot..]),
        _ => (name, ""),
    };
    (2..)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(free)
        .unwrap_or_else(|| name.to_owned())
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
            fs::remove_dir_all(&dir).missing_ok().log_err();
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).missing_ok().log_err();
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
    fn files_with_the_same_name_are_zipped_side_by_side() {
        let scratch = Scratch::new("many");
        for dir in ["x", "y"] {
            fs::create_dir_all(scratch.0.join(dir)).unwrap();
            fs::write(scratch.0.join(dir).join("a.jpg"), dir).unwrap();
        }
        let out = scratch.0.join("Archive.zip");
        File::create(&out).unwrap();
        let sources = [
            scratch.0.join("x").join("a.jpg"),
            scratch.0.join("y").join("a.jpg"),
        ];
        compress_files(&sources, &out, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            entries(&out),
            [
                ("a (2).jpg".into(), b"y".to_vec()),
                ("a.jpg".into(), b"x".to_vec())
            ]
        );
    }

    #[test]
    fn several_files_are_zipped_into_one_named_after_the_first() {
        let scratch = Scratch::new("all");
        let (a, b) = (scratch.0.join("a.txt"), scratch.0.join("b"));
        fs::write(&a, b"a").unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(b.join("c.txt"), b"c").unwrap();
        fs::write(scratch.0.join("a.zip"), b"not ours").unwrap();

        let zip = compress_all(&[a, b]).unwrap();
        assert_eq!(zip, scratch.0.join("a (2).zip"));
        assert_eq!(
            entries(&zip),
            [
                ("a.txt".into(), b"a".to_vec()),
                ("b/".into(), Vec::new()),
                ("b/c.txt".into(), b"c".to_vec()),
            ]
        );
        assert_eq!(fs::read(scratch.0.join("a.zip")).unwrap(), b"not ours");
    }

    /// A zip at `path` holding `files` (name and bytes), written as they are named.
    fn make_zip(path: &Path, files: &[(&str, &[u8])]) {
        let mut zip = ZipWriter::new(File::create(path).unwrap());
        for (name, data) in files {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn a_zip_is_extracted_beside_itself_and_never_over_anything() {
        let scratch = Scratch::new("extract");
        let zip = scratch.0.join("photos.zip");
        make_zip(&zip, &[("a.jpg", b"a"), ("2024/b.jpg", b"b")]);
        fs::create_dir_all(scratch.0.join("photos")).unwrap();

        let dir = extract(&zip).unwrap();
        assert_eq!(dir, scratch.0.join("photos (2)"));
        assert_eq!(fs::read(dir.join("a.jpg")).unwrap(), b"a");
        assert_eq!(fs::read(dir.join("2024").join("b.jpg")).unwrap(), b"b");
        assert_eq!(fs::read_dir(scratch.0.join("photos")).unwrap().count(), 0);
    }

    #[test]
    fn a_zip_that_climbs_out_extracts_nothing() {
        let scratch = Scratch::new("slip");
        let zip = scratch.0.join("evil.zip");
        make_zip(&zip, &[("fine.txt", b"x"), ("../escaped.txt", b"x")]);

        assert!(extract(&zip).is_err());
        let left: Vec<_> = fs::read_dir(&scratch.0)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, ["evil.zip"]);
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
