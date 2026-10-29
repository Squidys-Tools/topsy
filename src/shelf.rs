//! The shelf: files parked on the island.
//!
//! A plain model with no UI or OS calls, so it can be unit tested. `main.rs` renders it
//! and `win32::drag_files_out` drags items back out. Items are references to the original
//! files, not copies.

use std::path::{Path, PathBuf};

/// What a chip is on disk. A dropped path starts `Unknown`: the stat runs off the
/// UI thread (a dead network share can stall it for its timeout) and
/// `Shelf::resolve_kinds` merges the answer back, so the chip never waits on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Unknown,
    File,
    Dir,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub path: PathBuf,
    pub kind: Kind,
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
    /// `FILE` for a file with no extension, `…` until the stat comes back.
    pub fn badge_label(&self) -> String {
        match self.kind {
            Kind::Dir => return "DIR".into(),
            Kind::Unknown => return "…".into(),
            Kind::File => {}
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

    /// Add paths that aren't already on the shelf; returns the ones added, in
    /// order. New chips are `Kind::Unknown` until `resolve_kinds` applies the
    /// stat read off the UI thread.
    pub fn add(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
        let mut added = Vec::new();
        for path in paths {
            if self.items.iter().any(|i| same_path(&i.path, &path)) {
                continue;
            }
            self.items.push(Item {
                path: path.clone(),
                kind: Kind::Unknown,
            });
            added.push(path);
        }
        added
    }

    /// Merge stat answers (`path` → is-dir) into the chips still parked. Answers are
    /// matched by path, never position, so a chip removed or cleared before its
    /// answer arrived is skipped instead of reappearing. Returns how many applied.
    pub fn resolve_kinds(&mut self, resolved: impl IntoIterator<Item = (PathBuf, bool)>) -> usize {
        let mut applied = 0;
        for (path, is_dir) in resolved {
            if let Some(item) = self.items.iter_mut().find(|i| same_path(&i.path, &path)) {
                item.kind = if is_dir { Kind::Dir } else { Kind::File };
                applied += 1;
            }
        }
        applied
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

    /// Rebuild the shelf from saved paths, as at startup. `stat` answers `Some(is_dir)`
    /// for a file that is still there and `None` for one that has gone, so a chip never
    /// comes back for a file deleted while the app was off. Duplicates collapse as in
    /// `add`. Returns how many were restored.
    pub fn restore(
        &mut self,
        paths: impl IntoIterator<Item = PathBuf>,
        stat: impl Fn(&Path) -> Option<bool>,
    ) -> usize {
        let mut added = 0;
        for path in paths {
            if self.items.iter().any(|i| same_path(&i.path, &path)) {
                continue;
            }
            let Some(is_dir) = stat(&path) else {
                continue;
            };
            self.items.push(Item { path, is_dir });
            added += 1;
        }
        added
    }
}

/// The shelf's saved form: one absolute path per line (a path that isn't UTF-8 can't be
/// written plainly and is left out). `state` owns the file; this is only the format.
pub fn serialize(items: &[Item]) -> String {
    let mut text = String::new();
    for item in items {
        if let Some(path) = item.path.to_str() {
            text.push_str(path);
            text.push('\n');
        }
    }
    text
}

/// The paths in a saved file: one per line. Blank lines, junk and relative paths are
/// skipped; whether what is left still exists is not checked here — that is
/// `Shelf::restore`'s `stat`.
pub fn parse(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .filter(|line| Path::new(line).is_absolute())
        .map(PathBuf::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    /// Stat answers as the background read returns them: `(path, is_dir)`.
    fn resolved(list: &[(&str, bool)]) -> Vec<(PathBuf, bool)> {
        list.iter().map(|(p, d)| (PathBuf::from(p), *d)).collect()
    }

    #[test]
    fn add_skips_duplicates_even_with_different_case() {
        let mut shelf = Shelf::default();
        assert_eq!(
            shelf.add(paths(&[r"C:\a\Photo.PNG", r"C:\a\b.txt"])).len(),
            2
        );
        assert_eq!(
            shelf.add(paths(&[r"c:\a\photo.png", r"C:\a\c.txt"])).len(),
            1
        );
        assert_eq!(shelf.len(), 3);
    }

    #[test]
    fn duplicates_are_found_across_accented_case_too() {
        let mut shelf = Shelf::default();
        shelf.add(paths(&[r"C:\Users\Élodie\a.txt"]));
        assert_eq!(shelf.add(paths(&[r"C:\Users\élodie\A.TXT"])).len(), 0);
        assert!(shelf.remove_path(Path::new(r"c:\users\éLODIE\a.txt")));
    }

    #[test]
    fn add_skips_duplicates_within_one_drop() {
        let mut shelf = Shelf::default();
        assert_eq!(shelf.add(paths(&[r"C:\a.txt", r"C:\a.txt"])).len(), 1);
    }

    #[test]
    fn added_chips_are_unknown_until_their_stat_resolves() {
        let mut shelf = Shelf::default();
        // `add` returns the paths still needing a stat, in drop order.
        assert_eq!(
            shelf.add(paths(&[r"C:\a.txt", r"C:\dir"])),
            paths(&[r"C:\a.txt", r"C:\dir"])
        );
        assert_eq!(shelf.items()[0].badge_label(), "…");

        // Answers can come back in any order; only the kind changes, not the position.
        assert_eq!(
            shelf.resolve_kinds(resolved(&[(r"C:\dir", true), (r"C:\a.txt", false)])),
            2
        );
        assert_eq!(shelf.items()[0].badge_label(), "TXT");
        assert_eq!(shelf.items()[1].badge_label(), "DIR");
    }

    #[test]
    fn a_late_stat_for_a_chip_that_left_is_ignored() {
        let mut shelf = Shelf::default();
        shelf.add(paths(&[r"C:\a.txt", r"C:\b"]));
        shelf.remove_path(Path::new(r"C:\a.txt"));
        assert_eq!(
            shelf.resolve_kinds(resolved(&[(r"C:\a.txt", false), (r"C:\b", true)])),
            1
        );
        assert_eq!(shelf.len(), 1);
        assert_eq!(shelf.items()[0].name(), "b");

        // Cleared entirely before any answer arrives: nothing comes back.
        shelf.clear();
        assert_eq!(shelf.resolve_kinds(resolved(&[(r"C:\b", true)])), 0);
        assert!(shelf.is_empty());
    }

    #[test]
    fn re_dropping_a_resolved_chip_does_not_reset_it() {
        let mut shelf = Shelf::default();
        shelf.add(paths(&[r"C:\dir"]));
        shelf.resolve_kinds(resolved(&[(r"C:\dir", true)]));
        // A duplicate drop adds nothing, so nothing is re-statted or sent back to Unknown.
        assert!(shelf.add(paths(&[r"C:\DIR"])).is_empty());
        assert_eq!(shelf.items()[0].kind, Kind::Dir);
    }

    #[test]
    fn remove_by_index_and_by_path() {
        let mut shelf = Shelf::default();
        shelf.add(paths(&[r"C:\a.txt", r"C:\b.txt", r"C:\c.txt"]));
        assert_eq!(shelf.remove(1).unwrap().name(), "b.txt");
        assert!(shelf.remove(9).is_none());
        assert!(shelf.remove_path(Path::new(r"c:\C.TXT")));
        assert!(!shelf.remove_path(Path::new(r"C:\nope.txt")));
        assert_eq!(shelf.len(), 1);
    }

    #[test]
    fn prune_drops_only_missing_files() {
        let mut shelf = Shelf::default();
        shelf.add(paths(&[r"C:\keep.txt", r"C:\gone.txt"]));
        let pruned = shelf.prune(|p| p.ends_with("keep.txt"));
        assert_eq!(pruned, 1);
        assert_eq!(shelf.items()[0].name(), "keep.txt");
    }

    #[test]
    fn clear_empties_the_shelf() {
        let mut shelf = Shelf::default();
        shelf.add(paths(&[r"C:\a.txt"]));
        shelf.clear();
        assert!(shelf.is_empty());
    }

    #[test]
    fn badge_labels() {
        let item = |p: &str, kind| Item {
            path: PathBuf::from(p),
            kind,
        };
        assert_eq!(item(r"C:\x\pic.png", Kind::File).badge_label(), "PNG");
        assert_eq!(item(r"C:\x\a.torrent", Kind::File).badge_label(), "TORR");
        assert_eq!(item(r"C:\x\Makefile", Kind::File).badge_label(), "FILE");
        assert_eq!(item(r"C:\x\folder", Kind::Dir).badge_label(), "DIR");
        assert_eq!(item(r"C:\x\pic.png", Kind::Unknown).badge_label(), "…");
    }

    #[test]
    fn badge_hue_is_stable_and_in_range() {
        let a = Item {
            path: PathBuf::from(r"C:\x\a.png"),
            kind: Kind::File,
        };
        let b = Item {
            path: PathBuf::from(r"D:\y\b.PNG"),
            kind: Kind::File,
        };
        assert_eq!(a.badge_hue(), b.badge_hue());
        assert!((0.0..1.0).contains(&a.badge_hue()));
    }

    #[test]
    fn name_falls_back_to_the_path_for_a_drive_root() {
        let item = Item {
            path: PathBuf::from(r"C:\"),
            kind: Kind::Dir,
        };
        assert_eq!(item.name(), r"C:\");
    }

    #[test]
    fn restore_drops_gone_files_and_learns_their_kind() {
        let mut shelf = Shelf::default();
        let stat = |p: &Path| match p.file_name().and_then(|n| n.to_str()) {
            Some("gone.txt") => None,
            Some("a-dir") => Some(true),
            _ => Some(false),
        };
        let n = shelf.restore(paths(&[r"C:\kept.txt", r"C:\gone.txt", r"C:\a-dir"]), stat);
        assert_eq!(n, 2);
        assert_eq!(shelf.items()[0].name(), "kept.txt");
        assert!(!shelf.items()[0].is_dir);
        assert_eq!(shelf.items()[1].name(), "a-dir");
        assert!(shelf.items()[1].is_dir);
    }

    #[test]
    fn restore_keeps_order_and_collapses_duplicates() {
        let mut shelf = Shelf::default();
        let n = shelf.restore(
            paths(&[r"C:\b.txt", r"C:\A.TXT", r"C:\a.txt", r"C:\b.txt"]),
            |_| Some(false),
        );
        assert_eq!(n, 2);
        assert_eq!(shelf.items()[0].name(), "b.txt");
        assert_eq!(shelf.items()[1].name(), "A.TXT");
    }

    #[test]
    fn serialize_and_parse_round_trip() {
        let mut shelf = Shelf::default();
        shelf.add(
            paths(&[r"C:\a.txt", r"D:\dir", r"C:\Users\Zoë\b ü.txt"]),
            |p| p.ends_with("dir"),
        );
        let text = serialize(shelf.items());
        assert_eq!(text, "C:\\a.txt\nD:\\dir\nC:\\Users\\Zoë\\b ü.txt\n");

        let mut back = Shelf::default();
        assert_eq!(back.restore(parse(&text), |p| Some(p.ends_with("dir"))), 3);
        assert_eq!(shelf.items(), back.items());
    }

    #[test]
    fn parse_skips_blank_junk_and_relative_lines() {
        let text = "C:\\a.txt\r\n\n  \nnot a path\nrelative\\b.txt\nD:\\b.txt\n";
        assert_eq!(parse(text), paths(&[r"C:\a.txt", r"D:\b.txt"]));
    }

    #[test]
    fn serialize_skips_a_path_that_is_not_utf8() {
        use std::os::windows::ffi::OsStringExt;
        // `C:\` followed by a lone surrogate: a legal file name, but not writable as text.
        let weird = PathBuf::from(std::ffi::OsString::from_wide(&[0x43, 0x3A, 0x5C, 0xD800]));
        let items = [
            Item {
                path: weird,
                is_dir: false,
            },
            Item {
                path: PathBuf::from(r"C:\ok.txt"),
                is_dir: false,
            },
        ];
        assert_eq!(serialize(&items), "C:\\ok.txt\n");
    }
}
