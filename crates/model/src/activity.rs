//! Live Activities: what is going on right now, held in a small notch while it lasts. A download
//! stays while it grows, or [`STALL`] after it stops (paused or failed); a finished one, or earbuds
//! coming and going, or a Pomodoro phase running out, passes after [`LINGER`]. Earbuds connecting
//! first show as a card for [`CARD`], then shrink to the row.
//! The notch shows the [`SHOWN`] at the front, newest first.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::headset::{Form, Power};
use crate::hud::fell_low;
use crate::pomodoro::Phase;

/// How many show at once, side by side.
pub const SHOWN: usize = 2;
/// How long a passing activity stays.
pub const LINGER: Duration = Duration::from_millis(3000);
/// How long a connected headset's card shows before it shrinks to its row.
pub const CARD: Duration = Duration::from_millis(3000);
/// How long a download that stopped growing stays: the browser keeps the partial file when it
/// pauses or fails, so it never says the download ended.
pub const STALL: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Download(u64),
    /// An audio device, by its container id, so its several endpoints count once.
    Device(String),
    Timer,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Activity {
    Downloading {
        name: String,
        bytes: u64,
    },
    Downloaded {
        name: String,
        path: PathBuf,
    },
    Connected {
        name: String,
        form: Form,
        power: Option<Power>,
    },
    Disconnected {
        name: String,
        form: Form,
    },
    PhaseOver {
        ended: Phase,
    },
}

impl Activity {
    /// How long it stays after its last change.
    fn lasts(&self) -> Duration {
        match self {
            Activity::Downloading { .. } => STALL,
            Activity::Downloaded { .. }
            | Activity::Connected { .. }
            | Activity::Disconnected { .. }
            | Activity::PhaseOver { .. } => LINGER,
        }
    }

    /// A headset connecting opens with its card.
    fn opens_with_card(&self) -> bool {
        match self {
            Activity::Connected { .. } => true,
            Activity::Downloading { .. }
            | Activity::Downloaded { .. }
            | Activity::Disconnected { .. }
            | Activity::PhaseOver { .. } => false,
        }
    }
}

#[derive(Debug)]
struct Entry {
    key: Key,
    activity: Activity,
    /// While its card shows; it lingers from there.
    card_until: Option<Instant>,
    until: Instant,
}

impl Entry {
    fn linger_from(&self, now: Instant) -> Instant {
        self.card_until.map_or(now, |card| card.max(now))
    }
}

#[derive(Default, Debug)]
pub struct Activities {
    /// Front first.
    entries: Vec<Entry>,
}

impl Activities {
    /// Show `activity` under `key`. A new one, or one that changed what it is (a download that
    /// finished), goes to the front; a download that only grew keeps its place, and a card
    /// keeps showing.
    pub fn set(&mut self, key: Key, activity: Activity, now: Instant) {
        let card = activity.opens_with_card().then_some(now + CARD);
        self.place(key, activity, card, now);
    }

    fn place(&mut self, key: Key, activity: Activity, card: Option<Instant>, now: Instant) {
        let at = self.entries.iter().position(|e| e.key == key);
        if let Some(at) = at {
            let entry = &mut self.entries[at];
            if std::mem::discriminant(&entry.activity) == std::mem::discriminant(&activity) {
                entry.until = entry.linger_from(now) + activity.lasts();
                entry.activity = activity;
                return;
            }
            self.entries.remove(at);
        }
        let until = card.unwrap_or(now) + activity.lasts();
        self.entries.insert(
            0,
            Entry {
                key,
                activity,
                card_until: card,
                until,
            },
        );
    }

    /// A connected device's battery moved from `was` to `power`: a showing activity updates in
    /// place, and a gone one comes back, without its card, only when its lowest battery falls
    /// below a low mark. True if anything changed.
    pub fn battery(
        &mut self,
        key: Key,
        (name, form): (String, Form),
        was: Option<Power>,
        power: Power,
        now: Instant,
    ) -> bool {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.key == key)
            && let Activity::Connected { power: shown, .. } = &mut entry.activity
        {
            *shown = Some(power);
            entry.until = entry.linger_from(now) + LINGER;
            return true;
        }
        let low = match (was.and_then(Power::lowest), power.lowest()) {
            (Some(was), Some(now)) => fell_low(was, now),
            _ => false,
        };
        if low {
            let power = Some(power);
            self.place(key, Activity::Connected { name, form, power }, None, now);
        }
        low
    }

    /// Take one away at once (a cancelled download).
    pub fn remove(&mut self, key: &Key) {
        self.entries.retain(|e| &e.key != key);
    }

    /// Drop the ones whose time is up and close the cards whose time is up; true if any went.
    pub fn expire(&mut self, now: Instant) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.until > now);
        for entry in &mut self.entries {
            entry.card_until = entry.card_until.filter(|card| *card > now);
        }
        self.entries.len() != before
    }

    /// When the next one is due to go, or its card to close.
    pub fn next_expiry(&self) -> Option<Instant> {
        self.entries
            .iter()
            .flat_map(|e| [Some(e.until), e.card_until])
            .flatten()
            .min()
    }

    /// The card showing, if one is: it has the notch to itself.
    pub fn card(&self) -> Option<(&Key, &Activity)> {
        self.entries
            .iter()
            .find(|e| e.card_until.is_some())
            .map(|e| (&e.key, &e.activity))
    }

    pub fn shown(&self) -> impl Iterator<Item = (&Key, &Activity)> {
        self.entries
            .iter()
            .take(SHOWN)
            .map(|e| (&e.key, &e.activity))
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn downloading(name: &str, bytes: u64) -> Activity {
        Activity::Downloading {
            name: name.into(),
            bytes,
        }
    }

    fn names(a: &Activities) -> Vec<String> {
        a.shown()
            .map(|(_, a)| match a {
                Activity::Downloading { name, .. }
                | Activity::Downloaded { name, .. }
                | Activity::Connected { name, .. }
                | Activity::Disconnected { name, .. } => name.clone(),
                Activity::PhaseOver { ended } => format!("{ended:?}"),
            })
            .collect()
    }

    #[test]
    fn a_download_stays_while_it_grows_then_lingers() {
        let t = Instant::now();
        let mut a = Activities::default();
        for s in 0..10 {
            let at = t + Duration::from_secs(s * 59);
            assert!(!a.expire(at));
            a.set(Key::Download(1), downloading("a.zip", s), at);
        }

        let done = t + Duration::from_secs(600);
        a.set(
            Key::Download(1),
            Activity::Downloaded {
                name: "a.zip".into(),
                path: "a.zip".into(),
            },
            done,
        );
        assert_eq!(a.next_expiry(), Some(done + LINGER));
        assert!(!a.expire(done + LINGER - Duration::from_millis(1)));
        assert!(a.expire(done + LINGER));
        assert!(a.is_empty());

        a.set(Key::Download(2), downloading("b.zip", 0), done);
        assert!(!a.expire(done + STALL - Duration::from_millis(1)));
        assert!(a.expire(done + STALL));
    }

    #[test]
    fn newest_first_two_at_a_time_and_growth_keeps_its_place() {
        let t = Instant::now();
        let mut a = Activities::default();
        a.set(Key::Download(1), downloading("one", 0), t);
        a.set(Key::Download(2), downloading("two", 0), t);
        a.set(Key::Download(3), downloading("three", 0), t);
        assert_eq!(names(&a), ["three", "two"]);

        a.set(Key::Download(1), downloading("one", 500), t);
        assert_eq!(names(&a), ["three", "two"]);

        a.set(
            Key::Download(1),
            Activity::Downloaded {
                name: "one".into(),
                path: "one".into(),
            },
            t,
        );
        assert_eq!(names(&a), ["one", "three"]);

        a.remove(&Key::Download(3));
        assert_eq!(names(&a), ["one", "two"]);
    }

    fn connected(power: Option<Power>) -> Activity {
        Activity::Connected {
            name: "Buds".into(),
            form: Form::Buds,
            power,
        }
    }

    fn buds() -> (String, Form) {
        ("Buds".into(), Form::Buds)
    }

    #[test]
    fn a_device_counts_once_opens_with_its_card_and_a_repeat_keeps_it() {
        let t = Instant::now();
        let mut a = Activities::default();
        let key = Key::Device("{c0}".into());
        a.set(key.clone(), connected(None), t);
        assert!(a.card().is_some());
        assert_eq!(a.next_expiry(), Some(t + CARD));
        let later = t + Duration::from_secs(2);
        a.set(key.clone(), connected(None), later);
        assert_eq!(names(&a), ["Buds"]);
        assert!(a.battery(key.clone(), buds(), None, Power::Single(80), later));
        assert_eq!(
            a.card(),
            Some((&key, &connected(Some(Power::Single(80))))),
            "a late battery updates the card in place"
        );
        assert!(!a.expire(t + CARD));
        assert_eq!(a.card(), None, "the card closes into the row");
        assert_eq!(names(&a), ["Buds"]);
        assert_eq!(a.next_expiry(), Some(t + CARD + LINGER));
        assert!(a.expire(t + CARD + LINGER));
    }

    #[test]
    fn a_connected_device_comes_back_without_its_card_only_when_its_battery_falls_low() {
        let t = Instant::now();
        let mut a = Activities::default();
        let key = Key::Device("{c0}".into());
        let single = |p| Some(Power::Single(p));
        let mut battery = |was, p| a.battery(key.clone(), buds(), was, Power::Single(p), t);
        assert!(!battery(None, 50), "a first reading after it passed");
        assert!(!battery(single(50), 40));
        assert!(!battery(single(30), 20));
        assert!(battery(single(20), 10));
        assert_eq!(names(&a), ["Buds"]);
        assert_eq!(a.card(), None);
        a.expire(t + LINGER);
        let split = |left, right| Power::Split {
            left: Some(left),
            right: Some(right),
            case: Some(90),
        };
        let at = t + LINGER;
        assert!(!a.battery(key.clone(), buds(), Some(split(30, 25)), split(30, 21), at));
        assert!(a.battery(key, buds(), Some(split(30, 21)), split(30, 19), at));
    }

    #[test]
    fn a_phase_over_passes_and_the_next_replaces_it() {
        let t = Instant::now();
        let mut a = Activities::default();
        let over = |ended| Activity::PhaseOver { ended };
        a.set(Key::Timer, over(Phase::Focus), t);
        a.set(Key::Timer, over(Phase::Break), t + LINGER / 2);
        assert_eq!(names(&a), ["Break"]);
        assert!(!a.expire(t + LINGER));
        assert!(a.expire(t + LINGER / 2 + LINGER));
    }
}
