//! The HUDs, after Droppy's: when the volume, the brightness or the battery changes, the notch
//! drops down a little wider and shows it (an icon and a bar of segments), then folds away.
//! Over an open tray the HUD sits on top of it instead. The rules are in `model::hud`.

use std::time::Duration;

use gpui::{App, Context, Div, Entity, FontWeight, div, prelude::*, px, rgb, rgba};

use crate::autostart::RegString;
use crate::frame::{Mode, PEEK};
use crate::icons::{self, Icon};
use crate::model::hud::{Battery, BatteryWatch, Hud, Power, SEGMENTS, lit};
use crate::win32::{self, battery, brightness, volume};
use crate::{Island, PEEK_LINGER, config, log};

const VOLUME_KEYS: RegString<'static> = RegString::new(r"Software\topsy", "VolumeKeys");
/// How long a HUD stays after the last change.
const LINGER: Duration = Duration::from_millis(1500);
/// The notch's body while it shows a HUD: the peek's height, wider for the bar.
pub const SIZE: (f32, f32) = (280.0, PEEK.1);
const UNLIT: u32 = 0xffffff33;
const GREEN: u32 = 0x34c759;
const RED: u32 = 0xff453a;

#[derive(Default)]
pub struct State {
    shown: Option<Hud>,
    /// Bumped on every new HUD, so only the latest one's timer folds it away.
    generation: u64,
    battery: BatteryWatch,
}

impl State {
    pub fn is_shown(&self) -> bool {
        self.shown.is_some()
    }
}

pub enum Event {
    Volume { level: f32, muted: bool },
    Brightness(u8),
    Power(Power),
}

/// topsy has the volume keys unless switched off (`VolumeKeys` = `off`).
pub fn takes_volume_keys() -> bool {
    VOLUME_KEYS.value().as_deref() != Some("off")
}

pub fn set_volume_keys(on: bool) -> windows::core::Result<()> {
    if on {
        VOLUME_KEYS.remove()?;
    } else {
        VOLUME_KEYS.set("off")?;
    }
    volume::take_keys(on && !config::get().no_keys);
    Ok(())
}

/// Start watching the volume keys, the brightness and the battery, and show what they report.
pub fn start(cx: &mut App, island: &Entity<Island>) {
    let (events, heard) = async_channel::unbounded();
    let sound = events.clone();
    volume::start(move |level, muted| {
        let _ = sound.try_send(Event::Volume { level, muted });
    });
    volume::take_keys(takes_volume_keys() && !config::get().no_keys);
    let light = events.clone();
    brightness::watch(move |percent| {
        let _ = light.try_send(Event::Brightness(percent));
    });
    battery::watch(move |power| {
        let _ = events.try_send(Event::Power(power));
    });
    crate::app::forward(cx, island, heard, Island::on_hud);
}

impl Island {
    fn on_hud(&mut self, event: Event, cx: &mut Context<Self>) {
        let hud = match event {
            Event::Volume { level, muted } => Hud::Volume { level, muted },
            Event::Brightness(percent) => Hud::Brightness(f32::from(percent) / 100.0),
            Event::Power(power) => match self.hud.battery.read(power) {
                Some(battery) => Hud::Battery(battery),
                None => return,
            },
        };
        log(&format!("hud {hud:?} (mode {:?})", self.mode));
        self.hud.shown = Some(hud);
        self.hud.generation += 1;
        if self.mode == Mode::Tray {
            cx.notify();
        } else {
            // The HUD folds the notch itself; a collapse booked earlier must not cut it short.
            self.collapse_generation += 1;
            self.set_mode(Mode::Peek, cx);
        }
        let generation = self.hud.generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LINGER).await;
            this.update(cx, |this, cx| {
                if this.hud.generation == generation {
                    this.hud_done(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn hud_done(&mut self, cx: &mut Context<Self>) {
        self.hud.shown = None;
        if self.mode != Mode::Peek {
            cx.notify();
        } else if self.rest != Mode::Peek && self.hwnd.is_some_and(win32::cursor_over) {
            // Being looked at: back to the plain peek, which folds once the cursor leaves.
            self.set_mode(Mode::Peek, cx);
            self.schedule_collapse(PEEK_LINGER, cx);
        } else {
            self.set_mode(self.rest, cx);
        }
    }

    /// The HUD in the dropped notch, if one is showing.
    pub(crate) fn hud_peek(&self) -> Option<Div> {
        let hud = self.hud.shown?;
        Some(
            row(hud)
                .w(px(SIZE.0))
                .h(px(SIZE.1))
                .flex_shrink_0()
                .px(px(16.0)),
        )
    }

    /// The HUD over the top of the open tray, if one is showing.
    pub(crate) fn hud_over_tray(&self) -> Option<Div> {
        let hud = self.hud.shown?;
        Some(
            div()
                .absolute()
                .top(px(8.0))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    row(hud)
                        .w(px(SIZE.0 - 24.0))
                        .h(px(SIZE.1))
                        .px(px(14.0))
                        .rounded_full()
                        .bg(rgb(0x000000))
                        .border_1()
                        .border_color(rgba(0xffffff1f)),
                ),
        )
    }
}

/// An icon on the left, the value on the right.
fn row(hud: Hud) -> Div {
    let base = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .text_color(rgb(0xffffff));
    match hud {
        Hud::Volume { level, muted } => {
            let glyph = match level {
                _ if muted => Icon::VolumeMute,
                0.0 => Icon::VolumeOff,
                l if l < 0.5 => Icon::VolumeLow,
                _ => Icon::VolumeHigh,
            };
            base.child(icon(glyph))
                .child(bar(if muted { 0.0 } else { level }))
        }
        Hud::Brightness(level) => base.child(icon(Icon::Brightness)).child(bar(level)),
        Hud::Battery(battery) => {
            let (label, percent, fill) = match battery {
                Battery::Charging(p) => ("Charging", p, GREEN),
                Battery::Low(p) => ("Low battery", p, RED),
                Battery::Full => ("Fully charged", 100, GREEN),
            };
            base.text_xs()
                .child(
                    div()
                        .flex_1()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(label),
                )
                .child(
                    div()
                        .text_color(rgba(0xffffffb3))
                        .child(format!("{percent}%")),
                )
                .child(cell(percent, fill))
        }
    }
}

fn icon(i: Icon) -> gpui::Svg {
    icons::icon(i, 16.0, rgb(0xffffff))
}

/// Droppy's bar: a row of short dashes, lit up to the level.
fn bar(level: f32) -> Div {
    let lit = lit(level);
    div()
        .flex_1()
        .flex()
        .flex_row()
        .gap(px(3.0))
        .children((0..SEGMENTS).map(|i| {
            div().flex_1().h(px(3.0)).rounded_full().bg(if i < lit {
                rgba(0xffffffff)
            } else {
                rgba(UNLIT)
            })
        }))
}

/// A battery drawn as a cell filled to its charge, with the terminal on its right.
fn cell(percent: u8, fill: u32) -> Div {
    let inner = 20.0 * f32::from(percent.min(100)) / 100.0;
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(1.0))
        .child(
            div()
                .w(px(24.0))
                .h(px(12.0))
                .p(px(1.0))
                .rounded(px(3.0))
                .border_1()
                .border_color(rgba(0xffffff8c))
                .child(
                    div()
                        .w(px(inner.max(2.0)))
                        .h_full()
                        .rounded(px(1.5))
                        .bg(rgb(fill)),
                ),
        )
        .child(
            div()
                .w(px(2.0))
                .h(px(4.0))
                .rounded(px(1.0))
                .bg(rgba(0xffffff8c)),
        )
}
