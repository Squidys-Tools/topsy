//! The notification-area icon and its menu.
//!
//! GPUI owns the main thread's message loop and gives no way into it, so the icon lives on
//! its own thread with a hidden window, like the mouse hook. It reaches the app through a
//! channel of [`TrayEvent`]; the app reaches it through [`show_menu`], [`set_shelf_count`]
//! and [`remove`]. The menu itself is a glass window on the UI thread ([`menu`]).

mod lines;
pub(crate) mod menu;
mod menu_view;

use std::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicIsize, AtomicU32, AtomicUsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};

use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION, NIN_SELECT,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics,
    HICON, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW, MSG, PostMessageW, PostQuitMessage,
    RegisterClassW, RegisterWindowMessageW, SM_CXSMICON, SetForegroundWindow, TranslateMessage,
    WM_APP, WM_CONTEXTMENU, WM_DESTROY, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::log::LogErr;
use crate::settings::Setting;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    /// Left click on the icon: open the shelf, or close it if it is open.
    Toggle,
    /// Right click on the icon, or on the notch: open the menu at `at` (physical screen
    /// pixels). `previous` is the window that was foreground before, as for `OpenSettings`.
    Menu {
        at: (i32, i32),
        previous: usize,
    },
    /// "Show shelf" in the menu.
    Show,
    ClearShelf,
    /// "Show basket": bring the basket up with what it holds, as a jiggle would.
    ShowBasket,
    /// A setting was switched to this value; it is saved already.
    Setting(Setting, bool),
    /// "Settings…": open the settings window, or bring it to the front. Carries the window that
    /// was foreground before the menu (as an address: `HWND` can't cross threads), to hand
    /// foreground back to when the settings window closes.
    OpenSettings(usize),
    /// "Clipboard history": open the clipboard strip, to paste into `previous`.
    Clipboard(usize),
    /// "Add my headset…": hand the user's headsets to a coding agent (`headset::report`).
    AddHeadset,
    /// "Restart to update": install the downloaded nightly and start it.
    Update,
    Quit,
}

/// `NIN_SELECT` with the "from the keyboard" flag (`NINF_KEY`): Enter on the focused icon.
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;

const WM_TRAY: u32 = WM_APP + 1;
const WM_SHOW_MENU: u32 = WM_APP + 2;
const ICON_ID: u32 = 1;

/// The hidden window on the tray thread; 0 until it exists.
static WINDOW: AtomicIsize = AtomicIsize::new(0);
/// The icon handle, kept so it can be added again after Explorer restarts.
static ICON: AtomicIsize = AtomicIsize::new(0);
/// The "TaskbarCreated" broadcast Explorer sends when it (re)starts.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static SHELF_COUNT: AtomicUsize = AtomicUsize::new(0);
static BASKET_COUNT: AtomicUsize = AtomicUsize::new(0);
static EVENTS: OnceLock<async_channel::Sender<TrayEvent>> = OnceLock::new();

pub(crate) fn send(event: TrayEvent) {
    if let Some(tx) = EVENTS.get() {
        tx.try_send(event).log_err();
    }
}

fn window() -> Option<HWND> {
    let raw = WINDOW.load(Ordering::Acquire);
    (raw != 0).then_some(HWND(raw as *mut c_void))
}

/// Tell the menu how many files are on the shelf (it greys out "Clear shelf" at zero).
pub fn set_shelf_count(count: usize) {
    SHELF_COUNT.store(count, Ordering::Relaxed);
}

/// Tell the menu how many files are in the basket (it greys out "Show basket" at zero).
pub fn set_basket_count(count: usize) {
    BASKET_COUNT.store(count, Ordering::Relaxed);
}

fn shelf_count() -> usize {
    SHELF_COUNT.load(Ordering::Relaxed)
}

fn basket_count() -> usize {
    BASKET_COUNT.load(Ordering::Relaxed)
}

/// Make the hidden window foreground, so topsy keeps the right to bring a window of its own to
/// the front (the settings window, the clipboard strip) once the menu has closed.
fn hold_foreground() {
    if let Some(hwnd) = window() {
        unsafe {
            #[expect(
                clippy::let_underscore_must_use,
                reason = "Windows may refuse the foreground; the window opens behind then"
            )]
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

/// Pop the menu up at the cursor, as if the icon had been right-clicked. For the pill.
/// False if there is no tray to do it (it is switched off, or its window could not be made).
pub fn show_menu() -> bool {
    let Some(hwnd) = window() else {
        return false;
    };
    unsafe { PostMessageW(Some(hwnd), WM_SHOW_MENU, WPARAM(0), LPARAM(0)) }.is_ok()
}

/// Take the icon out of the notification area. Without this a killed-off icon lingers
/// until the mouse passes over it. Safe to call from any thread, and more than once.
pub fn remove() {
    if let Some(hwnd) = window() {
        let data = icon_data(hwnd, NIF_MESSAGE);
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &data).ok().log_err();
        }
    }
}

/// Start the tray thread. Later calls are no-ops that return a receiver which never fires.
pub fn spawn() -> async_channel::Receiver<TrayEvent> {
    // Unbounded: a setting is saved before its event is sent, so a dropped event would leave
    // the island disagreeing with the registry.
    let (tx, rx) = async_channel::unbounded();
    if EVENTS.set(tx).is_err() {
        return rx;
    }
    std::thread::Builder::new()
        .name("topsy-tray".into())
        .spawn(|| unsafe { run() })
        .log_err();
    rx
}

unsafe fn run() {
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return;
        };
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: module.into(),
            lpszClassName: w!("topsy-tray"),
            ..Default::default()
        };
        RegisterClassW(&class);
        // A real (never shown) top-level window rather than a message-only one: only
        // top-level windows get the "TaskbarCreated" broadcast.
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("topsy-tray"),
            w!("topsy"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(module.into()),
            None,
        ) else {
            eprintln!("topsy: could not create the tray window");
            return;
        };
        TASKBAR_CREATED.store(
            RegisterWindowMessageW(w!("TaskbarCreated")),
            Ordering::Relaxed,
        );
        if let Some(icon) = make_icon() {
            ICON.store(icon.0 as isize, Ordering::Release);
        }
        WINDOW.store(hwnd.0 as isize, Ordering::Release);
        add_icon(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            #[expect(
                clippy::let_underscore_must_use,
                reason = "says whether a character message was posted, not an error"
            )]
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn icon_data(
    hwnd: HWND,
    flags: windows::Win32::UI::Shell::NOTIFY_ICON_DATA_FLAGS,
) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: ICON_ID,
        uFlags: flags,
        ..Default::default()
    }
}

unsafe fn add_icon(hwnd: HWND) {
    let mut data = icon_data(hwnd, NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP);
    data.uCallbackMessage = WM_TRAY;
    data.hIcon = HICON(ICON.load(Ordering::Acquire) as *mut c_void);
    for (slot, unit) in data.szTip.iter_mut().zip("topsy".encode_utf16()) {
        *slot = unit;
    }
    unsafe {
        if !Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
            // Explorer isn't up yet (we may have started at logon). It broadcasts
            // "TaskbarCreated" when it is, and the window procedure adds the icon then.
            eprintln!("topsy: could not add the tray icon (yet)");
            return;
        }
        // Version 4 reports the click position and keyboard activation properly.
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        Shell_NotifyIconW(NIM_SETVERSION, &data).ok().log_err();
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            match (lparam.0 as u32) & 0xffff {
                NIN_SELECT | NIN_KEYSELECT => send(TrayEvent::Toggle),
                WM_CONTEXTMENU => {
                    // Version 4: wParam is the click position in screen coordinates.
                    let at = POINT {
                        x: (wparam.0 & 0xffff) as i16 as i32,
                        y: ((wparam.0 >> 16) & 0xffff) as i16 as i32,
                    };
                    unsafe { open_menu(hwnd, at) };
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_SHOW_MENU => {
            let mut at = POINT::default();
            unsafe {
                GetCursorPos(&mut at).log_err();
                open_menu(hwnd, at);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            unsafe { add_icon(hwnd) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Ask the UI thread for the menu at `at`. The click on the icon (or the notch) lets topsy
/// take the foreground now, so the hidden window takes it, for the menu to take from it.
unsafe fn open_menu(hwnd: HWND, at: POINT) {
    let previous = crate::win32::foreground_window();
    unsafe {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "Windows may refuse the foreground; the menu still opens"
        )]
        let _ = SetForegroundWindow(hwnd);
    }
    send(TrayEvent::Menu {
        at: (at.x, at.y),
        previous: previous.0 as usize,
    });
}

/// The exe's own icon (resource 1, written by `build.rs`) at the shell's small-icon size.
fn make_icon() -> Option<HICON> {
    unsafe {
        let module = GetModuleHandleW(None).ok()?;
        let side = GetSystemMetrics(SM_CXSMICON);
        let handle = LoadImageW(
            Some(module.into()),
            PCWSTR(1 as _),
            IMAGE_ICON,
            side,
            side,
            LR_DEFAULTCOLOR,
        )
        .ok()?;
        Some(HICON(handle.0))
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_exe_carries_its_icon() {
        assert!(make_icon().is_some());
    }
}
