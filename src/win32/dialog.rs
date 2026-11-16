//! An ordinary, focusable window of the app's own (the settings window), unlike the overlay.

use std::ffi::c_void;

use windows::Win32::Foundation::{HWND, TRUE};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::Win32::UI::WindowsAndMessaging::{
    IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow,
};

/// A dark caption whatever the system theme, to match the dark content under it.
pub fn dark_title_bar(hwnd: HWND) {
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&TRUE as *const _) as *const c_void,
            size_of_val(&TRUE) as u32,
        );
    }
}

/// Restore and raise the window. Called right after the menu that asked for it closes, while
/// the app still holds the foreground, so Windows lets it through. GPUI's own `activate_window`
/// injects a synthetic Alt press to get the same right, which is not ours to send.
pub fn bring_to_front(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}
