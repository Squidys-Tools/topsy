//! Watching the clipboard: a message-only window on a thread of its own that Windows tells
//! of each copy (`AddClipboardFormatListener`), so nothing polls. Each copy is read there,
//! files before text before an image, and handed on with the exe of the app that made it.
//! Copies marked private (password managers) are never read. The same window owns the
//! clipboard strip's hotkey and what topsy puts on the clipboard to paste.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, GetClipboardData, GetClipboardOwner,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Threading::{
    CreateMutexW, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey,
};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetForegroundWindow, GetMessageW,
    GetWindowThreadProcessId, HWND_MESSAGE, MSG, RegisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_CLIPBOARDUPDATE, WM_HOTKEY, WNDCLASSW,
};
use windows::core::{PCWSTR, PWSTR, w};

use crate::model::clipboard::{Content, fingerprint};
use crate::model::dib;

const CF_DIB: u32 = 8;
const CF_UNICODETEXT: u32 = 13;
const CF_HDROP: u32 = 15;
const CF_DIBV5: u32 = 17;

/// One copy, as read off the clipboard. An image's pixels come along to be saved.
pub struct Copy {
    pub content: Content,
    pub pixels: Option<dib::Rgba>,
    pub app: Option<PathBuf>,
}

struct Handlers {
    copy: Box<dyn Fn(Copy) + Send + Sync>,
    /// Given the window in front when the hotkey was pressed.
    hotkey: Box<dyn Fn(usize) + Send + Sync>,
}

static HANDLERS: OnceLock<Handlers> = OnceLock::new();
/// The watcher's window, which owns what topsy puts on the clipboard.
pub(super) static WINDOW: AtomicUsize = AtomicUsize::new(0);
/// Ctrl+Alt+V.
const HOTKEY: (HOT_KEY_MODIFIERS, u32) = (
    HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0 | MOD_NOREPEAT.0),
    b'V' as u32,
);
/// Off while the user has clipboard history turned off: copies are not even read.
pub static WATCHING: AtomicBool = AtomicBool::new(true);

/// Whether this process is the only one keeping history in `dir`: two would each number
/// entries from their own count and overwrite and expire each other's files. Held until exit.
pub fn claim(dir: &Path) -> bool {
    let name: String = dir
        .to_string_lossy()
        .to_lowercase()
        .chars()
        .map(|c| {
            if matches!(c, '\\' | '/' | ':') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name: Vec<u16> = format!(r"Local\topsy-clipboard-{name}")
        .encode_utf16()
        .chain([0])
        .collect();
    // SAFETY: `name` is NUL-terminated and outlives the call. The handle is left open on
    // purpose: the claim lasts as long as the process.
    unsafe {
        CreateMutexW(None, false, PCWSTR(name.as_ptr())).is_ok()
            && GetLastError() != ERROR_ALREADY_EXISTS
    }
}

/// Start watching; `on_copy` runs on the watcher's thread for each copy, `on_hotkey` for
/// each press of the strip's hotkey.
pub fn spawn(
    on_copy: impl Fn(Copy) + Send + Sync + 'static,
    on_hotkey: impl Fn(usize) + Send + Sync + 'static,
) {
    let handlers = Handlers {
        copy: Box::new(on_copy),
        hotkey: Box::new(on_hotkey),
    };
    if HANDLERS.set(handlers).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("topsy-clipboard".into())
        .spawn(|| unsafe { run() });
    if let Err(err) = spawned {
        eprintln!("topsy: no clipboard history: {err}");
    }
}

unsafe fn run() {
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return;
        };
        let class = w!("topsy.clipboard");
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: module.into(),
            lpszClassName: class,
            ..Default::default()
        });
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            PCWSTR::null(),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(module.into()),
            None,
        );
        let Ok(hwnd) = hwnd else {
            return;
        };
        if let Err(err) = AddClipboardFormatListener(hwnd) {
            eprintln!("topsy: no clipboard history: {err}");
            return;
        }
        WINDOW.store(hwnd.0 as usize, Ordering::Relaxed);
        if let Err(err) = RegisterHotKey(Some(hwnd), 1, HOTKEY.0, HOTKEY.1) {
            eprintln!("topsy: Ctrl+Alt+V is taken by another app: {err}");
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_CLIPBOARDUPDATE {
        if WATCHING.load(Ordering::Relaxed)
            && let Some(copy) = unsafe { read(hwnd) }
            && let Some(handlers) = HANDLERS.get()
        {
            (handlers.copy)(copy);
        }
        return LRESULT(0);
    }
    if msg == WM_HOTKEY {
        if let Some(handlers) = HANDLERS.get() {
            (handlers.hotkey)(unsafe { GetForegroundWindow() }.0 as usize);
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

unsafe fn read(hwnd: HWND) -> Option<Copy> {
    unsafe {
        let app = owner_exe();
        if !open(hwnd) {
            return None;
        }
        let (content, pixels) = if private() {
            (None, None)
        } else if IsClipboardFormatAvailable(CF_HDROP).is_ok() {
            (files().map(Content::Files), None)
        } else if IsClipboardFormatAvailable(CF_UNICODETEXT).is_ok() {
            (text().map(Content::Text), None)
        } else {
            let image = [CF_DIBV5, CF_DIB]
                .into_iter()
                .filter(|&f| IsClipboardFormatAvailable(f).is_ok())
                .find_map(|f| with_bytes(f, dib::decode));
            let content = image.as_ref().map(|i| Content::Image {
                width: i.width,
                height: i.height,
                hash: fingerprint(&i.pixels),
            });
            (content, image)
        };
        let _ = CloseClipboard();
        content.map(|content| Copy {
            content,
            pixels,
            app,
        })
    }
}

/// Open the clipboard for `hwnd`, waiting a little for an app that holds it open (one that
/// has just copied, often).
pub(super) unsafe fn open(hwnd: HWND) -> bool {
    for _ in 0..10 {
        if unsafe { OpenClipboard(Some(hwnd)) }.is_ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    false
}

/// The formats apps set to keep a copy out of clipboard managers and history.
unsafe fn private() -> bool {
    unsafe {
        let has = |name: PCWSTR| {
            let format = RegisterClipboardFormatW(name);
            format != 0 && IsClipboardFormatAvailable(format).is_ok()
        };
        if has(w!("ExcludeClipboardContentFromMonitorProcessing"))
            || has(w!("Clipboard Viewer Ignore"))
        {
            return true;
        }
        let history = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        history != 0
            && IsClipboardFormatAvailable(history).is_ok()
            && with_bytes(history, |b| b.get(..4).map(|v| v == [0, 0, 0, 0])) == Some(true)
    }
}

/// Lock a clipboard format's global memory for `f`.
unsafe fn with_bytes<T>(format: u32, f: impl FnOnce(&[u8]) -> Option<T>) -> Option<T> {
    unsafe {
        let handle = GetClipboardData(format).ok()?;
        let global = HGLOBAL(handle.0);
        let size = GlobalSize(global);
        let ptr = GlobalLock(global);
        if ptr.is_null() {
            return None;
        }
        let result = f(std::slice::from_raw_parts(ptr as *const u8, size));
        let _ = GlobalUnlock(global);
        result
    }
}

unsafe fn text() -> Option<String> {
    unsafe {
        with_bytes(CF_UNICODETEXT, |bytes| {
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take_while(|&u| u != 0)
                .collect();
            Some(String::from_utf16_lossy(&units))
        })
    }
}

unsafe fn files() -> Option<Vec<PathBuf>> {
    unsafe {
        let handle = GetClipboardData(CF_HDROP).ok()?;
        let drop = HDROP(handle.0);
        let count = DragQueryFileW(drop, u32::MAX, None);
        let paths = (0..count)
            .filter_map(|i| {
                let len = DragQueryFileW(drop, i, None) as usize;
                let mut buf = vec![0u16; len + 1];
                let got = DragQueryFileW(drop, i, Some(&mut buf)) as usize;
                (got > 0).then(|| PathBuf::from(OsString::from_wide(&buf[..got])))
            })
            .collect::<Vec<_>>();
        (!paths.is_empty()).then_some(paths)
    }
}

/// The exe of the app that owns the clipboard, or else of the window in front (some apps
/// copy through a window of no process of theirs).
unsafe fn owner_exe() -> Option<PathBuf> {
    unsafe {
        let window = GetClipboardOwner()
            .ok()
            .filter(|h| !h.is_invalid())
            .unwrap_or_else(|| GetForegroundWindow());
        // topsy pasting an entry back: the entry keeps the app that copied it.
        if window.0 as usize == WINDOW.load(Ordering::Relaxed) {
            return None;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(window, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let named = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        named.ok()?;
        Some(PathBuf::from(OsString::from_wide(&buf[..len as usize])))
    }
}
