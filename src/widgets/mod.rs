//! Shelf widgets, after Droppy's: a row of small cards (a calendar, a Pomodoro timer, system
//! stats, the weather) across the top of a tray opened to look. Which show, and in what order,
//! is the lineup in `model::widgets`, set in the settings window. Nothing here runs while the row
//! is off screen: each card books its own redraws only while it is drawn.

mod calendar;
mod pomodoro;
mod stats;
mod weather;

use gpui::{AnyElement, Context, MouseDownEvent, Window, div, prelude::*, px, rgba};

use crate::autostart::RegString;
use crate::{Island, config};
use model::pomodoro::Phase;
use model::widgets::{Lineup, Widget};

const SETTING: RegString<'static> = RegString::new(r"Software\topsy", "Widgets");
/// The row's height, padding included: what the tray grows by while it shows.
pub const HEIGHT: f32 = PAD_TOP + CARD + PAD_BOTTOM;
/// Every card's height; each has a width of its own.
pub const CARD: f32 = 104.0;
const PAD_TOP: f32 = 12.0;
const PAD_BOTTOM: f32 = 6.0;
const PAD_X: f32 = 14.0;
/// Between cards, with a hairline in the middle.
const GAP: f32 = 21.0;
const DIVIDER: u32 = 0xffffff1f;

pub struct State {
    lineup: Lineup,
    calendar: calendar::State,
    pomodoro: pomodoro::State,
    stats: stats::State,
    weather: weather::State,
}

impl Default for State {
    fn default() -> Self {
        Self {
            lineup: match config::get().no_widgets {
                true => Lineup::parse(""),
                false => saved(),
            },
            calendar: Default::default(),
            pomodoro: Default::default(),
            stats: Default::default(),
            weather: Default::default(),
        }
    }
}

/// What the island must do in turn for a change the cards made.
pub(crate) enum Effect {
    /// A card took the left button: no chip is being pressed.
    Pressed,
    /// A Pomodoro phase ran out; the island says so with a passing Live Activity.
    PhaseOver(Phase),
    /// The lineup changed: an open tray springs to its new size.
    Refit,
}

impl State {
    /// The tray opened: the calendar shows this month again.
    pub fn opened(&mut self) {
        self.calendar.reset();
    }

    pub(crate) fn is_on(&self) -> bool {
        !self.lineup.is_empty()
    }

    /// The row's width, padding included.
    pub(crate) fn width(&self) -> f32 {
        let widgets = self.lineup.widgets();
        let cards: f32 = widgets.iter().map(|w| card_width(*w)).sum();
        let gaps = widgets.len().saturating_sub(1) as f32 * GAP;
        cards + gaps + 2.0 * PAD_X
    }

    /// The settings window changed the lineup (it is saved already).
    pub(crate) fn set_lineup(&mut self, lineup: Lineup) -> Option<Effect> {
        if config::get().no_widgets {
            return None;
        }
        if !lineup.contains(Widget::Pomodoro) {
            self.pomodoro.stop();
        }
        self.lineup = lineup;
        Some(Effect::Refit)
    }

    /// The cards, side by side with a hairline between each. `visible`: the row is on screen, so
    /// a card's redraws are worth booking.
    pub(crate) fn row(&self, visible: bool, cx: &mut Context<Island>) -> impl IntoElement + use<> {
        let widgets = self.lineup.widgets();
        let cards = widgets.iter().enumerate().flat_map(|(i, widget)| {
            let divider = (i > 0).then(|| {
                div()
                    .w(px(1.0))
                    .h(px(CARD * 0.7))
                    .mx(px((GAP - 1.0) / 2.0))
                    .flex_shrink_0()
                    .bg(rgba(DIVIDER))
                    .into_any_element()
            });
            let card = self.card(*widget, visible, cx);
            divider.into_iter().chain([div()
                .w(px(card_width(*widget)))
                .h(px(CARD))
                .flex_shrink_0()
                .child(card)
                .into_any_element()])
        });
        div()
            .w_full()
            .h(px(HEIGHT))
            .flex_shrink_0()
            .flex()
            .flex_row()
            .items_center()
            .justify_center()
            .px(px(PAD_X))
            .pt(px(PAD_TOP))
            .pb(px(PAD_BOTTOM))
            .children(cards.collect::<Vec<_>>())
    }

    fn card(&self, widget: Widget, visible: bool, cx: &mut Context<Island>) -> AnyElement {
        match widget {
            Widget::Calendar => self.calendar.card(cx),
            Widget::Pomodoro => self.pomodoro.card(cx),
            Widget::Stats => self.stats.card(visible, cx),
            Widget::Weather => self.weather.card(visible, cx),
        }
    }
}

/// The lineup as saved, or the default one if it never was.
pub fn saved() -> Lineup {
    SETTING
        .value()
        .map_or_else(Lineup::default, |saved| Lineup::parse(&saved))
}

pub fn save(lineup: &Lineup) -> windows::core::Result<()> {
    SETTING.set(&lineup.serialize())
}

fn card_width(widget: Widget) -> f32 {
    match widget {
        Widget::Calendar => calendar::WIDTH,
        Widget::Pomodoro => pomodoro::WIDTH,
        Widget::Stats => stats::WIDTH,
        Widget::Weather => weather::WIDTH,
    }
}

/// The left button going down on a card stays there: the island under it neither starts a drag
/// nor opens. For `cx.listener`.
fn keep_press(island: &mut Island, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Island>) {
    island.update_widgets(cx, |_, _, _| Some(Effect::Pressed));
    cx.stop_propagation();
}
