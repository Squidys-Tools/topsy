//! The Win32 glue GPUI doesn't expose: overlay window styles, glass, window geometry,
//! and the global mouse hook that watches for the jiggle gesture.

use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GWL_EXSTYLE, GWL_STYLE, GetForegroundWindow, GetMessageW,
    GetWindowLongPtrW, HHOOK, HWND_TOPMOST, MSG, MSLLHOOKSTRUCT, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
    SetWindowsHookExW, TranslateMessage, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
};
use windows::core::{s, w};

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
///
/// It also makes the window a bare `WS_POPUP`. GPUI creates popups with style 0, which
/// Windows treats as an overlapped window: it enforces a minimum height and, once GPUI is
/// told not to hide the titlebar, reserves a caption. A bare popup has no non-client area,
/// so the client area is the whole window, which the pill-sized acrylic window relies on.
pub fn apply_overlay_styles(hwnd: HWND) {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let ex = ex | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
        SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_POPUP.0 | WS_VISIBLE.0) as isize);
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

#[repr(C)]
struct AccentPolicy {
    state: u32,
    flags: u32,
    /// 0xAABBGGRR
    gradient_color: u32,
    animation_id: u32,
}

#[repr(C)]
struct CompositionAttribData {
    attrib: u32,
    data: *mut c_void,
    size: usize,
}

const WCA_ACCENT_POLICY: u32 = 0x13;
const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;

/// Turn on the system acrylic blur behind the window, tinted with `tint` (0xRRGGBBAA).
///
/// Uses the undocumented `SetWindowCompositionAttribute`, which is how GPUI's own
/// `WindowBackgroundAppearance::Blurred` works on Windows. It is called directly here
/// only to choose the tint, since GPUI's version hardcodes a near-clear one.
///
/// The blur fills the window's whole rectangle and, on current Windows 11, ignores
/// `SetWindowRgn`, so the window has to *be* the shape that should be blurred
/// (see `Island::sync_window`) and DWM rounds its corners ([`round_corners`]).
pub fn enable_acrylic(hwnd: HWND, tint: u32) {
    type SetWindowCompositionAttribute =
        unsafe extern "system" fn(HWND, *mut CompositionAttribData) -> i32;
    unsafe {
        let Ok(user32) = GetModuleHandleW(w!("user32.dll")) else {
            return;
        };
        let Some(proc) = GetProcAddress(user32, s!("SetWindowCompositionAttribute")) else {
            return;
        };
        let set_attr: SetWindowCompositionAttribute = std::mem::transmute(proc);
        let [r, g, b, a] = tint.to_be_bytes();
        let mut accent = AccentPolicy {
            state: ACCENT_ENABLE_ACRYLICBLURBEHIND,
            flags: 0,
            gradient_color: u32::from_le_bytes([r, g, b, a]),
            animation_id: 0,
        };
        let mut data = CompositionAttribData {
            attrib: WCA_ACCENT_POLICY,
            data: &mut accent as *mut _ as *mut c_void,
            size: std::mem::size_of::<AccentPolicy>(),
        };
        if set_attr(hwnd, &mut data) == 0 {
            eprintln!("topsy: SetWindowCompositionAttribute failed; no glass blur");
        }
    }
}

/// Ask DWM to round the window's corners (and so its acrylic backdrop) by the system radius.
pub fn round_corners(hwnd: HWND) {
    let preference = DWMWCP_ROUND;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as *const c_void,
            std::mem::size_of_val(&preference) as u32,
        );
    }
}

/// Horizontal centre and top edge, in physical screen pixels, of the monitor the window is on.
pub fn monitor_top_center(hwnd: HWND) -> Option<(i32, i32)> {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
    }
    let r = info.rcMonitor;
    Some(((r.left + r.right) / 2, r.top))
}

/// Move and resize without activating.
///
/// Not for use from inside GPUI's `render`: this is synchronous (`SWP_ASYNCWINDOWPOS` only
/// defers across threads), so `WM_SIZE` would reach GPUI mid-draw, where its resize
/// callback cannot update the window, and GPUI would never learn the new size. Run it
/// from a foreground task instead, as GPUI's own `Window::resize` does.
pub fn move_window(hwnd: HWND, x: i32, y: i32, w: i32, h: i32) {
    unsafe {
        let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOACTIVATE | SWP_NOZORDER);
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
                WM_MOUSEMOVE if state.detector.moved(now, info.pt.x) => {
                    let _ = state.tx.try_send(HookEvent::Jiggle);
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
