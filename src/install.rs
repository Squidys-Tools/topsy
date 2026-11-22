//! What Windows knows of an installed topsy: a shortcut in Start, which Start's search and
//! launchers such as Raycast find, and an entry in Settings' Installed apps, whose Uninstall runs
//! `topsy.exe --uninstall`. Both are the user's own, so no admin rights. Only a nightly registers
//! itself, at every launch, so the entries follow the exe if it moves and a development build
//! never shows up in Start.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use windows::Win32::UI::Shell::FOLDERID_Programs;

use crate::autostart::{self, RegString, command_for, exe_in};
use crate::log::LogErr;
use crate::update;
use crate::win32::{inbox, same_path, sendto, shortcut};

const UNINSTALL: &str = "--uninstall";
const ENTRY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\topsy";
const DESCRIPTION: &str = "A glass island at the top of the screen";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn start_link() -> Option<PathBuf> {
    shortcut::in_folder(&FOLDERID_Programs, "topsy.lnk")
}

fn starts(link: &Path, exe: &Path) -> bool {
    shortcut::target(link).is_some_and(|target| same_path(&target, exe))
}

/// Put this exe in Start and in Installed apps, if it is a nightly.
pub fn register(exe: &Path) {
    if update::STAMP.is_none() {
        return;
    }
    if let Some(link) = start_link()
        && !starts(&link, exe)
    {
        shortcut::write(&link, exe, DESCRIPTION).log_err();
    }
    let dir = exe.parent().unwrap_or(exe).display().to_string();
    let values = [
        ("DisplayName", "topsy".to_string()),
        ("DisplayVersion", update::VERSION.to_string()),
        ("DisplayIcon", format!("{},0", exe.display())),
        ("Publisher", "Squidys-Tools".to_string()),
        (
            "URLInfoAbout",
            "https://github.com/Squidys-Tools/topsy".to_string(),
        ),
        ("InstallLocation", dir),
        (
            "UninstallString",
            format!("{} {UNINSTALL}", command_for(exe)),
        ),
    ];
    for (name, data) in values {
        if RegString::new(ENTRY, name).set(&data).log_err().is_none() {
            return;
        }
    }
}

/// `topsy.exe --uninstall`: quit the topsy running this exe, take away each entry that starts
/// this exe, and delete it once this process has gone. The shelf, the clipboard history and the
/// settings stay, as most uninstallers leave a user's data. True if this launch asked for it.
pub fn uninstall_if_asked() -> bool {
    if std::env::args_os()
        .nth(1)
        .is_none_or(|arg| arg != UNINSTALL)
    {
        return false;
    }
    if let Ok(exe) = std::env::current_exe() {
        uninstall(&exe);
    }
    true
}

fn uninstall(exe: &Path) {
    inbox::quit(exe);
    if let Some(link) = start_link()
        && starts(&link, exe)
    {
        shortcut::remove(Some(link)).log_err();
    }
    let registered = RegString::new(ENTRY, "UninstallString").value();
    if registered.is_some_and(|command| same_path(&exe_in(&command), exe)) {
        autostart::remove_key(ENTRY).log_err();
    }
    autostart::forget(exe).log_err();
    sendto::forget(exe).log_err();
    // A running exe can't delete itself; cmd waits a moment for this one to exit.
    Command::new("cmd")
        .raw_arg(format!(
            "/d /c ping -n 3 127.0.0.1 >nul & del /f /q \"{}\"",
            exe.display()
        ))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .log_err();
}
