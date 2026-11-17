//! Live Activities: what is going on right now, held in a small notch while it lasts. A download
//! stays until it ends; a finished one, or earbuds coming and going, passes after [`LINGER`].
//! The notch shows the [`SHOWN`] at the front, newest first.

use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How many show at once, side by side.
pub const SHOWN: usize = 2;
/// How long a passing activity stays.
pub const LINGER: Duration = Duration::from_millis(3000);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Download(u64),
    /// An audio device, by its container id, so its several endpoints count once.
    Device(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Activity {
    Downloading { name: String, bytes: u64 },
    Downloaded { name: String, path: PathBuf },
    Connected { name: String, battery: Option<u8> },
    Disconnected { name: String },
}

impl Activity {
    /// Shown for a moment and then gone, rather than for as long as it runs.
    fn passing(&self) -> bool {
        !matches!(self, Activity::Downloading { .. })
    }
}

#[derive(Debug)]
struct Entry {
    key: Key,
    activity: Activity,
    until: Option<Instant>,
}

#[derive(Default, Debug)]
pub struct Activities {
    /// Front first.
    entries: Vec<Entry>,
}

impl Activities {
    /// Show `activity` under `key`. A new one, or one that changed what it is (a download that
    /// finished), goes to the front; a download that only grew keeps its place.
    pub fn set(&mut self, key: Key, activity: Activity, now: Instant) {
        let until = activity.passing().then(|| now + LINGER);
        let at = self.entries.iter().position(|e| e.key == key);
        if let Some(at) = at {
            let entry = &mut self.entries[at];
            if std::mem::discriminant(&entry.activity) == std::mem::discriminant(&activity) {
                entry.activity = activity;
                entry.until = until;
                return;
            }
            self.entries.remove(at);
        }
        self.entries.insert(
            0,
            Entry {
                key,
                activity,
                until,
            },
        );
    }

    /// Take one away at once (a cancelled download).
    pub fn remove(&mut self, key: &Key) {
        self.entries.retain(|e| &e.key != key);
    }

    /// Drop the passing ones whose time is up; true if any went.
    pub fn expire(&mut self, now: Instant) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|e| e.until.is_none_or(|until| until > now));
        self.entries.len() != before
    }

    /// When the next passing one is due to go.
    pub fn next_expiry(&self) -> Option<Instant> {
        self.entries.iter().filter_map(|e| e.until).min()
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
                | Activity::Disconnected { name } => name.clone(),
            })
            .collect()
    }

    #[test]
    fn a_download_stays_until_it_ends_then_lingers() {
        let t = Instant::now();
        let mut a = Activities::default();
        a.set(Key::Download(1), downloading("a.zip", 0), t);
        assert!(!a.expire(t + Duration::from_secs(600)));
        assert_eq!(a.next_expiry(), None);

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

    #[test]
    fn a_device_counts_once_and_a_repeat_restarts_its_linger() {
        let t = Instant::now();
        let mut a = Activities::default();
        let key = Key::Device("{c0}".into());
        let connected = Activity::Connected {
            name: "Buds".into(),
            battery: None,
        };
        a.set(key.clone(), connected.clone(), t);
        let later = t + Duration::from_secs(2);
        a.set(key.clone(), connected, later);
        assert_eq!(names(&a), ["Buds"]);
        assert!(!a.expire(t + LINGER));
        assert!(a.expire(later + LINGER));
    }
}
