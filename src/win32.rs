//! The Win32 glue GPUI doesn't expose: overlay window styles, hit-test region,
//! and the global mouse hook that watches for the jiggle gesture.

use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, DeleteObject, HGDIOBJ, SetWindowRgn};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GWL_EXSTYLE, GetForegroundWindow, GetMessageW,
    GetWindowLongPtrW, HHOOK, HWND_TOPMOST, MSG, MSLLHOOKSTRUCT, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
    SetWindowsHookExW, TranslateMessage, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

use crate::jiggle::{JiggleConfig, JiggleDetector};

pub fn hwnd_from_raw(handle: raw_window_handle::RawWindowHandle) -> Option<HWND> {
    match handle {
        raw_window_handle::RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut c_void)),
        _ => None,
    }
}

/// Make the window an overlay: topmost, never takes focus from the app being used.
///
/// GPUI ignores `WindowOptions::focus` on Windows and activates the window when it is
/// created. A window created with `show: false` is not an option: GPUI defers its
/// placement until `activate()` and never paints it. So the window is created visible,
/// this applies `WS_EX_NOACTIVATE` so nothing activates it afterwards, and the caller
/// hands foreground back to whatever had it (see [`foreground_window`]).
pub fn apply_overlay_styles(hwnd: HWND) {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let ex = ex | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

pub fn foreground_window() -> HWND {
    unsafe { GetForegroundWindow() }
}

/// Give foreground back to `previous` (the window that had it before ours opened).
pub fn restore_foreground(previous: HWND) {
    if !previous.0.is_null() {
        unsafe {
            let _ = SetForegroundWindow(previous);
        }
    }
}

/// Clip the window (drawing *and* hit-testing) to a rounded rect, in physical
/// pixels relative to the window's top-left. Everything outside is click-through
/// because it is no longer part of the window.
pub fn set_hit_region(hwnd: HWND, left: i32, top: i32, right: i32, bottom: i32, radius: i32) {
    unsafe {
        let rgn = CreateRoundRectRgn(left, top, right + 1, bottom + 1, radius * 2, radius * 2);
        // On success the system owns the region; only free it if the call failed.
        if SetWindowRgn(hwnd, Some(rgn), true) == 0 {
            let _ = DeleteObject(HGDIOBJ(rgn.0));
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookEvent {
    Jiggle,
    ButtonUp,
}

struct HookState {
    detector: JiggleDetector,
    tx: async_channel::Sender<HookEvent>,
}

static HOOK_STATE: Mutex<Option<HookState>> = Mutex::new(None);
static STARTED: OnceLock<()> = OnceLock::new();

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        let now = Instant::now();
        if let Ok(mut guard) = HOOK_STATE.try_lock()
            && let Some(state) = guard.as_mut()
        {
            match wparam.0 as u32 {
                WM_LBUTTONDOWN => state.detector.button_down(now, info.pt.x),
                WM_LBUTTONUP => {
                    state.detector.button_up();
                    let _ = state.tx.try_send(HookEvent::ButtonUp);
                }
                WM_MOUSEMOVE => {
                    if state.detector.moved(now, info.pt.x) {
                        let _ = state.tx.try_send(HookEvent::Jiggle);
                    }
                }
                _ => {}
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Install the low-level mouse hook on a dedicated thread (hooks need a message
/// loop on the thread that installed them). Safe to call once; later calls are no-ops.
pub fn spawn_mouse_hook(config: JiggleConfig) -> async_channel::Receiver<HookEvent> {
    let (tx, rx) = async_channel::bounded(8);
    if STARTED.set(()).is_err() {
        return rx;
    }
    *HOOK_STATE.lock().unwrap() = Some(HookState {
        detector: JiggleDetector::new(config),
        tx,
    });
    std::thread::Builder::new()
        .name("topsy-mouse-hook".into())
        .spawn(|| unsafe {
            let hook: HHOOK = match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0) {
                Ok(h) => h,
                Err(err) => {
                    eprintln!("topsy: could not install mouse hook: {err}");
                    return;
                }
            };
            let _keep_alive = hook;
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .expect("spawn mouse hook thread");
    rx
}
