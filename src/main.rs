// A release build is a pure GUI app. Without this Windows opens a console window for it, and
// that window takes foreground focus at launch. Debug builds keep the console for logging.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod jiggle;
mod shelf;
mod spring;
mod tray;
mod win32;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{
    App, Application, Bounds, ClickEvent, Context, ExternalPaths, FontWeight, MouseButton,
    MouseDownEvent, MouseMoveEvent, Pixels, Point, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, hsla, point,
    prelude::*, px, rgb, rgba, size,
};
use raw_window_handle::HasWindowHandle;
use windows::Win32::Foundation::HWND;

use crate::jiggle::JiggleConfig;
use crate::shelf::{Item, Shelf};
use crate::spring::{Spring, SpringConfig};
use crate::tray::TrayEvent;
use crate::win32::{DragOutcome, HookEvent};

/// Gap between the top of the screen and the pill.
const TOP_INSET: f32 = 6.0;

const IDLE: (f32, f32) = (168.0, 34.0);
const TRAY: (f32, f32) = (400.0, 118.0);

/// Corner radius of the pill. DWM rounds the window itself (and so the acrylic behind it)
/// by its own fixed small radius, so the drawn pill has to use the same one.
const CORNER_RADIUS: f32 = 8.0;

/// The glass colour (0xRRGGBBAA): dark and fairly strong, so white text stays legible over
/// bright backdrops. It is the acrylic's tint, and the `tint` control's plain fill.
const GLASS_TINT: u32 = 0x0c101c8c;

const COLLAPSE_AFTER_RELEASE: Duration = Duration::from_millis(1200);
const COLLAPSE_AFTER_DROP: Duration = Duration::from_millis(4000);
/// How long the tray stays once the cursor has left it.
const COLLAPSE_AFTER_LEAVE: Duration = Duration::from_millis(1500);
/// How long a tray opened by a click (on the pill or the tray icon) waits for the cursor to arrive.
const COLLAPSE_AFTER_CLICK: Duration = Duration::from_millis(5000);

/// How far (logical pixels) a pressed chip must be dragged before it becomes a drag out.
const DRAG_OUT_THRESHOLD: f32 = 6.0;

const CHIP: (f32, f32) = (72.0, 70.0);

/// Event trace for scripted tests; silent unless TOPSY_LOG is set.
fn log(message: &str) {
    use std::sync::OnceLock;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    if *ENABLED.get_or_init(|| std::env::var_os("TOPSY_LOG").is_some()) {
        eprintln!("topsy: {message}");
    }
}

/// How the pill is filled. `TOPSY_GLASS=solid|tint|clear` overrides the default, which is
/// for A/B comparison and for the automated blur check (`tint` and `clear` are its controls).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Glass {
    /// Acrylic blur of whatever is behind the pill, tinted with [`GLASS_TINT`].
    Blur,
    /// The same colour as a plain translucent fill, no blur: the pill without its blur.
    Tint,
    /// Opaque black.
    Solid,
    /// Fully transparent, no blur.
    Clear,
}

impl Glass {
    fn from_env() -> Self {
        match std::env::var("TOPSY_GLASS").as_deref() {
            Ok("tint") => Glass::Tint,
            Ok("solid") => Glass::Solid,
            Ok("clear") => Glass::Clear,
            _ => Glass::Blur,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Idle,
    Tray,
}

impl Mode {
    fn size(self) -> (f32, f32) {
        match self {
            Mode::Idle => IDLE,
            Mode::Tray => TRAY,
        }
    }
}

struct Island {
    mode: Mode,
    width: Spring,
    height: Spring,
    last_frame: Option<Instant>,
    shelf: Shelf,
    hwnd: Option<HWND>,
    scale: f32,
    glass: Glass,
    /// Where the pill sits: horizontal centre and top edge, in physical screen pixels.
    /// Every resize keeps the window centred on, and hanging from, this point.
    anchor: Option<(i32, i32)>,
    /// Size in physical pixels last requested from the OS, to skip redundant calls.
    applied_size: Option<(i32, i32)>,
    /// Bumped on every state change that schedules a collapse, so stale timers do nothing.
    collapse_generation: u64,
    /// The chip the left button went down on, and where: it becomes a drag out once moved.
    press: Option<(usize, Point<Pixels>)>,
    /// A chip is being dragged out; the system's drag loop is running.
    drag_out: bool,
}

impl Island {
    fn new(hwnd: Option<HWND>, scale: f32, glass: Glass) -> Self {
        let anchor = hwnd
            .and_then(win32::monitor_top_center)
            .map(|(center_x, top)| (center_x, top + (TOP_INSET * scale).round() as i32));
        Self {
            mode: Mode::Idle,
            width: Spring::new(IDLE.0, SpringConfig::ISLAND),
            height: Spring::new(IDLE.1, SpringConfig::ISLAND),
            last_frame: None,
            shelf: Shelf::default(),
            hwnd,
            scale,
            glass,
            anchor,
            applied_size: None,
            collapse_generation: 0,
            press: None,
            drag_out: false,
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
        tray::set_shelf_count(self.shelf.len());
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
        cx.spawn(async move |this, cx| {
            let resolved: Vec<(PathBuf, bool)> = cx
                .background_executor()
                .spawn(async move {
                    added
                        .into_iter()
                        .map(|path| {
                            let is_dir = path.is_dir();
                            (path, is_dir)
                        })
                        .collect()
                })
                .await;
            this.update(cx, |this, cx| {
                if this.shelf.resolve_kinds(resolved) > 0 {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        count
    }

    /// Open the tray. `collapse_after` folds it away again unless the cursor arrives; `None`
    /// is for a drag in progress, which closes the tray itself when the button is released.
    fn open_tray(&mut self, collapse_after: Option<Duration>, cx: &mut Context<Self>) {
        // Cancels any pending collapse from earlier.
        self.collapse_generation += 1;
        self.set_mode(Mode::Tray, cx);
        if let Some(after) = collapse_after {
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
                    this.set_mode(Mode::Idle, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn on_hook(&mut self, event: HookEvent, cx: &mut Context<Self>) {
        log(&format!("hook {event:?} (mode {:?})", self.mode));
        match event {
            HookEvent::Jiggle => self.open_tray(None, cx),
            HookEvent::ButtonUp => {
                if self.mode == Mode::Tray && !self.drag_out {
                    self.schedule_collapse(COLLAPSE_AFTER_RELEASE, cx);
                }
            }
        }
    }

    fn on_tray(&mut self, event: TrayEvent, cx: &mut Context<Self>) {
        log(&format!("tray {event:?} (mode {:?})", self.mode));
        match event {
            TrayEvent::Toggle if self.mode == Mode::Tray => {
                self.collapse_generation += 1;
                self.set_mode(Mode::Idle, cx);
            }
            TrayEvent::Toggle | TrayEvent::Show => self.open_tray(Some(COLLAPSE_AFTER_CLICK), cx),
            TrayEvent::ClearShelf => {
                self.shelf.clear();
                self.shelf_changed(cx);
            }
            TrayEvent::Quit => cx.quit(),
        }
    }

    fn on_hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            // The cursor is here; don't fold away under it.
            self.collapse_generation += 1;
        } else if self.mode == Mode::Tray && !self.drag_out {
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

    /// Make the OS window exactly the pill, centred under the anchor.
    ///
    /// The window itself is the pill, resized every frame as the spring moves. That is
    /// what keeps the glass pill-shaped: the acrylic blur fills the whole window rectangle
    /// and ignores `SetWindowRgn`, so there is no way to clip it to a shape inside a larger
    /// window. It also means there is nothing around the pill to be click-through.
    ///
    /// The resize runs in a foreground task rather than here: this is called from `render`,
    /// and a resize mid-draw would reach GPUI's window handler where it cannot update the
    /// window (see [`win32::move_window`]).
    fn sync_window(&mut self, w: f32, h: f32, cx: &mut Context<Self>) {
        let (Some(hwnd), Some((center_x, top))) = (self.hwnd, self.anchor) else {
            return;
        };
        let size = (
            (w * self.scale).round() as i32,
            (h * self.scale).round() as i32,
        );
        if self.applied_size == Some(size) {
            return;
        }
        self.applied_size = Some(size);
        cx.spawn(async move |_, _| {
            win32::move_window(hwnd, center_x - size.0 / 2, top, size.0, size.1);
        })
        .detach();
    }

    /// One file on the shelf: a coloured type badge and the name. Press and drag to take it
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

        let badge = div()
            .size(px(38.0))
            .rounded(px(8.0))
            .bg(hsla(item.badge_hue(), 0.55, 0.45, 1.0))
            .flex()
            .items_center()
            .justify_center()
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .child(item.badge_label());

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
            .child(badge)
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
                .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                    this.shelf.clear();
                    this.shelf_changed(cx);
                }))
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

        let w = self.width.value.max(1.0);
        let h = self.height.value.max(1.0);
        self.sync_window(w, h, cx);
        // The window resizes asynchronously, a moment after this frame. Draw to the size it
        // really has, so the pill's edge always lines up with the window's (and the blur's).
        let viewport = window.viewport_size();
        let show_tray = w > 260.0 && h > 70.0;

        // The acrylic does the work in `Blur`; every mode gets a faint rim for the glass edge.
        let (fill, rim) = match self.glass {
            Glass::Blur | Glass::Clear => (rgba(0x00000000), rgba(0xffffff38)),
            Glass::Tint => (rgba(GLASS_TINT), rgba(0xffffff38)),
            Glass::Solid => (rgba(0x000000ff), rgba(0xffffff1a)),
        };

        let idle_label = match self.shelf.len() {
            0 => "topsy".to_string(),
            n => format!("{n} on shelf"),
        };

        div()
            .id("pill")
            .w(viewport.width)
            .h(viewport.height)
            .rounded(px(CORNER_RADIUS))
            .bg(fill)
            .border_1()
            .border_color(rim)
            .overflow_hidden()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .text_color(rgb(0xffffff))
            .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(rgb(0x14213d)))
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
                this.open_tray(Some(COLLAPSE_AFTER_DROP), cx);
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _window, cx| this.on_hover(*hovered, cx)))
            .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                if this.mode == Mode::Idle {
                    this.open_tray(Some(COLLAPSE_AFTER_CLICK), cx);
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
            .child(if show_tray {
                self.tray_content(cx).into_any_element()
            } else {
                div().text_xs().child(idle_label).into_any_element()
            })
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let display = cx.primary_display();
        let display_bounds = display
            .as_ref()
            .map(|d| d.bounds())
            .unwrap_or_else(|| Bounds::new(point(px(0.0), px(0.0)), size(px(1920.0), px(1080.0))));
        // The window is the pill: it opens as the idle pill, top-centre of the primary display.
        let origin = point(
            display_bounds.origin.x + (display_bounds.size.width - px(IDLE.0)) / 2.0,
            display_bounds.origin.y + px(TOP_INSET),
        );
        let bounds = Bounds::new(origin, size(px(IDLE.0), px(IDLE.1)));

        let previous_foreground = win32::foreground_window();
        let glass = Glass::from_env();
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
                |window, cx| {
                    let hwnd = window
                        .window_handle()
                        .ok()
                        .and_then(|h| win32::hwnd_from_raw(h.as_raw()));
                    if let Some(hwnd) = hwnd {
                        win32::apply_overlay_styles(hwnd);
                        win32::round_corners(hwnd);
                        if glass == Glass::Blur {
                            // After GPUI has applied its own background appearance, so ours wins.
                            win32::enable_acrylic(hwnd, GLASS_TINT);
                        }
                        win32::restore_foreground(previous_foreground);
                    }
                    cx.new(|_| Island::new(hwnd, window.scale_factor(), glass))
                },
            )
            .expect("open island window");

        let island = window.entity(cx).expect("island entity");

        // Test hook: TOPSY_SEED=path;path puts files on the shelf at startup, so a drag out
        // can be tested without first dragging in.
        if let Some(seed) = std::env::var_os("TOPSY_SEED") {
            let paths: Vec<PathBuf> = std::env::split_paths(&seed).collect();
            island.update(cx, |island, cx| {
                let added = island.add_paths(paths, cx);
                log(&format!("seeded {added}"));
            });
        }

        // The notification-area icon and its menu. TOPSY_NO_TRAY skips it (for measuring).
        if std::env::var_os("TOPSY_NO_TRAY").is_none() {
            if let Ok(exe) = std::env::current_exe() {
                autostart::refresh(&exe);
            }
            let tray_events = tray::spawn();
            let tray_island = island.clone();
            cx.spawn(async move |cx| {
                while let Ok(event) = tray_events.recv().await {
                    if tray_island
                        .update(cx, |island, cx| island.on_tray(event, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
            // However the app ends, don't leave a dead icon in the notification area.
            cx.on_app_quit(|_| {
                tray::remove();
                async {}
            })
            .detach();
        }

        // Real input: the global mouse hook reports jiggles and button releases.
        // TOPSY_NO_HOOK skips it, to measure the app's idle cost without hook traffic.
        let events = if std::env::var_os("TOPSY_NO_HOOK").is_some() {
            async_channel::bounded(1).1
        } else {
            win32::spawn_mouse_hook(JiggleConfig::default())
        };
        let hook_island = island.clone();
        cx.spawn(async move |cx| {
            while let Ok(event) = events.recv().await {
                if hook_island
                    .update(cx, |island, cx| island.on_hook(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        // Demo/test mode: expand and collapse on a timer so the animation can be
        // observed without a mouse. TOPSY_DEMO=1 enables it.
        if std::env::var_os("TOPSY_DEMO").is_some() {
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
                        .update(cx, |i, cx| i.set_mode(Mode::Idle, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }

        if let Some(ms) = std::env::var("TOPSY_EXIT_AFTER_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
        {
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(ms))
                    .await;
                cx.update(|cx| cx.quit()).ok();
            })
            .detach();
        }
    });
}
