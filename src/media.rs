//! Now Playing's glue to Windows' Global System Media Transport Controls (the session the
//! volume flyout shows).
//!
//! One thread owns the session manager and the current session. WinRT raises its events on
//! thread-pool threads; their handlers only wake that thread, which reads the session and
//! sends an [`Update`] to the UI thread. Button presses reach it the same way, so neither the
//! UI thread nor a GPUI handler ever waits on the media app. Nothing runs between events.

use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui::RenderImage;
use image::{Frame, RgbaImage};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapDecoder, BitmapInterpolationMode, BitmapPixelFormat, BitmapTransform,
    ColorManagementMode, ExifOrientationMode,
};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionMediaProperties as Properties,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
};
use windows::Win32::System::Com::COINIT_MULTITHREADED;

use crate::model::media::{Controls, Event, Status, Timeline, Track};
use crate::win32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Previous,
    PlayPause,
    Next,
}

/// An event for the island, with the track's art when the event is a track.
pub struct Update {
    pub event: Event,
    pub art: Option<Arc<RenderImage>>,
}

enum Wake {
    Session,
    Properties,
    Playback,
    Timeline,
    Press(Button),
}

static WAKE: OnceLock<mpsc::Sender<Wake>> = OnceLock::new();

/// Watch the current media session on a thread of its own. `art_size` is the art's shorter
/// side in physical pixels.
pub fn spawn(art_size: u32) -> async_channel::Receiver<Update> {
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

/// Ask the current session's app to act; it answers with a playback event. Never blocks.
pub fn press(button: Button) {
    if let Some(wake) = WAKE.get() {
        let _ = wake.send(Wake::Press(button));
    }
}

/// The current session and its event registrations, removed when it is replaced.
struct Watched {
    session: Session,
    tokens: (i64, i64, i64),
}

impl Watched {
    fn new(session: Session, wake: &mpsc::Sender<Wake>) -> windows::core::Result<Self> {
        let props = session.MediaPropertiesChanged(&handler(wake, || Wake::Properties))?;
        let playback = session.PlaybackInfoChanged(&handler(wake, || Wake::Playback))?;
        let timeline = session.TimelinePropertiesChanged(&handler(wake, || Wake::Timeline))?;
        Ok(Self {
            session,
            tokens: (props, playback, timeline),
        })
    }
}

impl Drop for Watched {
    fn drop(&mut self) {
        let _ = self.session.RemoveMediaPropertiesChanged(self.tokens.0);
        let _ = self.session.RemovePlaybackInfoChanged(self.tokens.1);
        let _ = self.session.RemoveTimelinePropertiesChanged(self.tokens.2);
    }
}

fn handler<A, B>(wake: &mpsc::Sender<Wake>, what: fn() -> Wake) -> TypedEventHandler<A, B>
where
    A: windows::core::RuntimeType + 'static,
    B: windows::core::RuntimeType + 'static,
{
    let wake = wake.clone();
    TypedEventHandler::new(move |_, _| {
        let _ = wake.send(what());
        Ok(())
    })
}

fn run(
    tx: &async_channel::Sender<Update>,
    wake: &mpsc::Sender<Wake>,
    woken: &mpsc::Receiver<Wake>,
    art_size: u32,
) -> windows::core::Result<()> {
    let manager = Manager::RequestAsync()?.join()?;
    manager.CurrentSessionChanged(&handler(wake, || Wake::Session))?;
    let _ = wake.send(Wake::Session);
    let send = |event: Event, art| tx.send_blocking(Update { event, art }).is_ok();
    // Nothing to send counts as sent.
    let report = |event: Option<Event>| event.is_none_or(|event| send(event, None));
    let mut current: Option<Watched> = None;
    while let Ok(wake_up) = woken.recv() {
        let session = current.as_ref().map(|w| &w.session);
        let sent = match (wake_up, session) {
            (Wake::Session, _) => {
                current = None;
                match manager.GetCurrentSession().ok() {
                    None => send(Event::Gone, None),
                    Some(session) => {
                        current = Watched::new(session.clone(), wake).ok();
                        let (track, art) = track(&session, art_size);
                        send(Event::Track(track), art)
                            && report(playback(&session))
                            && report(timeline(&session))
                    }
                }
            }
            (Wake::Properties, Some(session)) => {
                // A new track drops the old one's position, so read the new one's after it.
                let (track, art) = track(session, art_size);
                send(Event::Track(track), art) && report(timeline(session))
            }
            // Some apps only report the position along with a change of playback.
            (Wake::Playback, Some(session)) => {
                report(playback(session)) && report(timeline(session))
            }
            (Wake::Timeline, Some(session)) => report(timeline(session)),
            (Wake::Press(button), Some(session)) => {
                // Not waited on: the app reports the outcome as a playback event.
                let _ = match button {
                    Button::Previous => session.TrySkipPreviousAsync(),
                    Button::PlayPause => session.TryTogglePlayPauseAsync(),
                    Button::Next => session.TrySkipNextAsync(),
                };
                true
            }
            (_, None) => true,
        };
        if !sent {
            break;
        }
    }
    Ok(())
}

fn track(session: &Session, art_size: u32) -> (Track, Option<Arc<RenderImage>>) {
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
    (track, art(&props, art_size).ok())
}

/// `None` while the app is between tracks (`Changing`): what it reports next stands, and the
/// player doesn't fold away and back for a moment.
fn playback(session: &Session) -> Option<Event> {
    let at = Instant::now();
    let Ok(info) = session.GetPlaybackInfo() else {
        return Some(Event::Gone);
    };
    let status = match info.PlaybackStatus() {
        Ok(PlaybackStatus::Playing) => Status::Playing,
        Ok(PlaybackStatus::Paused) => Status::Paused,
        Ok(PlaybackStatus::Changing) => return None,
        _ => Status::Idle,
    };
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

/// The track's art, decoded and scaled down (never up) so its shorter side is `size`.
fn art(props: &Properties, size: u32) -> windows::core::Result<Arc<RenderImage>> {
    let stream = props.Thumbnail()?.OpenReadAsync()?.join()?;
    let decoder = BitmapDecoder::CreateAsync(&stream)?.join()?;
    let (w, h) = (decoder.PixelWidth()?, decoder.PixelHeight()?);
    let scale = (size as f32 / w.min(h).max(1) as f32).min(1.0);
    let (w, h) = (
        ((w as f32 * scale).round() as u32).max(1),
        ((h as f32 * scale).round() as u32).max(1),
    );
    let transform = BitmapTransform::new()?;
    transform.SetScaledWidth(w)?;
    transform.SetScaledHeight(h)?;
    transform.SetInterpolationMode(BitmapInterpolationMode::Fant)?;
    let pixels = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Straight,
            &transform,
            ExifOrientationMode::IgnoreExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )?
        .join()?
        .DetachPixelData()?;
    // GPUI's images are BGRA with straight alpha, whatever the buffer's type says.
    let buffer =
        RgbaImage::from_raw(w, h, pixels.to_vec()).ok_or_else(windows::core::Error::empty)?;
    Ok(Arc::new(RenderImage::new(vec![Frame::new(buffer)])))
}
