//! A popup menu at the cursor, owned by a hidden window on the calling thread.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    GetCursorPos, GetForegroundWindow, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    PostMessageW, RegisterClassW, SetForegroundWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenu, WM_NULL, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use super::{restore_foreground, wide};

use crate::log::LogErr;

pub enum Entry {
    /// A command and its label (`&` marks the access key). Ids start at 1.
    Item(usize, &'static str),
    /// One of a set of choices: checked when it is the current one, greyed when unavailable.
    Choice {
        id: usize,
        label: &'static str,
        checked: bool,
        enabled: bool,
    },
    Separator,
}

/// Show `entries` at the cursor and wait for a choice: its id, or `None` if dismissed.
///
/// Runs the menu's own message loop until it closes, so call it from a thread of its own, never
/// the UI thread (see [`super::open_path`]). The menu's owner must be foreground or the menu
/// won't close on a click elsewhere; it is a hidden window, and the window that was foreground
/// gets it back when the menu closes.
pub fn popup(entries: &[Entry]) -> Option<usize> {
    unsafe {
        let module = GetModuleHandleW(None).ok()?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: module.into(),
            lpszClassName: w!("topsy-menu"),
            ..Default::default()
        };
        // Fails harmlessly once the class exists, from an earlier menu.
        RegisterClassW(&class);
        let owner = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("topsy-menu"),
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
        )
        .ok()?;
        let choice = match CreatePopupMenu() {
            Ok(menu) => {
                let labels: Vec<Vec<u16>> = entries
                    .iter()
                    .map(|e| match e {
                        Entry::Item(_, label) | Entry::Choice { label, .. } => wide(label),
                        Entry::Separator => Vec::new(),
                    })
                    .collect();
                for (entry, label) in entries.iter().zip(&labels) {
                    match entry {
                        Entry::Item(id, _) => {
                            AppendMenuW(menu, MF_STRING, *id, PCWSTR(label.as_ptr()))
                        }
                        Entry::Choice {
                            id,
                            checked,
                            enabled,
                            ..
                        } => {
                            let mut flags = MF_STRING;
                            if *checked {
                                flags |= MF_CHECKED;
                            }
                            if !*enabled {
                                flags |= MF_GRAYED;
                            }
                            AppendMenuW(menu, flags, *id, PCWSTR(label.as_ptr()))
                        }
                        Entry::Separator => AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()),
                    }
                    .log_err();
                }
                let previous = GetForegroundWindow();
                let mut at = POINT::default();
                GetCursorPos(&mut at).log_err();
                #[expect(
                    clippy::let_underscore_must_use,
                    reason = "Windows may refuse the foreground; the menu still opens"
                )]
                let _ = SetForegroundWindow(owner);
                let choice = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_RIGHTBUTTON,
                    at.x,
                    at.y,
                    None,
                    owner,
                    None,
                );
                // Documented companion to the above: lets the menu dismiss properly next time.
                PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0)).log_err();
                DestroyMenu(menu).log_err();
                restore_foreground(previous);
                choice.0 as usize
            }
            Err(_) => 0,
        };
        DestroyWindow(owner).log_err();
        (choice != 0).then_some(choice)
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
