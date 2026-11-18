//! Watching one folder's files: a thread blocks in `ReadDirectoryChangesW` and reports each
//! change by file name. Nothing polls.

use std::path::PathBuf;

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ACTION_ADDED, FILE_ACTION_MODIFIED, FILE_ACTION_REMOVED,
    FILE_ACTION_RENAMED_NEW_NAME, FILE_ACTION_RENAMED_OLD_NAME, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_SIZE,
    FILE_NOTIFY_INFORMATION, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    ReadDirectoryChangesW,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{FOLDERID_Downloads, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
use windows::core::HSTRING;

use crate::log::LogErr;
use model::browser_download::Change;

/// The user's Downloads folder, wherever it has been moved.
pub fn downloads() -> Option<PathBuf> {
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, None).ok()?;
        let folder = path.to_string().ok();
        CoTaskMemFree(Some(path.0 as _));
        Some(PathBuf::from(folder?))
    }
}

/// Report the files added, removed, renamed or resized directly in `dir` (not below it).
pub fn watch(dir: PathBuf, report: impl FnMut(Change) + Send + 'static) {
    let spawned = std::thread::Builder::new()
        .name("topsy-folder".into())
        .spawn(move || {
            // SAFETY: the handle and buffer live for the thread.
            if let Err(err) = unsafe { run(&dir, report) } {
                crate::log(&format!("not watching {}: {err}", dir.display()));
            }
        });
    if let Err(err) = spawned {
        eprintln!("topsy: no folder watch: {err}");
    }
}

unsafe fn run(dir: &std::path::Path, mut report: impl FnMut(Change)) -> windows::core::Result<()> {
    unsafe {
        let handle = CreateFileW(
            &HSTRING::from(dir.as_os_str()),
            FILE_LIST_DIRECTORY.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )?;
        // FILE_NOTIFY_INFORMATION is DWORD-aligned.
        let mut buffer = vec![0u32; 16 * 1024];
        let mut renamed_from = None;
        let result = loop {
            let mut got = 0u32;
            if let Err(err) = ReadDirectoryChangesW(
                handle,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * 4) as u32,
                false,
                FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_SIZE,
                Some(&mut got),
                None,
                None,
            ) {
                break Err(err);
            }
            // Nothing returned means the buffer overflowed: those changes are lost.
            if got == 0 {
                continue;
            }
            let mut at = 0usize;
            loop {
                let info =
                    &*(buffer.as_ptr().cast::<u8>().add(at) as *const FILE_NOTIFY_INFORMATION);
                let name = std::slice::from_raw_parts(
                    info.FileName.as_ptr(),
                    info.FileNameLength as usize / 2,
                );
                let name = String::from_utf16_lossy(name);
                let change = match info.Action {
                    FILE_ACTION_ADDED => Some(Change::Added(name)),
                    FILE_ACTION_REMOVED => Some(Change::Removed(name)),
                    FILE_ACTION_MODIFIED => Some(Change::Modified(name)),
                    FILE_ACTION_RENAMED_OLD_NAME => {
                        renamed_from = Some(name);
                        None
                    }
                    FILE_ACTION_RENAMED_NEW_NAME => renamed_from
                        .take()
                        .map(|from| Change::Renamed { from, to: name }),
                    _ => None,
                };
                if let Some(change) = change {
                    report(change);
                }
                if info.NextEntryOffset == 0 {
                    break;
                }
                at += info.NextEntryOffset as usize;
            }
        };
        CloseHandle(handle).log_err();
        result
    }
}
