//! Naming a zip made from a shelf file, and the entries inside it.

use std::path::{Component, Path, PathBuf};

/// How many numbered names to try before giving up on finding a free one.
const MAX_TRIES: u32 = 999;

/// Where the zip of `source` may go, best first: next to it as `report.zip`, then
/// `report (2).zip`, `report (3).zip` and so on, as Explorer numbers copies. A file loses its
/// extension; a folder keeps its whole name, dots and all. `None` for a path with nowhere to
/// put a zip beside it (a drive root).
pub fn zip_paths(source: &Path, is_dir: bool) -> Option<impl Iterator<Item = PathBuf>> {
    let parent = source.parent()?.to_path_buf();
    let stem = if is_dir {
        source.file_name()?
    } else {
        source.file_stem()?
    }
    .to_os_string();
    Some((1..=MAX_TRIES).map(move |n| {
        let mut name = stem.clone();
        if n > 1 {
            name.push(format!(" ({n})"));
        }
        name.push(".zip");
        parent.join(name)
    }))
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
}
