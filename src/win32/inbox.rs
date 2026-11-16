//! The running topsy's inbox: a message-only window that takes the paths another topsy was
//! started with (Explorer's Send to), so they land on the shelf already on screen. The paths
//! come over `WM_COPYDATA` as UTF-16, each ended by a NUL.

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::sync::OnceLock;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowExW, GetMessageW, HWND_MESSAGE,
    MSG, RegisterClassW, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_COPYDATA, WNDCLASSW,
};
use windows::core::{PCWSTR, w};

const CLASS: PCWSTR = w!("topsy.inbox");
/// Marks topsy's own message, in case anything else sends the window `WM_COPYDATA`.
const PATHS: usize = 0x7470_7379;

static INBOX: OnceLock<async_channel::Sender<Vec<PathBuf>>> = OnceLock::new();

/// Give `paths` to a topsy already running. False if there is none, or it didn't take them.
pub fn hand_off(paths: &[PathBuf]) -> bool {
    let Ok(hwnd) = (unsafe { FindWindowExW(Some(HWND_MESSAGE), None, CLASS, PCWSTR::null()) })
    else {
        return false;
    };
    let data = encode(paths);
    let copy = COPYDATASTRUCT {
        dwData: PATHS,
        cbData: (data.len() * 2) as u32,
        lpData: data.as_ptr() as *mut _,
    };
    let mut taken = 0;
    let sent = unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_COPYDATA,
            WPARAM(0),
            LPARAM(&copy as *const _ as isize),
            SMTO_ABORTIFHUNG,
            5000,
            Some(&mut taken),
        )
    };
    sent.0 != 0 && taken == 1
}

/// Open the inbox on a thread of its own; each message arrives as one list of paths.
pub fn spawn() -> async_channel::Receiver<Vec<PathBuf>> {
    let (tx, rx) = async_channel::unbounded();
    if INBOX.set(tx).is_err() {
        return rx;
    }
    let spawned = std::thread::Builder::new()
        .name("topsy-inbox".into())
        .spawn(|| unsafe { run() });
    if let Err(err) = spawned {
        eprintln!("topsy: no inbox for Send to: {err}");
    }
    rx
}

unsafe fn run() {
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return;
        };
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: module.into(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassW(&class);
        let created = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS,
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
        if let Err(err) = created {
            eprintln!("topsy: no inbox for Send to: {err}");
            return;
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_COPYDATA {
        let copy = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
        if copy.dwData == PATHS && !copy.lpData.is_null() {
            let data = unsafe {
                std::slice::from_raw_parts(copy.lpData as *const u16, copy.cbData as usize / 2)
            };
            let taken = INBOX
                .get()
                .is_some_and(|tx| tx.try_send(decode(data)).is_ok());
            return LRESULT(taken as isize);
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn encode(paths: &[PathBuf]) -> Vec<u16> {
    paths
        .iter()
        .flat_map(|p| p.as_os_str().encode_wide().chain([0]))
        .collect()
}

fn decode(data: &[u16]) -> Vec<PathBuf> {
    data.split(|&c| c == 0)
        .filter(|p| !p.is_empty())
        .map(|p| PathBuf::from(OsString::from_wide(p)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_round_trip_through_the_wire_format() {
        let paths = vec![
            PathBuf::from(r"C:\Users\me\Desktop\report.pdf"),
            PathBuf::from(r"\\server\share\naïve 文件 🎵.mp3"),
        ];
        assert_eq!(decode(&encode(&paths)), paths);
        assert_eq!(decode(&[]), Vec::<PathBuf>::new());
    }
}
