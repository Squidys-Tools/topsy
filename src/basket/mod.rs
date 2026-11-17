//! The basket: Droppy's floating File Tray. Jiggling a held drag brings it up under the pointer,
//! in a glass window of its own, and it keeps what is dropped on it, wherever it is, until those
//! files are dragged out or it is closed. The notch and its shelf carry on beside it.

mod view;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{Context, Pixels, Point, RenderImage, WeakEntity};
use windows::Win32::Foundation::HWND;

use crate::chip::{self, ACTIONS, Action};
use crate::frame::{self, Frame};
use crate::model::float::{self, MARGIN};
use crate::model::shelf::{Item, Kind, Shelf};
use crate::model::spring::{Spring, SpringConfig};
use crate::model::tray_cause::HeldDrag;
use crate::win32::{self, DragOutcome, DropEvent, HookEvent, MenuEntry};
use crate::{Island, log};

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

const MENU: [MenuEntry; 6] = [
    MenuEntry::Item(Action::Transform as usize, "&Transform…"),
    MenuEntry::Item(Action::ShowInFolder as usize, "Show in &folder"),
    MenuEntry::Item(Action::CopyPath as usize, "Copy as &path"),
    MenuEntry::Item(Action::Compress as usize, "Compress to &ZIP file"),
    MenuEntry::Separator,
    MenuEntry::Item(Action::Remove as usize, "&Remove from basket"),
];

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
    island: WeakEntity<Island>,
}

impl Basket {
    pub fn new(hwnd: Option<HWND>, scale: f32, frame: Frame, island: WeakEntity<Island>) -> Self {
        Self {
            hwnd,
            frame,
            width: Spring::new(CARD.0, SpringConfig::ISLAND),
            height: Spring::new(0.0, SpringConfig::ISLAND),
            last_frame: None,
            files: Shelf::default(),
            icons: HashMap::new(),
            scale,
            open: false,
            list: false,
            held: HeldDrag::default(),
            generation: 0,
            press: None,
            drag_out: false,
            menu: false,
            island,
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

    /// Open under the pointer, on the pointer's monitor, kept whole on it. One already open
    /// stays where it is.
    fn summon(&mut self, cx: &mut Context<Self>) {
        if self.open || !self.held.may_open_tray() || frame::fullscreen(self.hwnd) {
            return;
        }
        let Some((cursor, work, scale)) = win32::cursor_monitor() else {
            return;
        };
        let px = |v: f32| (v * scale).round() as i32;
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
    }

    /// Put the basket away. Its files stay in it, and the next jiggle brings them back.
    pub(crate) fn close(&mut self, cx: &mut Context<Self>) {
        log("basket closed");
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

    fn add(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let added = self.files.add(paths);
        log(&format!("basket +{} = {}", added.len(), self.files.len()));
        self.files_changed(cx);
        let rx = crate::icon::resolve(added, (THUMB * self.scale).round() as i32);
        cx.spawn(async move |this, cx| {
            while let Ok((path, is_dir, icon)) = rx.recv().await {
                let updated = this.update(cx, |this, cx| {
                    if this.files.resolve_kinds([(path.clone(), is_dir)]) == 0 {
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

    /// The files changed: let go of pictures no file needs, and close once the last file has
    /// gone (dragged out or removed), unless a drag is on its way in.
    fn files_changed(&mut self, cx: &mut Context<Self>) {
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
    /// chips do, from a task: the loop dispatches this window's messages (README, Shelf).
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

    /// A tile's menu, the shelf's chip menu: on a thread of its own, as it pumps messages.
    fn tile_menu(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(item) = self.files.items().get(index).cloned() else {
            return;
        };
        if self.menu || self.drag_out {
            return;
        }
        let (tx, rx) = async_channel::bounded(1);
        // Transform, first, takes files only.
        let from = usize::from(item.kind != Kind::File);
        if !chip::on_thread("topsy-basket-menu", move || {
            let _ = tx.send_blocking(win32::popup_menu(&MENU[from..]));
        }) {
            return;
        }
        self.menu = true;
        cx.spawn(async move |this, cx| {
            let choice = rx.recv().await.ok().flatten();
            let action = ACTIONS.into_iter().find(|a| Some(*a as usize) == choice);
            this.update(cx, |this, cx| {
                this.menu = false;
                if let Some(action) = action {
                    this.act(action, item, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Acts on the file's path, not its index: the basket may have changed while the menu was up.
    fn act(&mut self, action: Action, item: Item, cx: &mut Context<Self>) {
        log(&format!("basket {action:?} {}", item.name()));
        match action {
            Action::ShowInFolder => chip::show_in_folder(item.path),
            Action::CopyPath => chip::copy_as_path(&item.path, cx),
            Action::Compress => chip::zip(item, cx, |this: &mut Self, zip, cx| {
                this.add(vec![zip], cx);
            }),
            Action::Transform => {
                if let Some(island) = self.island.upgrade() {
                    crate::transform::open(cx, &island, vec![item.path]);
                }
            }
            Action::Remove => {
                if self.files.remove_path(&item.path) {
                    self.files_changed(cx);
                    if self.files.is_empty() {
                        self.close(cx);
                    }
                }
            }
        }
    }

    pub fn follow_window(&mut self) {
        self.frame.follow_window();
    }

    pub fn set_black(&mut self, black: bool, cx: &mut Context<Self>) {
        self.frame.set_black(black, cx);
        cx.notify();
    }
}
