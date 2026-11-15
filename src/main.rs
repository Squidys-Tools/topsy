// A release build is a pure GUI app. Without this Windows opens a console window for it, and
// that window takes foreground focus at launch. Debug builds keep the console for logging.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod backdrop;
mod config;
mod edge;
mod icon;
mod jiggle;
mod look;
mod notch;
mod shelf;
mod spring;
mod state;
mod tray;
mod tray_cause;
mod win32;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    App, Application, Bounds, ClickEvent, Context, Entity, ExternalPaths, FontWeight, MouseButton,
    MouseDownEvent, MouseMoveEvent, ObjectFit, Pixels, Point, RenderImage, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, canvas, div, hsla, img,
    linear_color_stop, linear_gradient, point, prelude::*, px, rgb, rgba, size,
};
use raw_window_handle::HasWindowHandle;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::COINIT_MULTITHREADED;

use crate::edge::EdgeZone;
use crate::jiggle::JiggleConfig;
use crate::notch::Notch;
use crate::shelf::{Item, Shelf};
use crate::spring::{Spring, SpringConfig};
use crate::tray::TrayEvent;
use crate::tray_cause::TrayCause;
use crate::win32::{DragOutcome, DropEvent, HookEvent};

/// The notch's body when it peeks down from the top edge, and when it holds the tray.
const PEEK: (f32, f32) = (200.0, 32.0);
const TRAY: (f32, f32) = (400.0, 118.0);

/// DWM's fixed window corner, which clips the window-wide acrylic of [`Glass::Acrylic`].
const DWM_RADIUS: f32 = 8.0;

/// The acrylic fallback's own tint (0xRRGGBBAA).
const ACRYLIC_TINT: u32 = 0x0c101c8c;
/// Droppy's recipe, measured from its screenshots: solid black for the top 62% of the notch,
/// then fading to about 62% black at the bottom edge, so the backdrop's own colour shows
/// through, darkened but not greyed.
const BLACK_UNTIL: f32 = 0.62;
const GLASS_FLOOR: u32 = 0x0000009f;
/// A light rim along the bottom edge, fading out up the sides where the notch turns black.
const RIM: u32 = 0xffffff38;

/// How long the cursor rests on the top edge before the notch comes down.
const REVEAL_AFTER: Duration = Duration::from_millis(150);
/// How long a notch that peeked down stays once the cursor has left it.
const PEEK_LINGER: Duration = Duration::from_millis(400);
const COLLAPSE_AFTER_RELEASE: Duration = Duration::from_millis(1200);
const COLLAPSE_AFTER_DROP: Duration = Duration::from_millis(4000);
/// How long the tray stays once the cursor has left it.
const COLLAPSE_AFTER_LEAVE: Duration = Duration::from_millis(1500);
/// How long a tray opened by a click (on the pill or the tray icon) waits for the cursor to arrive.
const COLLAPSE_AFTER_CLICK: Duration = Duration::from_millis(5000);

/// How far (logical pixels) a pressed chip must be dragged before it becomes a drag out.
const DRAG_OUT_THRESHOLD: f32 = 6.0;

const CHIP: (f32, f32) = (72.0, 70.0);
/// The picture at the top of a chip, square.
const ICON: f32 = 38.0;

/// Event trace for scripted tests; silent unless TOPSY_LOG is set.
fn log(message: &str) {
    if config::get().log {
        eprintln!("topsy: {message}");
    }
}

/// How the notch is filled. `TOPSY_GLASS` can force one, for A/B comparison and for the
/// automated blur check (`tint` and `clear` are its controls).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Glass {
    /// The compositor's blur of whatever is behind the notch, in its own shape ([`backdrop`]),
    /// under Droppy's black-to-glass fill.
    Blur,
    /// Window-wide acrylic, which only takes DWM's fixed corner. The fallback where the
    /// compositor glass can't start.
    Acrylic,
    /// The same fill with no blur behind it: the notch without its blur.
    Tint,
    /// Opaque black.
    Solid,
    /// Fully transparent, no blur.
    Clear,
}

impl Glass {
    /// The window-wide acrylic can't leave the ears' corners clear, so it goes without them.
    fn has_ears(self) -> bool {
        self != Glass::Acrylic
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    /// Folded up past the top edge: nothing drawn, nothing to click.
    Hidden,
    /// The notch, showing what is on the shelf.
    Peek,
    Tray,
}

impl Mode {
    /// The notch body's size; hidden keeps the peek's width so it slides straight down.
    fn size(self) -> (f32, f32) {
        match self {
            Mode::Hidden => (PEEK.0, 0.0),
            Mode::Peek => PEEK,
            Mode::Tray => TRAY,
        }
    }
}

struct Island {
    mode: Mode,
    /// What the island folds back to.
    rest: Mode,
    /// The cursor is on the top edge above the notch, as last reported by the hook.
    at_edge: bool,
    width: Spring,
    height: Spring,
    last_frame: Option<Instant>,
    shelf: Shelf,
    hwnd: Option<HWND>,
    scale: f32,
    glass: Glass,
    /// The "Black notch" setting: draw [`Glass::Solid`] whatever the glass is.
    black: bool,
    /// The compositor glass, reshaped with the window ([`Glass::Blur`] only).
    backdrop: Option<backdrop::Backdrop>,
    /// Where the notch hangs: horizontal centre and top edge of its monitor, in physical
    /// screen pixels. Every resize keeps the window centred on this point.
    anchor: Option<(i32, i32)>,
    /// Size in physical pixels last requested from the OS, and whether it was hidden, to skip
    /// redundant calls.
    applied: Option<((i32, i32), bool)>,
    /// What the tray was opened for; decides which events fold it early.
    tray_cause: TrayCause,
    /// Bumped on every state change that schedules a collapse, so stale timers do nothing.
    collapse_generation: u64,
    /// The chip the left button went down on, and where: it becomes a drag out once moved.
    press: Option<(usize, Point<Pixels>)>,
    /// A chip is being dragged out; the system's drag loop is running.
    drag_out: bool,
    /// Writes the shelf to its state file on a thread of its own; `None` when saving is off.
    writer: Option<state::Writer>,
    /// Each chip's thumbnail or icon, by its path; a chip without one shows a type badge.
    icons: HashMap<PathBuf, Arc<RenderImage>>,
}

impl Island {
    fn new(
        hwnd: Option<HWND>,
        scale: f32,
        glass: Glass,
        black: bool,
        backdrop: Option<backdrop::Backdrop>,
        persistence: state::Persistence,
    ) -> Self {
        let anchor = hwnd.and_then(win32::monitor_top_center);
        let rest = config::get().rest;
        Self {
            mode: rest,
            rest,
            at_edge: false,
            width: Spring::new(rest.size().0, SpringConfig::ISLAND),
            height: Spring::new(rest.size().1, SpringConfig::ISLAND),
            last_frame: None,
            shelf: persistence.shelf,
            hwnd,
            scale,
            glass,
            black,
            backdrop,
            anchor,
            applied: None,
            tray_cause: TrayCause::Timed(Duration::ZERO),
            collapse_generation: 0,
            press: None,
            drag_out: false,
            writer: persistence.writer,
            icons: HashMap::new(),
        }
    }

    fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.mode = mode;
        let (w, h) = mode.size();
        self.width.set_target(w);
        self.height.set_target(h);
        if mode == Mode::Tray {
            self.prune_missing(cx);
        }
        cx.notify();
    }

    /// Files can move or vanish while parked; don't keep chips for ones that have.
    ///
    /// Looking asks the disk, which can take seconds for a share that has gone away, so it
    /// happens off the UI thread and the chips are dropped when the answer comes back.
    fn prune_missing(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = self.shelf.items().iter().map(|i| i.path.clone()).collect();
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
                let pruned = this.shelf.prune(|p| !gone.iter().any(|g| g == p));
                if pruned > 0 {
                    log(&format!("pruned {pruned} missing"));
                    this.shelf_changed(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn shelf_changed(&mut self, cx: &mut Context<Self>) {
        log(&format!("shelf now {}", self.shelf.len()));
        let shelf = &self.shelf;
        let gone: Vec<PathBuf> = (self.icons.keys())
            .filter(|path| !shelf.items().iter().any(|item| &item.path == *path))
            .cloned()
            .collect();
        let icons = gone
            .iter()
            .filter_map(|path| self.icons.remove(path))
            .collect();
        forget_icons(icons, cx);
        tray::set_shelf_count(self.shelf.len());
        if let Some(writer) = &self.writer {
            writer.send(shelf::serialize(self.shelf.items()));
        }
        cx.notify();
    }

    /// Park dropped paths on the shelf; returns how many were new.
    ///
    /// Chips go up right away: telling a folder from a file asks the disk, and a
    /// dead network share could stall the UI thread for its timeout, so the stat
    /// runs in the background (chips show `Kind::Unknown` meanwhile). Answers are
    /// merged back by path, so a chip removed or cleared before they arrive stays
    /// gone.
    fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) -> usize {
        let added = self.shelf.add(paths);
        self.shelf_changed(cx);
        if added.is_empty() {
            return 0;
        }
        let count = added.len();
        self.resolve_kinds(added, cx);
        count
    }

    /// Learn whether each chip is a folder or a file, and fetch its picture, on a worker
    /// thread of its own (the shell wants COM there). Each chip updates as its answer arrives.
    fn resolve_kinds(&mut self, added: Vec<PathBuf>, cx: &mut Context<Self>) {
        let size = (ICON * self.scale).round() as i32;
        let (tx, rx) = async_channel::unbounded();
        std::thread::spawn(move || {
            win32::with_com(COINIT_MULTITHREADED, || {
                for path in added {
                    let is_dir = path.is_dir();
                    let icon = icon::load(&path, size);
                    if tx.send_blocking((path, is_dir, icon)).is_err() {
                        break;
                    }
                }
            })
        });
        cx.spawn(async move |this, cx| {
            while let Ok((path, is_dir, icon)) = rx.recv().await {
                let updated = this.update(cx, |this, cx| {
                    if this.shelf.resolve_kinds([(path.clone(), is_dir)]) == 0 {
                        return;
                    }
                    if let Some(icon) = icon
                        && let Some(old) = this.icons.insert(path, icon)
                    {
                        forget_icons(vec![old], cx);
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

    /// The shelf was restored from disk without looking at its files: learn their kinds and
    /// drop the ones that went away while the app was off, both off the UI thread.
    fn settle_restored(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = self.shelf.items().iter().map(|i| i.path.clone()).collect();
        if paths.is_empty() {
            return;
        }
        self.resolve_kinds(paths, cx);
        self.prune_missing(cx);
    }

    /// Open the tray. `cause` is what it is waiting for: a `Timed` one folds on its
    /// own timer; a `Drag` one waits for a drag in progress to end, land, or be refused.
    fn open_tray(&mut self, cause: TrayCause, cx: &mut Context<Self>) {
        // Cancels any pending collapse from earlier.
        self.collapse_generation += 1;
        self.tray_cause = cause;
        self.set_mode(Mode::Tray, cx);
        if let TrayCause::Timed(after) = cause {
            self.schedule_collapse(after, cx);
        }
    }

    fn schedule_collapse(&mut self, after: Duration, cx: &mut Context<Self>) {
        self.collapse_generation += 1;
        let generation = self.collapse_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(after).await;
            this.update(cx, |this, cx| {
                if this.collapse_generation != generation || this.drag_out {
                    return;
                }
                // The cursor is asked directly rather than remembered: a hover state that
                // missed its "leave" (a drag took over the mouse) must not pin the tray open.
                if this.hwnd.is_some_and(win32::cursor_over) {
                    // Still being used; look again later, in case no "leave" ever arrives.
                    this.schedule_collapse(after, cx);
                } else {
                    this.set_mode(this.rest, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn on_hook(&mut self, event: HookEvent, cx: &mut Context<Self>) {
        log(&format!("hook {event:?} (mode {:?})", self.mode));
        match event {
            HookEvent::Jiggle if self.fullscreen() => {}
            HookEvent::Jiggle => self.open_tray(TrayCause::Drag, cx),
            HookEvent::Edge { at_edge, pressed } => {
                self.at_edge = at_edge;
                if at_edge && self.mode == Mode::Hidden && !self.fullscreen() {
                    if pressed {
                        // Something is being dragged up here: give it somewhere to land.
                        self.open_tray(TrayCause::Drag, cx);
                    } else {
                        self.schedule_reveal(cx);
                    }
                }
            }
            HookEvent::ButtonUp => {
                if self.mode == Mode::Tray && !self.drag_out {
                    self.schedule_collapse(COLLAPSE_AFTER_RELEASE, cx);
                }
            }
        }
    }

    /// A fullscreen video, game or slideshow has the screen: the hook's gestures stay out of its
    /// way. Read at the moment of the gesture rather than tracked, so nothing runs meanwhile.
    fn fullscreen(&self) -> bool {
        let fullscreen = self.hwnd.is_some_and(win32::fullscreen_over);
        if fullscreen {
            log("fullscreen app in front: staying hidden");
        }
        fullscreen
    }

    /// Bring the notch down if the cursor is still resting on the edge after a moment, so
    /// sweeping across the top of the screen doesn't flash it.
    fn schedule_reveal(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REVEAL_AFTER).await;
            this.update(cx, |this, cx| {
                if this.at_edge && this.mode == Mode::Hidden && !this.fullscreen() {
                    this.set_mode(Mode::Peek, cx);
                    this.schedule_collapse(PEEK_LINGER, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn on_tray(&mut self, event: TrayEvent, cx: &mut Context<Self>) {
        log(&format!("tray {event:?} (mode {:?})", self.mode));
        match event {
            TrayEvent::Toggle if self.mode == Mode::Tray => self.fold(cx),
            TrayEvent::Toggle | TrayEvent::Show => {
                self.open_tray(TrayCause::Timed(COLLAPSE_AFTER_CLICK), cx)
            }
            TrayEvent::Black(black) => {
                self.black = black;
                // Reshape (or empty) the glass on the next frame.
                self.applied = None;
                if let (Glass::Acrylic, Some(hwnd)) = (self.glass, self.hwnd) {
                    cx.spawn(async move |_, _| {
                        win32::set_acrylic(hwnd, (!black).then_some(ACRYLIC_TINT))
                    })
                    .detach();
                }
                cx.notify();
            }
            TrayEvent::ClearShelf => self.clear_shelf(cx),
            TrayEvent::Quit => cx.quit(),
        }
    }

    /// The drop target refused a drag crossing the island. If the tray only opened
    /// for that drag (a jiggle), its payload can never land: fold it away.
    fn on_drop_event(&mut self, event: DropEvent, cx: &mut Context<Self>) {
        log(&format!("drop-target {event:?} (mode {:?})", self.mode));
        if event == DropEvent::Refused
            && self.mode == Mode::Tray
            && self.tray_cause.closed_by_refusal()
        {
            self.fold(cx);
        }
    }

    /// Fold back to rest now, cancelling any collapse already scheduled.
    fn fold(&mut self, cx: &mut Context<Self>) {
        self.collapse_generation += 1;
        self.set_mode(self.rest, cx);
    }

    fn clear_shelf(&mut self, cx: &mut Context<Self>) {
        self.shelf.clear();
        self.shelf_changed(cx);
    }

    fn on_hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.drag_out || self.mode == self.rest {
        } else if self.mode == Mode::Peek {
            // The linger re-checks the cursor itself, so it needs no "leave", which a cursor
            // flicked off the top edge doesn't always deliver.
            if !hovered {
                self.schedule_collapse(PEEK_LINGER, cx);
            }
        } else if hovered {
            // The cursor is here; don't fold away under it.
            self.collapse_generation += 1;
        } else {
            self.schedule_collapse(COLLAPSE_AFTER_LEAVE, cx);
        }
    }

    fn remove_item(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(item) = self.shelf.remove(index) {
            log(&format!("removed {}", item.name()));
            self.shelf_changed(cx);
        }
    }

    /// Open a chip's file or folder with its default handler.
    ///
    /// On a thread of its own: `ShellExecute` can wait on the handler for a while and pumps
    /// messages meanwhile. Inline, that freezes the island, and a message for this window
    /// reaching GPUI from inside the click handler that got us here finds the app already
    /// borrowed and aborts the process.
    fn open_item(&self, index: usize) {
        let Some(item) = self.shelf.items().get(index).cloned() else {
            return;
        };
        let spawned = std::thread::Builder::new()
            .name("topsy-open".into())
            .spawn(move || {
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
        if let Err(err) = spawned {
            eprintln!("topsy: could not start opening the file: {err}");
        }
    }

    /// Hand a chip's file to the system's drag loop, so it can be dropped on any window.
    ///
    /// The loop blocks until the button is released, and dispatches this window's messages
    /// while it runs, so it can't run inside the event handler that got us here: that would
    /// re-enter GPUI. It runs from a task instead, outside any borrow of this entity.
    fn begin_drag_out(&mut self, index: usize, cx: &mut Context<Self>) {
        let (Some(hwnd), Some(item)) = (self.hwnd, self.shelf.items().get(index).cloned()) else {
            return;
        };
        if self.drag_out {
            return;
        }
        self.drag_out = true;
        // Whatever was about to fold the tray away must not, mid-drag.
        self.collapse_generation += 1;
        log(&format!("dragout start {}", item.name()));
        cx.spawn(async move |this, cx| {
            let outcome = win32::drag_files_out(hwnd, std::slice::from_ref(&item.path));
            this.update(cx, |this, cx| this.finish_drag_out(&item, outcome, cx))
                .ok();
        })
        .detach();
    }

    fn finish_drag_out(
        &mut self,
        item: &Item,
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
            item.name(),
            if moved { "dropped" } else { "cancelled" }
        ));
        if moved {
            self.shelf.remove_path(&item.path);
            self.shelf_changed(cx);
        }
        // The button is up and the cursor is usually elsewhere by now.
        self.schedule_collapse(COLLAPSE_AFTER_RELEASE, cx);
    }

    /// What the notch is drawn with right now.
    fn look(&self) -> Glass {
        if self.black { Glass::Solid } else { self.glass }
    }

    /// Make the OS window exactly the notch (body and ears), hanging from the anchor.
    ///
    /// The window is resized every frame as the spring moves, and its region and the glass's
    /// clip are reshaped with it. Hidden, it keeps a 1 px window (GPUI can't draw into an empty
    /// one) whose region is empty, so nothing shows and nothing takes clicks.
    ///
    /// The resize runs in a foreground task rather than here: this is called from `render`,
    /// and a resize mid-draw would reach GPUI's window handler where it cannot update the
    /// window (see [`win32::move_window`]).
    fn sync_window(&mut self, notch: Notch, cx: &mut Context<Self>) {
        let (Some(hwnd), Some((center_x, top))) = (self.hwnd, self.anchor) else {
            return;
        };
        let s = self.scale;
        let px = |v: f32| (v * s).round() as i32;
        let (w, h) = notch.window_size();
        let size = (px(w).max(1), px(h).max(1));
        let hidden = notch.is_hidden();
        if self.applied == Some((size, hidden)) {
            return;
        }
        let appearing = !hidden && self.applied.is_none_or(|(_, was_hidden)| was_hidden);
        self.applied = Some((size, hidden));
        let clip = if hidden {
            win32::Clip::Nothing
        } else if self.look() == Glass::Acrylic {
            win32::Clip::Everything
        } else {
            win32::Clip::Notch {
                width: size.0,
                height: size.1,
                ear_slot: px(notch.ear_slot),
                ear: px(notch.ear),
                radius: px(notch.radius),
            }
        };
        let shape = self.backdrop.as_ref().map(backdrop::Backdrop::shape);
        let r = notch.radius * s;
        let glass = (
            notch.ear_slot * s,
            -r,
            notch.body_width * s,
            if hidden || self.look() != Glass::Blur {
                0.0
            } else {
                size.1 as f32 + r
            },
            r,
        );
        cx.spawn(async move |_, _| {
            let x = center_x - size.0 / 2;
            win32::move_window(hwnd, x, top, size.0, size.1, appearing);
            win32::set_clip(hwnd, clip);
            if let Some(shape) = shape {
                shape.set(glass.0, glass.1, glass.2, glass.3, glass.4);
            }
        })
        .detach();
    }

    /// One file on the shelf: its thumbnail or icon (a coloured type badge until that arrives,
    /// or if the shell has none) and the name. Press and drag to take it
    /// out, click to open it, the corner cross to remove it from the shelf (not from disk).
    fn chip(&self, index: usize, item: &Item, cx: &mut Context<Self>) -> impl IntoElement + use<> {
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
            .text_xs()
            .child("×")
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
            .hover(|style| style.bg(rgba(0xffffff1a)))
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
            .on_click(cx.listener(move |this, _: &ClickEvent, _window, _cx| this.open_item(index)))
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

    /// The expanded tray: a header and a row of chips (or the hint, when the shelf is empty).
    fn tray_content(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let count = self.shelf.len();
        let title = if count == 0 {
            "Shelf".to_string()
        } else {
            format!("Shelf · {count}")
        };
        let clear = (count > 0).then(|| {
            div()
                .id("clear")
                .px(px(6.0))
                .rounded(px(4.0))
                .text_xs()
                .text_color(rgba(0xffffffb3))
                .cursor_pointer()
                .hover(|style| style.bg(rgba(0xffffff1a)).text_color(rgb(0xffffff)))
                .child("Clear")
                .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| this.clear_shelf(cx)))
        });

        let body = if self.shelf.is_empty() {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .child("Drop files to park them")
                .into_any_element()
        } else {
            let chips: Vec<_> = self
                .shelf
                .items()
                .iter()
                .enumerate()
                .map(|(i, item)| self.chip(i, item, cx))
                .collect();
            div()
                .id("chips")
                .w_full()
                .flex()
                .flex_row()
                .gap(px(8.0))
                .overflow_x_scroll()
                .children(chips)
                .into_any_element()
        };

        // Fixed size and centred in the window, so it is revealed (not squashed) while the
        // pill springs open.
        div()
            .w(px(TRAY.0))
            .h(px(TRAY.1))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .p(px(10.0))
            .gap(px(6.0))
            .child(
                div()
                    .w_full()
                    .h(px(16.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(div().text_xs().text_color(rgba(0xffffffb3)).child(title))
                    .children(clear),
            )
            .child(body)
    }
}

impl Render for Island {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        log("render");
        let now = Instant::now();
        let dt = self
            .last_frame
            .map(|t| now.duration_since(t).as_secs_f32())
            .unwrap_or(1.0 / 120.0);
        let moving = self.width.step(dt) | self.height.step(dt);
        self.last_frame = moving.then_some(now);
        if moving {
            window.request_animation_frame();
        }

        let look = self.look();
        let ears = look.has_ears();
        self.sync_window(
            Notch::new(self.width.value.max(1.0), self.height.value, ears),
            cx,
        );
        // The window resizes asynchronously, a moment after this frame. Draw to the size it
        // really has, so the notch's edge always lines up with the window's (and the blur's).
        let viewport = window.viewport_size();
        let notch = Notch::fitting(viewport.width.into(), viewport.height.into(), ears);
        let show_tray = notch.body_width > 260.0 && notch.height > 70.0;

        // Black at the screen edge, giving way to glass near the bottom. The closed notch is
        // black all the way down, like the real one.
        let black = rgba(0x000000ff);
        let h = notch.height.max(1.0);
        let black_until = (h * BLACK_UNTIL).max(PEEK.1).min(h) / h;
        let edge_fill = |floor: u32| {
            linear_gradient(
                180.0,
                linear_color_stop(black, black_until),
                linear_color_stop(rgba(floor), 1.0),
            )
        };
        let fill = match look {
            Glass::Blur | Glass::Tint => edge_fill(GLASS_FLOOR),
            Glass::Acrylic => edge_fill(0x00000000),
            Glass::Solid => black.into(),
            Glass::Clear => rgba(0x00000000).into(),
        };
        let radius = if ears { notch.radius } else { DWM_RADIUS };

        let peek_label = match self.shelf.len() {
            0 => "topsy".to_string(),
            n => format!("{n} on shelf"),
        };
        let content = if show_tray {
            self.tray_content(cx).into_any_element()
        } else if notch.height >= 20.0 {
            div().text_xs().child(peek_label).into_any_element()
        } else {
            div().into_any_element()
        };

        let body = div()
            .absolute()
            .left(px(notch.ear_slot))
            .top_0()
            .w(px(notch.body_width))
            .h(viewport.height)
            .rounded_bl(px(radius))
            .rounded_br(px(radius))
            .bg(fill)
            .overflow_hidden()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(rgb(0x14213d)))
            .child(content);

        // The ears, black like the edge they join, and the rim along the bottom. The ears are
        // softer than a circle, as Droppy's are: a cubic with its handles halfway in.
        let ear = notch.ear;
        let slot = notch.ear_slot;
        let rim_from = h * black_until;
        let rim = look != Glass::Solid && notch.height >= PEEK.1;
        let shapes = canvas(
            |_, _, _| {},
            move |bounds, (), window, _| {
                let o = bounds.origin;
                let width: f32 = bounds.size.width.into();
                let p = |x: f32, y: f32| point(o.x + px(x), o.y + px(y));
                if ear >= 0.5 {
                    for (edge, out) in [(slot, -1.0f32), (width - slot, 1.0)] {
                        let mut path = gpui::PathBuilder::fill();
                        path.move_to(p(edge + out * ear, 0.0));
                        path.line_to(p(edge, 0.0));
                        path.line_to(p(edge, ear));
                        path.cubic_bezier_to(
                            p(edge + out * ear, 0.0),
                            p(edge, ear * 0.5),
                            p(edge + out * ear * 0.5, 0.0),
                        );
                        if let Ok(path) = path.build() {
                            window.paint_path(path, black);
                        }
                    }
                }
                if rim {
                    // The stroke is centred on the path: half a pixel in keeps it whole.
                    let (l, r, b) = (slot + 0.5, width - slot - 0.5, h - 0.5);
                    let c = radius.min((b - rim_from).max(0.0));
                    let k = 0.45;
                    let mut path = gpui::PathBuilder::stroke(px(1.0));
                    path.move_to(p(l, rim_from));
                    path.line_to(p(l, b - c));
                    path.cubic_bezier_to(p(l + c, b), p(l, b - c * k), p(l + c * k, b));
                    path.line_to(p(r - c, b));
                    path.cubic_bezier_to(p(r, b - c), p(r - c * k, b), p(r, b - c * k));
                    path.line_to(p(r, rim_from));
                    if let Ok(path) = path.build() {
                        let glow = linear_gradient(
                            180.0,
                            linear_color_stop(rgba(RIM & 0xffffff00), 0.0),
                            linear_color_stop(rgba(RIM), 1.0),
                        );
                        window.paint_path(path, glow);
                    }
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();

        div()
            .id("island")
            .relative()
            .w(viewport.width)
            .h(viewport.height)
            .text_color(rgb(0xffffff))
            .child(body)
            .child(shapes)
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
                let names: Vec<String> = paths
                    .paths()
                    .iter()
                    .filter_map(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .collect();
                log(&format!("drop {names:?}"));
                let added = this.add_paths(paths.paths().to_vec(), cx);
                log(&format!("shelf {} (+{added})", this.shelf.len()));
                // Open so the file is seen to land, then fold away unless the cursor is on it.
                this.open_tray(TrayCause::Timed(COLLAPSE_AFTER_DROP), cx);
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _window, cx| this.on_hover(*hovered, cx)))
            .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                if this.mode != Mode::Tray {
                    this.open_tray(TrayCause::Timed(COLLAPSE_AFTER_CLICK), cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|_, _: &MouseDownEvent, _window, cx| {
                    // Without a tray (TOPSY_NO_TRAY, or it could not start) there is no menu,
                    // and this would otherwise leave no way to quit.
                    if !tray::show_menu() {
                        cx.quit();
                    }
                }),
            )
    }
}

/// Free chip pictures' textures in the window's atlas, which otherwise keeps them. Deferred:
/// inside a window's event handler GPUI has lent the window out, and `drop_image` can't reach it.
fn forget_icons(icons: Vec<Arc<RenderImage>>, cx: &mut App) {
    if !icons.is_empty() {
        cx.defer(move |cx| {
            for icon in icons {
                cx.drop_image(icon, None);
            }
        });
    }
}

/// Hand each event from a thread's channel to the island, on the UI thread, until either
/// side goes away.
fn forward<E: 'static>(
    cx: &mut App,
    island: &Entity<Island>,
    events: async_channel::Receiver<E>,
    handle: fn(&mut Island, E, &mut Context<Island>),
) {
    let island = island.clone();
    cx.spawn(async move |cx| {
        while let Ok(event) = events.recv().await {
            if island
                .update(cx, |island, cx| handle(island, event, cx))
                .is_err()
            {
                break;
            }
        }
    })
    .detach();
}

fn main() {
    // The shelf comes back before the window opens, without touching the disk for its files
    // (a dead share would stall launch); `settle_restored` checks them in the background.
    let config = config::get();
    let persistence = state::start(config.state.as_deref());
    log(&format!("restored {}", persistence.shelf.len()));
    Application::new().run(move |cx: &mut App| {
        let display = cx.primary_display();
        let display_bounds = display
            .as_ref()
            .map(|d| d.bounds())
            .unwrap_or_else(|| Bounds::new(point(px(0.0), px(0.0)), size(px(1920.0), px(1080.0))));
        // The window is the notch, top-centre of the primary display. It opens at the peek's
        // size and settles into its resting state on the first frame.
        let origin = point(
            display_bounds.origin.x + (display_bounds.size.width - px(PEEK.0)) / 2.0,
            display_bounds.origin.y,
        );
        let bounds = Bounds::new(origin, size(px(PEEK.0), px(PEEK.1)));

        let previous_foreground = win32::foreground_window();
        let mut glass = config.glass.unwrap_or(Glass::Blur);
        // A forced glass (the live checks) wins over the setting.
        let black = config.glass.is_none() && look::is_black();
        // Refused drags report here, so a jiggle that opened the tray for a payload
        // the island can't take (the hook can't tell text from files) folds it away.
        let (refuse_tx, refuse_rx) = async_channel::bounded(4);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    // `appears_transparent: false` stops GPUI insetting the client area by an
                    // invisible resize frame; `apply_overlay_styles` then removes the caption
                    // that Windows would otherwise add.
                    titlebar: Some(TitlebarOptions {
                        title: None,
                        appears_transparent: false,
                        traffic_light_position: None,
                    }),
                    window_min_size: Some(size(px(1.0), px(1.0))),
                    focus: false,
                    show: true,
                    kind: WindowKind::PopUp,
                    is_movable: false,
                    is_resizable: false,
                    is_minimizable: false,
                    window_background: WindowBackgroundAppearance::Transparent,
                    ..Default::default()
                },
                move |window, cx| {
                    let hwnd = window
                        .window_handle()
                        .ok()
                        .and_then(|h| win32::hwnd_from_raw(h.as_raw()));
                    let mut backdrop = None;
                    if let Some(hwnd) = hwnd {
                        win32::apply_overlay_styles(hwnd);
                        if glass == Glass::Blur {
                            match backdrop::Backdrop::new(hwnd) {
                                Ok(b) => backdrop = Some(b),
                                Err(e) => {
                                    log(&format!("compositor glass unavailable ({e}); acrylic"));
                                    glass = Glass::Acrylic;
                                }
                            }
                        }
                        if glass == Glass::Acrylic {
                            win32::round_corners(hwnd);
                            // After GPUI has applied its own background appearance, so ours wins.
                            if !black {
                                win32::set_acrylic(hwnd, Some(ACRYLIC_TINT));
                            }
                        }
                        win32::watch_drop_refusals(hwnd, refuse_tx);
                        win32::restore_foreground(previous_foreground);
                    }
                    let scale = window.scale_factor();
                    cx.new(move |_| Island::new(hwnd, scale, glass, black, backdrop, persistence))
                },
            )
            .expect("open island window");

        let island = window.entity(cx).expect("island entity");

        // The restored count reaches the tray icon's menu before the first shelf change.
        island.update(cx, |island, cx| {
            tray::set_shelf_count(island.shelf.len());
            island.settle_restored(cx);
        });

        // However the app ends, the writer thread lands the queued shelf save first.
        {
            let island = island.clone();
            cx.on_app_quit(move |cx| {
                island.update(cx, |island, _| {
                    if let Some(writer) = island.writer.take() {
                        writer.drain();
                    }
                });
                async {}
            })
            .detach();
        }

        // The wrapped drop target reports refusals while an external drag runs.
        forward(cx, &island, refuse_rx, Island::on_drop_event);

        // Lets a drag out be tested without first dragging in.
        if let Some(paths) = config.seed.clone() {
            island.update(cx, |island, cx| {
                let added = island.add_paths(paths, cx);
                log(&format!("seeded {added}"));
            });
        }

        if !config.no_tray {
            if let Ok(exe) = std::env::current_exe() {
                autostart::refresh(&exe);
            }
            forward(cx, &island, tray::spawn(), Island::on_tray);
            // However the app ends, don't leave a dead icon in the notification area.
            cx.on_app_quit(|_| {
                tray::remove();
                async {}
            })
            .detach();
        }

        // Real input: the global mouse hook reports jiggles, the top edge and button releases.
        let events = if config.no_hook {
            async_channel::bounded(1).1
        } else {
            // The top row above the notch reveals it.
            let zone = island.read(cx).anchor.map(|(center_x, top)| {
                EdgeZone::centered(
                    center_x,
                    top,
                    (PEEK.0 * island.read(cx).scale).round() as i32,
                )
            });
            win32::spawn_mouse_hook(JiggleConfig::default(), zone)
        };
        forward(cx, &island, events, Island::on_hook);

        if config.demo {
            let demo_island = island.clone();
            cx.spawn(async move |cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(2500))
                        .await;
                    if demo_island
                        .update(cx, |i, cx| i.set_mode(Mode::Tray, cx))
                        .is_err()
                    {
                        break;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(2500))
                        .await;
                    if demo_island
                        .update(cx, |i, cx| i.set_mode(i.rest, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }

        if let Some(after) = config.exit_after {
            cx.spawn(async move |cx| {
                cx.background_executor().timer(after).await;
                cx.update(|cx| cx.quit()).ok();
            })
            .detach();
        }
    });
}
