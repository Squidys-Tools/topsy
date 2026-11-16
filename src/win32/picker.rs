//! Windows' Open dialog, for picking more files to transform.

use std::path::PathBuf;

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoTaskMemFree,
};
use windows::Win32::UI::Shell::{
    FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FileOpenDialog, IFileOpenDialog,
    SIGDN_FILESYSPATH,
};
use windows::core::w;

/// Ask for files; empty if the dialog is cancelled. The dialog runs its own message loop, so
/// call it from a thread of its own, never the UI thread.
pub fn pick_files() -> Vec<PathBuf> {
    super::with_com(COINIT_APARTMENTTHREADED, || {
        unsafe { pick() }.unwrap_or_default()
    })
}

unsafe fn pick() -> windows::core::Result<Vec<PathBuf>> {
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        let options = dialog.GetOptions()?;
        dialog
            .SetOptions(options | FOS_ALLOWMULTISELECT | FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM)?;
        dialog.SetTitle(w!("Add files to transform"))?;
        dialog.SetOkButtonLabel(w!("Add"))?;
        // Cancelling is an error here; it just means no files.
        if dialog.Show(None).is_err() {
            return Ok(Vec::new());
        }
        let items = dialog.GetResults()?;
        let mut paths = Vec::new();
        for i in 0..items.GetCount()? {
            let name = items.GetItemAt(i)?.GetDisplayName(SIGDN_FILESYSPATH)?;
            paths.push(PathBuf::from(name.to_string()?));
            CoTaskMemFree(Some(name.0 as _));
        }
        Ok(paths)
    }
}
