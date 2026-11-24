//! Stay Awake's rules: keep the computer and its display awake until switched off, or for one of
//! a few set lengths. Choosing a length while awake starts it over from now.

use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Length {
    #[default]
    UntilOff,
    Minutes(u32),
}

/// The lengths offered, left to right.
pub const LENGTHS: [Length; 4] = [
    Length::UntilOff,
    Length::Minutes(30),
    Length::Minutes(60),
    Length::Minutes(120),
];

impl Length {
    pub fn label(self) -> String {
        match self {
            Length::UntilOff => "∞".to_owned(),
            Length::Minutes(minutes) if minutes % 60 == 0 => format!("{}h", minutes / 60),
            Length::Minutes(minutes) => format!("{minutes}m"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Awake {
    #[default]
    Off,
    /// `ends` is `None` until switched off.
    On { ends: Option<Instant> },
}

#[derive(Debug, Default)]
pub struct StayAwake {
    length: Length,
    awake: Awake,
}

impl StayAwake {
    pub fn length(&self) -> Length {
        self.length
    }

    pub fn awake(&self) -> Awake {
        self.awake
    }

    pub fn is_on(&self) -> bool {
        match self.awake {
            Awake::On { .. } => true,
            Awake::Off => false,
        }
    }

    /// On for the chosen length from `now`, or off.
    pub fn toggle(&mut self, now: Instant) {
        self.awake = match self.awake {
            Awake::Off => self.from(now),
            Awake::On { .. } => Awake::Off,
        };
    }

    pub fn stop(&mut self) {
        self.awake = Awake::Off;
    }

    /// Choose `length`; while awake it starts over from `now`.
    pub fn choose(&mut self, length: Length, now: Instant) {
        self.length = length;
        if self.is_on() {
            self.awake = self.from(now);
        }
    }

    fn from(&self, now: Instant) -> Awake {
        let ends = match self.length {
            Length::UntilOff => None,
            Length::Minutes(minutes) => Some(now + Duration::from_secs(u64::from(minutes) * 60)),
        };
        Awake::On { ends }
    }

    /// The time left of a timed stay.
    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        match self.awake {
            Awake::On { ends: Some(ends) } => Some(ends.saturating_duration_since(now)),
            Awake::On { ends: None } | Awake::Off => None,
        }
    }

    /// How long until the countdown next reads a new second.
    pub fn until_next_second(&self, now: Instant) -> Option<Duration> {
        let left = self.remaining(now)?;
        if left.is_zero() {
            return None;
        }
        let into = Duration::from_nanos(u64::from(left.subsec_nanos()));
        Some(if into.is_zero() {
            Duration::from_secs(1)
        } else {
            into
        })
    }

    /// Switch off a timed stay whose time is up; true if it did.
    pub fn finish(&mut self, now: Instant) -> bool {
        let Awake::On { ends: Some(ends) } = self.awake else {
            return false;
        };
        if now < ends {
            return false;
        }
        self.awake = Awake::Off;
        true
    }
}

/// `h:mm:ss`, or `mm:ss` under an hour, counting a started second as whole.
pub fn clock(left: Duration) -> String {
    let seconds = left.as_secs() + u64::from(left.subsec_nanos() > 0);
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    match hours {
        0 => format!("{minutes:02}:{seconds:02}"),
        _ => format!("{hours}:{minutes:02}:{seconds:02}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    #[test]
    fn until_off_stays_on_until_toggled() {
        let t = Instant::now();
        let mut stay = StayAwake::default();
        assert!(!stay.is_on());
        stay.toggle(t);
        assert_eq!(stay.awake(), Awake::On { ends: None });
        assert_eq!(stay.remaining(t), None);
        assert_eq!(stay.until_next_second(t), None, "nothing to redraw");
        assert!(!stay.finish(t + Duration::from_secs(1 << 30)));
        assert!(stay.is_on());
        stay.toggle(t);
        assert_eq!(stay.awake(), Awake::Off);
    }

    #[test]
    fn a_timed_stay_counts_down_and_switches_off() {
        let t = Instant::now();
        let mut stay = StayAwake::default();
        stay.choose(Length::Minutes(30), t);
        assert!(!stay.is_on(), "choosing a length doesn't switch it on");
        stay.toggle(t);
        assert_eq!(clock(stay.remaining(t).unwrap()), "30:00");
        assert_eq!(clock(stay.remaining(t + SECOND).unwrap()), "29:59");
        let ends = t + Duration::from_secs(30 * 60);
        assert!(!stay.finish(ends - Duration::from_millis(1)));
        assert!(stay.finish(ends));
        assert_eq!(stay.awake(), Awake::Off);
        assert_eq!(stay.length(), Length::Minutes(30), "the length is kept");
        assert!(!stay.finish(ends));
    }

    #[test]
    fn choosing_while_awake_starts_over() {
        let t = Instant::now();
        let mut stay = StayAwake::default();
        stay.choose(Length::Minutes(60), t);
        stay.toggle(t);
        let later = t + Duration::from_secs(600);
        stay.choose(Length::Minutes(120), later);
        assert_eq!(
            stay.awake(),
            Awake::On {
                ends: Some(later + Duration::from_secs(7200))
            }
        );
        stay.choose(Length::UntilOff, later);
        assert_eq!(stay.awake(), Awake::On { ends: None });
        stay.stop();
        assert!(!stay.is_on());
    }

    #[test]
    fn the_redraw_lands_on_the_next_second() {
        let t = Instant::now();
        let mut stay = StayAwake::default();
        stay.choose(Length::Minutes(30), t);
        assert_eq!(stay.until_next_second(t), None);
        stay.toggle(t);
        assert_eq!(stay.until_next_second(t), Some(SECOND));
        let at = t + Duration::from_millis(300);
        assert_eq!(stay.until_next_second(at), Some(Duration::from_millis(700)));
        assert_eq!(stay.until_next_second(t + Duration::from_secs(1800)), None);
    }

    #[test]
    fn labels_and_clock() {
        let labels: Vec<_> = LENGTHS.iter().map(|length| length.label()).collect();
        assert_eq!(labels, ["∞", "30m", "1h", "2h"]);
        assert_eq!(clock(Duration::from_secs(7200)), "2:00:00");
        assert_eq!(clock(Duration::from_millis(3_599_001)), "1:00:00");
        assert_eq!(clock(Duration::from_secs(3599)), "59:59");
        assert_eq!(clock(Duration::ZERO), "00:00");
    }
}
