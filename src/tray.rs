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
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
    DeleteObject, HGDIOBJ,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION, NIN_SELECT,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DispatchMessageW, GetCursorPos, GetMessageW, HICON, ICONINFO, MF_CHECKED, MF_GRAYED,
    MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SetForegroundWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
    TranslateMessage, WM_APP, WM_CONTEXTMENU, WM_DESTROY, WM_NULL, WNDCLASSW, WS_EX_TOOLWINDOW,
    WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::autostart;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    /// Left click on the icon: open the shelf, or close it if it is open.
    Toggle,
    /// "Show shelf" in the menu.
    Show,
    ClearShelf,
    Quit,
}

/// `NIN_SELECT` with the "from the keyboard" flag (`NINF_KEY`): Enter on the focused icon.
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;

const WM_TRAY: u32 = WM_APP + 1;
const WM_SHOW_MENU: u32 = WM_APP + 2;
const ICON_ID: u32 = 1;

const ID_SHOW: usize = 1;
const ID_CLEAR: usize = 2;
const ID_AUTOSTART: usize = 3;
const ID_QUIT: usize = 4;

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
pub fn show_menu() {
    if let Some(hwnd) = window() {
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_SHOW_MENU, WPARAM(0), LPARAM(0));
        }
    }
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
    let (tx, rx) = async_channel::bounded(8);
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

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Show the menu at `at` and carry out the choice.
unsafe fn open_menu(hwnd: HWND, at: POINT) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let count = SHELF_COUNT.load(Ordering::Relaxed);
        let clear = wide(&format!("&Clear shelf ({count})"));
        let clear_flags = if count > 0 {
            MF_STRING
        } else {
            MF_STRING | MF_GRAYED
        };
        let autostart_flags = if autostart::is_enabled() {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let _ = AppendMenuW(menu, MF_STRING, ID_SHOW, w!("&Show shelf"));
        let _ = AppendMenuW(menu, clear_flags, ID_CLEAR, PCWSTR(clear.as_ptr()));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(
            menu,
            autostart_flags,
            ID_AUTOSTART,
            w!("Start with &Windows"),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_QUIT, w!("&Quit topsy"));

        // The owner has to be foreground or the menu won't close when you click elsewhere.
        // This is the hidden window, so it takes focus from nothing visible.
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

        match choice.0 as usize {
            ID_SHOW => send(TrayEvent::Show),
            ID_CLEAR => send(TrayEvent::ClearShelf),
            ID_QUIT => send(TrayEvent::Quit),
            ID_AUTOSTART => toggle_autostart(),
            _ => {}
        }
    }
}

fn toggle_autostart() {
    let result = if autostart::is_enabled() {
        autostart::disable()
    } else {
        std::env::current_exe()
            .map_err(windows::core::Error::from)
            .and_then(|exe| autostart::enable(&exe))
    };
    if let Err(err) = result {
        eprintln!("topsy: could not change start-with-Windows: {err}");
    }
}

const ICON_SIZE: usize = 32;

/// The icon, drawn in code so there is no binary asset to ship: a glass-blue pill with a
/// lighter top edge, like the island. BGRA, top-down, straight alpha.
fn icon_pixels() -> Vec<u8> {
    let (cx, cy) = (ICON_SIZE as f32 / 2.0, ICON_SIZE as f32 / 2.0);
    let (half_length, radius) = (6.0_f32, 8.0_f32);
    let mut pixels = Vec::with_capacity(ICON_SIZE * ICON_SIZE * 4);
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let (px, py) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            // Distance to the capsule's spine, minus its radius: negative inside.
            let along = px.clamp(-half_length, half_length);
            let distance = ((px - along).powi(2) + py.powi(2)).sqrt() - radius;
            let coverage = (0.5 - distance).clamp(0.0, 1.0);
            // Top (lighter) to bottom (deeper) across the pill's height.
            let t = ((py + radius) / (2.0 * radius)).clamp(0.0, 1.0);
            let mix = |top: f32, bottom: f32| (top + (bottom - top) * t) as u8;
            pixels.extend_from_slice(&[
                mix(255.0, 224.0),
                mix(155.0, 102.0),
                mix(142.0, 90.0),
                (coverage * 255.0).round() as u8,
            ]);
        }
    }
    pixels
}

fn make_icon() -> Option<HICON> {
    let pixels = icon_pixels();
    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: ICON_SIZE as i32,
        biHeight: -(ICON_SIZE as i32), // top-down
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let info = BITMAPINFO {
        bmiHeader: header,
        ..Default::default()
    };
    unsafe {
        let mut bits: *mut c_void = std::ptr::null_mut();
        let color = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u8>(), pixels.len());
        // The mask is ignored for a 32-bit icon with alpha, but the call insists on one.
        let mask = CreateBitmap(ICON_SIZE as i32, ICON_SIZE as i32, 1, 1, None);
        let icon = CreateIconIndirect(&ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        })
        .ok();
        let _ = DeleteObject(HGDIOBJ(color.0));
        let _ = DeleteObject(HGDIOBJ(mask.0));
        icon
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(pixels: &[u8], x: usize, y: usize) -> u8 {
        pixels[(y * ICON_SIZE + x) * 4 + 3]
    }

    #[test]
    fn icon_is_a_centred_pill() {
        let pixels = icon_pixels();
        assert_eq!(pixels.len(), ICON_SIZE * ICON_SIZE * 4);
        // Solid in the middle, clear in the corners and above/below the pill.
        assert_eq!(alpha(&pixels, 16, 16), 255);
        assert_eq!(alpha(&pixels, 0, 0), 0);
        assert_eq!(alpha(&pixels, 31, 31), 0);
        assert_eq!(alpha(&pixels, 16, 2), 0);
        // Wider than tall: the pill reaches further sideways than vertically.
        assert!(alpha(&pixels, 28, 16) > 200);
        assert_eq!(alpha(&pixels, 16, 4), 0);
    }

    #[test]
    fn icon_is_mirror_symmetric() {
        let pixels = icon_pixels();
        for y in 0..ICON_SIZE {
            for x in 0..ICON_SIZE / 2 {
                assert_eq!(
                    alpha(&pixels, x, y),
                    alpha(&pixels, ICON_SIZE - 1 - x, y),
                    "row {y}, column {x}"
                );
            }
        }
    }

    #[test]
    fn a_real_icon_handle_can_be_made() {
        assert!(make_icon().is_some());
    }
}
