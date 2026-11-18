//! Shelf widgets, after Droppy's: a row of small cards (a calendar, a Pomodoro timer, system
//! stats, the weather) across the top of a tray opened to look. Which show, and in what order,
//! is the lineup in `model::widgets`, set in the settings window. Nothing here runs while the row
//! is off screen: each card books its own redraws only while it is drawn.

mod calendar;
mod pomodoro;
mod stats;
mod weather;

use gpui::{AnyElement, Context, div, prelude::*, px, rgba};

use crate::autostart::RegString;
use crate::model::widgets::{Lineup, Widget};
use crate::{Island, Mode, config};

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

impl State {
    /// The tray opened: the calendar shows this month again.
    pub fn opened(&mut self) {
        self.calendar.reset();
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

fn width(widget: Widget) -> f32 {
    match widget {
        Widget::Calendar => calendar::WIDTH,
        Widget::Pomodoro => pomodoro::WIDTH,
        Widget::Stats => stats::WIDTH,
        Widget::Weather => weather::WIDTH,
    }
}

impl Island {
    pub(crate) fn widgets_on(&self) -> bool {
        !self.widgets.lineup.is_empty()
    }

    /// The row is on screen: a card's redraws are worth booking.
    pub(crate) fn widgets_visible(&self) -> bool {
        self.mode == Mode::Tray && self.sections().widgets
    }

    /// The row's width, padding included.
    pub(crate) fn widgets_width(&self) -> f32 {
        let widgets = self.widgets.lineup.widgets();
        let cards: f32 = widgets.iter().map(|w| width(*w)).sum();
        let gaps = widgets.len().saturating_sub(1) as f32 * GAP;
        cards + gaps + 2.0 * PAD_X
    }

    /// The settings window changed the lineup (it is saved already).
    pub(crate) fn set_lineup(&mut self, lineup: Lineup, cx: &mut Context<Self>) {
        if config::get().no_widgets {
            return;
        }
        if !lineup.contains(Widget::Pomodoro) {
            self.widgets.pomodoro.stop();
        }
        self.widgets.lineup = lineup;
        if self.mode == Mode::Tray {
            self.set_mode(Mode::Tray, cx);
        }
    }

    /// The cards, side by side with a hairline between each.
    pub(crate) fn widgets_row(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let widgets = self.widgets.lineup.widgets();
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
            let card = self.card(*widget, cx);
            divider.into_iter().chain([div()
                .w(px(width(*widget)))
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

    fn card(&self, widget: Widget, cx: &mut Context<Self>) -> AnyElement {
        match widget {
            Widget::Calendar => self.calendar_card(cx),
            Widget::Pomodoro => self.pomodoro_card(cx),
            Widget::Stats => self.stats_card(cx),
            Widget::Weather => self.weather_card(cx),
        }
    }
}
