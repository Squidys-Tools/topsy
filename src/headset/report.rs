//! *Add my headset*: writes what topsy sees of the user's Bluetooth audio devices
//! (`context.json`) beside the playbook a coding agent follows to add one to the catalog, then
//! starts the first agent CLI it finds in a console of its own. Without one, it opens a
//! prefilled issue instead. The tray menu and `topsy --add-headset` reach it.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::log::LogErr;
use crate::update::VERSION;
use crate::win32::{self, bluetooth};
use model::headset::{Dump, issue_url};

const PLAYBOOK: &str = include_str!("../../.github/devices/PLAYBOOK.md");
const REPO: &str = env!("CARGO_PKG_REPOSITORY");
/// The agent CLIs tried, in order.
const AGENTS: [&str; 3] = ["claude", "codex", "agy"];
/// A Windows `.cmd` shim can't carry the playbook itself, so the prompt points at it.
const PROMPT: &str = "Read PLAYBOOK.md in this folder and follow it exactly: it is your topsy \
    Add my headset playbook, and it starts by asking the user which headset to add.";
const CREATE_NEW_CONSOLE: u32 = 0x10;

/// Start it on a thread of its own: reading the devices takes a moment, and opening a URL
/// pumps messages.
pub fn start() {
    let spawned = std::thread::Builder::new()
        .name("topsy-add-headset".into())
        .spawn(run);
    if let Err(err) = spawned {
        eprintln!("topsy: could not start Add my headset: {err}");
    }
}

pub fn run() {
    let dump = Dump {
        topsy: VERSION.to_owned(),
        windows: win32::sysinfo::windows_build().unwrap_or_default(),
        devices: bluetooth::dump()
            .into_iter()
            .filter(|device| device.is_audio())
            .collect(),
    };
    let session = match write_session(&dump) {
        Ok(session) => session,
        Err(err) => return eprintln!("topsy: could not write the Add my headset files: {err}"),
    };
    if let Some(agent) = AGENTS.into_iter().find(|agent| on_path(agent)) {
        crate::log(&format!(
            "add my headset with {agent} in {}",
            session.display()
        ));
        Command::new("cmd.exe")
            .raw_arg(format!("/k {agent} \"{PROMPT}\""))
            .current_dir(&session)
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn()
            .map(drop)
            .log_err();
        return;
    }
    crate::log("add my headset: no agent CLI, opening an issue");
    win32::open_path(Path::new(&issue_url(REPO, &dump)));
}

/// `%LOCALAPPDATA%\topsy\add-headset`, rewritten each time.
fn write_session(dump: &Dump) -> std::io::Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").ok_or(std::io::ErrorKind::NotFound)?;
    let session = PathBuf::from(base).join(r"topsy\add-headset");
    std::fs::create_dir_all(&session)?;
    let json = serde_json::to_string_pretty(dump).map_err(std::io::Error::other)?;
    std::fs::write(session.join("context.json"), json)?;
    std::fs::write(session.join("PLAYBOOK.md"), PLAYBOOK)?;
    Ok(session)
}

/// `name` runs from a console: an exe, `.cmd` or `.bat` of that name on `PATH`.
fn on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        ["exe", "cmd", "bat"]
            .iter()
            .any(|ext| dir.join(name).with_extension(ext).is_file())
    })
}
