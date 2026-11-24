//! The Stay Awake card, after Droppy's: a mug that keeps the computer and its display awake, until
//! pressed again or for a chosen length. The rules are in `model::stay_awake`; the keeping is a
//! power request (`win32::power`), held while it is on and dropped when it goes off, so quitting
//! topsy lets the computer sleep again. A timed stay books one timer for its end; the countdown is
//! redrawn only while the card is drawn.

use std::time::{Duration, Instant};

use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, MouseButton, Stateful, div, prelude::*, px,
    rgb, rgba,
};

use super::{Card, Droplet, Effect, keep_press};
use crate::booking::Booking;
use crate::icons::{self, Icon};
use crate::win32::power::KeepAwake;
use crate::{Island, log};
use model::stay_awake::{Awake, LENGTHS, Length, StayAwake, clock};

const WIDTH: f32 = 150.0;
const AMBER: u32 = 0xffb340;
const AMBER_TINT: u32 = 0xffb34033;
const DIM: u32 = 0xffffff80;
const GREY: u32 = 0xffffff26;
const BUTTON: f32 = 44.0;

#[derive(Default)]
pub struct State {
    stay: StayAwake,
    /// Held exactly while `stay` is on, unless Windows refused it.
    request: Option<KeepAwake>,
    ticking: Booking,
    /// Bumped on every change, so only the latest end timer switches it off.
    generation: u64,
}

#[derive(Clone, Copy)]
enum Press {
    Toggle,
    Choose(Length),
}

impl Droplet for State {
    fn switched_off(&mut self, _cx: &mut Context<Island>) {
        self.stay.stop();
        self.sync();
    }

    fn card(&self) -> Option<&dyn Card> {
        Some(self)
    }
}

impl Card for State {
    fn width(&self) -> f32 {
        WIDTH
    }

    fn render(&self, _visible: bool, cx: &mut Context<Island>) -> AnyElement {
        let now = Instant::now();
        if let Some(after) = self.stay.until_next_second(now) {
            self.tick(after, cx);
        }
        let on = self.stay.is_on();
        let lengths = div()
            .p(px(2.0))
            .rounded_full()
            .bg(rgba(0xffffff14))
            .flex()
            .flex_row()
            .children(LENGTHS.map(|length| tab(length, self.stay.length(), cx)));
        let (fill, color) = match on {
            true => (rgb(AMBER), rgb(0x000000)),
            false => (rgba(GREY), rgb(0xffffff)),
        };
        let mug = div()
            .id("stay-awake-toggle")
            .size(px(BUTTON))
            .flex_shrink_0()
            .rounded_full()
            .bg(fill)
            .flex()
            .items_center()
            .justify_center()
            .child(icons::icon(Icon::Coffee, 22.0, color));
        let status = match self.stay.awake() {
            Awake::Off => "Off".to_owned(),
            Awake::On { ends: None } => "On".to_owned(),
            Awake::On { ends: Some(ends) } => clock(ends.saturating_duration_since(now)),
        };
        let words = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgba(DIM))
                    .child("Stay awake"),
            )
            .child(
                div()
                    .text_size(px(20.0))
                    // GPUI's default line height (1.618) would push the card past its 104 px.
                    .line_height(px(24.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if on { rgb(AMBER) } else { rgb(0xffffff) })
                    .child(status),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .justify_between()
            .py(px(2.0))
            .child(div().flex().justify_center().child(lengths))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.0))
                    .pb(px(6.0))
                    .child(pressable(mug, Press::Toggle, cx))
                    .child(words),
            )
            .into_any_element()
    }
}

impl State {
    fn press(&mut self, press: Press, cx: &mut Context<Island>) {
        let now = Instant::now();
        match press {
            Press::Toggle => self.stay.toggle(now),
            Press::Choose(length) => self.stay.choose(length, now),
        }
        log(&format!("stay awake {:?}", self.stay.awake()));
        self.sync();
        self.book_end(now, cx);
        cx.notify();
    }

    /// Hold the power request while on, and only then.
    fn sync(&mut self) {
        self.generation += 1;
        match (self.stay.is_on(), self.request.is_some()) {
            (true, false) => match KeepAwake::new() {
                Ok(request) => self.request = Some(request),
                Err(error) => log(&format!("stay awake refused: {error}")),
            },
            (false, true) => self.request = None,
            (true, true) | (false, false) => {}
        }
    }

    /// Book a timed stay's end; `sync` cancelled any booked before.
    fn book_end(&mut self, now: Instant, cx: &mut Context<Island>) {
        let Awake::On { ends: Some(ends) } = self.stay.awake() else {
            return;
        };
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(ends.saturating_duration_since(now))
                .await;
            this.update(cx, |this, cx| {
                this.update_droplet(cx, |stay: &mut State, visible, cx| {
                    if stay.generation == generation {
                        stay.end(visible, cx);
                    }
                    None
                });
            })
            .ok();
        })
        .detach();
    }

    fn end(&mut self, visible: bool, cx: &mut Context<Island>) {
        let now = Instant::now();
        if !self.stay.finish(now) {
            // Woken a hair early: wait out the rest.
            self.book_end(now, cx);
            return;
        }
        log("stay awake over");
        self.sync();
        if visible {
            cx.notify();
        }
    }

    /// Redraw when the countdown next reads a new second, while the card is drawn.
    fn tick(&self, after: Duration, cx: &mut Context<Island>) {
        self.ticking.book(after, cx, |island, cx| {
            island.update_droplets(cx, |_, visible, cx| {
                if visible {
                    cx.notify();
                }
                None
            });
        });
    }
}

/// One length in the switch; the chosen one is tinted.
fn tab(length: Length, chosen: Length, cx: &mut Context<Island>) -> Stateful<Div> {
    let id = match length {
        Length::UntilOff => 0,
        Length::Minutes(minutes) => minutes as usize,
    };
    let base = div()
        .id(("stay-awake-length", id))
        .px(px(7.0))
        .py(px(2.0))
        .rounded_full()
        .text_size(px(11.0))
        .font_weight(FontWeight::SEMIBOLD)
        .child(length.label());
    if length == chosen {
        base.bg(rgba(AMBER_TINT)).text_color(rgb(AMBER))
    } else {
        pressable(base.text_color(rgba(DIM)), Press::Choose(length), cx)
    }
}

/// Make `element` a button for `press`. The press stays inside the card: the island under it
/// neither starts a drag nor opens.
fn pressable(element: Stateful<Div>, press: Press, cx: &mut Context<Island>) -> Stateful<Div> {
    element
        .cursor_pointer()
        .hover(|style| style.opacity(0.8))
        .on_mouse_down(MouseButton::Left, cx.listener(keep_press))
        .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
            cx.stop_propagation();
            this.update_droplet(cx, |stay: &mut State, _, cx| {
                stay.press(press, cx);
                None::<Effect>
            });
        }))
}
