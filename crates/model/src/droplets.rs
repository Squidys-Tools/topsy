//! Which droplets are on, and in what order: the lineup the settings window edits, kept as one
//! comma-separated value of their ids. The droplets themselves are the app's (`src/droplets/`);
//! here they are only their ids, each a lowercase word with no comma.

/// The droplets switched on, left to right. Each appears at most once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lineup(Vec<&'static str>);

impl Lineup {
    /// A lineup of `ids`, in their order; repeats are skipped.
    pub fn new(ids: impl IntoIterator<Item = &'static str>) -> Self {
        let mut lineup = Vec::new();
        for id in ids {
            if !lineup.contains(&id) {
                lineup.push(id);
            }
        }
        Lineup(lineup)
    }

    /// Read a saved lineup against `known`, every droplet this topsy has. Unknown ids (from a
    /// newer topsy) and repeats are skipped; an empty value is a lineup with every droplet off.
    pub fn parse(saved: &str, known: &[&'static str]) -> Self {
        let ids = saved.split(',').map(str::trim);
        Lineup::new(ids.filter_map(|id| known.iter().copied().find(|k| *k == id)))
    }

    pub fn serialize(&self) -> String {
        self.0.join(",")
    }

    pub fn ids(&self) -> &[&'static str] {
        &self.0
    }

    pub fn contains(&self, id: &str) -> bool {
        self.0.contains(&id)
    }

    /// Switch `id` on (at the right end) or off.
    pub fn set(&mut self, id: &'static str, on: bool) {
        self.0.retain(|kept| *kept != id);
        if on {
            self.0.push(id);
        }
    }

    /// Move a droplet that is on to `index` (past the end: the last place); one that is off stays
    /// off.
    pub fn move_to(&mut self, id: &str, index: usize) {
        let Some(at) = self.0.iter().position(|kept| *kept == id) else {
            return;
        };
        let moved = self.0.remove(at);
        self.0.insert(index.min(self.0.len()), moved);
    }

    /// Every droplet of `known`, those on first in their order, then those off in `known`'s: the
    /// settings' widget pills.
    pub fn rows(&self, known: &[&'static str]) -> Vec<(&'static str, bool)> {
        let off = known.iter().filter(|id| !self.contains(id));
        (self.0.iter().map(|id| (*id, true)))
            .chain(off.map(|id| (*id, false)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KNOWN: [&str; 5] = ["calendar", "pomodoro", "stats", "weather", "audio"];

    #[test]
    fn a_saved_lineup_round_trips_and_skips_what_it_does_not_know() {
        let lineup = Lineup::parse("weather, calendar,clock,weather,audio", &KNOWN);
        assert_eq!(lineup.ids(), ["weather", "calendar", "audio"]);
        assert_eq!(lineup.serialize(), "weather,calendar,audio");
        assert_eq!(Lineup::parse(&lineup.serialize(), &KNOWN), lineup);
        assert!(
            Lineup::parse("", &KNOWN).ids().is_empty(),
            "every droplet switched off"
        );
        let fresh = Lineup::new(["calendar", "stats", "calendar"]);
        assert_eq!(fresh.ids(), ["calendar", "stats"]);
        assert_eq!(Lineup::parse(&fresh.serialize(), &KNOWN), fresh);
    }

    #[test]
    fn switching_on_adds_at_the_end_and_off_removes() {
        let mut lineup = Lineup::parse("stats", &KNOWN);
        lineup.set("calendar", true);
        lineup.set("calendar", true);
        assert_eq!(lineup.ids(), ["stats", "calendar"]);
        lineup.set("stats", false);
        assert_eq!(lineup.ids(), ["calendar"]);
    }

    #[test]
    fn moving_puts_a_droplet_in_place_and_leaves_off_ones_off() {
        let mut lineup = Lineup::parse("calendar,pomodoro,stats", &KNOWN);
        lineup.move_to("stats", 0);
        assert_eq!(lineup.serialize(), "stats,calendar,pomodoro");
        lineup.move_to("stats", 1);
        assert_eq!(lineup.serialize(), "calendar,stats,pomodoro");
        lineup.move_to("calendar", 9);
        assert_eq!(
            lineup.serialize(),
            "stats,pomodoro,calendar",
            "past the end"
        );
        lineup.move_to("weather", 0);
        assert_eq!(
            lineup.serialize(),
            "stats,pomodoro,calendar",
            "an off droplet"
        );
    }

    #[test]
    fn rows_list_the_lineup_then_the_rest() {
        let rows = Lineup::parse("stats,calendar", &KNOWN).rows(&KNOWN);
        assert_eq!(
            rows,
            [
                ("stats", true),
                ("calendar", true),
                ("pomodoro", false),
                ("weather", false),
                ("audio", false),
            ]
        );
    }
}
