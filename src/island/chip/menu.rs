//! A chip's right-click menu, and what its choices do: to the chip, or to the selection it is in.

use std::path::PathBuf;

use gpui::Context;

use super::{copy_as_paths, file_job, names, on_thread, share, show_in_folder, zip};
use crate::island::COLLAPSE_AFTER_LEAVE;
use crate::log::LogErr;
use crate::win32::{self, MenuEntry};
use crate::{Island, archive, log, new_folder};
use model::selection::common_parent;
use model::shelf::{Item, Kind};

/// What a chip's right-click menu offers; each is its own menu id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    ShowInFolder = 1,
    CopyPath,
    Compress,
    Remove,
    Transform,
    Share,
    /// The basket's only: renaming types into the window, and the island never takes the keys.
    Rename,
    Extract,
    NewFolder,
}

pub(crate) const ACTIONS: [Action; 9] = [
    Action::ShowInFolder,
    Action::CopyPath,
    Action::Compress,
    Action::Remove,
    Action::Transform,
    Action::Share,
    Action::Rename,
    Action::Extract,
    Action::NewFolder,
];

/// The menu for one chip, or for the selection.
fn entries(items: &[Item]) -> Vec<MenuEntry> {
    let item = |action: Action, label| MenuEntry::Item(action as usize, label);
    let mut menu = Vec::new();
    // Transform takes files only.
    if items.iter().all(|i| i.kind == Kind::File) {
        menu.push(item(Action::Transform, "&Transform…"));
    }
    menu.push(item(Action::Share, "&Share…"));
    match items {
        [one] => {
            menu.push(item(Action::ShowInFolder, "Show in &folder"));
            menu.push(item(Action::CopyPath, "Copy as &path"));
            menu.push(item(Action::Compress, "Compress to &ZIP file"));
            if one.is_zip() {
                menu.push(item(Action::Extract, "&Extract here"));
            }
        }
        many => {
            menu.push(item(Action::CopyPath, "Copy as &paths"));
            menu.push(item(Action::Compress, "Compress to &ZIP file"));
            let paths: Vec<PathBuf> = many.iter().map(|i| i.path.clone()).collect();
            if common_parent(&paths).is_some() {
                menu.push(item(Action::NewFolder, "&New folder with selection"));
            }
        }
    }
    menu.push(MenuEntry::Separator);
    menu.push(item(Action::Remove, "&Remove from shelf"));
    menu
}

impl Island {
    /// Pop up the menu for the chip at `index` (for the selection, if the chip is in it) at the
    /// cursor and carry out the choice.
    ///
    /// The menu runs its own message loop, so it runs on a thread of its own (see
    /// [`win32::popup_menu`]) and the choice comes back here. The tray stays open meanwhile.
    pub(super) fn chip_menu(&mut self, index: usize, cx: &mut Context<Self>) {
        let items = self.shelf.targets(index);
        if items.is_empty() || self.chip_menu || self.drag_out {
            return;
        }
        let (tx, rx) = async_channel::bounded(1);
        let menu = entries(&items);
        if !on_thread("topsy-chip-menu", move || {
            tx.send_blocking(win32::popup_menu(&menu)).log_err();
        }) {
            return;
        }
        self.chip_menu = true;
        self.collapse_generation += 1;
        cx.spawn(async move |this, cx| {
            let choice = rx.recv().await.ok().flatten();
            let action = ACTIONS.into_iter().find(|a| Some(*a as usize) == choice);
            this.update(cx, |this, cx| {
                this.chip_menu = false;
                if let Some(action) = action {
                    this.chip_action(action, items, cx);
                }
                this.schedule_collapse(COLLAPSE_AFTER_LEAVE, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Acts on the chips' paths, not their indexes: the shelf may have changed while the menu
    /// was up.
    fn chip_action(&mut self, action: Action, items: Vec<Item>, cx: &mut Context<Self>) {
        let name = names(&items);
        log(&format!("chip {action:?} {name}"));
        let paths: Vec<PathBuf> = items.into_iter().map(|i| i.path).collect();
        match action {
            Action::ShowInFolder => {
                if let Some(first) = paths.into_iter().next() {
                    show_in_folder(first);
                }
            }
            Action::CopyPath => copy_as_paths(&paths, cx),
            Action::Compress => zip(paths, cx, |this: &mut Self, zip, cx| {
                this.add_paths(vec![zip], cx);
            }),
            Action::Extract => {
                if let Some(zip) = paths.into_iter().next() {
                    let job = move || archive::extract(&zip);
                    file_job("extract", name, job, cx, |this: &mut Self, folder, cx| {
                        this.add_paths(vec![folder], cx);
                    });
                }
            }
            Action::NewFolder => {
                let moved = paths.clone();
                let job = move || new_folder::gather(&paths);
                file_job(
                    "move into a new folder",
                    name,
                    job,
                    cx,
                    move |this, folder, cx| {
                        for path in &moved {
                            this.shelf.remove_path(path);
                        }
                        this.add_paths(vec![folder], cx);
                    },
                );
            }
            Action::Transform => {
                let island = cx.entity();
                crate::transform::open(cx, &island, paths);
            }
            Action::Share => share(self.hwnd, paths, cx),
            Action::Remove => {
                let mut removed = false;
                for path in &paths {
                    removed |= self.shelf.remove_path(path);
                }
                if removed {
                    self.shelf_changed(cx);
                }
            }
            Action::Rename => {}
        }
    }
}
