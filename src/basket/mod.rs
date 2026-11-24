//! The basket: Droppy's floating File Tray. Jiggling a held drag brings it up under the pointer,
//! in a glass window of its own, and it keeps what is dropped on it, wherever it is, until those
//! files are dragged out or it is closed, across restarts too. The notch and its shelf carry
//! on beside it.

mod actions;
mod rename;
mod view;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{Context, FocusHandle, Pixels, Point, RenderImage, WeakEntity};
use windows::Win32::Foundation::HWND;

use crate::frame::{self, Frame};
use crate::state::{Persistence, Writer};
use crate::win32::{self, DragOutcome, DropEvent, HookEvent};
use crate::{Island, log, tray};
use model::float::{self, MARGIN};
use model::shelf::{self, Shelf};
use model::spring::{Spring, SpringConfig};
use model::tray_cause::HeldDrag;

/// The closed basket, measured from Droppy's: the card with its handle, buttons, stack and pill.
pub const CARD: (f32, f32) = (208.0, 228.0);
/// Opened from the pill: every file as a tile.
pub const LIST: (f32, f32) = (300.0, 300.0);
/// The front card of the stack, square.
pub const THUMB: f32 = 88.0;
/// How long an empty basket waits after the button comes up, for a drop still on its way.
const FOLD_EMPTY_AFTER: Duration = Duration::from_millis(1200);
/// Where it springs out from, as a share of its size.
const FLY_IN_FROM: f32 = 0.82;

/// What a press in the basket is on: the whole stack, or one tile of the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Grip {
    Stack,
    Tile(usize),
}

pub struct Basket {
    hwnd: Option<HWND>,
    frame: Frame,
    width: Spring,
    height: Spring,
    last_frame: Option<Instant>,
    files: Shelf,
    /// Each file's picture, by path, at the stack's size.
    icons: HashMap<PathBuf, Arc<RenderImage>>,
    scale: f32,
    open: bool,
    /// Showing every file as a tile, opened from the pill.
    list: bool,
    held: HeldDrag,
    /// Bumped whenever the basket opens or closes, so a stale fold does nothing.
    generation: u64,
    press: Option<(Grip, Point<Pixels>)>,
    drag_out: bool,
    menu: bool,
    /// A file's new name being typed, in place of the pill.
    rename: Option<rename::Rename>,
    focus: FocusHandle,
    writer: Option<Writer>,
    island: WeakEntity<Island>,
}

impl Basket {
    pub fn new(
        hwnd: Option<HWND>,
        scale: f32,
        frame: Frame,
        island: WeakEntity<Island>,
        saved: Persistence,
        cx: &mut Context<Self>,
    ) -> Self {
        tray::set_basket_count(saved.shelf.len());
        Self {
            hwnd,
            frame,
            width: Spring::new(CARD.0, SpringConfig::ISLAND),
            height: Spring::new(0.0, SpringConfig::ISLAND),
            last_frame: None,
            files: saved.shelf,
            icons: HashMap::new(),
            scale,
            open: false,
            list: false,
            held: HeldDrag::default(),
            generation: 0,
            press: None,
            drag_out: false,
            menu: false,
            rename: None,
            focus: cx.focus_handle(),
            writer: saved.writer,
            island,
        }
    }

    /// Back from the last run without looking at the disk: learn the files' kinds and pictures
    /// and drop the ones that went away meanwhile, off the UI thread.
    pub fn settle_restored(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<_> = self.files.items().iter().map(|i| i.path.clone()).collect();
        self.resolve(paths, cx);
        self.prune_missing(cx);
    }

    /// Land the queued save before the app goes.
    pub fn drain_writer(&mut self) {
        if let Some(writer) = self.writer.take() {
            writer.drain();
        }
    }

    fn size(&self) -> (f32, f32) {
        match (self.open, self.list) {
            (false, _) => (CARD.0 * FLY_IN_FROM, 0.0),
            (true, false) => CARD,
            (true, true) => LIST,
        }
    }

    fn spring_to_size(&mut self, cx: &mut Context<Self>) {
        let (w, h) = self.size();
        self.width.set_target(w);
        self.height.set_target(h);
        cx.notify();
    }

    /// The mouse hook's events: a jiggle brings the basket up, and a button coming up ends the
    /// drag an empty basket was waiting for.
    pub fn on_hook(&mut self, event: HookEvent, cx: &mut Context<Self>) {
        match event {
            HookEvent::Jiggle => self.summon(cx),
            HookEvent::ButtonUp => {
                self.held.release();
                if self.open && self.files.is_empty() && !self.drag_out {
                    self.fold_if_still_empty(cx);
                }
            }
            HookEvent::Edge { .. } => {}
        }
    }

    fn summon(&mut self, cx: &mut Context<Self>) {
        if self.held.may_open_tray() {
            self.show(cx);
        }
    }

    /// Open under the pointer, on the pointer's monitor, kept whole on it. One already open
    /// stays where it is.
    pub fn show(&mut self, cx: &mut Context<Self>) {
        self.open_below(None, cx);
    }

    /// Open as [`Basket::show`] does, or, given `below` (a physical screen y), in line with the
    /// pointer but with its top that far down.
    fn open_below(&mut self, below: Option<i32>, cx: &mut Context<Self>) {
        if self.open || frame::fullscreen(self.hwnd) {
            return;
        }
        let Some((mut cursor, work, scale)) = win32::cursor_monitor() else {
            return;
        };
        let px = |v: f32| (v * scale).round() as i32;
        if let Some(below) = below {
            cursor.1 = below + px(MARGIN) + px(CARD.1) / 2;
        }
        let center = float::center_under(cursor, (px(CARD.0), px(CARD.1)), work, px(MARGIN));
        log(&format!(
            "basket at {center:?} ({} files)",
            self.files.len()
        ));
        self.frame.float_at(center, scale);
        self.open = true;
        self.list = false;
        self.generation += 1;
        self.width = Spring::new(CARD.0 * FLY_IN_FROM, SpringConfig::ISLAND);
        self.height = Spring::new(CARD.1 * FLY_IN_FROM, SpringConfig::ISLAND);
        self.spring_to_size(cx);
        self.prune_missing(cx);
    }

    /// Put the basket away. Its files stay in it, and the next jiggle brings them back.
    pub(crate) fn close(&mut self, cx: &mut Context<Self>) {
        log("basket closed");
        self.end_rename(cx);
        self.open = false;
        self.list = false;
        self.generation += 1;
        self.spring_to_size(cx);
    }

    /// The close button: empty the basket (the files themselves are untouched) and put it away.
    fn clear(&mut self, cx: &mut Context<Self>) {
        self.files.clear();
        self.files_changed(cx);
        self.close(cx);
    }

    fn fold_if_still_empty(&mut self, cx: &mut Context<Self>) {
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FOLD_EMPTY_AFTER).await;
            this.update(cx, |this, cx| {
                if this.generation == generation && this.files.is_empty() && this.open {
                    this.close(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// The drop target refused the drag over the basket: its payload (text, say) can never land,
    /// so an empty basket has nothing to wait for, and the drag doesn't bring it back.
    pub fn on_drop_event(&mut self, event: DropEvent, cx: &mut Context<Self>) {
        log(&format!("basket drop-target {event:?}"));
        self.held.refuse();
        if self.open && self.files.is_empty() {
            self.close(cx);
        }
    }

    /// Files sent over from the shelf: into the basket, which comes up under the island, whose
    /// window ends at `below` (a physical screen y), in line with the pointer.
    pub(crate) fn take(&mut self, paths: Vec<PathBuf>, below: Option<i32>, cx: &mut Context<Self>) {
        self.add(paths, cx);
        self.open_below(below, cx);
    }

    fn add(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let added = self.files.add(paths);
        log(&format!("basket +{} = {}", added.len(), self.files.len()));
        self.files_changed(cx);
        self.resolve(added, cx);
    }

    /// Whether each file is a folder, and its picture, off the UI thread.
    fn resolve(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        let rx = crate::icon::resolve(paths, (THUMB * self.scale).round() as i32);
        cx.spawn(async move |this, cx| {
            while let Ok((path, is_dir, bytes, icon)) = rx.recv().await {
                let updated = this.update(cx, |this, cx| {
                    if this.files.resolve_kinds([(path.clone(), is_dir, bytes)]) == 0 {
                        return;
                    }
                    if let Some(icon) = icon
                        && let Some(old) = this.icons.insert(path, icon)
                    {
                        crate::icon::forget(vec![old], cx);
                    }
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Files can move or vanish while the basket holds them, or while the app is off. Looking
    /// can stall on a dead share, so it happens off the UI thread.
    fn prune_missing(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = self.files.items().iter().map(|i| i.path.clone()).collect();
        if paths.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let gone: Vec<PathBuf> = cx
                .background_executor()
                .spawn(async move { paths.into_iter().filter(|p| !p.exists()).collect() })
                .await;
            if gone.is_empty() {
                return;
            }
            this.update(cx, |this, cx| {
                if this.files.prune(|p| !gone.iter().any(|g| g == p)) > 0 {
                    log(&format!("basket pruned, {} left", this.files.len()));
                    this.files_changed(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Files renamed on disk (by Transform, or here): they keep their place and picture.
    pub fn follow_renames(&mut self, renamed: &[(PathBuf, PathBuf)], cx: &mut Context<Self>) {
        let mut moved = false;
        for (from, to) in renamed {
            if self.files.rename(from, to.clone()) {
                if let Some(icon) = self.icons.remove(from) {
                    self.icons.insert(to.clone(), icon);
                }
                moved = true;
            }
        }
        if moved {
            self.files_changed(cx);
        }
    }

    /// The files changed: save them, and let go of pictures no file needs.
    fn files_changed(&mut self, cx: &mut Context<Self>) {
        if let Some(writer) = &self.writer {
            writer.send(shelf::serialize(self.files.items()));
        }
        tray::set_basket_count(self.files.len());
        let gone: Vec<PathBuf> = (self.icons.keys())
            .filter(|path| !self.files.items().iter().any(|item| &item.path == *path))
            .cloned()
            .collect();
        let icons = gone.iter().filter_map(|p| self.icons.remove(p)).collect();
        crate::icon::forget(icons, cx);
        if self.files.is_empty() && self.list {
            self.list = false;
            self.spring_to_size(cx);
        }
        cx.notify();
    }

    fn toggle_list(&mut self, cx: &mut Context<Self>) {
        self.list = !self.list;
        // Grown about its centre, then nudged back whole onto its monitor.
        let (w, h) = self.size();
        if let (Some(center), Some((_, work, scale))) =
            (self.frame.center(), win32::cursor_monitor())
        {
            let px = |v: f32| (v * scale).round() as i32;
            let center = float::center_under(center, (px(w), px(h)), work, px(MARGIN));
            self.frame.float_at(center, scale);
        }
        self.spring_to_size(cx);
    }

    fn paths(&self, grip: Grip) -> Vec<PathBuf> {
        match grip {
            Grip::Stack => self.files.items().iter().map(|i| i.path.clone()).collect(),
            Grip::Tile(i) => self
                .files
                .items()
                .get(i)
                .map(|i| i.path.clone())
                .into_iter()
                .collect(),
        }
    }

    /// Hand the stack (every file) or one tile's file to the system's drag loop. As the shelf's
    /// chips do, from a task: the loop dispatches this window's messages (docs/internals/gpui.md).
    fn begin_drag_out(&mut self, grip: Grip, cx: &mut Context<Self>) {
        let paths = self.paths(grip);
        let Some(hwnd) = self.hwnd.filter(|_| !paths.is_empty() && !self.drag_out) else {
            return;
        };
        self.drag_out = true;
        log(&format!("basket dragout start {}", paths.len()));
        cx.spawn(async move |this, cx| {
            let outcome = win32::drag_files_out(hwnd, &paths);
            this.update(cx, |this, cx| {
                this.drag_out = false;
                this.press = None;
                let dropped = matches!(outcome, Ok(DragOutcome::Dropped));
                if let Err(err) = &outcome {
                    eprintln!("topsy: drag out failed: {err}");
                }
                log(&format!(
                    "basket dragout {}",
                    if dropped { "dropped" } else { "cancelled" }
                ));
                if dropped {
                    for path in &paths {
                        this.files.remove_path(path);
                    }
                    this.files_changed(cx);
                    if this.files.is_empty() {
                        this.close(cx);
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn follow_window(&mut self, cx: &mut Context<Self>) {
        self.frame.follow_window(cx);
    }

    pub fn set_black(&mut self, black: bool, cx: &mut Context<Self>) {
        self.frame.set_black(black, cx);
        cx.notify();
    }
}
