//! Shell shortcuts (`.lnk`) to the exe, in one of the user's known folders: Send to, Start.

use std::path::{Path, PathBuf};

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoTaskMemFree, IPersistFile,
    STGM_READ,
};
use windows::Win32::UI::Shell::{IShellLinkW, KF_FLAG_DEFAULT, SHGetKnownFolderPath, ShellLink};
use windows::core::{GUID, HSTRING, Interface, Result};

use crate::win32::with_com;

/// `name` in the known folder `folder`, if the folder can be found.
pub fn in_folder(folder: &GUID, name: &str) -> Option<PathBuf> {
    unsafe {
        let path = SHGetKnownFolderPath(folder, KF_FLAG_DEFAULT, None).ok()?;
        let found = path.to_string().ok();
        CoTaskMemFree(Some(path.0 as _));
        Some(PathBuf::from(found?).join(name))
    }
}

fn shell_link() -> Result<IShellLinkW> {
    unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
}

/// Write a shortcut at `link` that starts `exe`.
pub fn write(link: &Path, exe: &Path, description: &str) -> Result<()> {
    with_com(COINIT_APARTMENTTHREADED, || unsafe {
        let shortcut = shell_link()?;
        shortcut.SetPath(&HSTRING::from(exe.as_os_str()))?;
        if let Some(dir) = exe.parent() {
            shortcut.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))?;
        }
        shortcut.SetDescription(&HSTRING::from(description))?;
        shortcut
            .cast::<IPersistFile>()?
            .Save(&HSTRING::from(link.as_os_str()), true)
    })
}

/// The exe a shortcut starts, if it can be read.
pub fn target(link: &Path) -> Option<PathBuf> {
    with_com(COINIT_APARTMENTTHREADED, || unsafe {
        let shortcut = shell_link().ok()?;
        shortcut
            .cast::<IPersistFile>()
            .ok()?
            .Load(&HSTRING::from(link.as_os_str()), STGM_READ)
            .ok()?;
        let mut buf = [0u16; 32768];
        shortcut.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let len = buf.iter().position(|&c| c == 0)?;
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..len])))
    })
}

/// Remove the shortcut. Succeeds if it was already gone.
pub fn remove(link: Option<PathBuf>) -> std::io::Result<()> {
    match link.map(std::fs::remove_file) {
        Some(Err(err)) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::{LogErr, MissingOk};

    #[test]
    fn a_shortcut_round_trips_its_target() {
        let dir = std::env::temp_dir().join(format!("topsy-test-{}-shortcut", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let link = dir.join("topsy.lnk");
        let exe = dir.join("topsy.exe");
        write(&link, &exe, "test").unwrap();
        let read = target(&link);
        remove(Some(link.clone())).unwrap();
        let gone = !link.exists();
        remove(Some(link)).unwrap();
        std::fs::remove_dir_all(&dir).missing_ok().log_err();
        assert_eq!(read, Some(exe));
        assert!(gone);
    }
}
