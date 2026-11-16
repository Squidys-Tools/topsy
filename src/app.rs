//! The app's wiring: opening the island, and connecting the threads, the tray icon and the
//! settings window to it.

use std::rc::Rc;
use std::time::Duration;

use gpui::{
    App, Application, Bounds, Context, Entity, TitlebarOptions, WindowBackgroundAppearance,
    WindowBounds, WindowKind, WindowOptions, point, prelude::*, px, size,
};
use raw_window_handle::HasWindowHandle;

use crate::frame::{ACRYLIC_TINT, Glass, PEEK};
use crate::model::edge::EdgeZone;
use crate::model::jiggle::JiggleConfig;
use crate::model::tray_cause::TrayCause;
use crate::settings::Setting;
use crate::settings_window::{self, OnChange};
use crate::tray::{self, TrayEvent};
use crate::{Island, Mode, autostart, backdrop, config, log, look, state, win32};

impl Island {
    fn on_tray(&mut self, event: TrayEvent, cx: &mut Context<Self>) {
        log(&format!("tray {event:?} (mode {:?})", self.mode));
        match event {
            TrayEvent::Toggle if self.mode == Mode::Tray => self.fold(cx),
            TrayEvent::Toggle | TrayEvent::Show => self.open_notch_tray(TrayCause::Opened, cx),
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
            }
            Setting::Autostart => {}
        }
    }
}

/// Hand each event from a thread's channel to the island, on the UI thread, until either
/// side goes away.
pub(crate) fn forward<E: 'static>(
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

pub fn run() {
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

        crate::now_playing::start(cx, &island);

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
