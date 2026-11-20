//! Naming what the shelf's file actions make (a zip, the folder it extracts to, a new folder),
//! and the entries inside a zip.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

/// How many numbered names to try before giving up on finding a free one.
const MAX_TRIES: u32 = 999;

/// Where the zip of `source` may go, best first: next to it as `report.zip`, then
/// `report (2).zip`, `report (3).zip` and so on, as Explorer numbers copies. A file loses its
/// extension; a folder keeps its whole name, dots and all. `None` for a path with nowhere to
/// put a zip beside it (a drive root).
pub fn zip_paths(source: &Path, is_dir: bool) -> Option<impl Iterator<Item = PathBuf>> {
    let stem = if is_dir {
        source.file_name()?
    } else {
        source.file_stem()?
    };
    Some(numbered(source.parent()?, stem, ".zip"))
}

/// Where the zip at `zip` may be extracted to, best first: a folder beside it named after it
/// (`photos` for `photos.zip`), then `photos (2)` and so on. `None` for a path with no name.
pub fn extract_paths(zip: &Path) -> Option<impl Iterator<Item = PathBuf>> {
    Some(numbered(zip.parent()?, zip.file_stem()?, ""))
}

/// Where a new folder in `parent` may go, best first, as Explorer names them: `New folder`,
/// then `New folder (2)` and so on.
pub fn new_folder_paths(parent: &Path) -> impl Iterator<Item = PathBuf> {
    numbered(parent, OsStr::new("New folder"), "")
}

/// `stem`, then `stem (2)`, `stem (3)` .., each followed by `ext`, in `parent`.
fn numbered(parent: &Path, stem: &OsStr, ext: &str) -> impl Iterator<Item = PathBuf> {
    let (parent, stem, ext) = (parent.to_path_buf(), stem.to_os_string(), ext.to_owned());
    (1..=MAX_TRIES).map(move |n| {
        let mut name = stem.clone();
        if n > 1 {
            name.push(format!(" ({n})"));
        }
        name.push(&ext);
        parent.join(name)
    })
}

/// The name a file or folder at `relative` (to the folder the zip sits in) has inside the zip:
/// `/`-separated, as the format requires. `None` for a path that climbs out or isn't relative.
pub fn entry_name(relative: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Where an entry named `name` inside a zip lands, relative to the folder it is extracted to.
/// `None` for a name that could land anywhere else or that Windows would not keep as written:
/// absolute, a drive, `..`, a stream (`a:b`), a device (`NUL`), or a part ending in a dot or a
/// space, which Windows trims (so `.. ` would climb out).
pub fn entry_path(name: &str) -> Option<PathBuf> {
    let mut path = PathBuf::new();
    let mut parts = name.split(['/', '\\']).peekable();
    while let Some(part) = parts.next() {
        // A folder's entry ends with a slash; any other empty part is a root or a doubled slash.
        if part.is_empty() && parts.peek().is_none() && !path.as_os_str().is_empty() {
            break;
        }
        if part == "." {
            continue;
        }
        if !safe_part(part) {
            return None;
        }
        path.push(part);
    }
    (!path.as_os_str().is_empty()).then_some(path)
}

fn safe_part(part: &str) -> bool {
    let device = (part.split('.').next().unwrap_or(part))
        .trim_end()
        .to_ascii_uppercase();
    let numbered_device = (device.starts_with("COM") || device.starts_with("LPT"))
        && device.len() == 4
        && device.as_bytes()[3].is_ascii_digit();
    !part.is_empty()
        && !part.ends_with(['.', ' '])
        && !part
            .chars()
            .any(|c| c.is_control() || "<>:\"|?*".contains(c))
        && !["CON", "PRN", "AUX", "NUL"].contains(&device.as_str())
        && !numbered_device
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first(source: &str, is_dir: bool, n: usize) -> Vec<PathBuf> {
        zip_paths(Path::new(source), is_dir)
            .unwrap()
            .take(n)
            .collect()
    }

    #[test]
    fn a_file_is_zipped_beside_itself_without_its_extension() {
        assert_eq!(
            first(r"C:\work\report.pdf", false, 3),
            [
                PathBuf::from(r"C:\work\report.zip"),
                PathBuf::from(r"C:\work\report (2).zip"),
                PathBuf::from(r"C:\work\report (3).zip"),
            ]
        );
        assert_eq!(
            first(r"C:\work\a.tar.gz", false, 1),
            [PathBuf::from(r"C:\work\a.tar.zip")]
        );
        assert_eq!(
            first(r"C:\work\.env", false, 1),
            [PathBuf::from(r"C:\work\.env.zip")]
        );
    }

    #[test]
    fn a_folder_keeps_its_whole_name() {
        assert_eq!(
            first(r"C:\work\v1.2 photos", true, 2),
            [
                PathBuf::from(r"C:\work\v1.2 photos.zip"),
                PathBuf::from(r"C:\work\v1.2 photos (2).zip"),
            ]
        );
    }

    #[test]
    fn a_zip_of_a_zip_does_not_name_itself() {
        // `a.zip` is the source, so the first free candidate is the numbered one.
        let source = Path::new(r"C:\work\a.zip");
        let free = zip_paths(source, false).unwrap().find(|p| p != source);
        assert_eq!(free, Some(PathBuf::from(r"C:\work\a (2).zip")));
    }

    #[test]
    fn the_names_run_out() {
        let names = zip_paths(Path::new(r"C:\work\x.txt"), false).unwrap();
        assert_eq!(names.count(), MAX_TRIES as usize);
    }

    #[test]
    fn a_drive_root_has_nowhere_to_put_a_zip() {
        assert!(zip_paths(Path::new(r"C:\"), true).is_none());
    }

    #[test]
    fn a_zip_extracts_to_a_folder_named_after_it() {
        let names: Vec<_> = (extract_paths(Path::new(r"C:\work\photos.v2.zip")).unwrap())
            .take(2)
            .collect();
        assert_eq!(
            names,
            [
                PathBuf::from(r"C:\work\photos.v2"),
                PathBuf::from(r"C:\work\photos.v2 (2)")
            ]
        );
    }

    #[test]
    fn new_folders_are_numbered_as_explorer_numbers_them() {
        let names: Vec<_> = new_folder_paths(Path::new(r"C:\work")).take(3).collect();
        assert_eq!(
            names,
            [
                PathBuf::from(r"C:\work\New folder"),
                PathBuf::from(r"C:\work\New folder (2)"),
                PathBuf::from(r"C:\work\New folder (3)"),
            ]
        );
    }

    #[test]
    fn entries_use_forward_slashes() {
        assert_eq!(
            entry_name(Path::new(r"photos\2024\a.jpg")).as_deref(),
            Some("photos/2024/a.jpg")
        );
        assert_eq!(
            entry_name(Path::new("report.pdf")).as_deref(),
            Some("report.pdf")
        );
    }

    #[test]
    fn entries_never_climb_out() {
        assert_eq!(entry_name(Path::new(r"..\secret")), None);
        assert_eq!(entry_name(Path::new(r"C:\abs")), None);
        assert_eq!(entry_name(Path::new("")), None);
    }

    #[test]
    fn extracted_entries_land_inside_their_folder() {
        let lands = |name: &str| entry_path(name);
        assert_eq!(lands("a.txt"), Some(PathBuf::from("a.txt")));
        assert_eq!(
            lands("photos/2024/b.jpg"),
            Some(PathBuf::from(r"photos\2024\b.jpg"))
        );
        assert_eq!(lands(r"photos\b.jpg"), Some(PathBuf::from(r"photos\b.jpg")));
        assert_eq!(lands("photos/"), Some(PathBuf::from("photos")));
        assert_eq!(lands("./a/./b"), Some(PathBuf::from(r"a\b")));
        assert_eq!(
            lands("v1.2 notes.txt"),
            Some(PathBuf::from("v1.2 notes.txt"))
        );
        assert_eq!(lands("console.log"), Some(PathBuf::from("console.log")));
        assert_eq!(lands("COM10.txt"), Some(PathBuf::from("COM10.txt")));
    }

    #[test]
    fn extracted_entries_never_climb_out() {
        for name in [
            "../evil.exe",
            r"..\evil.exe",
            "a/../../evil.exe",
            "/etc/passwd",
            r"\Windows\evil.dll",
            r"C:\Windows\evil.dll",
            "C:evil.dll",
            r"\\server\share\evil",
            ".. /evil.exe",
            "../",
            "a//b",
            "a.txt:stream",
            "NUL",
            "con.txt",
            "COM1.log",
            "lpt9",
            "trail.",
            "trail ",
            "bad\u{1}name",
            "",
            "/",
            "./",
        ] {
            assert_eq!(entry_path(name), None, "{name:?}");
        }
    }
}
