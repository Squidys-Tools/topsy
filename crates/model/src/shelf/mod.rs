//! The shelf: files parked on the island.
//!
//! A plain model with no UI or OS calls, so it can be unit tested. The island renders it
//! and `win32::drag_files_out` drags items back out. Items are references to the original
//! files, not copies.

mod item;

use std::path::{Path, PathBuf};

use crate::selection::Selection;

pub(crate) use item::same_path;
pub use item::{Item, Kind};

#[derive(Default)]
pub struct Shelf {
    items: Vec<Item>,
    selection: Selection,
    /// The select button is on: a plain click picks a chip instead of opening it.
    picking: bool,
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
                bytes: None,
            });
            added.push(path);
        }
        added
    }

    /// Merge stat answers (`path` → is-dir, size if the stat worked) into the chips still
    /// parked. Answers are matched by path, never position, so a chip removed or cleared before
    /// its answer arrived is skipped instead of reappearing. Returns how many applied.
    pub fn resolve_kinds(
        &mut self,
        resolved: impl IntoIterator<Item = (PathBuf, bool, Option<u64>)>,
    ) -> usize {
        let mut applied = 0;
        for (path, is_dir, bytes) in resolved {
            if let Some(item) = self.items.iter_mut().find(|i| same_path(&i.path, &path)) {
                item.kind = if is_dir { Kind::Dir } else { Kind::File };
                item.bytes = bytes.filter(|_| !is_dir);
                applied += 1;
            }
        }
        applied
    }

    pub fn remove(&mut self, index: usize) -> Option<Item> {
        let removed = (index < self.items.len()).then(|| self.items.remove(index));
        self.keep_selection();
        removed
    }

    /// Remove the item for `path`; false if it wasn't there.
    pub fn remove_path(&mut self, path: &Path) -> bool {
        let before = self.items.len();
        self.items.retain(|i| !same_path(&i.path, path));
        self.keep_selection();
        self.items.len() != before
    }

    /// Forget picks whose chips left; an emptied shelf leaves picking too.
    fn keep_selection(&mut self) {
        self.selection.keep(&self.items);
        if self.items.is_empty() {
            self.picking = false;
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.deselect();
    }

    /// A file was renamed on disk: its chip follows, where it was. False if it isn't parked.
    pub fn rename(&mut self, from: &Path, to: PathBuf) -> bool {
        match self.items.iter_mut().find(|i| same_path(&i.path, from)) {
            Some(item) => {
                self.selection.rename(from, &to);
                item.path = to;
                true
            }
            None => false,
        }
    }

    /// Drop items whose file has gone (moved or deleted since it was parked).
    pub fn prune(&mut self, exists: impl Fn(&Path) -> bool) -> usize {
        let before = self.items.len();
        self.items.retain(|i| exists(&i.path));
        self.keep_selection();
        before - self.items.len()
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    pub fn is_selected(&self, index: usize) -> bool {
        (self.items.get(index)).is_some_and(|i| self.selection.contains(&i.path))
    }

    /// Ctrl-click: pick the chip at `index`, or put it back.
    pub fn toggle_selected(&mut self, index: usize) {
        self.selection.toggle(&self.items, index);
    }

    /// Shift-click: pick the run of chips from the last one Ctrl-clicked to `index`.
    pub fn select_through(&mut self, index: usize) {
        self.selection.extend(&self.items, index);
    }

    /// Let the picked chips go, and leave picking.
    pub fn deselect(&mut self) {
        self.selection.clear();
        self.picking = false;
    }

    /// Picking chips: the select button is on, or some are picked with Ctrl or Shift.
    pub fn is_picking(&self) -> bool {
        self.picking || !self.selection.is_empty()
    }

    /// The select button: start picking, or let the picked chips go.
    pub fn toggle_picking(&mut self) {
        if self.is_picking() {
            self.deselect();
        } else {
            self.picking = true;
        }
    }

    /// A click on the chip at `index`, with Ctrl or Shift held: true if it opens the file.
    /// Ctrl, or any click while picking, picks the chip or puts it back; Shift picks the run up
    /// to it; a plain click opens it.
    pub fn click(&mut self, index: usize, ctrl: bool, shift: bool) -> bool {
        if shift {
            self.select_through(index);
        } else if ctrl || self.picking {
            self.toggle_selected(index);
        } else {
            self.deselect();
            return true;
        }
        false
    }

    /// What the shelf's buttons act on: the picked chips, in shelf order, or all of them when
    /// none are.
    pub fn picked_or_all(&self) -> Vec<Item> {
        let none = self.selection.is_empty();
        (self.items.iter())
            .filter(|i| none || self.selection.contains(&i.path))
            .cloned()
            .collect()
    }

    /// What dragging or a menu on the chip at `index` acts on: the selection, in shelf order,
    /// when the chip is in it, else the chip alone.
    pub fn targets(&self, index: usize) -> Vec<Item> {
        if !self.is_selected(index) {
            return self.items.get(index).cloned().into_iter().collect();
        }
        (self.items.iter())
            .filter(|i| self.selection.contains(&i.path))
            .cloned()
            .collect()
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
/// skipped; whether what is left still exists is not checked here. The app restores them
/// with `Shelf::add` and prunes the gone ones off the UI thread once the island is up.
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

    /// Stat answers as the background read returns them: `(path, is_dir, size)`.
    fn resolved(list: &[(&str, bool)]) -> Vec<(PathBuf, bool, Option<u64>)> {
        list.iter()
            .map(|(p, d)| (PathBuf::from(p), *d, Some(2_400_000)))
            .collect()
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
        assert_eq!(shelf.items()[0].detail(), "");

        // Answers can come back in any order; only the kind changes, not the position.
        assert_eq!(
            shelf.resolve_kinds(resolved(&[(r"C:\dir", true), (r"C:\a.txt", false)])),
            2
        );
        assert_eq!(shelf.items()[0].badge_label(), "TXT");
        assert_eq!(shelf.items()[1].badge_label(), "DIR");
        assert_eq!(shelf.items()[0].detail(), "2.4 MB");
        assert_eq!(shelf.items()[1].detail(), "Folder");
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
    fn picking_turns_a_click_into_a_pick_until_let_go() {
        let mut shelf = Shelf::default();
        shelf.add(paths(&[r"C:\a.txt", r"C:\b.txt", r"C:\c.txt"]));
        assert!(shelf.click(0, false, false), "a plain click opens");
        assert!(!shelf.is_picking());

        shelf.toggle_picking();
        assert!(shelf.is_picking(), "on before anything is picked");
        assert!(!shelf.click(0, false, false));
        assert!(!shelf.click(2, false, false));
        assert!(shelf.is_selected(0) && !shelf.is_selected(1) && shelf.is_selected(2));
        assert_eq!(shelf.picked_or_all().len(), 2);
        assert!(!shelf.click(2, false, false), "a second click puts it back");
        assert!(!shelf.is_selected(2));
        assert_eq!(shelf.picked_or_all()[0].name(), "a.txt");

        shelf.toggle_picking();
        assert!(!shelf.is_picking() && shelf.selection().is_empty());
        assert_eq!(
            shelf.picked_or_all().len(),
            3,
            "none picked: the whole shelf"
        );

        // Ctrl-picking lights the button, which then lets them go.
        assert!(!shelf.click(1, true, false));
        assert!(shelf.is_picking());
        shelf.toggle_picking();
        assert!(!shelf.is_picking() && !shelf.is_selected(1));

        // Picking on a shelf whose chips all leave: the next file opens on a click again.
        shelf.toggle_picking();
        while !shelf.is_empty() {
            shelf.remove(0);
        }
        assert!(!shelf.is_picking());
    }

    #[test]
    fn clear_empties_the_shelf() {
        let mut shelf = Shelf::default();
        shelf.add(paths(&[r"C:\a.txt"]));
        shelf.clear();
        assert!(shelf.is_empty());
    }

    #[test]
    fn serialize_and_parse_round_trip() {
        let mut shelf = Shelf::default();
        shelf.add(paths(&[r"C:\a.txt", r"D:\dir", r"C:\Users\Zoë\b ü.txt"]));
        let text = serialize(shelf.items());
        assert_eq!(text, "C:\\a.txt\nD:\\dir\nC:\\Users\\Zoë\\b ü.txt\n");

        // A restored shelf is rebuilt with `add`, so a hand-edited file's duplicates collapse.
        let mut back = Shelf::default();
        assert_eq!(back.add(parse(&text)).len(), 3);
        assert_eq!(shelf.items(), back.items());
        assert_eq!(back.add(parse("C:\\B.TXT\nC:\\b.txt\n")).len(), 1);
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
                kind: Kind::File,
                bytes: None,
            },
            Item {
                path: PathBuf::from(r"C:\ok.txt"),
                kind: Kind::File,
                bytes: None,
            },
        ];
        assert_eq!(serialize(&items), "C:\\ok.txt\n");
    }

    #[test]
    fn a_renamed_file_keeps_its_place() {
        let mut shelf = Shelf::default();
        shelf.add([PathBuf::from(r"C:\a.jpg"), PathBuf::from(r"C:\b.jpg")]);
        assert!(shelf.rename(Path::new(r"C:\A.JPG"), PathBuf::from(r"C:\Trip 1.jpg")));
        assert_eq!(shelf.items()[0].path, PathBuf::from(r"C:\Trip 1.jpg"));
        assert_eq!(shelf.items()[1].path, PathBuf::from(r"C:\b.jpg"));
        assert!(!shelf.rename(Path::new(r"C:\gone.jpg"), PathBuf::from(r"C:\x.jpg")));
    }
}
