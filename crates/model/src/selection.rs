//! Which chips on the shelf are picked, as in Explorer: Ctrl-click picks one or puts it back,
//! Shift-click picks the run from the last one Ctrl-clicked. Held by path, so it follows the
//! files as chips come and go; `Shelf` keeps it to what is still parked.

use std::path::{Path, PathBuf};

use crate::shelf::{Item, same_path};

#[derive(Default, Debug)]
pub struct Selection {
    picked: Vec<PathBuf>,
    /// Where a Shift-click's run starts.
    anchor: Option<PathBuf>,
}

impl Selection {
    pub fn contains(&self, path: &Path) -> bool {
        self.picked.iter().any(|p| same_path(p, path))
    }

    pub fn is_empty(&self) -> bool {
        self.picked.is_empty()
    }

    pub(crate) fn clear(&mut self) {
        self.picked.clear();
        self.anchor = None;
    }

    pub(crate) fn toggle(&mut self, items: &[Item], index: usize) {
        let Some(item) = items.get(index) else {
            return;
        };
        if self.contains(&item.path) {
            self.picked.retain(|p| !same_path(p, &item.path));
        } else {
            self.picked.push(item.path.clone());
        }
        self.anchor = Some(item.path.clone());
    }

    /// The run from the anchor to `index`, in place of what was picked. With no anchor, the
    /// chip alone, which becomes the anchor.
    pub(crate) fn extend(&mut self, items: &[Item], index: usize) {
        let Some(item) = items.get(index) else {
            return;
        };
        let from = (self.anchor.as_ref())
            .and_then(|anchor| items.iter().position(|i| same_path(&i.path, anchor)));
        let Some(from) = from else {
            self.picked = vec![item.path.clone()];
            self.anchor = Some(item.path.clone());
            return;
        };
        let run = &items[from.min(index)..=from.max(index)];
        self.picked = run.iter().map(|i| i.path.clone()).collect();
    }

    /// Forget what is no longer among `items`.
    pub(crate) fn keep(&mut self, items: &[Item]) {
        let parked = |p: &Path| items.iter().any(|i| same_path(&i.path, p));
        self.picked.retain(|p| parked(p));
        if self.anchor.as_deref().is_some_and(|a| !parked(a)) {
            self.anchor = None;
        }
    }

    pub(crate) fn rename(&mut self, from: &Path, to: &Path) {
        for path in self.picked.iter_mut().chain(self.anchor.as_mut()) {
            if same_path(path, from) {
                *path = to.to_path_buf();
            }
        }
    }
}

/// The folder all of `paths` sit in, if they share one (a new folder for them goes there).
pub fn common_parent(paths: &[PathBuf]) -> Option<PathBuf> {
    let parent = paths.first()?.parent()?;
    (paths.iter())
        .all(|p| p.parent().is_some_and(|other| same_path(other, parent)))
        .then(|| parent.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shelf::Shelf;

    fn shelf(names: &[&str]) -> Shelf {
        let mut shelf = Shelf::default();
        shelf.add(names.iter().map(|n| PathBuf::from(format!(r"C:\x\{n}"))));
        shelf
    }

    fn picked(shelf: &Shelf) -> Vec<String> {
        (shelf.items().iter())
            .filter(|i| shelf.selection().contains(&i.path))
            .map(|i| i.name())
            .collect()
    }

    #[test]
    fn ctrl_click_picks_and_puts_back() {
        let mut shelf = shelf(&["a", "b", "c"]);
        shelf.toggle_selected(0);
        shelf.toggle_selected(2);
        assert_eq!(picked(&shelf), ["a", "c"]);
        shelf.toggle_selected(0);
        assert_eq!(picked(&shelf), ["c"]);
        shelf.toggle_selected(9);
        assert_eq!(picked(&shelf), ["c"]);
    }

    #[test]
    fn shift_click_picks_the_run_from_the_anchor_either_way() {
        let mut shelf = shelf(&["a", "b", "c", "d", "e"]);
        shelf.toggle_selected(2);
        shelf.select_through(4);
        assert_eq!(picked(&shelf), ["c", "d", "e"]);
        // The anchor stays put: a second Shift-click replaces the run.
        shelf.select_through(0);
        assert_eq!(picked(&shelf), ["a", "b", "c"]);
    }

    #[test]
    fn shift_click_with_no_anchor_picks_the_chip_alone() {
        let mut shelf = shelf(&["a", "b", "c"]);
        shelf.select_through(1);
        assert_eq!(picked(&shelf), ["b"]);
        shelf.select_through(2);
        assert_eq!(picked(&shelf), ["b", "c"]);
    }

    #[test]
    fn chips_that_leave_leave_the_selection() {
        let mut shelf = shelf(&["a", "b", "c"]);
        shelf.toggle_selected(0);
        shelf.toggle_selected(1);
        shelf.remove(1);
        assert_eq!(picked(&shelf), ["a"]);
        shelf.remove_path(Path::new(r"C:\X\A"));
        assert!(shelf.selection().is_empty());

        // The anchor went with them, and the file coming back does not bring it back: the shelf is
        // now c, a, and a Shift-click with no anchor picks the chip alone.
        shelf.add([PathBuf::from(r"C:\x\a")]);
        assert!(shelf.selection().is_empty());
        shelf.select_through(0);
        assert_eq!(picked(&shelf), ["c"]);

        shelf.prune(|_| false);
        assert!(shelf.selection().is_empty());
    }

    #[test]
    fn clearing_the_shelf_or_the_selection_forgets_it() {
        let mut shelf = shelf(&["a", "b"]);
        shelf.toggle_selected(1);
        shelf.deselect();
        assert!(shelf.selection().is_empty());
        // The anchor is gone too.
        shelf.select_through(0);
        assert_eq!(picked(&shelf), ["a"]);
        shelf.clear();
        shelf.add([PathBuf::from(r"C:\x\a")]);
        assert!(shelf.selection().is_empty());
    }

    #[test]
    fn a_renamed_file_stays_picked() {
        let mut shelf = shelf(&["a", "b"]);
        shelf.toggle_selected(0);
        shelf.rename(Path::new(r"C:\x\a"), PathBuf::from(r"C:\x\z"));
        assert_eq!(picked(&shelf), ["z"]);
    }

    #[test]
    fn actions_take_the_selection_when_the_chip_is_in_it() {
        let mut shelf = shelf(&["a", "b", "c"]);
        let names = |items: Vec<Item>| items.iter().map(Item::name).collect::<Vec<_>>();
        assert_eq!(names(shelf.targets(1)), ["b"]);
        shelf.toggle_selected(2);
        shelf.toggle_selected(0);
        // In shelf order, whichever chip was clicked.
        assert_eq!(names(shelf.targets(2)), ["a", "c"]);
        // A chip outside the selection is acted on alone.
        assert_eq!(names(shelf.targets(1)), ["b"]);
        assert!(shelf.targets(9).is_empty());
    }

    #[test]
    fn a_common_parent_is_one_folder_for_all() {
        let paths = |list: &[&str]| list.iter().map(PathBuf::from).collect::<Vec<_>>();
        assert_eq!(
            common_parent(&paths(&[r"C:\x\a.txt", r"c:\X\b"])),
            Some(PathBuf::from(r"C:\x"))
        );
        assert_eq!(common_parent(&paths(&[r"C:\x\a.txt", r"C:\y\b"])), None);
        assert_eq!(common_parent(&paths(&[r"C:\"])), None);
        assert_eq!(common_parent(&[]), None);
    }
}
