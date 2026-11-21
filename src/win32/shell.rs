//! Handing paths to the shell.

use std::io;
use std::path::Path;

use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Storage::FileSystem::{MOVE_FILE_FLAGS, MoveFileExW};

use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoTaskMemFree};
use windows::Win32::UI::Shell::{
    FOLDERID_AppsFolder, ILCreateFromPathW, ILFree, IShellItem, IShellItem2, KF_FLAG_DEFAULT,
    SHCreateItemFromParsingName, SHCreateItemInKnownFolder, SHOpenFolderAndSelectItems,
    SIGDN_NORMALDISPLAY, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{GUID, HSTRING, PCWSTR, w};

use super::{wide, with_com};

/// `PKEY_FileDescription`: the name an exe gives itself ("Google Chrome").
const DESCRIPTION: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x0cef7d53_fa64_11d1_a203_0000f81fedee),
    pid: 3,
};

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

/// Move a file to a name that must still be free: unlike `fs::rename`, never replaces a file
/// that appeared there since the check.
pub fn move_new(from: &Path, to: &Path) -> io::Result<()> {
    let wide = |p: &Path| windows::core::HSTRING::from(p.as_os_str());
    unsafe { MoveFileExW(&wide(from), &wide(to), MOVE_FILE_FLAGS(0)) }.map_err(io::Error::from)
}

/// The name Start shows for the app with this AppUserModelID (`Chrome` reads `Google Chrome`),
/// if it is installed under it. Wants COM on the calling thread.
pub fn app_name(app_id: &str) -> Option<String> {
    unsafe {
        let item: IShellItem = SHCreateItemInKnownFolder(
            &FOLDERID_AppsFolder,
            KF_FLAG_DEFAULT,
            &HSTRING::from(app_id),
        )
        .ok()?;
        let name = item.GetDisplayName(SIGDN_NORMALDISPLAY).ok()?;
        let text = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as _));
        text.filter(|t| !t.is_empty())
    }
}

/// The name an exe gives itself (`chrome.exe` reads `Google Chrome`), if it gives one. Wants
/// COM on the calling thread.
pub fn exe_description(exe: &Path) -> Option<String> {
    unsafe {
        let item: IShellItem2 =
            SHCreateItemFromParsingName(&HSTRING::from(exe.as_os_str()), None).ok()?;
        let name = item.GetString(&DESCRIPTION).ok()?;
        let text = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as _));
        text.filter(|t| !t.trim().is_empty())
    }
}
