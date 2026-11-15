//! The notification-area icon and its menu.
//!
//! GPUI owns the main thread's message loop and gives no way into it, so the icon lives on
//! its own thread with a hidden window, like the mouse hook. It reaches the app through a
//! channel of [`TrayEvent`]; the app reaches it through [`show_menu`], [`set_shelf_count`]
//! and [`remove`].

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
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW,
    GetCursorPos, GetMessageW, GetSystemMetrics, HICON, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW,
    MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage,
    RegisterClassW, RegisterWindowMessageW, SM_CXSMICON, SetForegroundWindow, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP, WM_CONTEXTMENU, WM_DESTROY, WM_NULL,
    WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::settings::Setting;
use crate::win32::wide;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    /// Left click on the icon: open the shelf, or close it if it is open.
    Toggle,
    /// "Show shelf" in the menu.
    Show,
    ClearShelf,
    /// A setting was switched to this value; it is saved already.
    Setting(Setting, bool),
    /// "Settings…": open the settings window, or bring it to the front. Carries the window that
    /// was foreground before the menu (as an address: `HWND` can't cross threads), to hand
    /// foreground back to when the settings window closes.
    OpenSettings(usize),
    Quit,
}

/// `NIN_SELECT` with the "from the keyboard" flag (`NINF_KEY`): Enter on the focused icon.
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;

const WM_TRAY: u32 = WM_APP + 1;
const WM_SHOW_MENU: u32 = WM_APP + 2;
const ICON_ID: u32 = 1;

const ID_SHOW: usize = 1;
const ID_CLEAR: usize = 2;
const ID_QUIT: usize = 3;
const ID_SETTINGS: usize = 4;
/// `Setting::ALL[i]` is item `ID_SETTING + i`.
const ID_SETTING: usize = 100;

/// The hidden window on the tray thread; 0 until it exists.
static WINDOW: AtomicIsize = AtomicIsize::new(0);
/// The icon handle, kept so it can be added again after Explorer restarts.
static ICON: AtomicIsize = AtomicIsize::new(0);
/// The "TaskbarCreated" broadcast Explorer sends when it (re)starts.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static SHELF_COUNT: AtomicUsize = AtomicUsize::new(0);
static EVENTS: OnceLock<async_channel::Sender<TrayEvent>> = OnceLock::new();

fn send(event: TrayEvent) {
    if let Some(tx) = EVENTS.get() {
        let _ = tx.try_send(event);
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
            let _ = Shell_NotifyIconW(NIM_DELETE, &data);
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
        .expect("spawn tray thread");
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
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
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
                let _ = GetCursorPos(&mut at);
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

/// Show the menu at `at` and carry out the choice.
unsafe fn open_menu(hwnd: HWND, at: POINT) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let count = SHELF_COUNT.load(Ordering::Relaxed);
        let clear = wide(format!("&Clear shelf ({count})"));
        let clear_flags = if count > 0 {
            MF_STRING
        } else {
            MF_STRING | MF_GRAYED
        };
        let _ = AppendMenuW(menu, MF_STRING, ID_SHOW, w!("&Show shelf"));
        let _ = AppendMenuW(menu, clear_flags, ID_CLEAR, PCWSTR(clear.as_ptr()));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        for (i, setting) in Setting::ALL.into_iter().enumerate() {
            let flags = if setting.is_on() {
                MF_STRING | MF_CHECKED
            } else {
                MF_STRING
            };
            let label = wide(setting.menu_label());
            let _ = AppendMenuW(menu, flags, ID_SETTING + i, PCWSTR(label.as_ptr()));
        }
        let _ = AppendMenuW(menu, MF_STRING, ID_SETTINGS, w!("Se&ttings…"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_QUIT, w!("&Quit topsy"));

        // The owner has to be foreground or the menu won't close when you click elsewhere.
        // This is the hidden window, so it takes focus from nothing visible; it is handed back
        // afterwards, or keys would go nowhere until the next click.
        let previous = crate::win32::foreground_window();
        let _ = SetForegroundWindow(hwnd);
        let choice = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            at.x,
            at.y,
            None,
            hwnd,
            None,
        );
        // Documented companion to the above: lets the menu dismiss properly next time.
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        // The settings window takes foreground itself, which needs the app to still hold it.
        if choice.0 as usize != ID_SETTINGS {
            crate::win32::restore_foreground(previous);
        }

        match choice.0 as usize {
            ID_SHOW => send(TrayEvent::Show),
            ID_CLEAR => send(TrayEvent::ClearShelf),
            ID_QUIT => send(TrayEvent::Quit),
            ID_SETTINGS => send(TrayEvent::OpenSettings(previous.0 as usize)),
            id => {
                if let Some(&setting) = id.checked_sub(ID_SETTING).and_then(|i| Setting::ALL.get(i))
                    && let Some(on) = setting.toggle()
                {
                    send(TrayEvent::Setting(setting, on));
                }
            }
        }
    }
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
