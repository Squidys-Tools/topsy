//! "Send to > topsy" in Explorer's right-click menu: a shortcut to the exe in the user's SendTo
//! folder, which needs no admin rights. Explorer starts the exe with the selected paths; the
//! running topsy takes them (`win32::inbox`). Off until switched on, like start-with-Windows.

use std::path::{Path, PathBuf};

use windows::Win32::UI::Shell::FOLDERID_SendTo;
use windows::core::Result;

use crate::win32::{same_path, shortcut};

/// The shortcut's file name, which is what the Send to menu shows.
const NAME: &str = "topsy.lnk";
const DESCRIPTION: &str = "Park on topsy's shelf";

fn system() -> Option<PathBuf> {
    shortcut::in_folder(&FOLDERID_SendTo, NAME)
}

pub fn is_enabled() -> bool {
    system().is_some_and(|link| link.exists())
}

pub fn enable(exe: &Path) -> Result<()> {
    let link = system().ok_or_else(windows::core::Error::empty)?;
    shortcut::write(&link, exe, DESCRIPTION)
}

/// Remove the shortcut. Succeeds if it was already gone.
pub fn disable() -> Result<()> {
    Ok(shortcut::remove(system())?)
}

/// Remove the shortcut if it starts `exe`, and leave another exe's alone.
pub fn forget(exe: &Path) -> std::io::Result<()> {
    match system() {
        Some(link) if shortcut::target(&link).is_some_and(|target| same_path(&target, exe)) => {
            shortcut::remove(Some(link))
        }
        Some(_) | None => Ok(()),
    }
}

/// If the shortcut is there but points at an exe that no longer exists (the app was moved or
/// reinstalled), point it at this one. A different exe that still exists is left alone, so
/// running a development build never takes over the installed app's entry.
pub fn refresh(exe: &Path) {
    if let Some(link) = system()
        && let Some(target) = shortcut::target(&link)
        && !target.exists()
        && let Err(err) = shortcut::write(&link, exe, DESCRIPTION)
    {
        eprintln!("topsy: could not update the Send to entry: {err}");
    }
}
