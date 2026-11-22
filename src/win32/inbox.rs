//! The running topsy's inbox: a message-only window that another topsy, started later, hands its
//! launch to. Explorer's Send to brings paths for the shelf (as UTF-16 over `WM_COPYDATA`, each
//! ended by a NUL); a plain launch from Start asks to be shown; `--uninstall` asks it to quit.

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowExW, GetMessageW,
    GetWindowThreadProcessId, HWND_MESSAGE, MSG, RegisterClassW, SMTO_ABORTIFHUNG,
    SendMessageTimeoutW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_COPYDATA, WNDCLASSW,
};
use windows::core::{PCWSTR, w};

use crate::win32::process;

const CLASS: PCWSTR = w!("topsy.inbox");
/// Mark topsy's own messages, in case anything else sends the window `WM_COPYDATA`.
const PATHS: usize = 0x7470_7379;
const SHOW: usize = 0x7470_7380;
const QUIT: usize = 0x7470_7381;

/// What a later launch hands the running topsy.
#[derive(Debug, PartialEq)]
pub enum Delivery {
    /// Paths from Explorer's Send to, for the shelf.
    Paths(Vec<PathBuf>),
    /// Started again from Start or a launcher: show the island that is already there.
    Show,
    /// `--uninstall`: the exe is about to go.
    Quit,
}

static INBOX: OnceLock<async_channel::Sender<Delivery>> = OnceLock::new();

/// A running topsy's inbox and its process. With `exe`, only a topsy running that exe, so a
/// development build never hands its launch to the installed one, or the other way round.
fn find(exe: Option<&Path>) -> Option<(HWND, u32)> {
    let mut after = None;
    loop {
        let hwnd =
            unsafe { FindWindowExW(Some(HWND_MESSAGE), after, CLASS, PCWSTR::null()) }.ok()?;
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        let matches = exe.is_none_or(|exe| {
            process::exe_of(pid).is_some_and(|running| super::same_path(&running, exe))
        });
        if matches {
            return Some((hwnd, pid));
        }
        after = Some(hwnd);
    }
}

fn send(hwnd: HWND, delivery: &Delivery) -> bool {
    let (marker, data) = match delivery {
        Delivery::Paths(paths) => (PATHS, encode(paths)),
        Delivery::Show => (SHOW, Vec::new()),
        Delivery::Quit => (QUIT, Vec::new()),
    };
    let copy = COPYDATASTRUCT {
        dwData: marker,
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

/// Give `paths` to a topsy already running. False if there is none, or it didn't take them.
pub fn hand_off(paths: Vec<PathBuf>) -> bool {
    find(None).is_some_and(|(hwnd, _)| send(hwnd, &Delivery::Paths(paths)))
}

/// Show a topsy already running this exe. False if there is none, or it didn't answer.
pub fn show() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| find(Some(&exe)))
        .is_some_and(|(hwnd, _)| send(hwnd, &Delivery::Show))
}

/// Ask a topsy running `exe` to quit, and wait until it has.
pub fn quit(exe: &Path) {
    if let Some((hwnd, pid)) = find(Some(exe))
        && send(hwnd, &Delivery::Quit)
    {
        process::wait_for_exit(pid);
    }
}

/// Open the inbox on a thread of its own.
pub fn spawn() -> async_channel::Receiver<Delivery> {
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
        let delivery = match copy.dwData {
            PATHS if !copy.lpData.is_null() => {
                let data = unsafe {
                    std::slice::from_raw_parts(copy.lpData as *const u16, copy.cbData as usize / 2)
                };
                Some(Delivery::Paths(decode(data)))
            }
            SHOW => Some(Delivery::Show),
            QUIT => Some(Delivery::Quit),
            _ => None,
        };
        if let Some(delivery) = delivery {
            let taken = INBOX.get().is_some_and(|tx| tx.try_send(delivery).is_ok());
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
