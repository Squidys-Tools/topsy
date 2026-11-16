//! The volume keys, taken from Windows so only topsy's HUD shows. A low-level keyboard hook on
//! a thread of its own swallows VolumeUp, VolumeDown and Mute (every other key passes straight
//! through) and hands them to a second thread, which changes the default output device's
//! volume by the device's own step, as Windows does, and reports where it got to.

use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator, eMultimedia, eRender};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_VOLUME_DOWN, VK_VOLUME_MUTE, VK_VOLUME_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG,
    PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN,
};

#[derive(Clone, Copy, Debug)]
enum Key {
    Up,
    Down,
    Mute,
}

static KEYS: OnceLock<mpsc::Sender<Key>> = OnceLock::new();
/// The hook thread's id while the keys are taken.
static HOOK: Mutex<Option<u32>> = Mutex::new(None);

/// Start the thread that changes the volume; `report` hears the level (0 to 1) and whether it
/// is muted after each key. The keys reach it only while [`take_keys`] has them.
pub fn start(report: impl Fn(f32, bool) + Send + 'static) {
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("topsy-volume".into())
        .spawn(move || {
            // SAFETY: COM for this thread only.
            let devices = unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
            };
            let devices = match devices {
                Ok(devices) => devices,
                Err(err) => return eprintln!("topsy: no volume control: {err}"),
            };
            // Only now does the hook take the keys; without a volume control Windows keeps them.
            let _ = KEYS.set(tx);
            for key in rx {
                // SAFETY: COM calls on the thread that created `devices`.
                match unsafe { press(&devices, key) } {
                    Ok((level, muted)) => report(level, muted),
                    Err(err) => crate::log(&format!("volume {key:?}: {err}")),
                }
            }
        });
    if let Err(err) = spawned {
        eprintln!("topsy: no volume control: {err}");
    }
}

/// The device playing now, fetched on each press so a change of output device just works.
unsafe fn press(devices: &IMMDeviceEnumerator, key: Key) -> windows::core::Result<(f32, bool)> {
    unsafe {
        let device = devices.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
        let volume: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;
        let none = std::ptr::null();
        match key {
            // Turning it up or down unmutes, as Windows' own keys do.
            Key::Up => {
                volume.SetMute(false, none)?;
                volume.VolumeStepUp(none)?;
            }
            Key::Down => {
                volume.SetMute(false, none)?;
                volume.VolumeStepDown(none)?;
            }
            Key::Mute => volume.SetMute(!volume.GetMute()?.as_bool(), none)?,
        }
        Ok((
            volume.GetMasterVolumeLevelScalar()?,
            volume.GetMute()?.as_bool(),
        ))
    }
}

/// Take the volume keys from Windows, or hand them back.
pub fn take_keys(on: bool) {
    let mut hook = HOOK.lock().unwrap_or_else(|e| e.into_inner());
    match (on, *hook) {
        (true, None) => *hook = spawn_hook(),
        (false, Some(thread)) => {
            // SAFETY: posting to a thread of ours; it unhooks and ends on WM_QUIT.
            let _ = unsafe { PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0)) };
            *hook = None;
        }
        _ => {}
    }
}

/// The hook's thread, once its hook is in, or `None` if it could not be installed.
fn spawn_hook() -> Option<u32> {
    let (ready, installed) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("topsy-keys".into())
        .spawn(move || unsafe {
            let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_proc), None, 0) {
                Ok(hook) => hook,
                Err(err) => {
                    eprintln!("topsy: could not take the volume keys: {err}");
                    let _ = ready.send(None);
                    return;
                }
            };
            let _ = ready.send(Some(GetCurrentThreadId()));
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let _ = UnhookWindowsHookEx(hook);
        });
    spawned.ok()?;
    installed.recv().ok().flatten()
}

unsafe extern "system" fn key_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && let Some(keys) = KEYS.get()
    {
        // SAFETY: for HC_ACTION, `lparam` points at the key's KBDLLHOOKSTRUCT.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let key = match VIRTUAL_KEY(info.vkCode as u16) {
            VK_VOLUME_UP => Some(Key::Up),
            VK_VOLUME_DOWN => Some(Key::Down),
            VK_VOLUME_MUTE => Some(Key::Mute),
            _ => None,
        };
        if let Some(key) = key {
            // Held keys repeat their key-down, and each one is a step, as in Windows.
            if matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN) {
                let _ = keys.send(key);
            }
            // Both halves of the press are swallowed, so Windows never sees it.
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
