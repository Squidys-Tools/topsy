// A release build is a pure GUI app. Without this Windows opens a console window for it, and
// that window takes foreground focus at launch. Debug builds keep the console for logging.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod jiggle;
mod spring;
mod win32;

use std::time::{Duration, Instant};

use gpui::{
    App, Application, Bounds, Context, ExternalPaths, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, point, prelude::*,
    px, rgb, rgba, size,
};
use raw_window_handle::HasWindowHandle;
use windows::Win32::Foundation::HWND;

use crate::jiggle::JiggleConfig;
use crate::spring::{Spring, SpringConfig};
use crate::win32::HookEvent;

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
    files: Vec<String>,
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
            files: Vec::new(),
            hwnd,
            scale,
            glass,
            anchor,
            applied_size: None,
            collapse_generation: 0,
        }
    }

    fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.mode = mode;
        let (w, h) = mode.size();
        self.width.set_target(w);
        self.height.set_target(h);
        if mode == Mode::Idle {
            self.files.clear();
        }
        cx.notify();
    }

    fn schedule_collapse(&mut self, after: Duration, cx: &mut Context<Self>) {
        self.collapse_generation += 1;
        let generation = self.collapse_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(after).await;
            this.update(cx, |this, cx| {
                if this.collapse_generation == generation {
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
            HookEvent::Jiggle => {
                // Cancels any pending collapse from an earlier drag.
                self.collapse_generation += 1;
                self.set_mode(Mode::Tray, cx);
            }
            HookEvent::ButtonUp => {
                if self.mode == Mode::Tray {
                    self.schedule_collapse(COLLAPSE_AFTER_RELEASE, cx);
                }
            }
        }
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

        let label = if self.files.is_empty() {
            "Drop files to park them".to_string()
        } else {
            self.files.join("\n")
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
                this.files = paths
                    .paths()
                    .iter()
                    .filter_map(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .collect();
                log(&format!("drop {:?}", this.files));
                this.schedule_collapse(COLLAPSE_AFTER_DROP, cx);
                cx.notify();
            }))
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(|_, _, _, cx| cx.quit()),
            )
            .child(if show_tray {
                div().text_sm().child(label)
            } else {
                div().text_xs().child("topsy")
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
