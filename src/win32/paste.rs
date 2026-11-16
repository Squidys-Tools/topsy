//! Putting a history entry back on the clipboard and pasting it into the app that was in
//! front. Both wait on other apps, so call them from a worker thread, never the UI thread.

use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;

use windows::Win32::Foundation::{GlobalFree, HANDLE, HWND};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_CONTROL,
};

use super::clipboard::{WINDOW, open};
use super::restore_foreground;

const CF_DIB: u32 = 8;
const CF_UNICODETEXT: u32 = 13;
const CF_HDROP: u32 = 15;

pub enum Data {
    Text(String),
    Files(Vec<PathBuf>),
    /// `CF_DIB` bytes (see `model::dib::encode`).
    Dib(Vec<u8>),
}

/// Put `data` on the clipboard, in place of what is there.
pub fn put(data: &Data) -> bool {
    let (format, bytes) = match data {
        Data::Text(text) => (CF_UNICODETEXT, wide_bytes(text.encode_utf16())),
        Data::Files(files) => (CF_HDROP, drop_files(files)),
        Data::Dib(dib) => (CF_DIB, dib.clone()),
    };
    let owner = HWND(WINDOW.load(Ordering::Relaxed) as *mut _);
    if owner.0.is_null() {
        return false;
    }
    unsafe {
        if !open(owner) {
            return false;
        }
        let set = EmptyClipboard().is_ok() && set(format, &bytes);
        let _ = CloseClipboard();
        set
    }
}

/// Hand the global memory to the clipboard, which owns it once this succeeds.
unsafe fn set(format: u32, bytes: &[u8]) -> bool {
    unsafe {
        let Ok(global) = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) else {
            return false;
        };
        let ptr = GlobalLock(global);
        if ptr.is_null() {
            let _ = GlobalFree(Some(global));
            return false;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
        let _ = GlobalUnlock(global);
        if SetClipboardData(format, Some(HANDLE(global.0))).is_err() {
            let _ = GlobalFree(Some(global));
            return false;
        }
        true
    }
}

/// UTF-16 code units, then a terminating zero, as bytes.
fn wide_bytes(units: impl Iterator<Item = u16>) -> Vec<u8> {
    units.chain([0]).flat_map(u16::to_le_bytes).collect()
}

/// A `DROPFILES` header (20 bytes: where the list starts, a point, two flags with wide names
/// set) and the paths, each ending in a zero, the list in another.
fn drop_files(files: &[PathBuf]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for v in [20u32, 0, 0, 0, 1] {
        bytes.extend(v.to_le_bytes());
    }
    for file in files {
        bytes.extend(wide_bytes(file.as_os_str().encode_wide()));
    }
    bytes.extend([0, 0]);
    bytes
}

/// Bring `previous` back to the front and press Ctrl+V in it.
pub fn paste_into(previous: HWND) {
    restore_foreground(previous);
    // Let the app take the focus back before the keys arrive.
    std::thread::sleep(Duration::from_millis(80));
    let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                ..Default::default()
            },
        },
    };
    let v = VIRTUAL_KEY(b'V' as u16);
    let keys = [
        key(VK_CONTROL, false),
        key(v, false),
        key(v, true),
        key(VK_CONTROL, true),
    ];
    unsafe {
        SendInput(&keys, size_of::<INPUT>() as i32);
    }
}
