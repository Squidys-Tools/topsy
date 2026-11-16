//! The overlay window and its geometry: styles, glass, placement, clipping, and foreground.

use std::ffi::c_void;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, GetMonitorInfoW, HMONITOR,
    MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
    MonitorFromPoint, MonitorFromWindow, PtInRect, RGN_OR, SetWindowRgn,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GWL_STYLE, GetClassNameW, GetCursorPos, GetForegroundWindow, GetShellWindow,
    GetWindowLongPtrW, GetWindowRect, HWND_TOPMOST, IsZoomed, MA_NOACTIVATE, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetWindowLongPtrW,
    SetWindowPos, WM_MOUSEACTIVATE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
};
use windows::core::{s, w};

use crate::model::edge::fills_monitor;
use crate::model::float::Rect;

/// The monitor under the cursor, and where the cursor is on it.
fn cursor_at() -> Option<(POINT, HMONITOR)> {
    let mut at = POINT::default();
    unsafe { GetCursorPos(&mut at) }.ok()?;
    Some((at, unsafe {
        MonitorFromPoint(at, MONITOR_DEFAULTTONEAREST)
    }))
}

/// Where the cursor is, the work area of its monitor (less the taskbar) and that monitor's
/// scale factor, all in physical screen pixels.
pub fn cursor_monitor() -> Option<((i32, i32), Rect, f32)> {
    let (at, monitor) = cursor_at()?;
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let (mut dpi, mut _dpi_y) = (96u32, 96u32);
    unsafe {
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut _dpi_y);
    }
    let w = info.rcWork;
    let work = Rect {
        left: w.left,
        top: w.top,
        right: w.right,
        bottom: w.bottom,
    };
    Some(((at.x, at.y), work, dpi as f32 / 96.0))
}

/// Whether the foreground window is a fullscreen app on the monitor under the cursor, where
/// every gesture that summons the island happens. The desktop counts as neither: it fills the
/// monitor too, but its windows are `Progman` and `WorkerW`.
pub fn fullscreen_over(island: HWND) -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() || fg == island || fg == GetShellWindow() {
            return false;
        }
        let mut class = [0u16; 16];
        let len = GetClassNameW(fg, &mut class) as usize;
        if matches!(
            String::from_utf16_lossy(&class[..len]).as_str(),
            "Progman" | "WorkerW"
        ) {
            return false;
        }
        let Some((_, monitor)) = cursor_at() else {
            return false;
        };
        if MonitorFromWindow(fg, MONITOR_DEFAULTTONULL) != monitor {
            return false;
        }
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let mut rect = RECT::default();
        if !GetMonitorInfoW(monitor, &mut info).as_bool() || GetWindowRect(fg, &mut rect).is_err() {
            return false;
        }
        let m = info.rcMonitor;
        fills_monitor(
            (rect.left, rect.top, rect.right, rect.bottom),
            (m.left, m.top, m.right, m.bottom),
            IsZoomed(fg).as_bool(),
        )
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
        let _ = SetWindowSubclass(hwnd, Some(no_mouse_activate), 1, 0);
    }
}

/// `WS_EX_NOACTIVATE` keeps a click from making the island the foreground window, but while
/// topsy already holds the foreground (its settings window is in front) the default answer to
/// `WM_MOUSEACTIVATE`, which GPUI leaves to `DefWindowProc`, still makes it the active window.
unsafe extern "system" fn no_mouse_activate(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if msg == WM_MOUSEACTIVATE {
        return LRESULT(MA_NOACTIVATE as isize);
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
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
const ACCENT_DISABLED: u32 = 0;
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
pub fn set_acrylic(hwnd: HWND, tint: Option<u32>) {
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
        let [r, g, b, a] = tint.unwrap_or(0).to_be_bytes();
        let mut accent = AccentPolicy {
            state: if tint.is_some() {
                ACCENT_ENABLE_ACRYLICBLURBEHIND
            } else {
                ACCENT_DISABLED
            },
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
///
/// `raise` also puts it above every other topmost window: the one given foreground back at
/// launch, or any that came along while the notch was hidden, would otherwise cover it.
pub fn move_window(hwnd: HWND, x: i32, y: i32, w: i32, h: i32, raise: bool) {
    let (after, order) = if raise {
        (Some(HWND_TOPMOST), Default::default())
    } else {
        (None, SWP_NOZORDER)
    };
    unsafe {
        let _ = SetWindowPos(hwnd, after, x, y, w, h, SWP_NOACTIVATE | order);
    }
}

/// What of the window may draw and take clicks, in physical pixels of the window.
pub enum Clip {
    /// Nothing: the notch is hidden.
    Nothing,
    /// The whole window (the acrylic fallback, which DWM rounds itself).
    Everything,
    /// A rounded rectangle filling the window: the floating tray.
    Rounded {
        width: i32,
        height: i32,
        radius: i32,
    },
    /// A notch body with square top, rounded bottom and an ear strip along the top edge.
    Notch {
        width: i32,
        height: i32,
        ear_slot: i32,
        ear: i32,
        radius: i32,
    },
}

/// Shape the window with a region, so the transparent corners outside the notch are
/// click-through.
///
/// The region clips drawing too, without anti-aliasing, so its corners are a pixel looser than
/// the drawn ones: it then falls just outside the compositor's smooth edge instead of cutting it.
/// The ears get their whole strip: what lies outside their curve is a few pixels.
pub fn set_clip(hwnd: HWND, clip: Clip) {
    unsafe {
        let region = match clip {
            Clip::Everything => None,
            Clip::Nothing => Some(CreateRectRgn(0, 0, 0, 0)),
            Clip::Rounded {
                width,
                height,
                radius,
            } => {
                let d = 2 * (radius - 1).max(0);
                Some(CreateRoundRectRgn(0, 0, width + 1, height + 1, d, d))
            }
            Clip::Notch {
                width,
                height,
                ear_slot,
                ear,
                radius,
            } => {
                let r = (radius - 1).max(0);
                // Starting the round rectangle above the window squares off its top corners.
                // The ellipse is given by its diameter, and the right/bottom edges are exclusive.
                let body = CreateRoundRectRgn(
                    ear_slot,
                    -2 * r,
                    width - ear_slot + 1,
                    height + 1,
                    2 * r,
                    2 * r,
                );
                let ears = CreateRectRgn(ear_slot - ear, 0, width - ear_slot + ear, ear);
                CombineRgn(Some(body), Some(body), Some(ears), RGN_OR);
                let _ = DeleteObject(ears.into());
                Some(body)
            }
        };
        // The system owns the region from here on.
        SetWindowRgn(hwnd, region, false);
    }
}

/// Is the cursor over the window's rectangle right now? Asked of the system directly
/// rather than inferred from hover events, which can miss a "leave" while a drag has the mouse.
pub fn cursor_over(hwnd: HWND) -> bool {
    let mut at = POINT::default();
    let mut rect = RECT::default();
    unsafe {
        GetCursorPos(&mut at).is_ok()
            && GetWindowRect(hwnd, &mut rect).is_ok()
            && PtInRect(&rect, at).as_bool()
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
