//! Raising a focusable window of the app's own (the settings window, Transform), unlike the overlay.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow,
};

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
