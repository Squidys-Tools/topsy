//! Which of the system's media sessions Now Playing shows. Windows' current session by default;
//! one the user switches to stays shown while it exists. A session that starts playing while the
//! shown one isn't takes over, so pausing Spotify and starting a video shows the video.

use crate::media::Status;

/// One session as the glue reads it: an id unique among the sessions, and its playback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub id: String,
    pub status: Status,
}

#[derive(Debug, Default)]
pub struct Sessions {
    sources: Vec<Source>,
    current: Option<String>,
    /// The session the user switched to, or that took over.
    pinned: Option<String>,
}

impl Sessions {
    /// Take in the sessions as they are now, in the system's order, and Windows' current one.
    pub fn update(&mut self, sources: Vec<Source>, current: Option<String>) {
        let started = sources
            .iter()
            .find(|new| {
                new.status == Status::Playing
                    && !self
                        .sources
                        .iter()
                        .any(|old| old.id == new.id && old.status == Status::Playing)
            })
            .map(|source| source.id.clone());
        self.sources = sources;
        self.current = current.filter(|id| self.find(id).is_some());
        if self
            .pinned
            .as_deref()
            .is_some_and(|id| self.find(id).is_none())
        {
            self.pinned = None;
        }
        let shown_plays = self
            .shown()
            .and_then(|id| self.find(id))
            .is_some_and(|source| source.status == Status::Playing);
        if let Some(started) = started
            && !shown_plays
        {
            self.pinned = (self.current.as_ref() != Some(&started)).then_some(started);
        }
    }

    /// The session to show: the pinned one, else Windows' current, else one playing, else one
    /// paused. An idle pinned or current session shows nothing rather than another app.
    pub fn shown(&self) -> Option<&str> {
        self.live(self.pinned.as_deref())
            .or_else(|| self.live(self.current.as_deref()))
            .or_else(|| self.first(Status::Playing))
            .or_else(|| self.first(Status::Paused))
            .or(self.pinned.as_deref())
            .or(self.current.as_deref())
    }

    /// Whether there is another session to switch to: more than one playing or paused.
    pub fn others(&self) -> bool {
        self.showable().nth(1).is_some()
    }

    /// Switch to the next playing or paused session after the shown one, round to the first.
    pub fn next(&mut self) {
        let shown = self.shown();
        let mut showable = self.showable();
        let after = showable
            .by_ref()
            .position(|id| Some(id) == shown)
            .and_then(|_| showable.next());
        let next = after.or_else(|| self.showable().next()).map(str::to_owned);
        drop(showable);
        if next.is_some() {
            self.pinned = next;
        }
    }

    fn showable(&self) -> impl Iterator<Item = &str> {
        self.sources
            .iter()
            .filter(|s| s.status != Status::Idle)
            .map(|s| s.id.as_str())
    }

    fn live<'a>(&self, id: Option<&'a str>) -> Option<&'a str> {
        id.filter(|id| self.find(id).is_some_and(|s| s.status != Status::Idle))
    }

    fn first(&self, status: Status) -> Option<&str> {
        self.sources
            .iter()
            .find(|s| s.status == status)
            .map(|s| s.id.as_str())
    }

    fn find(&self, id: &str) -> Option<&Source> {
        self.sources.iter().find(|s| s.id == id)
    }
}

/// Ids for sessions named by their apps (`app_ids`, in order): an app's second session (two
/// browser tabs) gets `#2`, and so on.
pub fn ids(app_ids: &[String]) -> Vec<String> {
    app_ids
        .iter()
        .enumerate()
        .map(
            |(i, app)| match app_ids[..i].iter().filter(|a| *a == app).count() {
                0 => app.clone(),
                n => format!("{app}#{}", n + 1),
            },
        )
        .collect()
}

/// A readable name from an app's id when the shell knows none: `Spotify.exe` reads `Spotify`,
/// a packaged app's `Publisher.App_hash!App` reads `App`.
pub fn app_name(app_id: &str) -> String {
    let app = app_id.rsplit('!').next().unwrap_or(app_id);
    let app = match app.len().checked_sub(4) {
        Some(cut)
            if app
                .get(cut..)
                .is_some_and(|e| e.eq_ignore_ascii_case(".exe")) =>
        {
            &app[..cut]
        }
        _ => app,
    };
    let app = app.rsplit(['.', '\\']).next().unwrap_or(app);
    let mut chars = app.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => app_id.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Status::{Idle, Paused, Playing};

    fn source(id: &str, status: Status) -> Source {
        Source {
            id: id.into(),
            status,
        }
    }

    fn sessions(sources: &[(&str, Status)], current: &str) -> Sessions {
        let mut sessions = Sessions::default();
        sessions.update(list(sources), Some(current.into()));
        sessions
    }

    fn list(sources: &[(&str, Status)]) -> Vec<Source> {
        sources.iter().map(|(id, s)| source(id, *s)).collect()
    }

    #[test]
    fn windows_current_session_is_shown_by_default_and_followed() {
        let mut s = sessions(&[("spotify", Playing), ("chrome", Playing)], "chrome");
        assert_eq!(s.shown(), Some("chrome"));
        s.update(
            list(&[("spotify", Playing), ("chrome", Playing)]),
            Some("spotify".into()),
        );
        assert_eq!(s.shown(), Some("spotify"));
        assert!(Sessions::default().shown().is_none());
    }

    #[test]
    fn a_switched_to_session_sticks_until_it_is_gone() {
        let both = [("spotify", Playing), ("chrome", Playing)];
        let mut s = sessions(&both, "spotify");
        assert!(s.others());
        s.next();
        assert_eq!(s.shown(), Some("chrome"));
        s.update(list(&both), Some("spotify".into()));
        assert_eq!(s.shown(), Some("chrome"), "Windows changing its mind");
        s.next();
        assert_eq!(s.shown(), Some("spotify"), "round to the first");
        s.next();
        s.update(list(&[("spotify", Playing)]), Some("spotify".into()));
        assert_eq!(s.shown(), Some("spotify"), "chrome closed");
        assert!(!s.others());
        s.update(list(&both), Some("spotify".into()));
        assert_eq!(s.shown(), Some("spotify"), "a new chrome isn't pinned");
    }

    #[test]
    fn switching_skips_idle_sessions() {
        let mut s = sessions(
            &[("spotify", Paused), ("old-tab", Idle), ("chrome", Paused)],
            "spotify",
        );
        assert!(s.others());
        s.next();
        assert_eq!(s.shown(), Some("chrome"));
        s.update(
            list(&[("spotify", Paused), ("old-tab", Idle), ("chrome", Idle)]),
            Some("spotify".into()),
        );
        assert!(!s.others());
        assert_eq!(s.shown(), Some("spotify"), "pinned and idle shows another");
    }

    #[test]
    fn playback_starting_elsewhere_takes_over_a_paused_session() {
        let mut s = sessions(&[("spotify", Playing), ("chrome", Paused)], "spotify");
        s.update(
            list(&[("spotify", Paused), ("chrome", Paused)]),
            Some("spotify".into()),
        );
        s.update(
            list(&[("spotify", Paused), ("chrome", Playing)]),
            Some("spotify".into()),
        );
        assert_eq!(s.shown(), Some("chrome"));
        // While the shown one plays, another starting doesn't take over.
        s.update(
            list(&[("spotify", Playing), ("chrome", Playing)]),
            Some("spotify".into()),
        );
        assert_eq!(s.shown(), Some("chrome"));
    }

    #[test]
    fn a_takeover_by_windows_current_session_follows_windows_again() {
        let mut s = sessions(&[("spotify", Paused), ("chrome", Paused)], "spotify");
        s.next();
        assert_eq!(s.shown(), Some("chrome"));
        s.update(
            list(&[("spotify", Playing), ("chrome", Paused)]),
            Some("spotify".into()),
        );
        assert_eq!(s.shown(), Some("spotify"));
        s.update(
            list(&[("spotify", Playing), ("chrome", Playing)]),
            Some("chrome".into()),
        );
        assert_eq!(s.shown(), Some("chrome"), "unpinned, it follows Windows");
    }

    #[test]
    fn without_a_current_session_one_playing_is_shown() {
        let mut s = Sessions::default();
        s.update(list(&[("a", Paused), ("b", Playing)]), None);
        assert_eq!(s.shown(), Some("b"));
    }

    #[test]
    fn an_apps_second_session_gets_an_id_of_its_own() {
        let apps = ["chrome", "spotify", "chrome", "chrome"].map(String::from);
        assert_eq!(ids(&apps), ["chrome", "spotify", "chrome#2", "chrome#3"]);
    }

    #[test]
    fn app_names_read_without_the_id_noise() {
        assert_eq!(app_name("Spotify.exe"), "Spotify");
        assert_eq!(app_name("chrome"), "Chrome");
        assert_eq!(
            app_name("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"),
            "ZuneMusic"
        );
        assert_eq!(app_name(r"C:\Apps\vlc.EXE"), "Vlc");
        assert_eq!(app_name(""), "");
    }
}
