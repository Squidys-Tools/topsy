//! New folder with selection: shelf files moved into a new folder beside them, as Explorer's.
//!
//! Touches the disk, so run it on a worker thread, never the UI thread.

use std::fs;
use std::io;
use std::path::PathBuf;

use crate::archive::reserve_dir;
use crate::log::LogErr;
use crate::win32;
use model::archive::new_folder_paths;
use model::selection::common_parent;

/// Move `paths`, which must share a folder, into a new folder there (`New folder`, or
/// `New folder (2)` if that is taken) and return it. Nothing is overwritten; if any move fails,
/// the ones done are moved back and the new folder removed, so the files stay where they were.
pub fn gather(paths: &[PathBuf]) -> io::Result<PathBuf> {
    let parent = common_parent(paths)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not all in one folder"))?;
    let folder = reserve_dir(new_folder_paths(&parent))?;
    let mut moved: Vec<(&PathBuf, PathBuf)> = Vec::new();
    for path in paths {
        let moved_one = match path.file_name() {
            Some(name) => {
                let into = folder.join(name);
                win32::move_new(path, &into).map(|()| into)
            }
            None => Err(io::Error::new(io::ErrorKind::InvalidInput, "no name")),
        };
        match moved_one {
            Ok(into) => moved.push((path, into)),
            Err(err) => {
                for (from, into) in moved.iter().rev() {
                    win32::move_new(into, from).log_err();
                }
                fs::remove_dir(&folder).log_err();
                return Err(err);
            }
        }
    }
    Ok(folder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::MissingOk;
    use std::path::Path;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("topsy-gather-{}-{tag}", std::process::id()));
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

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = (fs::read_dir(dir).unwrap())
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn files_and_folders_move_into_a_new_folder_beside_them() {
        let scratch = Scratch::new("move");
        fs::create_dir_all(scratch.0.join("New folder")).unwrap();
        fs::write(scratch.0.join("a.txt"), b"a").unwrap();
        fs::create_dir_all(scratch.0.join("b")).unwrap();
        fs::write(scratch.0.join("b").join("c.txt"), b"c").unwrap();

        let folder = gather(&[scratch.0.join("a.txt"), scratch.0.join("b")]).unwrap();
        assert_eq!(folder, scratch.0.join("New folder (2)"));
        assert_eq!(names(&folder), ["a.txt", "b"]);
        assert_eq!(fs::read(folder.join("b").join("c.txt")).unwrap(), b"c");
        assert_eq!(names(&scratch.0), ["New folder", "New folder (2)"]);
    }

    #[test]
    fn a_failed_move_puts_everything_back() {
        let scratch = Scratch::new("fail");
        fs::write(scratch.0.join("a.txt"), b"a").unwrap();
        let gone = scratch.0.join("gone.txt");
        assert!(gather(&[scratch.0.join("a.txt"), gone]).is_err());
        assert_eq!(names(&scratch.0), ["a.txt"]);
    }

    #[test]
    fn files_from_two_folders_are_not_gathered() {
        let scratch = Scratch::new("apart");
        fs::create_dir_all(scratch.0.join("x")).unwrap();
        fs::write(scratch.0.join("a.txt"), b"a").unwrap();
        fs::write(scratch.0.join("x").join("b.txt"), b"b").unwrap();
        let apart = [scratch.0.join("a.txt"), scratch.0.join("x").join("b.txt")];
        assert!(gather(&apart).is_err());
        assert_eq!(names(&scratch.0), ["a.txt", "x"]);
    }
}
