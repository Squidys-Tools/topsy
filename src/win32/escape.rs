//! Escape folds a tray opened to look. The island never has the focus, so the key is read by a
//! low-level keyboard hook, installed only while such a tray is open. It takes Escape from the
//! app in front, unless a mouse button is down (Escape cancels a drag), a menu is up (Escape
//! closes it) or a window of topsy's is in front (the Transform panel and the clipboard strip
//! take Escape themselves); every other key passes straight through.

use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_ESCAPE, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GUI_INMENUMODE, GUI_POPUPMENUMODE, GUITHREADINFO,
    GetForegroundWindow, GetGUIThreadInfo, GetMessageW, GetWindowThreadProcessId, HC_ACTION,
    KBDLLHOOKSTRUCT, MSG, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WM_KEYDOWN, WM_QUIT,
};

static PRESSED: OnceLock<async_channel::Sender<()>> = OnceLock::new();
/// The hook thread's id while Escape is taken.
static HOOK: Mutex<Option<u32>> = Mutex::new(None);

/// Each Escape taken while [`take`] has it.
pub fn presses() -> async_channel::Receiver<()> {
    let (tx, rx) = async_channel::unbounded();
    let _ = PRESSED.set(tx);
    rx
}

/// Take Escape from the app in front, or give it back.
pub fn take(on: bool) {
    let mut hook = HOOK.lock().unwrap_or_else(|e| e.into_inner());
    match (on, *hook) {
        (true, None) => *hook = spawn_hook(),
        (false, Some(thread)) => {
            // SAFETY: posting to a thread of ours; it unhooks and ends on WM_QUIT.
            let _ = unsafe { PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0)) };
            *hook = None;
        }
        _ => {}
    }
}

/// The hook's thread, once its hook is in, or `None` if it could not be installed.
fn spawn_hook() -> Option<u32> {
    let (ready, installed) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("topsy-escape".into())
        .spawn(move || unsafe {
            let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_proc), None, 0) {
                Ok(hook) => hook,
                Err(err) => {
                    eprintln!("topsy: could not watch for Escape: {err}");
                    let _ = ready.send(None);
                    return;
                }
            };
            let _ = ready.send(Some(GetCurrentThreadId()));
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                DispatchMessageW(&msg);
            }
            let _ = UnhookWindowsHookEx(hook);
        });
    spawned.ok()?;
    installed.recv().ok().flatten()
}

/// Escape belongs to whatever is in the middle of something: a drag, a menu, or one of our
/// own windows.
fn busy() -> bool {
    // SAFETY: plain state queries; GetGUIThreadInfo(0) reads the foreground thread.
    unsafe {
        let held = [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON]
            .into_iter()
            .any(|b| GetAsyncKeyState(b.0 as i32) < 0);
        let mut gui = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        let menu = GetGUIThreadInfo(0, &mut gui).is_ok()
            && (gui.flags.0 & (GUI_INMENUMODE.0 | GUI_POPUPMENUMODE.0)) != 0;
        let mut pid = 0;
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
        held || menu || pid == GetCurrentProcessId()
    }
}

unsafe extern "system" fn key_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && let Some(pressed) = PRESSED.get()
    {
        // SAFETY: for HC_ACTION, `lparam` points at the key's KBDLLHOOKSTRUCT.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if VIRTUAL_KEY(info.vkCode as u16) == VK_ESCAPE && !busy() {
            if wparam.0 as u32 == WM_KEYDOWN {
                let _ = pressed.try_send(());
            }
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
