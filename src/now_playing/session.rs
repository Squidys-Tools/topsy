//! Now Playing's glue to Windows' Global System Media Transport Controls (the session the
//! volume flyout shows).
//!
//! One thread owns the session manager and every session, and watches the one shown
//! (`model::sessions` picks it). WinRT raises its events on thread-pool threads; their handlers
//! only wake that thread, which reads the session and sends an [`Update`] to the UI thread.
//! Button presses reach it the same way, so neither the UI thread nor a GPUI handler ever waits
//! on the media app. Nothing runs between events.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
};
use windows::Win32::System::Com::COINIT_MULTITHREADED;

use crate::log::LogErr;
use crate::now_playing::art::{self, Art, ArtSizes};
use crate::win32;
use model::media::{Controls, Event, Status, Timeline, Track};
use model::sessions::{self, Sessions, Source};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Previous,
    PlayPause,
    Next,
}

/// An event for the island and the floating player, with the track's art when the event is a
/// track.
#[derive(Clone)]
pub struct Update {
    pub event: Event,
    pub art: Option<Art>,
}

enum Wake {
    /// A session came, went, or Windows picked another as current.
    Sessions,
    Properties,
    /// Any session's playback changed.
    Playback,
    Timeline,
    Press(Button),
    Switch,
}

static WAKE: OnceLock<mpsc::Sender<Wake>> = OnceLock::new();

/// Watch the media sessions on a thread of its own.
pub fn spawn(art_size: ArtSizes) -> async_channel::Receiver<Update> {
    let (tx, rx) = async_channel::unbounded();
    let (wake, woken) = mpsc::channel();
    if WAKE.set(wake.clone()).is_err() {
        return rx;
    }
    let spawned = std::thread::Builder::new()
        .name("topsy-media".into())
        .spawn(move || {
            win32::with_com(COINIT_MULTITHREADED, || {
                if let Err(err) = run(&tx, &wake, &woken, art_size) {
                    eprintln!("topsy: no media controls: {err}");
                }
            })
        });
    if let Err(err) = spawned {
        eprintln!("topsy: could not start media controls: {err}");
    }
    rx
}

/// Ask the shown session's app to act; it answers with a playback event. Never blocks.
pub fn press(button: Button) {
    wake(Wake::Press(button));
}

/// Show the next session that is playing or paused.
pub fn switch() {
    wake(Wake::Switch);
}

fn wake(what: Wake) {
    if let Some(wake) = WAKE.get() {
        wake.send(what).log_err();
    }
}

/// A session, its playback as last read, and its playback registration, removed on drop.
struct Tracked {
    id: String,
    app: String,
    session: Session,
    status: Status,
    token: i64,
}

impl Tracked {
    fn new(
        id: String,
        app: String,
        session: Session,
        wake: &mpsc::Sender<Wake>,
    ) -> windows::core::Result<Self> {
        let token = session.PlaybackInfoChanged(&handler(wake, || Wake::Playback))?;
        Ok(Self {
            id,
            app,
            session,
            status: Status::Idle,
            token,
        })
    }
}

impl Drop for Tracked {
    fn drop(&mut self) {
        self.session.RemovePlaybackInfoChanged(self.token).log_err();
    }
}

/// The shown session under its id, and its track and timeline registrations, removed on drop.
struct Watched {
    id: String,
    session: Session,
    tokens: (i64, i64),
}

impl Watched {
    fn new(id: String, session: Session, wake: &mpsc::Sender<Wake>) -> windows::core::Result<Self> {
        let props = session.MediaPropertiesChanged(&handler(wake, || Wake::Properties))?;
        let timeline = session.TimelinePropertiesChanged(&handler(wake, || Wake::Timeline))?;
        Ok(Self {
            id,
            session,
            tokens: (props, timeline),
        })
    }
}

impl Drop for Watched {
    fn drop(&mut self) {
        self.session
            .RemoveMediaPropertiesChanged(self.tokens.0)
            .log_err();
        self.session
            .RemoveTimelinePropertiesChanged(self.tokens.1)
            .log_err();
    }
}

/// Every session, the one shown, and what the island was last told of its app.
struct Watch<'a> {
    manager: Manager,
    wake: &'a mpsc::Sender<Wake>,
    tx: &'a async_channel::Sender<Update>,
    art_size: ArtSizes,
    tracked: Vec<Tracked>,
    sessions: Sessions,
    shown: Option<Watched>,
    source: Option<String>,
    names: HashMap<String, String>,
}

impl Watch<'_> {
    fn send(&self, event: Event, art: Option<Art>) -> bool {
        self.tx.send_blocking(Update { event, art }).is_ok()
    }

    /// Nothing to send counts as sent.
    fn report(&self, event: Option<Event>) -> bool {
        event.is_none_or(|event| self.send(event, None))
    }

    /// Track the sessions there are now, each under an id of its own.
    fn reload(&mut self) {
        let old = std::mem::take(&mut self.tracked);
        let Some(all) = self.manager.GetSessions().log_err() else {
            return;
        };
        let all: Vec<Session> = all.into_iter().collect();
        let apps: Vec<String> = all
            .iter()
            .map(|s| {
                s.SourceAppUserModelId()
                    .map(|a| a.to_string_lossy())
                    .unwrap_or_default()
            })
            .collect();
        let ids = sessions::ids(&apps);
        for ((session, id), app) in all.into_iter().zip(ids).zip(apps) {
            if let Some(mut tracked) = Tracked::new(id, app, session, self.wake).log_err() {
                // A session between tracks reads no status, so it keeps the one it had.
                let before = (old
                    .iter()
                    .find(|previous| previous.session == tracked.session))
                .or_else(|| old.iter().find(|previous| previous.id == tracked.id));
                if let Some(before) = before {
                    tracked.status = before.status;
                }
                self.tracked.push(tracked);
            }
        }
    }

    /// Read every session's playback, let the rules pick the one to show, and tell the island
    /// what changed. `recheck` rewatches the shown session when its id now names another.
    /// False once the island is gone.
    fn pick(&mut self, recheck: bool) -> bool {
        for tracked in &mut self.tracked {
            if let Some(status) = status(&tracked.session) {
                tracked.status = status;
            }
        }
        let sources = self.tracked.iter().map(|t| Source {
            id: t.id.clone(),
            status: t.status,
        });
        let current = self.manager.GetCurrentSession().ok().and_then(|current| {
            let app = current.SourceAppUserModelId().ok()?.to_string_lossy();
            let same = self.tracked.iter().find(|t| t.session == current);
            same.or_else(|| self.tracked.iter().find(|t| t.app == app))
                .map(|t| t.id.clone())
        });
        self.sessions.update(sources.collect(), current);
        let shown = (self.sessions.shown())
            .and_then(|id| self.tracked.iter().find(|t| t.id == id))
            .map(|t| (t.id.clone(), t.session.clone()));
        let same = match (&self.shown, &shown) {
            (Some(watched), Some((id, session))) => {
                watched.id == *id && (!recheck || watched.session == *session)
            }
            (None, None) => true,
            (Some(_), None) | (None, Some(_)) => false,
        };
        (same || self.show(shown)) && self.tell_source()
    }

    /// Watch `shown` (an id and its session) and send all about it, or that there is none.
    fn show(&mut self, shown: Option<(String, Session)>) -> bool {
        self.shown = None;
        let Some((id, session)) = shown else {
            // The island forgets the source along with the session.
            self.source = None;
            return self.send(Event::Gone, None);
        };
        self.shown = Watched::new(id, session.clone(), self.wake).log_err();
        let (track, art) = track(&session, self.art_size);
        self.send(Event::Track(track), art)
            && self.report(playback(&session))
            && self.report(timeline(&session))
    }

    /// Send the shown session's app, while there are others to switch to, when it changed.
    fn tell_source(&mut self) -> bool {
        let app = (self.shown.as_ref())
            .filter(|_| self.sessions.others())
            .and_then(|w| self.tracked.iter().find(|t| t.id == w.id))
            .map(|t| t.app.clone());
        let source = app.map(|app| {
            (self.names.entry(app))
                .or_insert_with_key(|app| {
                    win32::app_name(app).unwrap_or_else(|| sessions::app_name(app))
                })
                .clone()
        });
        if source == self.source {
            return true;
        }
        self.source = source.clone();
        self.send(Event::Source(source), None)
    }
}

fn handler<A, B>(wake: &mpsc::Sender<Wake>, what: fn() -> Wake) -> TypedEventHandler<A, B>
where
    A: windows::core::RuntimeType + 'static,
    B: windows::core::RuntimeType + 'static,
{
    let wake = wake.clone();
    TypedEventHandler::new(move |_, _| {
        wake.send(what()).log_err();
        Ok(())
    })
}

fn run(
    tx: &async_channel::Sender<Update>,
    wake: &mpsc::Sender<Wake>,
    woken: &mpsc::Receiver<Wake>,
    art_size: ArtSizes,
) -> windows::core::Result<()> {
    let manager = Manager::RequestAsync()?.join()?;
    manager.CurrentSessionChanged(&handler(wake, || Wake::Sessions))?;
    manager.SessionsChanged(&handler(wake, || Wake::Sessions))?;
    wake.send(Wake::Sessions).log_err();
    let mut watch = Watch {
        manager,
        wake,
        tx,
        art_size,
        tracked: Vec::new(),
        sessions: Sessions::default(),
        shown: None,
        source: None,
        names: HashMap::new(),
    };
    while let Ok(wake_up) = woken.recv() {
        let session = watch.shown.as_ref().map(|w| w.session.clone());
        let sent = match (wake_up, session) {
            (Wake::Sessions, _) => {
                watch.reload();
                watch.pick(true)
            }
            (Wake::Switch, _) => {
                watch.sessions.next();
                watch.pick(false)
            }
            // Any session's playback may change which is shown; a newly shown one is reported
            // whole, else the shown one's playback, and its position, which some apps only
            // report along with it.
            (Wake::Playback, session) => {
                let before = watch.shown.as_ref().map(|w| w.id.clone());
                let picked = watch.pick(false);
                let after = watch.shown.as_ref().map(|w| w.id.clone());
                picked
                    && (before != after
                        || session.is_none_or(|session| {
                            watch.report(playback(&session)) && watch.report(timeline(&session))
                        }))
            }
            (Wake::Properties, Some(session)) => {
                // A new track drops the old one's position, so read the new one's after it.
                let (track, art) = track(&session, art_size);
                watch.send(Event::Track(track), art) && watch.report(timeline(&session))
            }
            (Wake::Timeline, Some(session)) => watch.report(timeline(&session)),
            (Wake::Press(button), Some(session)) => {
                // Not waited on: the app reports the outcome as a playback event.
                match button {
                    Button::Previous => session.TrySkipPreviousAsync(),
                    Button::PlayPause => session.TryTogglePlayPauseAsync(),
                    Button::Next => session.TrySkipNextAsync(),
                }
                .log_err();
                true
            }
            (Wake::Properties | Wake::Timeline | Wake::Press(_), None) => true,
        };
        if !sent {
            break;
        }
    }
    Ok(())
}

fn track(session: &Session, art_size: ArtSizes) -> (Track, Option<Art>) {
    let Ok(props) = session
        .TryGetMediaPropertiesAsync()
        .and_then(|op| op.join())
    else {
        return (Track::default(), None);
    };
    let text = |s: windows::core::Result<windows::core::HSTRING>| {
        s.map(|s| s.to_string_lossy()).unwrap_or_default()
    };
    let track = Track::new(&text(props.Title()), &text(props.Artist()));
    (track, art::read(&props, art_size).ok())
}

/// `None` while the app is between tracks (`Changing`): what it reports next stands, and the
/// player doesn't fold away and back for a moment.
fn playback(session: &Session) -> Option<Event> {
    let at = Instant::now();
    let Ok(info) = session.GetPlaybackInfo() else {
        return Some(Event::Gone);
    };
    let status = status_of(info.PlaybackStatus())?;
    let controls = info.Controls().ok().map(|c| {
        let on = |b: windows::core::Result<bool>| b.unwrap_or(false);
        Controls {
            previous: on(c.IsPreviousEnabled()),
            play_pause: on(c.IsPlayPauseToggleEnabled())
                || on(c.IsPlayEnabled())
                || on(c.IsPauseEnabled()),
            next: on(c.IsNextEnabled()),
        }
    });
    Some(Event::Playback {
        status,
        controls: controls.unwrap_or_default(),
        at,
    })
}

/// The session's playback; `None` while it is between tracks, as for [`playback`].
fn status(session: &Session) -> Option<Status> {
    status_of(session.GetPlaybackInfo().ok()?.PlaybackStatus())
}

fn status_of(status: windows::core::Result<PlaybackStatus>) -> Option<Status> {
    match status {
        Ok(PlaybackStatus::Playing) => Some(Status::Playing),
        Ok(PlaybackStatus::Paused) => Some(Status::Paused),
        Ok(PlaybackStatus::Changing) => None,
        _ => Some(Status::Idle),
    }
}

/// Where the track is, as of when the app last said. WinRT counts in 100 ns ticks; its clock
/// starts in 1601. `None` if it can't be read, so the last report stands.
fn timeline(session: &Session) -> Option<Event> {
    const UNIX_IN_TICKS: i64 = 11_644_473_600 * 10_000_000;
    let ticks = |t: i64| Duration::from_nanos(t.max(0) as u64 * 100);
    let props = session.GetTimelineProperties().ok()?;
    let get = || -> windows::core::Result<(i64, i64, i64, i64)> {
        let start = props.StartTime()?.Duration;
        Ok((
            start,
            props.EndTime()?.Duration,
            props.Position()?.Duration,
            props.LastUpdatedTime()?.UniversalTime,
        ))
    };
    let (start, end, position, updated) = get().ok()?;
    let now = Instant::now();
    let updated = UNIX_EPOCH + ticks(updated - UNIX_IN_TICKS);
    // A report from the future (a clock change) counts as now.
    let age = SystemTime::now()
        .duration_since(updated)
        .unwrap_or_default();
    Some(Event::Timeline((end > start).then(|| Timeline {
        position: ticks(position - start),
        length: ticks(end - start),
        at: now.checked_sub(age).unwrap_or(now),
    })))
}
