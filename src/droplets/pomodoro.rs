//! The Pomodoro card, after Droppy's: a Focus | Break switch, the countdown, play or pause and
//! reset, and while idle a minus and a plus for the phase's minutes. The rules are in
//! `model::pomodoro`. A running phase books one timer for its end, which fires whether or not the
//! card is on screen and says so with a sound and a passing Live Activity; the countdown itself
//! is redrawn only while the card is drawn.

use std::time::{Duration, Instant};

use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, MouseButton, Rgba, Stateful, div, prelude::*,
    px, rgb, rgba,
};

use super::{Card, Droplet, Effect, keep_press};
use crate::booking::Booking;
use crate::icons::{self, Icon};
use crate::{Island, log};
use model::activity::{Activity, Key};
use model::pomodoro::{Clock, Phase, Pomodoro, clock};

const WIDTH: f32 = 176.0;
const ORANGE: u32 = 0xff9f0a;
const ORANGE_TINT: u32 = 0xff9f0a33;
const DIM: u32 = 0xffffff80;
const GREY: u32 = 0xffffff26;
const BUTTON: f32 = 30.0;
const STEP: f32 = 22.0;

#[derive(Default)]
pub struct State {
    timer: Pomodoro,
    /// The countdown's next second.
    ticking: Booking,
    /// Bumped on every change, so only the latest end timer finishes the phase.
    generation: u64,
}

#[derive(Clone, Copy)]
enum Press {
    Choose(Phase),
    PlayPause,
    Reset,
    Adjust(i32),
}

impl Droplet for State {
    /// A running timer stops, and its end is never shown.
    fn switched_off(&mut self, _cx: &mut Context<Island>) {
        self.timer.reset();
        self.generation += 1;
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
        let timer = &self.timer;
        let now = Instant::now();
        if let Some(after) = timer.until_next_second(now) {
            self.tick(after, cx);
        }
        let running = match timer.clock() {
            Clock::Running { .. } => true,
            Clock::Idle | Clock::Paused { .. } => false,
        };
        let tabs = div()
            .p(px(2.0))
            .rounded_full()
            .bg(rgba(0xffffff14))
            .flex()
            .flex_row()
            .children([Phase::Focus, Phase::Break].map(|phase| tab(phase, timer.phase(), cx)));
        let time = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(step(timer, -1, cx))
            .child(
                div()
                    .text_size(px(28.0))
                    // GPUI's default line height (1.618) would push the card past its 104 px.
                    .line_height(px(32.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(ORANGE))
                    .child(clock(timer.remaining(now))),
            )
            .child(step(timer, 1, cx));
        let play = round(
            "pomodoro-play",
            if running { Icon::Pause } else { Icon::Play },
            rgb(ORANGE),
            rgb(0x000000),
        );
        let reset = round("pomodoro-reset", Icon::Reset, rgba(GREY), rgb(0xffffff));
        let buttons = div()
            .flex()
            .flex_row()
            .justify_center()
            .gap(px(12.0))
            .child(pressable(play, Press::PlayPause, cx))
            .child(pressable(reset, Press::Reset, cx));
        div()
            .size_full()
            .flex()
            .flex_col()
            .justify_between()
            .py(px(2.0))
            .child(div().flex().justify_center().child(tabs))
            .child(time)
            .child(buttons)
            .into_any_element()
    }
}

impl State {
    fn press(&mut self, press: Press, cx: &mut Context<Island>) {
        let now = Instant::now();
        let timer = &mut self.timer;
        match press {
            Press::Choose(phase) => timer.choose(phase),
            Press::PlayPause => match timer.clock() {
                Clock::Running { .. } => timer.pause(now),
                Clock::Idle | Clock::Paused { .. } => timer.start(now),
            },
            Press::Reset => timer.reset(),
            Press::Adjust(steps) => {
                timer.adjust(steps);
            }
        }
        log(&format!("pomodoro {:?} {:?}", timer.phase(), timer.clock()));
        self.book_end(now, cx);
        cx.notify();
    }

    /// Book the running phase's end, cancelling any booked before.
    fn book_end(&mut self, now: Instant, cx: &mut Context<Island>) {
        self.generation += 1;
        let Clock::Running { ends } = self.timer.clock() else {
            return;
        };
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(ends.saturating_duration_since(now))
                .await;
            this.update(cx, |this, cx| {
                this.update_droplet(cx, |pomodoro: &mut State, visible, cx| {
                    if pomodoro.generation != generation {
                        return None;
                    }
                    pomodoro.end(visible, cx)
                });
            })
            .ok();
        })
        .detach();
    }

    fn end(&mut self, visible: bool, cx: &mut Context<Island>) -> Option<Effect> {
        let now = Instant::now();
        let Some(ended) = self.timer.finish(now) else {
            // Woken a hair early: wait out the rest.
            self.book_end(now, cx);
            return None;
        };
        log(&format!("pomodoro {ended:?} over"));
        self.generation += 1;
        crate::win32::sound::chime();
        if visible {
            cx.notify();
        }
        Some(Effect::Activity(Key::Timer, Activity::PhaseOver { ended }))
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

/// Five minutes less or more; only an idle timer shows them, but they keep their room.
fn step(timer: &Pomodoro, steps: i32, cx: &mut Context<Island>) -> Stateful<Div> {
    let (id, glyph) = if steps < 0 {
        ("pomodoro-less", Icon::Minus)
    } else {
        ("pomodoro-more", Icon::Add)
    };
    let base = div()
        .id(id)
        .size(px(STEP))
        .flex_shrink_0()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center();
    if timer.clock() != Clock::Idle {
        return base;
    }
    let base = base
        .bg(rgba(0xffffff14))
        .child(icons::icon(glyph, 12.0, rgb(0xffffff)));
    if timer.can_adjust(steps) {
        pressable(base, Press::Adjust(steps), cx)
    } else {
        base.opacity(0.3)
    }
}

/// One half of the Focus | Break switch; the chosen one is tinted.
fn tab(phase: Phase, chosen: Phase, cx: &mut Context<Island>) -> Stateful<Div> {
    let (id, label) = match phase {
        Phase::Focus => ("pomodoro-focus", "Focus"),
        Phase::Break => ("pomodoro-break", "Break"),
    };
    let base = div()
        .id(id)
        .px(px(10.0))
        .py(px(2.0))
        .rounded_full()
        .text_size(px(11.0))
        .font_weight(FontWeight::SEMIBOLD)
        .child(label);
    if phase == chosen {
        base.bg(rgba(ORANGE_TINT)).text_color(rgb(ORANGE))
    } else {
        pressable(base.text_color(rgba(DIM)), Press::Choose(phase), cx)
    }
}

fn round(id: &'static str, glyph: Icon, fill: Rgba, color: Rgba) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(BUTTON))
        .rounded_full()
        .bg(fill)
        .flex()
        .items_center()
        .justify_center()
        .child(icons::icon(glyph, 14.0, color))
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
            this.update_droplet(cx, |pomodoro: &mut State, _, cx| {
                pomodoro.press(press, cx);
                None
            });
        }))
}
