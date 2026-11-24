//! Droplets, after Droppy's: small self-contained features that can be switched on and off one by
//! one. Each is a type of its own implementing [`Droplet`] and a line in [`CATALOG`]; the island
//! owns one of each and hands them the moments they need. Which are on, and in what order, is the
//! lineup in `model::droplets`, set in the settings window. A droplet that is on and has a
//! [`Card`] shows it in the row across the top of Home; nothing runs while the row is off screen,
//! since each card books its own redraws only while it is drawn.

mod audio;
mod calendar;
mod pomodoro;
mod stats;
mod weather;

use std::any::Any;

use gpui::{AnyElement, Context, MouseDownEvent, Window, div, prelude::*, px, rgba};

use crate::autostart::RegString;
use crate::icons::Icon;
use crate::{Island, config};
use model::activity::{Activity, Key};
use model::droplets::Lineup;

/// Every droplet topsy has, in the order a new lineup offers them. Weather is off at first, since
/// finding where you are asks Windows for your location; Audio too, the row is wide enough already.
pub(crate) const CATALOG: [Entry; 5] = [
    Entry::new::<calendar::State>("calendar", "Calendar", Icon::Calendar, 0xe9453a, true),
    Entry::new::<pomodoro::State>("pomodoro", "Pomodoro", Icon::Timer, 0xf2811d, true),
    Entry::new::<stats::State>("stats", "System stats", Icon::Cpu, 0x48484f, true),
    Entry::new::<weather::State>("weather", "Weather", Icon::SunCloud, 0x2f8cff, false),
    Entry::new::<audio::State>("audio", "Audio", Icon::Headphones, 0x9b51e0, false),
];

/// One droplet. Every method has a default, so a droplet implements only the moments and the
/// surfaces it uses. It is made once, when the island starts, whether it is on or off, and lives
/// as long as the island: switching it off must leave nothing of it running.
pub(crate) trait Droplet: Any {
    /// The tray opened with it on, or it was switched on while the tray is open.
    fn opened(&mut self, _cx: &mut Context<Island>) {}

    /// Switched on in the settings. One that is on at launch isn't told; it waits for `opened`.
    fn switched_on(&mut self, _cx: &mut Context<Island>) {}

    /// Switched off: stop its threads, timers and listeners, and forget what they found.
    fn switched_off(&mut self, _cx: &mut Context<Island>) {}

    /// Its card in Home's row, if it has one.
    fn card(&self) -> Option<&dyn Card> {
        None
    }
}

/// A droplet's card in Home's row: as tall as every card, as wide as it says.
pub(crate) trait Card {
    fn width(&self) -> f32;

    /// `visible`: the row is on screen, so its redraws are worth booking.
    fn render(&self, visible: bool, cx: &mut Context<Island>) -> AnyElement;
}

/// A droplet as the catalog lists it.
pub(crate) struct Entry {
    /// Its name in the saved lineup: a lowercase word, never changed once shipped.
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    /// Its icon in the settings window, on a square of `tint`.
    pub(crate) icon: Icon,
    pub(crate) tint: u32,
    on_at_first: bool,
    make: fn() -> Box<dyn Droplet>,
}

impl Entry {
    const fn new<D: Droplet + Default>(
        id: &'static str,
        name: &'static str,
        icon: Icon,
        tint: u32,
        on: bool,
    ) -> Self {
        Entry {
            id,
            name,
            icon,
            tint,
            on_at_first: on,
            make: make::<D>,
        }
    }
}

fn make<D: Droplet + Default>() -> Box<dyn Droplet> {
    Box::new(D::default())
}

/// What the island must do in turn for a change a droplet made: the only ways a droplet reaches
/// beyond itself.
pub(crate) enum Effect {
    /// A card took the left button: no chip is being pressed.
    Pressed,
    /// Show a passing Live Activity.
    Activity(Key, Activity),
    /// The lineup changed: an open tray springs to its new size.
    Refit,
}

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

/// Every droplet, in the catalog's order, and which are on.
pub struct State {
    lineup: Lineup,
    droplets: Vec<Box<dyn Droplet>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            lineup: match config::get().no_widgets {
                true => Lineup::new([]),
                false => saved(),
            },
            droplets: CATALOG.iter().map(|entry| (entry.make)()).collect(),
        }
    }
}

impl State {
    /// The droplet of type `D`, on or off: its own tasks reach it through here.
    pub(crate) fn get_mut<D: Droplet>(&mut self) -> Option<&mut D> {
        (self.droplets.iter_mut())
            .find_map(|droplet| (droplet.as_mut() as &mut dyn Any).downcast_mut::<D>())
    }

    /// The droplets that are on, left to right.
    fn on(&self) -> impl Iterator<Item = &dyn Droplet> {
        self.lineup
            .ids()
            .iter()
            .filter_map(|id| CATALOG.iter().position(|entry| entry.id == *id))
            .map(|at| self.droplets[at].as_ref())
    }

    /// The tray opened.
    pub(crate) fn opened(&mut self, cx: &mut Context<Island>) {
        for (entry, droplet) in CATALOG.iter().zip(&mut self.droplets) {
            if self.lineup.contains(entry.id) {
                droplet.opened(cx);
            }
        }
    }

    /// The row has a card to show.
    pub(crate) fn has_row(&self) -> bool {
        self.on().any(|droplet| droplet.card().is_some())
    }

    /// The row's width, padding included.
    pub(crate) fn width(&self) -> f32 {
        let cards: Vec<_> = self.on().filter_map(|droplet| droplet.card()).collect();
        let widths: f32 = cards.iter().map(|card| card.width()).sum();
        let gaps = cards.len().saturating_sub(1) as f32 * GAP;
        widths + gaps + 2.0 * PAD_X
    }

    /// The settings window changed the lineup (it is saved already). `open`: the tray is open, so
    /// a droplet switched on is opened at once.
    pub(crate) fn set_lineup(
        &mut self,
        lineup: Lineup,
        open: bool,
        cx: &mut Context<Island>,
    ) -> Option<Effect> {
        if config::get().no_widgets {
            return None;
        }
        for (entry, droplet) in CATALOG.iter().zip(&mut self.droplets) {
            match (self.lineup.contains(entry.id), lineup.contains(entry.id)) {
                (true, false) => droplet.switched_off(cx),
                (false, true) => {
                    droplet.switched_on(cx);
                    if open {
                        droplet.opened(cx);
                    }
                }
                (true, true) | (false, false) => {}
            }
        }
        self.lineup = lineup;
        Some(Effect::Refit)
    }

    /// The cards, side by side with a hairline between each. `visible`: the row is on screen, so
    /// a card's redraws are worth booking.
    pub(crate) fn row(&self, visible: bool, cx: &mut Context<Island>) -> impl IntoElement + use<> {
        let cards: Vec<_> = self.on().filter_map(|droplet| droplet.card()).collect();
        let cards = cards.into_iter().enumerate().flat_map(|(i, card)| {
            let divider = (i > 0).then(|| {
                div()
                    .w(px(1.0))
                    .h(px(CARD * 0.7))
                    .mx(px((GAP - 1.0) / 2.0))
                    .flex_shrink_0()
                    .bg(rgba(DIVIDER))
                    .into_any_element()
            });
            divider.into_iter().chain([div()
                .w(px(card.width()))
                .h(px(CARD))
                .flex_shrink_0()
                .child(card.render(visible, cx))
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
}

/// Every droplet's id, in the catalog's order.
pub(crate) fn ids() -> [&'static str; CATALOG.len()] {
    CATALOG.map(|entry| entry.id)
}

/// The catalog's entry for `id`.
pub(crate) fn entry(id: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|entry| entry.id == id)
}

/// The lineup as saved, or the first one if it never was.
pub fn saved() -> Lineup {
    match SETTING.value() {
        Some(saved) => Lineup::parse(&saved, &ids()),
        None => Lineup::new(
            (CATALOG.iter())
                .filter(|entry| entry.on_at_first)
                .map(|entry| entry.id),
        ),
    }
}

pub fn save(lineup: &Lineup) -> windows::core::Result<()> {
    SETTING.set(&lineup.serialize())
}

/// The left button going down on a card stays there: the island under it neither starts a drag
/// nor opens. For `cx.listener`.
fn keep_press(island: &mut Island, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Island>) {
    island.update_droplets(cx, |_, _, _| Some(Effect::Pressed));
    cx.stop_propagation();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_droplet_is_found_by_its_type() {
        let mut state = State {
            lineup: Lineup::new([]),
            droplets: CATALOG.iter().map(|entry| (entry.make)()).collect(),
        };
        assert!(state.get_mut::<calendar::State>().is_some());
        assert!(state.get_mut::<pomodoro::State>().is_some());
        assert!(state.get_mut::<audio::State>().is_some());
    }

    #[test]
    fn catalog_ids_are_distinct_lowercase_words() {
        for (at, entry) in CATALOG.iter().enumerate() {
            assert!(
                !entry.id.is_empty() && entry.id.bytes().all(|b| b.is_ascii_lowercase()),
                "{}: a saved lineup splits on commas and trims spaces",
                entry.id
            );
            assert!(
                CATALOG[..at].iter().all(|before| before.id != entry.id),
                "{} is listed twice",
                entry.id
            );
        }
    }
}
