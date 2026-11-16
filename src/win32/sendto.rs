//! "Send to > topsy" in Explorer's right-click menu: a shortcut to the exe in the user's SendTo
//! folder, which needs no admin rights. Explorer starts the exe with the selected paths; the
//! running topsy takes them (`win32::inbox`). Off until switched on, like start-with-Windows.

use std::path::{Path, PathBuf};

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoTaskMemFree, IPersistFile,
    STGM_READ,
};
use windows::Win32::UI::Shell::{
    FOLDERID_SendTo, IShellLinkW, KF_FLAG_DEFAULT, SHGetKnownFolderPath, ShellLink,
};
use windows::core::{HSTRING, Interface, Result};

use crate::win32::with_com;

/// The shortcut's file name, which is what the Send to menu shows.
const NAME: &str = "topsy.lnk";

/// The shortcut in the user's SendTo folder, if the folder can be found.
fn system() -> Option<PathBuf> {
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_SendTo, KF_FLAG_DEFAULT, None).ok()?;
        let folder = path.to_string().ok();
        CoTaskMemFree(Some(path.0 as _));
        Some(PathBuf::from(folder?).join(NAME))
    }
}

pub fn is_enabled() -> bool {
    system().is_some_and(|link| link.exists())
}

pub fn enable(exe: &Path) -> Result<()> {
    let link = system().ok_or_else(windows::core::Error::empty)?;
    write(&link, exe)
}

/// Remove the shortcut. Succeeds if it was already gone.
pub fn disable() -> Result<()> {
    match system().map(std::fs::remove_file) {
        Some(Err(err)) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
        _ => Ok(()),
    }
}

/// If the shortcut is there but points at an exe that no longer exists (the app was moved or
/// reinstalled), point it at this one. A different exe that still exists is left alone, so
/// running a development build never takes over the installed app's entry.
pub fn refresh(exe: &Path) {
    if let Some(link) = system()
        && let Some(target) = target(&link)
        && !target.exists()
        && let Err(err) = write(&link, exe)
    {
        eprintln!("topsy: could not update the Send to entry: {err}");
    }
}

fn shell_link() -> Result<IShellLinkW> {
    unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
}

/// Write a shortcut at `link` that starts `exe`.
fn write(link: &Path, exe: &Path) -> Result<()> {
    with_com(COINIT_APARTMENTTHREADED, || unsafe {
        let shortcut = shell_link()?;
        shortcut.SetPath(&HSTRING::from(exe.as_os_str()))?;
        shortcut.SetDescription(&HSTRING::from("Park on topsy's shelf"))?;
        shortcut
            .cast::<IPersistFile>()?
            .Save(&HSTRING::from(link.as_os_str()), true)
    })
}

/// The exe a shortcut starts, if it can be read.
fn target(link: &Path) -> Option<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shortcut_round_trips_its_target() {
        let dir = std::env::temp_dir().join(format!("topsy-test-{}-sendto", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let link = dir.join(NAME);
        let exe = dir.join("topsy.exe");
        write(&link, &exe).unwrap();
        let read = target(&link);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(read, Some(exe));
    }
}
