//! The shelf: files parked on the island.
//!
//! A plain model with no UI or OS calls, so it can be unit tested. `main.rs` renders it
//! and `win32::drag_files_out` drags items back out. Items are references to the original
//! files, not copies.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub path: PathBuf,
    pub is_dir: bool,
}

impl Item {
    /// The file name, or the whole path for something without one (a drive root).
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .unwrap_or(self.path.as_os_str())
            .to_string_lossy()
            .into_owned()
    }

    /// Up to four capital letters for the badge: the extension, `DIR` for a folder,
    /// `FILE` for a file with no extension.
    pub fn badge_label(&self) -> String {
        if self.is_dir {
            return "DIR".into();
        }
        match self.path.extension().and_then(|e| e.to_str()) {
            Some(ext) if !ext.is_empty() => ext.chars().take(4).collect::<String>().to_uppercase(),
            _ => "FILE".into(),
        }
    }

    /// A stable hue in `0.0..1.0` for the badge, so the same type always has the same colour.
    pub fn badge_hue(&self) -> f32 {
        // FNV-1a over the label: spread out, and the same on every run.
        let mut hash: u32 = 0x811c_9dc5;
        for byte in self.badge_label().bytes() {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(0x0100_0193);
        }
        (hash % 360) as f32 / 360.0
    }
}

/// Windows paths compare case-insensitively, accents and all.
fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

#[derive(Default)]
pub struct Shelf {
    items: Vec<Item>,
}

impl Shelf {
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Add paths that aren't already on the shelf; returns how many were added.
    pub fn add(
        &mut self,
        paths: impl IntoIterator<Item = PathBuf>,
        is_dir: impl Fn(&Path) -> bool,
    ) -> usize {
        let mut added = 0;
        for path in paths {
            if self.items.iter().any(|i| same_path(&i.path, &path)) {
                continue;
            }
            let is_dir = is_dir(&path);
            self.items.push(Item { path, is_dir });
            added += 1;
        }
        added
    }

    pub fn remove(&mut self, index: usize) -> Option<Item> {
        (index < self.items.len()).then(|| self.items.remove(index))
    }

    /// Remove the item for `path`; false if it wasn't there.
    pub fn remove_path(&mut self, path: &Path) -> bool {
        let before = self.items.len();
        self.items.retain(|i| !same_path(&i.path, path));
        self.items.len() != before
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Drop items whose file has gone (moved or deleted since it was parked).
    pub fn prune(&mut self, exists: impl Fn(&Path) -> bool) -> usize {
        let before = self.items.len();
        self.items.retain(|i| exists(&i.path));
        before - self.items.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    fn files(_: &Path) -> bool {
        false
    }

    #[test]
    fn add_skips_duplicates_even_with_different_case() {
        let mut shelf = Shelf::default();
        assert_eq!(
            shelf.add(paths(&[r"C:\a\Photo.PNG", r"C:\a\b.txt"]), files),
            2
        );
        assert_eq!(
            shelf.add(paths(&[r"c:\a\photo.png", r"C:\a\c.txt"]), files),
            1
        );
        assert_eq!(shelf.len(), 3);
    }

    #[test]
    fn duplicates_are_found_across_accented_case_too() {
        let mut shelf = Shelf::default();
        shelf.add(paths(&[r"C:\Users\Élodie\a.txt"]), files);
        assert_eq!(shelf.add(paths(&[r"C:\Users\élodie\A.TXT"]), files), 0);
        assert!(shelf.remove_path(Path::new(r"c:\users\éLODIE\a.txt")));
    }

    #[test]
    fn add_skips_duplicates_within_one_drop() {
        let mut shelf = Shelf::default();
        assert_eq!(shelf.add(paths(&[r"C:\a.txt", r"C:\a.txt"]), files), 1);
    }

    #[test]
    fn remove_by_index_and_by_path() {
        let mut shelf = Shelf::default();
        shelf.add(paths(&[r"C:\a.txt", r"C:\b.txt", r"C:\c.txt"]), files);
        assert_eq!(shelf.remove(1).unwrap().name(), "b.txt");
        assert!(shelf.remove(9).is_none());
        assert!(shelf.remove_path(Path::new(r"c:\C.TXT")));
        assert!(!shelf.remove_path(Path::new(r"C:\nope.txt")));
        assert_eq!(shelf.len(), 1);
    }

    #[test]
    fn prune_drops_only_missing_files() {
        let mut shelf = Shelf::default();
        shelf.add(paths(&[r"C:\keep.txt", r"C:\gone.txt"]), files);
        let pruned = shelf.prune(|p| p.ends_with("keep.txt"));
        assert_eq!(pruned, 1);
        assert_eq!(shelf.items()[0].name(), "keep.txt");
    }

    #[test]
    fn clear_empties_the_shelf() {
        let mut shelf = Shelf::default();
        shelf.add(paths(&[r"C:\a.txt"]), files);
        shelf.clear();
        assert!(shelf.is_empty());
    }

    #[test]
    fn badge_labels() {
        let item = |p: &str, is_dir| Item {
            path: PathBuf::from(p),
            is_dir,
        };
        assert_eq!(item(r"C:\x\pic.png", false).badge_label(), "PNG");
        assert_eq!(item(r"C:\x\a.torrent", false).badge_label(), "TORR");
        assert_eq!(item(r"C:\x\Makefile", false).badge_label(), "FILE");
        assert_eq!(item(r"C:\x\folder", true).badge_label(), "DIR");
    }

    #[test]
    fn badge_hue_is_stable_and_in_range() {
        let a = Item {
            path: PathBuf::from(r"C:\x\a.png"),
            is_dir: false,
        };
        let b = Item {
            path: PathBuf::from(r"D:\y\b.PNG"),
            is_dir: false,
        };
        assert_eq!(a.badge_hue(), b.badge_hue());
        assert!((0.0..1.0).contains(&a.badge_hue()));
    }

    #[test]
    fn name_falls_back_to_the_path_for_a_drive_root() {
        let item = Item {
            path: PathBuf::from(r"C:\"),
            is_dir: true,
        };
        assert_eq!(item.name(), r"C:\");
    }
}
