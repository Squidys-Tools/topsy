//! The Win32 glue GPUI doesn't expose: the overlay window, the settings window, drag and drop,
//! the shell, and the global mouse hook, plus the small helpers they share.

pub mod battery;
pub mod brightness;
pub mod clipboard;
pub mod clock;
mod dialog;
mod dragdrop;
pub mod earbuds;
pub mod escape;
pub mod folder;
mod hook;
pub mod http;
pub mod inbox;
pub mod location;
mod menu;
pub mod meter;
pub mod paste;
mod picker;
pub mod sendto;
pub mod share;
mod shell;
pub mod sound;
pub mod sysinfo;
pub mod volume;
mod window;

pub use dialog::{bring_to_front, dark_title_bar, mica};
pub use dragdrop::{DragOutcome, DropEvent, drag_files_out, watch_drop_refusals};
pub use hook::{HookEvent, spawn_mouse_hook};
pub use menu::{Entry as MenuEntry, popup as popup_menu};
pub use picker::pick_files;
pub use shell::{move_new, open_path, show_in_folder};
pub use window::{
    Capsule, Clip, apply_overlay_styles, apply_strip_styles, cursor_monitor, cursor_over,
    foreground_window, fullscreen_over, monitor_top_center, move_window, restore_foreground,
    round_corners, set_acrylic, set_clip, set_visible, window_rect,
};

use std::ffi::c_void;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{COINIT, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_SHIFT,
};

pub fn hwnd_from_raw(handle: raw_window_handle::RawWindowHandle) -> Option<HWND> {
    match handle {
        raw_window_handle::RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut c_void)),
        _ => None,
    }
}

/// `s` as the NUL-terminated UTF-16 the Windows APIs take.
pub fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

/// Run `f` with COM set up on this thread, which a fresh thread doesn't have and the shell's
/// APIs need. A single-threaded apartment only answers calls from other apartments while it
/// pumps messages, so a thread that doesn't pump and calls out-of-process handlers (thumbnails)
/// wants `COINIT_MULTITHREADED`.
pub fn with_com<T>(apartment: COINIT, f: impl FnOnce() -> T) -> T {
    let com = unsafe { CoInitializeEx(None, apartment | COINIT_DISABLE_OLE1DDE) };
    let result = f();
    if com.is_ok() {
        unsafe { CoUninitialize() };
    }
    result
}

/// The modifier keys down right now.
pub struct HeldKeys {
    pub ctrl: bool,
    pub shift: bool,
}

/// Which modifiers are down, read from the keyboard itself: the island never has the keyboard,
/// so its thread's key state (what GPUI's event modifiers come from) can be stale.
pub fn held_keys() -> HeldKeys {
    let down = |key: VIRTUAL_KEY| unsafe { GetAsyncKeyState(i32::from(key.0)) < 0 };
    HeldKeys {
        ctrl: down(VK_CONTROL),
        shift: down(VK_SHIFT),
    }
}
