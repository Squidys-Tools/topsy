//! The island: the one GPUI view, its modes and springs, the shelf it holds, and the policy that
//! decides what it shows. Features keep their own state and rendering; they reach the island
//! through its entry points (`update_widgets`, `update_media`, `update_hud`,
//! `update_activities`) and the effects those apply.

pub(crate) mod chip;
pub(crate) mod pager;
mod shelf_keeping;
mod tray_view;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    ClickEvent, Context, Entity, ExternalPaths, MouseButton, MouseDownEvent, Pixels, Point,
    RenderImage, Window, div, prelude::*, rgb,
};
use windows::Win32::Foundation::HWND;

use crate::frame::{self, Frame, Mode};
use crate::log::log;
use crate::win32::{DropEvent, HookEvent};
use crate::{activity, basket, config, hud, now_playing, state, tray, widgets, win32};
use model::shelf::Shelf;
use model::spring::{Spring, SpringConfig};
use model::tray_cause::{HeldDrag, TrayCause};

/// How long the cursor rests on the top edge before the notch comes down.
const REVEAL_AFTER: Duration = Duration::from_millis(150);
/// How long a notch that peeked down stays once the cursor has left it.
pub(crate) const PEEK_LINGER: Duration = Duration::from_millis(400);
pub(crate) const COLLAPSE_AFTER_RELEASE: Duration = Duration::from_millis(1200);
/// How long the tray stays once the cursor has left it.
pub(crate) const COLLAPSE_AFTER_LEAVE: Duration = Duration::from_millis(1500);

pub(crate) struct Island {
    pub(crate) mode: Mode,
    /// What the island folds back to.
    pub(crate) rest: Mode,
    /// The cursor is on the top edge above the notch, as last reported by the hook.
    pub(crate) at_edge: bool,
    pub(crate) width: Spring,
    pub(crate) height: Spring,
    pub(crate) last_frame: Option<Instant>,
    pub(crate) shelf: Shelf,
    pub(crate) hwnd: Option<HWND>,
    pub(crate) scale: f32,
    pub(crate) frame: Frame,
    /// What the tray was opened for; decides which events fold it early.
    pub(crate) tray_cause: TrayCause,
    pub(crate) held: HeldDrag,
    /// Bumped on every state change that schedules a collapse, so stale timers do nothing.
    pub(crate) collapse_generation: u64,
    /// The chip the left button went down on, and where: it becomes a drag out once moved.
    pub(crate) press: Option<(usize, Point<Pixels>)>,
    /// A chip is being dragged out; the system's drag loop is running.
    pub(crate) drag_out: bool,
    /// A chip's right-click menu is up.
    pub(crate) chip_menu: bool,
    /// Writes the shelf to its state file on a thread of its own; `None` when saving is off.
    pub(crate) writer: Option<state::Writer>,
    /// Each chip's thumbnail or icon, by its path; a chip without one shows a type badge.
    pub(crate) icons: HashMap<PathBuf, Arc<RenderImage>>,
    pub(crate) media: now_playing::Media,
    pub(crate) hud: hud::State,
    pub(crate) activity: activity::State,
    pub(crate) widgets: widgets::State,
    pub(crate) pager: pager::State,
    pub(crate) basket: Option<Entity<basket::Basket>>,
}

impl Island {
    pub(crate) fn new(
        hwnd: Option<HWND>,
        scale: f32,
        frame: Frame,
        persistence: state::Persistence,
    ) -> Self {
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
            widgets: Default::default(),
            pager: Default::default(),
            basket: None,
        }
    }

    pub(crate) fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        if mode == Mode::Tray && self.mode != Mode::Tray {
            self.opened(cx);
        }
        self.mode = mode;
        win32::escape::take(mode == Mode::Tray && self.tray_cause == TrayCause::Opened);
        self.retarget();
        if mode == Mode::Tray {
            self.prune_missing(cx);
        }
        cx.notify();
    }

    /// Open the tray. `cause` is what it is waiting for: a `Timed` one folds on its
    /// own timer; a `Drag` one waits for a drag in progress to end, land, or be refused.
    pub(crate) fn open_tray(&mut self, cause: TrayCause, cx: &mut Context<Self>) {
        self.collapse_generation += 1;
        self.show_tray_for(cause, cx);
        if let Some(after) = cause.folds_after() {
            self.schedule_collapse(after, cx);
        }
    }

    pub(crate) fn schedule_collapse(&mut self, after: Duration, cx: &mut Context<Self>) {
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

    pub(crate) fn on_hook(&mut self, event: HookEvent, cx: &mut Context<Self>) {
        log(&format!("hook {event:?} (mode {:?})", self.mode));
        match event {
            // The basket's.
            HookEvent::Jiggle => {}
            HookEvent::Edge { at_edge, pressed } => {
                self.at_edge = at_edge;
                if at_edge && self.at_rest() && !frame::fullscreen(self.hwnd) {
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
    pub(crate) fn schedule_reveal(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REVEAL_AFTER).await;
            this.update(cx, |this, cx| {
                if this.at_edge && this.mode == Mode::Hidden && !frame::fullscreen(this.hwnd) {
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
    pub(crate) fn on_drop_event(&mut self, event: DropEvent, cx: &mut Context<Self>) {
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

    pub(crate) fn on_hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
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
        let moving = self.width.step(dt) | self.height.step(dt) | self.pager.step(dt);
        self.last_frame = moving.then_some(now);
        if moving {
            window.request_animation_frame();
        }
        let (width, height, scale) = (self.width.value, self.height.value, window.scale_factor());
        let (strip, open) = (self.pager.strip(), self.mode == Mode::Tray);
        self.frame.sync(width, height, strip, scale, open, cx);
        // The window resizes asynchronously, a moment after this frame. Draw to the size it
        // really has, so the notch's edge always lines up with the window's (and the blur's).
        let (viewport, target) = (window.viewport_size(), self.height.target);
        let (notch, _) = self.frame.outline(viewport);
        let show_tray = notch.body_width > 260.0 && notch.height > 70.0;

        let content = if show_tray {
            self.tray_content(cx).into_any_element()
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
            .children(self.frame.draw(viewport, target, content, self.capsule(cx)))
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
            .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| this.clicked(cx)))
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
