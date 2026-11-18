//! The Pomodoro card, after Droppy's: a Focus | Break switch, the countdown, play or pause and
//! reset, and while idle a minus and a plus for the phase's minutes. The rules are in
//! `model::pomodoro`. A running phase books one timer for its end, which fires whether or not the
//! card is on screen and says so with a sound and a passing Live Activity; the countdown itself
//! is redrawn only while the card is drawn.

use std::cell::Cell;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, MouseButton, MouseDownEvent, Rgba, Stateful,
    div, prelude::*, px, rgb, rgba,
};

use crate::icons::{self, Icon};
use crate::model::activity::{Activity, Key};
use crate::model::pomodoro::{Clock, Phase, Pomodoro, clock};
use crate::{Island, log};

pub const WIDTH: f32 = 176.0;
const ORANGE: u32 = 0xff9f0a;
const ORANGE_TINT: u32 = 0xff9f0a33;
const DIM: u32 = 0xffffff80;
const GREY: u32 = 0xffffff26;
const BUTTON: f32 = 30.0;
const STEP: f32 = 22.0;

#[derive(Default)]
pub struct State {
    timer: Pomodoro,
    /// A redraw is booked for the countdown's next second.
    ticking: Cell<bool>,
    /// Bumped on every change, so only the latest end timer finishes the phase.
    generation: u64,
}

impl State {
    /// Switched off: a running timer stops, and its end is never shown.
    pub fn stop(&mut self) {
        self.timer.reset();
        self.generation += 1;
    }
}

#[derive(Clone, Copy)]
enum Press {
    Choose(Phase),
    PlayPause,
    Reset,
    Adjust(i32),
}

impl Island {
    pub(crate) fn pomodoro_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let timer = &self.widgets.pomodoro.timer;
        let now = Instant::now();
        if let Some(after) = timer.until_next_second(now) {
            self.pomodoro_tick(after, cx);
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

    fn pomodoro_press(&mut self, press: Press, cx: &mut Context<Self>) {
        let now = Instant::now();
        let timer = &mut self.widgets.pomodoro.timer;
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
        self.pomodoro_book_end(now, cx);
        cx.notify();
    }

    /// Book the running phase's end, cancelling any booked before.
    fn pomodoro_book_end(&mut self, now: Instant, cx: &mut Context<Self>) {
        let state = &mut self.widgets.pomodoro;
        state.generation += 1;
        let Clock::Running { ends } = state.timer.clock() else {
            return;
        };
        let generation = state.generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(ends.saturating_duration_since(now))
                .await;
            this.update(cx, |this, cx| {
                if this.widgets.pomodoro.generation == generation {
                    this.pomodoro_end(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn pomodoro_end(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let Some(ended) = self.widgets.pomodoro.timer.finish(now) else {
            // Woken a hair early: wait out the rest.
            self.pomodoro_book_end(now, cx);
            return;
        };
        log(&format!("pomodoro {ended:?} over"));
        self.widgets.pomodoro.generation += 1;
        crate::win32::sound::chime();
        self.show_activity(Key::Timer, Activity::PhaseOver { ended }, cx);
        if self.widgets_visible() {
            cx.notify();
        }
    }

    /// Redraw when the countdown next reads a new second, while the card is drawn.
    fn pomodoro_tick(&self, after: Duration, cx: &mut Context<Self>) {
        if self.widgets.pomodoro.ticking.replace(true) {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(after).await;
            this.update(cx, |this, cx| {
                this.widgets.pomodoro.ticking.set(false);
                if this.widgets_visible() {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
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
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _window, cx| {
                this.press = None;
                cx.stop_propagation();
            }),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
            cx.stop_propagation();
            this.pomodoro_press(press, cx);
        }))
}
