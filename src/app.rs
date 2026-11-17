//! The app's wiring: opening the island, and connecting the threads, the tray icon and the
//! settings window to it.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    App, Application, Bounds, Context, Entity, Pixels, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, point, prelude::*, px,
    size,
};
use raw_window_handle::HasWindowHandle;
use windows::Win32::Foundation::HWND;

use crate::basket::Basket;
use crate::frame::{ACRYLIC_TINT, Frame, Glass, PEEK};
use crate::model::edge::EdgeZone;
use crate::model::jiggle::JiggleConfig;
use crate::model::tray_cause::TrayCause;
use crate::settings::Setting;
use crate::settings_window::{self, OnChange};
use crate::tray::{self, TrayEvent};
use crate::win32::DropEvent;
use crate::win32::sendto;
use crate::{Island, Mode, autostart, backdrop, config, log, look, state, win32};

impl Island {
    fn on_tray(&mut self, event: TrayEvent, cx: &mut Context<Self>) {
        log(&format!("tray {event:?} (mode {:?})", self.mode));
        match event {
            TrayEvent::Toggle if self.mode == Mode::Tray => self.fold(cx),
            TrayEvent::Toggle | TrayEvent::Show => self.open_tray(TrayCause::Opened, cx),
            TrayEvent::Setting(setting, on) => {
                self.on_setting(setting, on, cx);
                settings_window::refresh(cx);
            }
            TrayEvent::OpenSettings(previous) => {
                let island = cx.entity().downgrade();
                let on_change: OnChange = Rc::new(move |setting, on, cx| {
                    let _ = island.update(cx, |island, cx| island.on_setting(setting, on, cx));
                });
                // Out of the island's update: a new window must not be made while it is leased.
                cx.defer(move |cx| settings_window::open(cx, on_change, previous));
            }
            TrayEvent::ClearShelf => self.clear_shelf(cx),
            TrayEvent::Quit => cx.quit(),
        }
    }

    /// A setting was switched, from the menu or the settings window, and is saved already.
    fn on_setting(&mut self, setting: Setting, on: bool, cx: &mut Context<Self>) {
        log(&format!("setting {setting:?} {on}"));
        match setting {
            Setting::Black => {
                self.frame.set_black(on, cx);
                cx.notify();
                if let Some(basket) = self.basket.clone() {
                    basket.update(cx, |basket, cx| basket.set_black(on, cx));
                }
            }
            Setting::Autostart | Setting::SendTo | Setting::Clipboard | Setting::VolumeHud => {}
        }
    }

    /// Paths sent from Explorer's Send to (or given on the command line): park them and open
    /// the tray so they are seen to land.
    fn on_sent(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let added = self.add_paths(paths, cx);
        log(&format!("sent {added}"));
        self.open_tray(TrayCause::Dropped, cx);
    }
}

/// Hand each event from a thread's channel to a view (the island, the basket), on the UI
/// thread, until either side goes away.
pub(crate) fn forward<V: 'static, E: 'static>(
    cx: &mut App,
    island: &Entity<V>,
    events: async_channel::Receiver<E>,
    handle: fn(&mut V, E, &mut Context<V>),
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

pub fn run() {
    // Explorer's Send to starts topsy with the selected paths: a topsy already running takes
    // them, and this one is done.
    let sent: Vec<PathBuf> = std::env::args_os()
        .skip(1)
        .filter_map(|arg| std::path::absolute(arg).ok())
        .collect();
    if !sent.is_empty() && win32::inbox::hand_off(&sent) {
        return;
    }
    // Up before GPUI, so a second Send to during startup is buffered rather than missed.
    let inbox = win32::inbox::spawn();
    // The shelf comes back before the window opens, without touching the disk for its files
    // (a dead share would stall launch); `settle_restored` checks them in the background.
    let config = config::get();
    let persistence = state::start(config.state.as_deref());
    log(&format!("restored {}", persistence.shelf.len()));
    let app = Application::new().with_assets(crate::icons::Assets);
    app.run(move |cx: &mut App| {
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
        let glass = config.glass.unwrap_or(Glass::Blur);
        // A forced glass (the live checks) wins over the setting.
        let black = config.glass.is_none() && look::is_black();
        // Refused drags report here, so a tray or basket opened for a payload the island can't
        // take (the hook can't tell text from files) folds it away.
        let (refuse_tx, refuse_rx) = async_channel::bounded(4);
        let window = open_glass(cx, bounds, glass, black, refuse_tx, previous_foreground, {
            move |hwnd, glass, backdrop, window, cx| {
                let scale = window.scale_factor();
                let frame = Frame::new(hwnd, scale, glass, black, backdrop);
                cx.new(move |_| Island::new(hwnd, scale, frame, persistence))
            }
        });
        let island = window.entity(cx).expect("island entity");

        // The basket gets its window now, empty and click-through until a jiggle: made later,
        // its creation would take the foreground from the app being dragged out of.
        let (basket_refuse_tx, basket_refuse_rx) = async_channel::bounded(4);
        let tiny = Bounds::new(display_bounds.origin, size(px(1.0), px(1.0)));
        let weak = island.downgrade();
        let basket = open_glass(
            cx,
            tiny,
            glass,
            black,
            basket_refuse_tx,
            previous_foreground,
            {
                move |hwnd, glass, backdrop, window, cx| {
                    let scale = window.scale_factor();
                    let frame = Frame::new(hwnd, scale, glass, black, backdrop).floating();
                    cx.new(|cx| {
                        // Moved by its handle: float from where it was put.
                        cx.observe_window_bounds(window, |basket: &mut Basket, _, _| {
                            basket.follow_window()
                        })
                        .detach();
                        Basket::new(hwnd, scale, frame, weak)
                    })
                }
            },
        )
        .entity(cx)
        .expect("basket entity");
        island.update(cx, |island, _| island.basket = Some(basket.clone()));
        forward(cx, &basket, basket_refuse_rx, Basket::on_drop_event);

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

        forward(cx, &island, inbox, Island::on_sent);
        if !sent.is_empty() {
            island.update(cx, |island, cx| island.on_sent(sent, cx));
        }

        if !config.no_tray {
            if let Ok(exe) = std::env::current_exe() {
                autostart::refresh(&exe);
                sendto::refresh(&exe);
            }
            forward(cx, &island, tray::spawn(), Island::on_tray);
            // However the app ends, don't leave a dead icon in the notification area.
            cx.on_app_quit(|_| {
                tray::remove();
                async {}
            })
            .detach();
        }

        forward(cx, &island, win32::escape::presses(), |island, (), cx| {
            island.fold(cx)
        });
        crate::now_playing::start(cx, &island);
        crate::hud::start(cx, &island);
        if let Some(files) = config.transform.clone() {
            crate::transform::open(cx, &island, files);
        }
        if let Some(events) = crate::clipboard::start(config.clipboard.clone()) {
            crate::clipboard::strip::listen(cx, events);
        }

        // Real input: the global mouse hook reports jiggles, the top edge and button releases.
        let events = if config.no_hook {
            async_channel::bounded(1).1
        } else {
            // The top row above the notch reveals it.
            let zone = island.read(cx).frame.anchor.map(|(center_x, top)| {
                EdgeZone::centered(
                    center_x,
                    top,
                    (PEEK.0 * island.read(cx).scale).round() as i32,
                )
            });
            win32::spawn_mouse_hook(JiggleConfig::default(), zone)
        };
        // Both hear the hook: the island the top edge, the basket the jiggle.
        let (island_hook, basket_hook) = (island.clone(), basket.clone());
        cx.spawn(async move |cx| {
            while let Ok(event) = events.recv().await {
                let heard = island_hook.update(cx, |island, cx| island.on_hook(event, cx));
                let basket = basket_hook.update(cx, |basket, cx| basket.on_hook(event, cx));
                if heard.is_err() || basket.is_err() {
                    break;
                }
            }
        })
        .detach();

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

/// Open one of topsy's glass windows: transparent, never activated, click-through until it is
/// given a shape, with the compositor's blur behind it (or acrylic where that can't start), and
/// a drop target that reports refused drags on `refusals`. `build` makes its view.
fn open_glass<V: 'static + Render>(
    cx: &mut App,
    bounds: Bounds<Pixels>,
    mut glass: Glass,
    black: bool,
    refusals: async_channel::Sender<DropEvent>,
    previous_foreground: HWND,
    build: impl FnOnce(
        Option<HWND>,
        Glass,
        Option<backdrop::Backdrop>,
        &mut Window,
        &mut App,
    ) -> Entity<V>
    + 'static,
) -> gpui::WindowHandle<V> {
    cx.open_window(
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
            // Only lets a `WindowControlArea::Drag` element be a caption (the basket's handle);
            // every other hit test is still Windows' own.
            is_movable: true,
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
                    match backdrop::Backdrop::new(hwnd, cx) {
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
                win32::watch_drop_refusals(hwnd, refusals);
                win32::restore_foreground(previous_foreground);
            }
            build(hwnd, glass, backdrop, window, cx)
        },
    )
    .expect("open a glass window")
}
