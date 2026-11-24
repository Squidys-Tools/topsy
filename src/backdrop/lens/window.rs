//! The lens's window: one that never takes a click or activates, either just under the island
//! or a child of a floating window.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateRoundRectRgn, GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST,
    MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint, MonitorFromWindow, SetWindowRgn,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HTTRANSPARENT, RegisterClassW,
    SET_WINDOW_POS_FLAGS, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SWP_SHOWWINDOW, SetWindowPos, WM_NCHITTEST, WNDCLASSW, WS_CHILD, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::w;

use super::Placed;
use crate::backdrop::Outline;
use crate::log::LogErr;
use model::float::Rect;

unsafe extern "system" fn lens_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCHITTEST {
        return LRESULT(HTTRANSPARENT as isize);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// The lens's window: a topmost tool window, or a child of `parent`. Either answers
/// `HTTRANSPARENT`, so clicks go to the window it is behind. (A layered, click-through window
/// would too, but DirectComposition content on one doesn't show.) A child's content shows
/// between its parent's compositor glass and GPUI's content, which is the parent's topmost tree.
pub fn create(parent: Option<HWND>) -> windows::core::Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = w!("topsy lens");
        // Fails harmlessly for the second lens: the class is already there.
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(lens_proc),
            hInstance: instance.into(),
            lpszClassName: class,
            ..Default::default()
        });
        let (ex_style, style) = match parent {
            Some(_) => (WS_EX_NOREDIRECTIONBITMAP, WS_CHILD),
            None => (
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
                WS_POPUP,
            ),
        };
        CreateWindowExW(
            ex_style,
            class,
            w!("topsy lens"),
            style,
            0,
            0,
            1,
            1,
            parent,
            None,
            Some(instance.into()),
            None,
        )
    }
}

pub fn destroy(window: HWND) {
    unsafe { DestroyWindow(window) }.log_err();
}

/// Just under `island`, wherever it is in the topmost band, with a region of the notch's body,
/// which the island covers.
pub fn place_under(
    window: HWND,
    island: HWND,
    placed: Placed,
    opening: bool,
) -> windows::core::Result<()> {
    let (x, y) = placed.origin;
    let (width, height) = placed.size;
    let Outline {
        x: left,
        y: top,
        width: body_width,
        height: body_height,
        radius,
    } = placed.outline;
    let corner = (2.0 * radius).round() as i32;
    unsafe {
        SetWindowPos(
            window,
            Some(island),
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | shown(opening),
        )?;
        let region = CreateRoundRectRgn(
            left.floor() as i32,
            top.floor() as i32,
            (left + body_width).ceil() as i32 + 1,
            (top + body_height).ceil() as i32 + 1,
            corner,
            corner,
        );
        // The window owns the region from here on.
        SetWindowRgn(window, Some(region), true);
    }
    Ok(())
}

/// A child lens fills its parent, whose region shapes it.
pub fn fill(window: HWND, size: (i32, i32), opening: bool) -> windows::core::Result<()> {
    let flags = SWP_NOACTIVATE | SWP_NOZORDER | shown(opening);
    unsafe { SetWindowPos(window, None, 0, 0, size.0, size.1, flags) }
}

fn shown(opening: bool) -> SET_WINDOW_POS_FLAGS {
    if opening {
        SWP_SHOWWINDOW
    } else {
        Default::default()
    }
}

pub fn restack(window: HWND, island: HWND) {
    let keep = SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE;
    unsafe { SetWindowPos(window, Some(island), 0, 0, 0, 0, keep) }.log_err();
}

pub fn hide(window: HWND) {
    let flags = SWP_HIDEWINDOW | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER;
    unsafe { SetWindowPos(window, None, 0, 0, 0, 0, flags) }.log_err();
}

/// The monitor `window` is on, in physical screen pixels.
pub fn monitor_of(window: HWND) -> windows::core::Result<Rect> {
    bounds(unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTOPRIMARY) })
}

/// The monitor nearest `point` (physical screen pixels).
pub fn monitor_at(point: (i32, i32)) -> windows::core::Result<Rect> {
    let point = POINT {
        x: point.0,
        y: point.1,
    };
    bounds(unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) })
}

fn bounds(monitor: HMONITOR) -> windows::core::Result<Rect> {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetMonitorInfoW(monitor, &mut info) }.ok()?;
    let r = info.rcMonitor;
    Ok(Rect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    })
}
