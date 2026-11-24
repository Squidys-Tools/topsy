//! Handing a press that started in one window to Windows' move loop in another: the floating
//! player, pulled out of the notch, goes on with the drag as if its own caption had been pressed.

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, ReleaseCapture, VK_LBUTTON, VK_RBUTTON,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, HTCAPTION, PostMessageW, SM_SWAPBUTTON, WM_NCLBUTTONDOWN,
};

use crate::log::LogErr;

/// If the primary button is still held, let `hwnd` be moved by it from where the pointer is.
/// Posted, not sent: the move loop pumps messages, so it must start from GPUI's own loop rather
/// than inside a handler (docs/internals/gpui.md).
pub fn move_with_pointer(hwnd: HWND) {
    let primary = if unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0 {
        VK_RBUTTON
    } else {
        VK_LBUTTON
    };
    if unsafe { GetAsyncKeyState(i32::from(primary.0)) } >= 0 {
        return;
    }
    let mut at = Default::default();
    if unsafe { GetCursorPos(&mut at) }.log_err().is_none() {
        return;
    }
    // The window the press started in holds the mouse until the button comes up.
    unsafe { ReleaseCapture() }.log_err();
    let packed = ((at.y as u16 as u32) << 16) | (at.x as u16 as u32);
    unsafe {
        PostMessageW(
            Some(hwnd),
            WM_NCLBUTTONDOWN,
            WPARAM(HTCAPTION as usize),
            LPARAM(packed as isize),
        )
    }
    .log_err();
}
