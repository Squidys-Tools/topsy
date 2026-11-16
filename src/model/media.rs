//! Now Playing: what the system's current media session reports, reduced to what the island
//! shows. The glue (`src/media.rs`) turns each WinRT event into an [`Event`]; nothing here
//! ticks, so a song playing costs nothing until the app reports a change.

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
    },
}

/// The tray's row: what is playing and which buttons work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card<'a> {
    pub title: &'a str,
    pub artist: Option<&'a str>,
    pub playing: bool,
    pub controls: Controls,
}

#[derive(Debug, Default)]
pub struct NowPlaying {
    track: Option<Track>,
    status: Status,
    controls: Controls,
}

impl NowPlaying {
    /// Take in an event; true when what the island shows changed.
    pub fn apply(&mut self, event: Event) -> bool {
        // The hint is drawn from the card, so the card alone tells.
        let before = self.card().map(Card::owned);
        match event {
            Event::Gone => *self = Self::default(),
            Event::Track(track) => self.track = Some(track),
            Event::Playback { status, controls } => {
                self.status = status;
                self.controls = controls;
            }
        }
        before != self.card().map(Card::owned)
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
        })
    }

    /// The peek's hint: the title, only while it is actually playing.
    pub fn hint(&self) -> Option<&str> {
        self.card().filter(|c| c.playing).map(|c| c.title)
    }
}

impl Card<'_> {
    fn owned(self) -> (String, Option<String>, bool, Controls) {
        let artist = self.artist.map(str::to_owned);
        (self.title.to_owned(), artist, self.playing, self.controls)
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
        Event::Playback {
            status,
            controls: ALL,
        }
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
    fn controls_follow_the_app() {
        let mut media = playing("Song", "Band");
        let controls = Controls {
            previous: false,
            ..ALL
        };
        let event = Event::Playback {
            status: Status::Playing,
            controls,
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
}
