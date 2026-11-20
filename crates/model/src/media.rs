//! Now Playing: what the system's current media session reports, reduced to what the island
//! shows. The glue (`src/now_playing/session.rs`) turns each WinRT event into an [`Event`]; nothing here
//! ticks, so a song playing costs nothing until the app reports a change. The position is
//! carried forward from the app's last report when it is drawn, at the moment it is drawn.

use std::time::{Duration, Instant};

/// Longest title or artist kept, in characters. The row truncates to its width anyway; this
/// only stops an app's runaway string (a page title, a whole lyric) from being shaped at all.
const MAX_CHARS: usize = 120;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Track {
    pub title: String,
    /// Empty when the app gives none, or gives the title again.
    pub artist: String,
}

impl Track {
    pub fn new(title: &str, artist: &str) -> Self {
        let title = clean(title);
        let mut artist = clean(artist);
        if artist == title {
            artist.clear();
        }
        Self { title, artist }
    }
}

/// One line, single-spaced, at most [`MAX_CHARS`] (an ellipsis marks the cut).
fn clean(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let line = words.join(" ");
    match line.char_indices().nth(MAX_CHARS) {
        Some((cut, _)) => format!("{}…", line[..cut].trim_end()),
        None => line,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    /// Stopped, closed, opening or changing: nothing to show.
    #[default]
    Idle,
    Playing,
    Paused,
}

/// Which buttons the app currently accepts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Controls {
    pub previous: bool,
    pub play_pause: bool,
    pub next: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// There is no current session any more.
    Gone,
    Track(Track),
    Playback {
        status: Status,
        controls: Controls,
        /// When the app reported it.
        at: Instant,
    },
    /// Where the track is; `None` when the app gives no length (a live stream).
    Timeline(Option<Timeline>),
    /// The shown session's app, while there are others to switch to.
    Source(Option<String>),
}

/// The app's report of where the track was at `at`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeline {
    pub position: Duration,
    pub length: Duration,
    pub at: Instant,
}

/// The progress bar: how far in, out of how long.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub elapsed: Duration,
    pub length: Duration,
}

impl Progress {
    pub fn fraction(self) -> f32 {
        (self.elapsed.as_secs_f32() / self.length.as_secs_f32()).clamp(0.0, 1.0)
    }

    pub fn remaining(self) -> Duration {
        self.length.saturating_sub(self.elapsed)
    }

    /// How long until the elapsed time reads a new second: when the times next need a redraw.
    /// A hair past it, so a timer that fires on time doesn't land just short.
    pub fn until_next_second(self) -> Duration {
        let into = Duration::from_nanos(self.elapsed.subsec_nanos().into());
        Duration::from_secs(1) - into + Duration::from_millis(5)
    }
}

/// A duration as a player shows it: `1:29`, or `1:02:09` past the hour.
pub fn clock(d: Duration) -> String {
    let s = d.as_secs();
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// The tray's row: what is playing and which buttons work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card<'a> {
    pub title: &'a str,
    pub artist: Option<&'a str>,
    pub playing: bool,
    pub controls: Controls,
    /// The app playing, while there are others to switch to.
    pub source: Option<&'a str>,
}

#[derive(Debug, Default)]
pub struct NowPlaying {
    track: Option<Track>,
    status: Status,
    controls: Controls,
    timeline: Option<Timeline>,
    source: Option<String>,
}

impl NowPlaying {
    /// Take in an event; true when what the island shows changed.
    pub fn apply(&mut self, event: Event) -> bool {
        // The hint is drawn from the card, so the card alone tells.
        let before = self.card().map(Card::owned);
        match event {
            Event::Gone => *self = Self::default(),
            Event::Track(track) => {
                if self.track.as_ref() != Some(&track) {
                    self.timeline = None;
                }
                self.track = Some(track);
            }
            Event::Playback {
                status,
                controls,
                at,
            } => {
                // Apps don't always report the position when playback stops or starts: hold it
                // where it got to, and carry it forward from now.
                if (status == Status::Playing) != (self.status == Status::Playing)
                    && let Some(t) = self.timeline
                {
                    self.timeline = Some(Timeline {
                        position: self.position(t, at),
                        at,
                        ..t
                    });
                }
                self.status = status;
                self.controls = controls;
            }
            Event::Timeline(timeline) => {
                // A report from before the held position is stale.
                let fresh = match (timeline, self.timeline) {
                    (Some(new), Some(old)) => new.at >= old.at,
                    _ => true,
                };
                let changed = fresh && self.timeline != timeline;
                if changed {
                    self.timeline = timeline;
                }
                return changed && self.card().is_some();
            }
            Event::Source(source) => self.source = source,
        }
        before != self.card().map(Card::owned)
    }

    fn position(&self, t: Timeline, now: Instant) -> Duration {
        let ran = match self.status {
            Status::Playing => now.saturating_duration_since(t.at),
            _ => Duration::ZERO,
        };
        (t.position + ran).min(t.length)
    }

    /// Where the card's track is at `now`, when the app says how long it is.
    pub fn progress(&self, now: Instant) -> Option<Progress> {
        self.card()?;
        let t = self.timeline.filter(|t| !t.length.is_zero())?;
        Some(Progress {
            elapsed: self.position(t, now),
            length: t.length,
        })
    }

    /// Shown in the tray while a titled track is playing or paused; stopped shows nothing.
    pub fn card(&self) -> Option<Card<'_>> {
        let track = self.track.as_ref().filter(|t| !t.title.is_empty())?;
        if self.status == Status::Idle {
            return None;
        }
        Some(Card {
            title: &track.title,
            artist: (!track.artist.is_empty()).then_some(track.artist.as_str()),
            playing: self.status == Status::Playing,
            controls: self.controls,
            source: self.source.as_deref(),
        })
    }

    /// The peek's hint: the title, only while it is actually playing.
    pub fn hint(&self) -> Option<&str> {
        self.card().filter(|c| c.playing).map(|c| c.title)
    }
}

impl Card<'_> {
    fn owned(self) -> (String, Option<String>, bool, Controls, Option<String>) {
        let artist = self.artist.map(str::to_owned);
        let source = self.source.map(str::to_owned);
        (
            self.title.to_owned(),
            artist,
            self.playing,
            self.controls,
            source,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: Controls = Controls {
        previous: true,
        play_pause: true,
        next: true,
    };

    fn playback(status: Status) -> Event {
        playback_at(status, Instant::now())
    }

    fn playback_at(status: Status, at: Instant) -> Event {
        Event::Playback {
            status,
            controls: ALL,
            at,
        }
    }

    fn timeline(position: u64, length: u64, at: Instant) -> Event {
        Event::Timeline(Some(Timeline {
            position: Duration::from_secs(position),
            length: Duration::from_secs(length),
            at,
        }))
    }

    fn playing(title: &str, artist: &str) -> NowPlaying {
        let mut media = NowPlaying::default();
        media.apply(Event::Track(Track::new(title, artist)));
        media.apply(playback(Status::Playing));
        media
    }

    #[test]
    fn nothing_playing_shows_nothing() {
        let media = NowPlaying::default();
        assert_eq!(media.card(), None);
        assert_eq!(media.hint(), None);
    }

    #[test]
    fn a_playing_track_shows_a_card_and_a_hint() {
        let media = playing("Song", "Band");
        let card = media.card().unwrap();
        assert_eq!(
            (card.title, card.artist, card.playing),
            ("Song", Some("Band"), true)
        );
        assert_eq!(media.hint(), Some("Song"));
    }

    #[test]
    fn paused_keeps_the_card_but_not_the_hint() {
        let mut media = playing("Song", "Band");
        assert!(media.apply(playback(Status::Paused)));
        assert!(!media.card().unwrap().playing);
        assert_eq!(media.hint(), None);
    }

    #[test]
    fn stopped_untitled_or_gone_shows_nothing() {
        let mut media = playing("Song", "Band");
        assert!(media.apply(playback(Status::Idle)));
        assert_eq!(media.card(), None);

        let mut media = playing("", "Band");
        assert_eq!(media.card(), None);
        assert!(!media.apply(playback(Status::Paused)));

        let mut media = playing("Song", "Band");
        assert!(media.apply(Event::Gone));
        assert_eq!(media.card(), None);
        // Playback arriving before the next track's properties shows nothing yet.
        assert!(!media.apply(playback(Status::Playing)));
    }

    #[test]
    fn repeated_events_are_not_changes() {
        let mut media = playing("Song", "Band");
        assert!(!media.apply(Event::Track(Track::new("Song", "Band"))));
        assert!(!media.apply(playback(Status::Playing)));
        assert!(media.apply(Event::Track(Track::new("Next", "Band"))));
    }

    #[test]
    fn the_source_shows_on_the_card_until_the_session_is_gone() {
        let mut media = playing("Song", "Band");
        assert!(media.apply(Event::Source(Some("Spotify".into()))));
        assert_eq!(media.card().unwrap().source, Some("Spotify"));
        assert!(!media.apply(Event::Source(Some("Spotify".into()))));
        media.apply(Event::Gone);
        media.apply(Event::Track(Track::new("Song", "Band")));
        media.apply(playback(Status::Playing));
        assert_eq!(media.card().unwrap().source, None);
    }

    #[test]
    fn controls_follow_the_app() {
        let mut media = playing("Song", "Band");
        let controls = Controls {
            previous: false,
            ..ALL
        };
        let event = Event::Playback {
            status: Status::Playing,
            controls,
            at: Instant::now(),
        };
        assert!(media.apply(event));
        assert_eq!(media.card().unwrap().controls, controls);
    }

    #[test]
    fn text_is_one_trimmed_line_and_a_repeated_artist_is_dropped() {
        let track = Track::new("  Song \n Title  ", "\tBand ");
        assert_eq!(
            (track.title.as_str(), track.artist.as_str()),
            ("Song Title", "Band")
        );
        assert_eq!(Track::new("Podcast", "Podcast").artist, "");
        assert_eq!(playing("Song", " ").card().unwrap().artist, None);
    }

    #[test]
    fn runaway_text_is_cut_with_an_ellipsis_on_a_char_boundary() {
        let long = "é".repeat(MAX_CHARS + 10);
        let title = Track::new(&long, "").title;
        assert_eq!(title.chars().count(), MAX_CHARS + 1);
        assert!(title.ends_with('…'));
        let exact = "a".repeat(MAX_CHARS);
        assert_eq!(Track::new(&exact, "").title, exact);
    }

    #[test]
    fn progress_runs_on_from_the_last_report_while_playing() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        let mut media = playing("Song", "Band");
        assert_eq!(media.progress(t0), None, "no timeline yet");
        assert!(media.apply(timeline(89, 200, t0)));
        let at = |secs| media.progress(t0 + s(secs)).unwrap().elapsed;
        assert_eq!(at(0), s(89));
        assert_eq!(at(10), s(99));
        assert_eq!(at(500), s(200), "never past the end");
        let p = media.progress(t0).unwrap();
        assert_eq!(p.remaining(), s(111));
        assert!((p.fraction() - 0.445).abs() < 1e-6);
    }

    #[test]
    fn pausing_holds_the_position_even_when_the_app_reports_none() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        let mut media = playing("Song", "Band");
        media.apply(timeline(10, 200, t0));
        media.apply(playback_at(Status::Paused, t0 + s(5)));
        assert_eq!(media.progress(t0 + s(60)).unwrap().elapsed, s(15));
        // A report from before the pause is stale; one after it is taken.
        assert!(!media.apply(timeline(12, 200, t0 + s(2))));
        assert_eq!(media.progress(t0 + s(60)).unwrap().elapsed, s(15));
        media.apply(playback_at(Status::Playing, t0 + s(30)));
        assert_eq!(media.progress(t0 + s(40)).unwrap().elapsed, s(25));
        assert!(media.apply(timeline(50, 200, t0 + s(41))));
        assert_eq!(media.progress(t0 + s(41)).unwrap().elapsed, s(50));
    }

    #[test]
    fn no_progress_without_a_length_a_card_or_the_same_track() {
        let t0 = Instant::now();
        let mut media = playing("Radio", "");
        media.apply(timeline(0, 0, t0));
        assert_eq!(media.progress(t0), None, "a live stream has no length");
        media.apply(timeline(5, 100, t0));
        media.apply(Event::Track(Track::new("Next", "")));
        assert_eq!(media.progress(t0), None, "the old track's position");
        media.apply(timeline(5, 100, t0));
        media.apply(playback(Status::Idle));
        assert_eq!(media.progress(t0), None, "stopped");
    }

    #[test]
    fn the_times_redraw_as_the_next_second_turns() {
        let progress = |ms| Progress {
            elapsed: Duration::from_millis(ms),
            length: Duration::from_secs(200),
        };
        let ms = Duration::from_millis;
        assert_eq!(progress(89_300).until_next_second(), ms(705));
        assert_eq!(progress(90_000).until_next_second(), ms(1005));
    }

    #[test]
    fn clock_reads_like_a_player() {
        let s = Duration::from_secs;
        assert_eq!(clock(s(0)), "0:00");
        assert_eq!(clock(s(89)), "1:29");
        assert_eq!(clock(s(3729)), "1:02:09");
    }
}
