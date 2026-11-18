// A release build is a GUI app: a console window would take the focus at launch. Debug keeps it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod activity;
mod app;
mod archive;
mod autostart;
mod backdrop;
mod basket;
mod chip;
mod clipboard;
mod config;
mod frame;
mod hud;
mod icon;
mod icons;
mod look;
mod media;
mod model;
mod now_playing;
mod settings;
mod settings_window;
mod state;
mod transform;
mod tray;
mod tray_view;
mod win32;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    ClickEvent, Context, Entity, ExternalPaths, MouseButton, MouseDownEvent, Pixels, Point,
    RenderImage, Window, div, prelude::*, rgb,
};
use windows::Win32::Foundation::HWND;

use crate::frame::{Frame, Glass, Mode, PEEK};
use crate::model::shelf::{self, Shelf};
use crate::model::spring::{Spring, SpringConfig};
use crate::model::tray_cause::{HeldDrag, TrayCause};
use crate::win32::{DropEvent, HookEvent};

/// How long the cursor rests on the top edge before the notch comes down.
const REVEAL_AFTER: Duration = Duration::from_millis(150);
/// How long a notch that peeked down stays once the cursor has left it.
const PEEK_LINGER: Duration = Duration::from_millis(400);
const COLLAPSE_AFTER_RELEASE: Duration = Duration::from_millis(1200);
/// How long the tray stays once the cursor has left it.
const COLLAPSE_AFTER_LEAVE: Duration = Duration::from_millis(1500);

/// Event trace for scripted tests; silent unless TOPSY_LOG is set.
fn log(message: &str) {
    if config::get().log {
        eprintln!("topsy: {message}");
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
    frame: Frame,
    /// What the tray was opened for; decides which events fold it early.
    tray_cause: TrayCause,
    held: HeldDrag,
    /// Bumped on every state change that schedules a collapse, so stale timers do nothing.
    collapse_generation: u64,
    /// The chip the left button went down on, and where: it becomes a drag out once moved.
    press: Option<(usize, Point<Pixels>)>,
    /// A chip is being dragged out; the system's drag loop is running.
    drag_out: bool,
    /// A chip's right-click menu is up.
    chip_menu: bool,
    /// Writes the shelf to its state file on a thread of its own; `None` when saving is off.
    writer: Option<state::Writer>,
    /// Each chip's thumbnail or icon, by its path; a chip without one shows a type badge.
    icons: HashMap<PathBuf, Arc<RenderImage>>,
    media: now_playing::Media,
    hud: hud::State,
    activity: activity::State,
    basket: Option<Entity<basket::Basket>>,
}

impl Island {
    fn new(hwnd: Option<HWND>, scale: f32, frame: Frame, persistence: state::Persistence) -> Self {
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
            frame,
            tray_cause: TrayCause::Opened,
            held: HeldDrag::default(),
            collapse_generation: 0,
            press: None,
            drag_out: false,
            chip_menu: false,
            writer: persistence.writer,
            icons: HashMap::new(),
            media: Default::default(),
            hud: Default::default(),
            activity: Default::default(),
            basket: None,
        }
    }

    fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.mode = mode;
        win32::escape::take(mode == Mode::Tray && self.tray_cause == TrayCause::Opened);
        let (w, h) = self.size_for(mode);
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
        let gone: Vec<PathBuf> = (self.icons.keys())
            .filter(|path| !self.shelf.items().iter().any(|item| &item.path == *path))
            .cloned()
            .collect();
        let icons = gone
            .iter()
            .filter_map(|path| self.icons.remove(path))
            .collect();
        icon::forget(icons, cx);
        tray::set_shelf_count(self.shelf.len());
        self.fit_tray();
        if let Some(writer) = &self.writer {
            writer.send(shelf::serialize(self.shelf.items()));
        }
        cx.notify();
    }

    /// Park dropped paths on the shelf; returns how many were new.
    ///
    /// Chips go up right away: telling a folder from a file asks the disk, and a dead network
    /// share could stall the UI thread for its timeout, so the stat runs in the background
    /// (chips show `Kind::Unknown` meanwhile). Answers are merged back by path, so a chip
    /// removed or cleared before they arrive stays gone.
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

    /// Learn whether each chip is a folder or a file, and fetch its picture, off the UI thread.
    /// Each chip updates as its answer arrives.
    fn resolve_kinds(&mut self, added: Vec<PathBuf>, cx: &mut Context<Self>) {
        let rx = icon::resolve(added, (chip::ICON * self.scale).round() as i32);
        cx.spawn(async move |this, cx| {
            while let Ok((path, is_dir, icon)) = rx.recv().await {
                let updated = this.update(cx, |this, cx| {
                    if this.shelf.resolve_kinds([(path.clone(), is_dir)]) == 0 {
                        return;
                    }
                    if let Some(icon) = icon
                        && let Some(old) = this.icons.insert(path, icon)
                    {
                        icon::forget(vec![old], cx);
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
        self.collapse_generation += 1;
        self.tray_cause = cause;
        self.set_mode(Mode::Tray, cx);
        if let Some(after) = cause.folds_after() {
            self.schedule_collapse(after, cx);
        }
    }

    fn schedule_collapse(&mut self, after: Duration, cx: &mut Context<Self>) {
        self.collapse_generation += 1;
        let generation = self.collapse_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(after).await;
            this.update(cx, |this, cx| {
                if this.collapse_generation != generation || this.drag_out || this.chip_menu {
                    return;
                }
                // The cursor is asked directly rather than remembered: a hover state that
                // missed its "leave" (a drag took over the mouse) must not pin the tray open.
                if this.hwnd.is_some_and(win32::cursor_over) {
                    // Still being used; look again later, in case no "leave" ever arrives.
                    this.schedule_collapse(after, cx);
                } else {
                    this.set_mode(this.resting(), cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn on_hook(&mut self, event: HookEvent, cx: &mut Context<Self>) {
        log(&format!("hook {event:?} (mode {:?})", self.mode));
        match event {
            // The basket's.
            HookEvent::Jiggle => {}
            HookEvent::Edge { at_edge, pressed } => {
                self.at_edge = at_edge;
                // An activity holds the notch down in the peek, which a drag still opens.
                let at_rest = self.mode == Mode::Hidden
                    || (self.mode == Mode::Peek && self.mode == self.resting());
                if at_edge && at_rest && !self.fullscreen() {
                    if pressed {
                        // Something is being dragged up here: give it somewhere to land.
                        if self.held.may_open_tray() {
                            self.open_tray(TrayCause::Drag, cx);
                        }
                    } else if self.mode == Mode::Hidden {
                        self.schedule_reveal(cx);
                    }
                }
            }
            HookEvent::ButtonUp => {
                self.held.release();
                if self.mode == Mode::Tray && !self.drag_out {
                    self.schedule_collapse(COLLAPSE_AFTER_RELEASE, cx);
                }
            }
        }
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

    /// The drop target refused a drag crossing the island. If the tray only opened
    /// for that drag (one brought to the edge), its payload can never land: fold it away.
    fn on_drop_event(&mut self, event: DropEvent, cx: &mut Context<Self>) {
        log(&format!("drop-target {event:?} (mode {:?})", self.mode));
        if event == DropEvent::Refused {
            self.held.refuse();
        }
        if event == DropEvent::Refused
            && self.mode == Mode::Tray
            && self.tray_cause.closed_by_refusal()
        {
            self.fold(cx);
        }
    }

    /// Fold back to rest now, cancelling any collapse already scheduled (Escape, the tray icon).
    fn fold(&mut self, cx: &mut Context<Self>) {
        self.collapse_generation += 1;
        self.set_mode(self.resting(), cx);
    }

    fn on_hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.drag_out || self.chip_menu || self.mode == self.resting() {
        } else if self.mode == Mode::Peek {
            // The linger re-checks the cursor itself, so it needs no "leave", which a cursor
            // flicked off the top edge doesn't always deliver.
            if !hovered && !self.hud.is_shown() {
                self.schedule_collapse(PEEK_LINGER, cx);
            }
        } else if hovered {
            // The cursor is here; don't fold away under it.
            self.collapse_generation += 1;
        } else {
            self.schedule_collapse(COLLAPSE_AFTER_LEAVE, cx);
        }
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
        let (width, height) = (self.width.value, self.height.value);
        self.frame.sync(width, height, window.scale_factor(), cx);
        // The window resizes asynchronously, a moment after this frame. Draw to the size it
        // really has, so the notch's edge always lines up with the window's (and the blur's).
        let viewport = window.viewport_size();
        let notch = self.frame.outline(viewport);
        let show_tray = notch.body_width > 260.0 && notch.height > 70.0;

        let content = if show_tray {
            self.tray_content(window, cx).into_any_element()
        } else if notch.height >= 20.0 {
            self.peek_content(cx).into_any_element()
        } else {
            div().into_any_element()
        };

        div()
            .id("island")
            .relative()
            .w(viewport.width)
            .h(viewport.height)
            .text_color(rgb(0xffffff))
            .children(self.frame.draw(viewport, content))
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
                this.open_tray(TrayCause::Dropped, cx);
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _window, cx| this.on_hover(*hovered, cx)))
            .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                if this.mode != Mode::Tray {
                    this.open_tray(TrayCause::Opened, cx);
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

fn main() {
    app::run();
}
