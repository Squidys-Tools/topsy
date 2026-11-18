//! An ordinary, focusable window of the app's own (the settings window), unlike the overlay.

use std::ffi::c_void;

use windows::Win32::Foundation::{HWND, TRUE};
use windows::Win32::Graphics::Dwm::{
    DWMSBT_MAINWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::{
    IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow,
};

use crate::log::LogErr;

/// A dark caption whatever the system theme, to match the dark content under it.
pub fn dark_title_bar(hwnd: HWND) {
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&TRUE as *const _) as *const c_void,
            size_of_val(&TRUE) as u32,
        )
        .log_err();
    }
}

/// Mica behind the whole window, caption included. False before Windows 11 22H2, which has no
/// system backdrop; the caller then paints its own fill. Mica only shows through where the
/// content is transparent, so the frame is extended over the client area.
pub fn mica(hwnd: HWND) -> bool {
    unsafe {
        let backdrop = DWMSBT_MAINWINDOW;
        let set = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            (&backdrop as *const _) as *const c_void,
            size_of_val(&backdrop) as u32,
        );
        let whole = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        set.is_ok() && DwmExtendFrameIntoClientArea(hwnd, &whole).is_ok()
    }
}

/// Restore and raise the window. Called right after the menu that asked for it closes, while
/// the app still holds the foreground, so Windows lets it through. GPUI's own `activate_window`
/// injects a synthetic Alt press to get the same right, which is not ours to send.
pub fn bring_to_front(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            #[expect(
                clippy::let_underscore_must_use,
                reason = "returns whether it was visible before, not an error"
            )]
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        #[expect(
            clippy::let_underscore_must_use,
            reason = "Windows may refuse the foreground; nothing to do then"
        )]
        let _ = SetForegroundWindow(hwnd);
    }
}
