//! What the basket's right-click menus do: a tile's acts on its file, the stack's on them all
//! (Droppy's quick actions: share, convert, zip, rename, reveal).

use std::path::PathBuf;

use gpui::{Context, Window};

use super::Basket;
use crate::island::chip::{self, ACTIONS, Action};
use crate::log;
use crate::log::LogErr;
use crate::win32::{self, MenuEntry};
use model::shelf::{Item, Kind};

const TILE: [MenuEntry; 8] = [
    MenuEntry::Item(Action::Transform as usize, "&Transform…"),
    MenuEntry::Item(Action::Share as usize, "&Share…"),
    MenuEntry::Item(Action::Rename as usize, "Re&name"),
    MenuEntry::Item(Action::ShowInFolder as usize, "Show in &folder"),
    MenuEntry::Item(Action::CopyPath as usize, "Copy as &path"),
    MenuEntry::Item(Action::Compress as usize, "Compress to &ZIP file"),
    MenuEntry::Separator,
    MenuEntry::Item(Action::Remove as usize, "&Remove from basket"),
];

const STACK: [MenuEntry; 5] = [
    MenuEntry::Item(Action::Transform as usize, "&Transform…"),
    MenuEntry::Item(Action::Share as usize, "&Share all…"),
    MenuEntry::Item(Action::CopyPath as usize, "Copy as &paths"),
    MenuEntry::Separator,
    MenuEntry::Item(Action::Remove as usize, "&Clear basket"),
];

/// What a menu's choice acts on.
#[derive(Clone)]
enum Target {
    One(Item),
    All(Vec<PathBuf>),
}

impl Basket {
    /// A tile's menu: on a thread of its own, as it pumps messages.
    pub(super) fn tile_menu(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.files.items().get(index).cloned() else {
            return;
        };
        // Transform, first, takes files only.
        let from = usize::from(item.kind != Kind::File);
        self.menu(&TILE[from..], Target::One(item), window, cx);
    }

    /// The stack's menu acts on every file; with one, it is that file's menu.
    pub(super) fn stack_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items = self.files.items();
        if items.len() == 1 {
            return self.tile_menu(0, window, cx);
        }
        let paths: Vec<PathBuf> = items.iter().map(|i| i.path.clone()).collect();
        let from = usize::from(items.iter().any(|i| i.kind != Kind::File));
        self.menu(&STACK[from..], Target::All(paths), window, cx);
    }

    fn menu(
        &mut self,
        entries: &'static [MenuEntry],
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.menu || self.drag_out {
            return;
        }
        self.end_rename(cx);
        let (tx, rx) = async_channel::bounded(1);
        if !chip::on_thread("topsy-basket-menu", move || {
            tx.send_blocking(win32::popup_menu(entries)).log_err();
        }) {
            return;
        }
        self.menu = true;
        cx.spawn_in(window, async move |this, cx| {
            let choice = rx.recv().await.ok().flatten();
            let action = ACTIONS.into_iter().find(|a| Some(*a as usize) == choice);
            let rename = this
                .update(cx, |this, cx| {
                    this.menu = false;
                    match (action, target) {
                        (Some(Action::Rename), Target::One(item)) => Some(item),
                        (Some(action), target) => {
                            this.act(action, target, cx);
                            None
                        }
                        (None, _) => None,
                    }
                })
                .ok()
                .flatten();
            if let Some(item) = rename {
                Self::begin_rename(this, item, cx).await;
            }
        })
        .detach();
    }

    /// Acts on paths, not indexes: the basket may have changed while the menu was up.
    fn act(&mut self, action: Action, target: Target, cx: &mut Context<Self>) {
        let paths = match &target {
            Target::One(item) => vec![item.path.clone()],
            Target::All(paths) => paths.clone(),
        };
        log(&format!("basket {action:?} {}", paths.len()));
        match (action, target) {
            (Action::ShowInFolder, Target::One(item)) => chip::show_in_folder(item.path),
            (Action::CopyPath, _) => chip::copy_as_paths(&paths, cx),
            (Action::Compress, Target::One(item)) => {
                chip::zip(vec![item.path], cx, |this: &mut Self, zip, cx| {
                    this.add(vec![zip], cx)
                });
            }
            (Action::Transform, _) => {
                if let Some(island) = self.island.upgrade() {
                    crate::transform::open(cx, &island, paths);
                }
            }
            (Action::Share, _) => chip::share(self.hwnd, paths, cx),
            (Action::Remove, Target::All(_)) => self.clear(cx),
            (Action::Remove, Target::One(item)) => {
                if self.files.remove_path(&item.path) {
                    self.files_changed(cx);
                    if self.files.is_empty() {
                        self.close(cx);
                    }
                }
            }
            (
                Action::ShowInFolder
                | Action::Compress
                | Action::Rename
                | Action::Extract
                | Action::NewFolder,
                _,
            ) => {}
        }
    }
}
