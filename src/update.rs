//! Updating a nightly build in place: a check at launch and every 4 hours, a quiet download of
//! the newer exe, then the swap when the user restarts into it (from the settings page's About,
//! which a click on the HUD opens, or the tray icon's menu), or at the next launch. Only a nightly updates: a
//! local or CI build carries no release version. The rules are in `model::update`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui::{App, Entity};

use crate::autostart::RegString;
use crate::log::{LogErr, MissingOk};
use crate::transform::fetch::{self, Pinned};
use crate::win32::http::{self, Response};
use crate::{Island, config, hud, log};
use model::download::split_url;
use model::update::{self, EXE, Nightly};

/// What the settings page shows: the release version a nightly is stamped with
/// (`.github/workflows/nightly.yml`), else the crate's.
pub const VERSION: &str = match STAMP {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};
pub(crate) const STAMP: Option<&str> = option_env!("TOPSY_RELEASE_VERSION");

const API: &str = "api.github.com";
const RELEASES: &str = "/repos/Squidys-Tools/topsy/releases?per_page=20";
/// GitHub's release list, with every asset; a page of 20 is well under this.
const LIST_LIMIT: usize = 4 << 20;
const MAX_REDIRECTS: usize = 4;
const EVERY: Duration = Duration::from_secs(4 * 60 * 60);
const SETTING: RegString<'static> = RegString::new(r"Software\topsy", "Updates");

/// The downloaded, checked nightly waiting to be installed.
static READY: Mutex<Option<Nightly>> = Mutex::new(None);
static RESTARTING: AtomicBool = AtomicBool::new(false);

fn running() -> Option<Nightly> {
    STAMP.and_then(Nightly::parse)
}

pub fn is_enabled() -> bool {
    SETTING.value().as_deref() != Some("off")
}

pub fn set_enabled(on: bool) -> windows::core::Result<()> {
    if on {
        SETTING.remove()
    } else {
        SETTING.set("off")
    }
}

pub fn ready() -> Option<Nightly> {
    READY.lock().ok().and_then(|ready| *ready)
}

fn dir() -> io::Result<PathBuf> {
    Ok(fetch::root()?.join("update"))
}

/// The newest downloaded nightly newer than `current`, with its file.
fn downloaded(current: Nightly) -> Option<(Nightly, PathBuf)> {
    fs::read_dir(dir().ok()?)
        .ok()?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let version = Nightly::from_file_name(path.file_name()?.to_str()?)?;
            (version > current).then_some((version, path))
        })
        .max_by_key(|(version, _)| *version)
}

/// At launch, before anything starts: a nightly downloaded earlier and never restarted into is
/// installed now, and started in this one's place with the same arguments. True if it was.
pub fn install_downloaded() -> bool {
    let Some(current) = running() else {
        return false;
    };
    if !is_enabled() {
        return false;
    }
    let Some((version, path)) = downloaded(current) else {
        return false;
    };
    log(&format!("update: installing {version} at launch"));
    install(&path).log_err().is_some() && relaunch().log_err().is_some()
}

/// Check now and every 4 hours, downloading a newer nightly in the background; when one is
/// ready, the island shows the HUD and every surface offers the restart.
pub fn start(cx: &mut App, island: &Entity<Island>) {
    let Some(current) = running() else {
        return;
    };
    if let Ok(exe) = std::env::current_exe() {
        fs::remove_file(old(&exe)).missing_ok().log_err();
    }
    let (found, heard) = async_channel::bounded(1);
    let spawned = std::thread::Builder::new()
        .name("update".into())
        .spawn(move || {
            let mut offered = None;
            loop {
                if is_enabled()
                    && let Some(version) = check(current).log_err().flatten()
                    && offered != Some(version)
                {
                    offered = Some(version);
                    if found.send_blocking(version).is_err() {
                        return;
                    }
                }
                std::thread::sleep(EVERY);
            }
        });
    spawned.log_err();
    crate::app::forward(cx, island, heard, |island, version, cx| {
        if let Ok(mut ready) = READY.lock() {
            *ready = Some(version);
        }
        log(&format!("update: {version} ready"));
        island.update_hud(cx, |hud, mode, cx| {
            hud.show(hud::Event::Update(version), mode, cx)
        });
    });
    cx.on_app_quit(|_| {
        if RESTARTING.load(Ordering::Relaxed) {
            relaunch().log_err();
        }
        async {}
    })
    .detach();
}

/// Install the downloaded nightly and restart into it. If the install fails topsy keeps
/// running as it was.
pub fn restart(cx: &mut App) {
    let Some(version) = ready() else {
        return;
    };
    let installed = dir().and_then(|dir| install(&dir.join(version.file_name())));
    if installed.log_err().is_some() {
        RESTARTING.store(true, Ordering::Relaxed);
        cx.quit();
    }
}

/// The newest nightly newer than `current`, downloaded and checked, if there is one.
fn check(current: Nightly) -> io::Result<Option<Nightly>> {
    let Some(list) = get(API, RELEASES, LIST_LIMIT) else {
        return Ok(None);
    };
    let Some(offer) = update::newest(&list, current) else {
        return Ok(None);
    };
    let name = offer.version.file_name();
    let dir = dir()?;
    // Only the newest is kept: older downloads and their partial files go.
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let file = entry.file_name();
            if file != name.as_str() && file != format!("{name}.part").as_str() {
                fs::remove_file(entry.path()).missing_ok().log_err();
            }
        }
    }
    let (sums_host, sums_path) = split_url(&offer.sums.url, API)
        .ok_or_else(|| io::Error::other("an odd checksum address"))?;
    let sums = get(sums_host, sums_path, 64 << 10)
        .ok_or_else(|| io::Error::other("couldn't get the checksums"))?;
    let sha256 = update::sha256_of(&String::from_utf8_lossy(&sums), EXE)
        .ok_or_else(|| io::Error::other("no checksum for the exe"))?;
    let (host, path) = split_url(&offer.exe.url, API)
        .ok_or_else(|| io::Error::other("an odd download address"))?;
    Pinned {
        label: "the update",
        host: host.to_string(),
        path: path.to_string(),
        size: offer.exe.size,
        sha256,
        file: format!(r"update\{name}"),
    }
    .download(&|_| {}, &AtomicBool::new(false))?;
    Ok(Some(offer.version))
}

/// A small GET, following redirects.
fn get(host: &str, path: &str, limit: usize) -> Option<Vec<u8>> {
    let (mut host, mut path) = (host.to_string(), path.to_string());
    for _ in 0..=MAX_REDIRECTS {
        match http::get(&host, &path, limit) {
            Response::Body(body) => return Some(body),
            Response::Redirect(to) => {
                let (next_host, next_path) = split_url(&to, &host)?;
                (host, path) = (next_host.to_string(), next_path.to_string());
            }
            Response::Missing | Response::Failed => return None,
        }
    }
    None
}

fn old(exe: &Path) -> PathBuf {
    exe.with_extension("exe.old")
}

/// Put `new` where the running exe is. Windows lets a running exe be renamed, not replaced, so
/// it steps aside first (and is deleted at the next start); on failure it is put back.
fn install(new: &Path) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let old = old(&exe);
    fs::remove_file(&old).missing_ok()?;
    fs::rename(&exe, &old)?;
    if let Err(err) = fs::copy(new, &exe) {
        // A failed copy can leave part of the new exe behind, which the rename back can't replace.
        fs::remove_file(&exe).missing_ok().log_err();
        fs::rename(&old, &exe).log_err();
        return Err(err);
    }
    fs::remove_file(new).missing_ok().log_err();
    Ok(())
}

/// Start the exe now in place, with this one's arguments, to take over once this one exits.
fn relaunch() -> io::Result<()> {
    Command::new(std::env::current_exe()?)
        .args(std::env::args_os().skip(1))
        .env(config::REPLACES, std::process::id().to_string())
        .spawn()
        .map(drop)
}
