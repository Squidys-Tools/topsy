//! A chip: one file on the shelf as the tray draws it, and what can be done with it (open it,
//! pick it with Ctrl or Shift, drag it out, the actions in its right-click menu).

mod menu;

use std::io;
use std::path::PathBuf;

use gpui::{
    App, ClickEvent, ClipboardItem, Context, FontWeight, MouseButton, MouseDownEvent,
    MouseMoveEvent, ObjectFit, div, hsla, img, prelude::*, px, rgba,
};

use crate::icons::{Icon, icon};
use crate::log::LogErr;
use model::shelf::Item;
use windows::Win32::Foundation::HWND;

use crate::island::COLLAPSE_AFTER_RELEASE;
use crate::win32::{self, DragOutcome};
use crate::{Island, archive, log};

pub(crate) use menu::{ACTIONS, Action};

/// How far (logical pixels) a pressed chip must be dragged before it becomes a drag out.
pub(crate) const DRAG_OUT_THRESHOLD: f32 = 6.0;

pub(crate) const CHIP: (f32, f32) = (72.0, 70.0);
/// The picture at the top of a chip, square.
pub const ICON: f32 = 38.0;

/// Run `work` on a thread of its own, for shell calls that pump messages or take a while.
pub(crate) fn on_thread(name: &str, work: impl FnOnce() + Send + 'static) -> bool {
    match std::thread::Builder::new().name(name.into()).spawn(work) {
        Ok(_) => true,
        Err(err) => {
            eprintln!("topsy: could not start {name}: {err}");
            false
        }
    }
}

impl Island {
    pub(crate) fn clear_shelf(&mut self, cx: &mut Context<Self>) {
        self.shelf.clear();
        self.shelf_changed(cx);
    }

    pub(crate) fn remove_item(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(item) = self.shelf.remove(index) {
            log(&format!("removed {}", item.name()));
            self.shelf_changed(cx);
        }
    }

    /// One file on the shelf: its thumbnail or icon (a coloured type badge until that arrives,
    /// or if the shell has none) and the name. Press and drag to take it out (with the rest of
    /// the selection, if it is picked), click to open it, Ctrl- or Shift-click to pick it,
    /// right-click for its actions, the corner cross to remove it from the shelf (not from disk).
    pub(crate) fn chip(
        &self,
        index: usize,
        item: &Item,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let remove = div()
            .id(("remove", index))
            .absolute()
            .top(px(2.0))
            .right(px(2.0))
            .size(px(16.0))
            .rounded_full()
            .bg(rgba(0x000000b8))
            .flex()
            .items_center()
            .justify_center()
            .child(icon(Icon::Close, 10.0, rgba(0xffffffff)))
            .invisible()
            .group_hover("chip", |style| style.visible())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _window, cx| {
                    this.press = None;
                    cx.stop_propagation();
                }),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                cx.stop_propagation();
                this.remove_item(index, cx);
            }));

        let picture = match self.icons.get(&item.path) {
            Some(icon) => img(icon.clone())
                .size(px(ICON))
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => div()
                .size(px(ICON))
                .rounded(px(8.0))
                .bg(hsla(item.badge_hue(), 0.55, 0.45, 1.0))
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .child(item.badge_label())
                .into_any_element(),
        };

        let (rest, hover) = match self.shelf.is_selected(index) {
            true => (rgba(0x0a84ff66), rgba(0x0a84ff80)),
            false => (rgba(0x00000000), rgba(0xffffff1a)),
        };
        div()
            .id(("chip", index))
            .group("chip")
            .relative()
            .w(px(CHIP.0))
            .h(px(CHIP.1))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.0))
            .pt(px(6.0))
            .rounded(px(8.0))
            .cursor_pointer()
            .bg(rest)
            .hover(move |style| style.bg(hover))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _window, _cx| {
                    this.press = Some((index, event.position));
                }),
            )
            .on_mouse_move(
                cx.listener(move |this, event: &MouseMoveEvent, _window, cx| {
                    if !event.dragging() {
                        this.press = None;
                        return;
                    }
                    let Some((pressed, start)) = this.press else {
                        return;
                    };
                    let dx = f32::from(event.position.x - start.x);
                    let dy = f32::from(event.position.y - start.y);
                    if pressed == index && dx * dx + dy * dy >= DRAG_OUT_THRESHOLD.powi(2) {
                        this.press = None;
                        this.begin_drag_out(index, cx);
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseUpEvent, _window, _cx| this.press = None),
            )
            // Released somewhere else (slid off the chip first): that press is over too.
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseUpEvent, _window, _cx| this.press = None),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                // The shelf's own click would clear the selection.
                cx.stop_propagation();
                this.click_chip(index, cx);
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _: &MouseDownEvent, _window, cx| {
                    // This chip's menu, not the island's.
                    cx.stop_propagation();
                    this.chip_menu(index, cx);
                }),
            )
            .child(picture)
            .child(
                div()
                    .w(px(CHIP.0 - 8.0))
                    .text_xs()
                    .text_center()
                    .truncate()
                    .child(item.name()),
            )
            .child(remove)
    }

    /// Ctrl-click picks the chip or puts it back, Shift-click picks the run up to it, and a plain
    /// click opens it and lets the selection go.
    fn click_chip(&mut self, index: usize, cx: &mut Context<Self>) {
        let held = win32::held_keys();
        if held.ctrl {
            self.shelf.toggle_selected(index);
        } else if held.shift {
            self.shelf.select_through(index);
        } else {
            if let Some(item) = self.shelf.items().get(index) {
                open(item.clone());
            }
            if self.shelf.selection().is_empty() {
                return;
            }
            self.shelf.deselect();
        }
        cx.notify();
    }

    /// Hand a chip's file to the system's drag loop, so it can be dropped on any window; a picked
    /// chip takes the whole selection with it.
    ///
    /// The loop blocks until the button is released, and dispatches this window's messages
    /// while it runs, so it can't run inside the event handler that got us here: that would
    /// re-enter GPUI. It runs from a task instead, outside any borrow of this entity.
    fn begin_drag_out(&mut self, index: usize, cx: &mut Context<Self>) {
        let items = self.shelf.targets(index);
        let Some(hwnd) = self.hwnd.filter(|_| !items.is_empty()) else {
            return;
        };
        if self.drag_out {
            return;
        }
        self.drag_out = true;
        // Whatever was about to fold the tray away must not, mid-drag.
        self.collapse_generation += 1;
        log(&format!("dragout start {}", names(&items)));
        cx.spawn(async move |this, cx| {
            let paths: Vec<PathBuf> = items.iter().map(|i| i.path.clone()).collect();
            let outcome = win32::drag_files_out(hwnd, &paths);
            this.update(cx, |this, cx| this.finish_drag_out(&items, outcome, cx))
                .ok();
        })
        .detach();
    }

    fn finish_drag_out(
        &mut self,
        items: &[Item],
        outcome: windows::core::Result<DragOutcome>,
        cx: &mut Context<Self>,
    ) {
        self.drag_out = false;
        self.press = None;
        let moved = matches!(outcome, Ok(DragOutcome::Dropped));
        if let Err(err) = &outcome {
            eprintln!("topsy: drag out failed: {err}");
        }
        log(&format!(
            "dragout done {} ({})",
            names(items),
            if moved { "dropped" } else { "cancelled" }
        ));
        if moved {
            for item in items {
                self.shelf.remove_path(&item.path);
            }
            self.shelf_changed(cx);
        }
        // The button is up and the cursor is usually elsewhere by now.
        self.schedule_collapse(COLLAPSE_AFTER_RELEASE, cx);
    }
}

/// Open a file or folder with its default handler.
///
/// On a thread of its own: `ShellExecute` can wait on the handler for a while and pumps
/// messages meanwhile. Inline, that freezes the island, and a message for this window
/// reaching GPUI from inside the click handler that got us here finds the app already
/// borrowed and aborts the process.
pub(crate) fn open(item: Item) {
    on_thread("topsy-open", move || {
        let opened = win32::open_path(&item.path);
        log(&format!(
            "open {} ({})",
            item.name(),
            if opened { "ok" } else { "failed" }
        ));
        if !opened {
            eprintln!("topsy: could not open {}", item.path.display());
        }
    });
}

/// Explorer, opened on the file's folder with it selected.
pub(crate) fn show_in_folder(path: PathBuf) {
    on_thread("topsy-reveal", move || {
        if !win32::show_in_folder(&path) {
            eprintln!("topsy: could not show {} in its folder", path.display());
        }
    });
}

/// Windows' share sheet for `paths`, over `hwnd`. From a task: the sheet may call back into
/// the window's thread before it returns (see [`win32::share::share`]).
pub(crate) fn share<T: 'static>(hwnd: Option<HWND>, paths: Vec<PathBuf>, cx: &mut Context<T>) {
    let Some(hwnd) = hwnd.filter(|_| !paths.is_empty()) else {
        return;
    };
    let title = match paths.as_slice() {
        [one] => one
            .file_name()
            .unwrap_or(one.as_os_str())
            .to_string_lossy()
            .into_owned(),
        many => format!("{} items", many.len()),
    };
    let raw = hwnd.0 as usize;
    cx.spawn(async move |_, _| {
        log(&format!("share {title}"));
        if let Err(err) = win32::share::share(HWND(raw as *mut _), title, paths) {
            eprintln!("topsy: could not open the share sheet: {err}");
        }
    })
    .detach();
}

/// The paths on the clipboard, quoted, one per line, as Explorer's "Copy as path" does.
pub(crate) fn copy_as_paths(paths: &[PathBuf], cx: &mut App) {
    let quoted: Vec<String> = (paths.iter())
        .map(|p| format!("\"{}\"", p.display()))
        .collect();
    cx.write_to_clipboard(ClipboardItem::new_string(quoted.join("\r\n")));
}

/// The chips' names, for the log.
fn names(items: &[Item]) -> String {
    let names: Vec<String> = items.iter().map(Item::name).collect();
    names.join(", ")
}

/// Zip files and folders into one zip beside the first on a worker thread, and hand the zip to
/// `park` when it is done. A failed zip leaves nothing on disk and nothing to park.
pub(crate) fn zip<T: 'static>(
    paths: Vec<PathBuf>,
    cx: &mut Context<T>,
    park: fn(&mut T, PathBuf, &mut Context<T>),
) {
    let name = (paths.iter())
        .map(|p| p.file_name().unwrap_or(p.as_os_str()).to_string_lossy())
        .collect::<Vec<_>>()
        .join(", ");
    file_job("zip", name, move || archive::compress_all(&paths), cx, park);
}

/// Run `job`, which makes a file or folder (a zip, an extracted folder), on a worker thread and
/// hand what it made to `done`. A failed job is logged and `done` never runs.
fn file_job<T: 'static>(
    what: &'static str,
    name: String,
    job: impl FnOnce() -> io::Result<PathBuf> + Send + 'static,
    cx: &mut Context<T>,
    done: impl FnOnce(&mut T, PathBuf, &mut Context<T>) + 'static,
) {
    let (tx, rx) = async_channel::bounded(1);
    if !on_thread("topsy-file-job", move || {
        tx.send_blocking(job()).log_err();
    }) {
        return;
    }
    cx.spawn(async move |this, cx| {
        let Ok(made) = rx.recv().await else {
            return;
        };
        this.update(cx, |this, cx| match made {
            Ok(path) => {
                log(&format!("{what} {name}: {}", path.display()));
                done(this, path, cx);
            }
            Err(err) => {
                log(&format!("{what} failed {name}"));
                eprintln!("topsy: could not {what} {name}: {err}");
            }
        })
        .ok();
    })
    .detach();
}
