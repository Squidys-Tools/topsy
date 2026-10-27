mod jiggle;
mod spring;
mod win32;

use std::time::{Duration, Instant};

use gpui::{
    App, Application, Bounds, Context, ExternalPaths, Window, WindowBackgroundAppearance,
    WindowBounds, WindowKind, WindowOptions, div, point, prelude::*, px, rgb, rgba, size,
};
use raw_window_handle::HasWindowHandle;
use windows::Win32::Foundation::HWND;

use crate::jiggle::JiggleConfig;
use crate::spring::{Spring, SpringConfig};
use crate::win32::HookEvent;

/// The OS window is a fixed transparent canvas; the pill is drawn inside it and
/// the window region clips hit-testing to the pill, so nothing else is blocked.
const WIN_W: f32 = 440.0;
const WIN_H: f32 = 150.0;
const TOP_INSET: f32 = 6.0;

const IDLE: (f32, f32) = (168.0, 34.0);
const TRAY: (f32, f32) = (400.0, 118.0);
/// Region padding while moving, so spring overshoot isn't clipped.
const OVERSHOOT_PAD: f32 = 16.0;

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
    /// Last region pushed to the OS, to avoid redundant calls.
    applied_region: Option<(Mode, bool)>,
    /// Bumped on every state change that schedules a collapse, so stale timers do nothing.
    collapse_generation: u64,
}

impl Island {
    fn new(hwnd: Option<HWND>, scale: f32) -> Self {
        Self {
            mode: Mode::Idle,
            width: Spring::new(IDLE.0, SpringConfig::ISLAND),
            height: Spring::new(IDLE.1, SpringConfig::ISLAND),
            last_frame: None,
            files: Vec::new(),
            hwnd,
            scale,
            applied_region: None,
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

    /// Clip the OS window to what is currently visible. While moving, use the
    /// large tray rect (plus padding) so overshoot and shrinking content aren't cut off.
    fn sync_region(&mut self, animating: bool) {
        let Some(hwnd) = self.hwnd else { return };
        let key = (self.mode, animating);
        if self.applied_region == Some(key) {
            return;
        }
        self.applied_region = Some(key);

        let (w, h) = if animating {
            (TRAY.0 + OVERSHOOT_PAD * 2.0, TRAY.1 + OVERSHOOT_PAD)
        } else {
            self.mode.size()
        };
        let w = w.min(WIN_W);
        let s = self.scale;
        let left = ((WIN_W - w) / 2.0 * s).round() as i32;
        let top = (TOP_INSET * s).round() as i32;
        let right = left + (w * s).round() as i32;
        let bottom = top + (h * s).round() as i32;
        let radius = ((h / 2.0).min(28.0) * s).round() as i32;
        win32::set_hit_region(hwnd, left, top, right, bottom, radius);
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
        self.sync_region(moving);

        let w = self.width.value.max(1.0);
        let h = self.height.value.max(1.0);
        let radius = (h / 2.0).min(28.0);
        let show_tray = w > 260.0 && h > 70.0;

        let label = if self.files.is_empty() {
            "Drop files to park them".to_string()
        } else {
            self.files.join("\n")
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(TOP_INSET))
            .child(
                div()
                    .id("pill")
                    .w(px(w))
                    .h(px(h))
                    .rounded(px(radius))
                    .bg(rgb(0x000000))
                    .border_1()
                    .border_color(rgba(0xffffff1a))
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
                    }),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let display = cx.primary_display();
        let display_bounds = display
            .as_ref()
            .map(|d| d.bounds())
            .unwrap_or_else(|| Bounds::new(point(px(0.0), px(0.0)), size(px(1920.0), px(1080.0))));
        let origin = point(
            display_bounds.origin.x + (display_bounds.size.width - px(WIN_W)) / 2.0,
            display_bounds.origin.y,
        );
        let bounds = Bounds::new(origin, size(px(WIN_W), px(WIN_H)));

        let previous_foreground = win32::foreground_window();
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: None,
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
                        win32::restore_foreground(previous_foreground);
                    }
                    cx.new(|_| Island::new(hwnd, window.scale_factor()))
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
