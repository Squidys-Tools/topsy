//! Every app's volume, for the Audio widget. A thread of its own in the MTA opens the default
//! output device's session manager, lists its sessions and hears new ones
//! (`IAudioSessionNotification`), and each one's volume and end (`IAudioSessionEvents`). The
//! callbacks only post to that thread, which reads the session and reports a plain event.
//! Nothing polls; the device is looked at again only when asked to ([`Watch::refresh`]).

use std::collections::HashMap;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};

use gpui::RenderImage;
use windows::Win32::Foundation::{CloseHandle, PROPERTYKEY, S_OK};
use windows::Win32::Media::Audio::{
    AudioSessionDisconnectReason, AudioSessionState, AudioSessionStateExpired,
    IAudioSessionControl, IAudioSessionControl2, IAudioSessionEvents, IAudioSessionEvents_Impl,
    IAudioSessionManager2, IAudioSessionNotification, IAudioSessionNotification_Impl,
    IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, eMultimedia, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoTaskMemFree,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::{IShellItem2, SHCreateItemFromParsingName};
use windows::core::{BOOL, GUID, HSTRING, Interface, PCWSTR, PWSTR, Ref, implement};

use crate::log::LogErr;
use model::mixer::Session;

/// Marks topsy's own volume changes, so their echoes are told apart from other apps'.
const OURS: GUID = GUID::from_u128(0x5f1c2b7e_9a43_4d0e_b8a1_3c6e2d7f9a10);
/// `PKEY_FileDescription`: the name an exe gives itself ("Google Chrome").
const DESCRIPTION: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x0cef7d53_fa64_11d1_a203_0000f81fedee),
    pid: 3,
};
/// The app icon's side in physical pixels, enough for the row's icon at 200%.
const ICON: i32 = 40;

pub enum Event {
    /// A session appeared, with its app's icon.
    Added(Session, Option<Arc<RenderImage>>),
    /// Something other than topsy changed a session's volume.
    Changed {
        id: String,
        volume: f32,
        muted: bool,
    },
    Gone(String),
}

enum Message {
    Created(Created),
    Volume {
        id: String,
        volume: f32,
        muted: bool,
    },
    Gone(String),
    Set {
        app: String,
        volume: f32,
        muted: bool,
    },
    Refresh,
    Stop,
}

/// A new session, handed from the notification's thread to the watching one.
struct Created(IAudioSessionControl);

// SAFETY: audio session objects live in the MTA, which the watching thread joins.
unsafe impl Send for Created {}

/// The watching thread; dropping this stops it.
pub struct Watch(mpsc::Sender<Message>);

impl Watch {
    pub fn start(events: async_channel::Sender<Event>) -> Self {
        let (sender, messages) = mpsc::channel();
        let own = sender.clone();
        let spawned = std::thread::Builder::new()
            .name("topsy-mixer".into())
            .spawn(move || {
                crate::win32::with_com(COINIT_MULTITHREADED, || run(&own, messages, &events));
            });
        if let Err(err) = spawned {
            eprintln!("topsy: no audio sessions: {err}");
        }
        Self(sender)
    }

    /// Set every session of `app` (a [`Session::app`]).
    pub fn set(&self, app: &str, volume: f32, muted: bool) {
        let app = app.to_owned();
        (self.0.send(Message::Set { app, volume, muted })).log_err();
    }

    /// Follow the default output device if it changed.
    pub fn refresh(&self) {
        self.0.send(Message::Refresh).log_err();
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.0.send(Message::Stop).log_err();
    }
}

fn run(
    own: &mpsc::Sender<Message>,
    messages: mpsc::Receiver<Message>,
    events: &async_channel::Sender<Event>,
) {
    // SAFETY: COM is set up on this thread for its whole life.
    let devices = unsafe {
        CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
    };
    let devices = match devices {
        Ok(devices) => devices,
        Err(err) => return eprintln!("topsy: no audio sessions: {err}"),
    };
    let notification: IAudioSessionNotification = Notification(own.clone()).into();
    // SAFETY: COM calls on this thread, which owns every object they touch.
    unsafe {
        let mut device = Device::open(&devices, &notification, own, events);
        for message in messages {
            match message {
                Message::Created(Created(control)) => {
                    if let Some(device) = &mut device {
                        device.track(control, own, events);
                    }
                }
                Message::Volume { id, volume, muted } => {
                    events
                        .send_blocking(Event::Changed { id, volume, muted })
                        .log_err();
                }
                Message::Gone(id) => {
                    if let Some(device) = &mut device
                        && device.forget(&id)
                    {
                        events.send_blocking(Event::Gone(id)).log_err();
                    }
                }
                Message::Set { app, volume, muted } => {
                    if let Some(device) = &device {
                        device.set(&app, volume, muted);
                    }
                }
                Message::Refresh => {
                    let now = default_id(&devices);
                    if now.is_some() && device.as_ref().map(|d| &d.id) == now.as_ref() {
                        continue;
                    }
                    if let Some(old) = device.take() {
                        for id in old.sessions.keys() {
                            events.send_blocking(Event::Gone(id.clone())).log_err();
                        }
                    }
                    device = Device::open(&devices, &notification, own, events);
                }
                Message::Stop => break,
            }
        }
    }
}

/// The output device watched, and its sessions by instance id. Dropping it stops listening.
struct Device {
    id: String,
    manager: IAudioSessionManager2,
    notification: IAudioSessionNotification,
    sessions: HashMap<String, Tracked>,
}

struct Tracked {
    app: String,
    control: IAudioSessionControl,
    listener: IAudioSessionEvents,
    volume: ISimpleAudioVolume,
}

impl Device {
    unsafe fn open(
        devices: &IMMDeviceEnumerator,
        notification: &IAudioSessionNotification,
        own: &mpsc::Sender<Message>,
        events: &async_channel::Sender<Event>,
    ) -> Option<Self> {
        unsafe {
            let endpoint = devices.GetDefaultAudioEndpoint(eRender, eMultimedia).ok()?;
            let id = take(endpoint.GetId().log_err()?);
            let manager: IAudioSessionManager2 = endpoint.Activate(CLSCTX_ALL, None).log_err()?;
            // Registered before listing, so a session starting in between is heard twice at
            // worst (and tracked once), never missed.
            manager
                .RegisterSessionNotification(notification)
                .log_err()?;
            let mut device = Device {
                id,
                manager,
                notification: notification.clone(),
                sessions: HashMap::new(),
            };
            let list = device.manager.GetSessionEnumerator().log_err()?;
            for at in 0..list.GetCount().log_err()? {
                if let Some(control) = list.GetSession(at).log_err() {
                    device.track(control, own, events);
                }
            }
            Some(device)
        }
    }

    unsafe fn track(
        &mut self,
        control: IAudioSessionControl,
        own: &mpsc::Sender<Message>,
        events: &async_channel::Sender<Event>,
    ) {
        match unsafe { self.read(control, own) } {
            Ok(Some((session, tracked))) => {
                let icon = crate::icon::load(Path::new(&session.app), ICON);
                self.sessions.insert(session.id.clone(), tracked);
                events.send_blocking(Event::Added(session, icon)).log_err();
            }
            Ok(None) => {}
            Err(err) => crate::log(&format!("audio session: {err}")),
        }
    }

    /// The session, or `None` for one not shown: the system's sounds, one expired or already
    /// tracked, one whose process is gone.
    unsafe fn read(
        &self,
        control: IAudioSessionControl,
        own: &mpsc::Sender<Message>,
    ) -> windows::core::Result<Option<(Session, Tracked)>> {
        unsafe {
            let control2: IAudioSessionControl2 = control.cast()?;
            if control2.IsSystemSoundsSession() == S_OK
                || control.GetState()? == AudioSessionStateExpired
            {
                return Ok(None);
            }
            let id = take(control2.GetSessionInstanceIdentifier()?);
            if self.sessions.contains_key(&id) {
                return Ok(None);
            }
            let Some(exe) = process_exe(control2.GetProcessId()?) else {
                return Ok(None);
            };
            let volume: ISimpleAudioVolume = control.cast()?;
            let listener: IAudioSessionEvents = Listener {
                id: id.clone(),
                sender: own.clone(),
            }
            .into();
            let session = Session {
                id,
                app: exe.to_string_lossy().to_lowercase(),
                name: name(&control, &exe),
                volume: volume.GetMasterVolume()?,
                muted: volume.GetMute()?.as_bool(),
            };
            control.RegisterAudioSessionNotification(&listener)?;
            let tracked = Tracked {
                app: session.app.clone(),
                control,
                listener,
                volume,
            };
            Ok(Some((session, tracked)))
        }
    }

    /// Stop listening to a session that ended. Whether it was tracked.
    unsafe fn forget(&mut self, id: &str) -> bool {
        let Some(tracked) = self.sessions.remove(id) else {
            return false;
        };
        unsafe { tracked.unregister() };
        true
    }

    unsafe fn set(&self, app: &str, volume: f32, muted: bool) {
        for tracked in self.sessions.values().filter(|t| t.app == app) {
            unsafe {
                tracked.volume.SetMasterVolume(volume, &OURS).log_err();
                tracked.volume.SetMute(muted, &OURS).log_err();
            }
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: on the watching thread, which registered both.
        unsafe {
            (self.manager)
                .UnregisterSessionNotification(&self.notification)
                .log_err();
            for tracked in self.sessions.values() {
                tracked.unregister();
            }
        }
    }
}

impl Tracked {
    unsafe fn unregister(&self) {
        let unregistered =
            unsafe { (self.control).UnregisterAudioSessionNotification(&self.listener) };
        unregistered.log_err();
    }
}

unsafe fn default_id(devices: &IMMDeviceEnumerator) -> Option<String> {
    unsafe {
        let endpoint = devices.GetDefaultAudioEndpoint(eRender, eMultimedia).ok()?;
        Some(take(endpoint.GetId().ok()?))
    }
}

/// What the row calls the app: the name its session gives, else its exe's description, else the
/// exe's own name. A name starting with `@` is an unresolved resource string.
unsafe fn name(control: &IAudioSessionControl, exe: &Path) -> String {
    let given = unsafe { control.GetDisplayName() }
        .ok()
        .map(|name| unsafe { take(name) });
    let described = || {
        let item: IShellItem2 =
            unsafe { SHCreateItemFromParsingName(&HSTRING::from(exe.as_os_str()), None) }.ok()?;
        Some(unsafe { take(item.GetString(&DESCRIPTION).ok()?) })
    };
    let stem = || exe.file_stem().map(|s| s.to_string_lossy().into_owned());
    (given.filter(|name| !name.is_empty() && !name.starts_with('@')))
        .or_else(|| described().filter(|name| !name.trim().is_empty()))
        .or_else(stem)
        .unwrap_or_default()
}

/// The exe of process `pid`, or `None` if it is gone or out of reach.
fn process_exe(pid: u32) -> Option<PathBuf> {
    if pid == 0 {
        return None;
    }
    // SAFETY: the handle is closed before returning; the buffer outlives the call.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let named = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        CloseHandle(process).log_err();
        named.ok()?;
        Some(PathBuf::from(OsString::from_wide(&buf[..len as usize])))
    }
}

/// A string COM allocated, read and freed.
unsafe fn take(text: PWSTR) -> String {
    if text.is_null() {
        return String::new();
    }
    unsafe {
        let read = text.to_string().unwrap_or_default();
        CoTaskMemFree(Some(text.0 as *const _));
        read
    }
}

#[implement(IAudioSessionNotification)]
struct Notification(mpsc::Sender<Message>);

impl IAudioSessionNotification_Impl for Notification_Impl {
    fn OnSessionCreated(&self, session: Ref<IAudioSessionControl>) -> windows::core::Result<()> {
        // Registering for the session's events in here could deadlock: the thread does it.
        if let Ok(session) = session.ok() {
            let created = Created(session.clone());
            self.0.send(Message::Created(created)).log_err();
        }
        Ok(())
    }
}

#[implement(IAudioSessionEvents)]
struct Listener {
    id: String,
    sender: mpsc::Sender<Message>,
}

impl IAudioSessionEvents_Impl for Listener_Impl {
    fn OnDisplayNameChanged(&self, _: &PCWSTR, _: *const GUID) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnIconPathChanged(&self, _: &PCWSTR, _: *const GUID) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnSimpleVolumeChanged(
        &self,
        volume: f32,
        muted: BOOL,
        context: *const GUID,
    ) -> windows::core::Result<()> {
        // SAFETY: the context is null or points at a GUID for the call's length.
        if unsafe { context.as_ref() } != Some(&OURS) {
            let (id, muted) = (self.id.clone(), muted.as_bool());
            (self.sender.send(Message::Volume { id, volume, muted })).log_err();
        }
        Ok(())
    }

    fn OnChannelVolumeChanged(
        &self,
        _: u32,
        _: *const f32,
        _: u32,
        _: *const GUID,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnGroupingParamChanged(&self, _: *const GUID, _: *const GUID) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnStateChanged(&self, state: AudioSessionState) -> windows::core::Result<()> {
        if state == AudioSessionStateExpired {
            self.sender.send(Message::Gone(self.id.clone())).log_err();
        }
        Ok(())
    }

    fn OnSessionDisconnected(&self, _: AudioSessionDisconnectReason) -> windows::core::Result<()> {
        self.sender.send(Message::Gone(self.id.clone())).log_err();
        Ok(())
    }
}
