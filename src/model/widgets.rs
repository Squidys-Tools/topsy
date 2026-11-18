//! Which shelf widgets the tray shows, and in what order: the lineup the settings window edits,
//! kept as one comma-separated value.

/// Every widget topsy has, in the order a new lineup offers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Widget {
    Calendar,
    Pomodoro,
    Stats,
    Weather,
}

impl Widget {
    pub const ALL: [Widget; 4] = [
        Widget::Calendar,
        Widget::Pomodoro,
        Widget::Stats,
        Widget::Weather,
    ];

    /// Its name in the saved lineup.
    fn key(self) -> &'static str {
        match self {
            Widget::Calendar => "calendar",
            Widget::Pomodoro => "pomodoro",
            Widget::Stats => "stats",
            Widget::Weather => "weather",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Widget::Calendar => "Calendar",
            Widget::Pomodoro => "Pomodoro",
            Widget::Stats => "System stats",
            Widget::Weather => "Weather",
        }
    }
}

/// The widgets switched on, left to right. Each appears at most once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lineup(Vec<Widget>);

impl Default for Lineup {
    /// Weather is left off: finding where you are asks Windows for your location.
    fn default() -> Self {
        Lineup(vec![Widget::Calendar, Widget::Pomodoro, Widget::Stats])
    }
}

impl Lineup {
    /// Read a saved lineup. Unknown names (from a newer topsy) and repeats are skipped; an empty
    /// value is a lineup with every widget off.
    pub fn parse(saved: &str) -> Self {
        let mut lineup = Vec::new();
        for name in saved.split(',').map(str::trim) {
            if let Some(widget) = Widget::ALL.into_iter().find(|w| w.key() == name)
                && !lineup.contains(&widget)
            {
                lineup.push(widget);
            }
        }
        Lineup(lineup)
    }

    pub fn serialize(&self) -> String {
        let keys: Vec<_> = self.0.iter().map(|w| w.key()).collect();
        keys.join(",")
    }

    pub fn widgets(&self) -> &[Widget] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(&self, widget: Widget) -> bool {
        self.0.contains(&widget)
    }

    /// Switch `widget` on (at the right end) or off.
    pub fn set(&mut self, widget: Widget, on: bool) {
        self.0.retain(|w| *w != widget);
        if on {
            self.0.push(widget);
        }
    }

    /// Move a widget that is on one place left (`-1`) or right (`1`); at either end it stays.
    pub fn shift(&mut self, widget: Widget, by: isize) {
        let Some(at) = self.0.iter().position(|w| *w == widget) else {
            return;
        };
        let to = at.saturating_add_signed(by);
        if to < self.0.len() {
            self.0.swap(at, to);
        }
    }

    /// Every widget, those on first in their order, then those off: the settings window's rows.
    pub fn rows(&self) -> Vec<(Widget, bool)> {
        let off = Widget::ALL.into_iter().filter(|w| !self.contains(*w));
        (self.0.iter().map(|w| (*w, true)))
            .chain(off.map(|w| (w, false)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_lineup_round_trips_and_skips_what_it_does_not_know() {
        let lineup = Lineup::parse("weather, calendar,clock,weather");
        assert_eq!(lineup.widgets(), [Widget::Weather, Widget::Calendar]);
        assert_eq!(lineup.serialize(), "weather,calendar");
        assert_eq!(Lineup::parse(&lineup.serialize()), lineup);
        assert!(Lineup::parse("").is_empty(), "every widget switched off");
        assert_eq!(
            Lineup::parse(&Lineup::default().serialize()),
            Lineup::default()
        );
    }

    #[test]
    fn switching_on_adds_at_the_end_and_off_removes() {
        let mut lineup = Lineup::parse("stats");
        lineup.set(Widget::Calendar, true);
        lineup.set(Widget::Calendar, true);
        assert_eq!(lineup.widgets(), [Widget::Stats, Widget::Calendar]);
        lineup.set(Widget::Stats, false);
        assert_eq!(lineup.widgets(), [Widget::Calendar]);
    }

    #[test]
    fn shifting_moves_one_place_and_stops_at_the_ends() {
        let mut lineup = Lineup::parse("calendar,pomodoro,stats");
        lineup.shift(Widget::Stats, -1);
        assert_eq!(lineup.serialize(), "calendar,stats,pomodoro");
        lineup.shift(Widget::Calendar, -1);
        lineup.shift(Widget::Pomodoro, 1);
        assert_eq!(lineup.serialize(), "calendar,stats,pomodoro");
        lineup.shift(Widget::Weather, 1);
        assert_eq!(
            lineup.serialize(),
            "calendar,stats,pomodoro",
            "an off widget"
        );
    }

    #[test]
    fn rows_list_the_lineup_then_the_rest() {
        let rows = Lineup::parse("stats,calendar").rows();
        assert_eq!(
            rows,
            [
                (Widget::Stats, true),
                (Widget::Calendar, true),
                (Widget::Pomodoro, false),
                (Widget::Weather, false),
            ]
        );
    }
}
