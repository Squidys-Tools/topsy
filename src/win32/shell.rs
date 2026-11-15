//! Handing paths to the shell.

use std::path::Path;

use windows::Win32::System::Com::COINIT_APARTMENTTHREADED;
use windows::Win32::UI::Shell::{
    ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{PCWSTR, w};

use super::{wide, with_com};

/// Open a file or folder with its default handler, as a double-click would.
///
/// Blocks until the handler has been started, which can be a while (a UAC prompt, a cold
/// network path), and pumps messages while it waits. Call it from a thread of its own, never
/// the UI thread: it would freeze the island, and a message for the island's window reaching
/// GPUI from inside one of its event handlers aborts the process.
pub fn open_path(path: &Path) -> bool {
    let wide = wide(path);
    // ShellExecute wants COM on the calling thread.
    let result = with_com(COINIT_APARTMENTTHREADED, || unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    });
    // ShellExecute reports success as a value above 32.
    result.0 as usize > 32
}

/// Open the folder holding `path` in Explorer with `path` selected, as "Show in folder" does
/// elsewhere. Pumps messages like [`open_path`]: call it from a thread of its own.
pub fn show_in_folder(path: &Path) -> bool {
    let wide = wide(path);
    with_com(COINIT_APARTMENTTHREADED, || unsafe {
        let item = ILCreateFromPathW(PCWSTR(wide.as_ptr()));
        if item.is_null() {
            return false;
        }
        let shown = SHOpenFolderAndSelectItems(item, None, 0).is_ok();
        ILFree(Some(item.cast_const()));
        shown
    })
}
