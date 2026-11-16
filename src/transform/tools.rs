//! The mature tools Transform runs instead of reimplementing them (FFmpeg, ImageMagick, ...):
//! each is a pinned release archive, downloaded on first use (see `fetch`), unpacked under
//! `%LOCALAPPDATA%\topsy\tools`, and run as a process of its own, without a window.

#![allow(dead_code)] // Used once the engines that run tools are wired in.

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::fetch::{Pinned, Status, root};

/// One downloadable tool: its archive, and the program inside it.
pub struct Tool {
    pub archive: Pinned,
    /// The folder it is unpacked into, under `tools`.
    pub dir: &'static str,
    /// The program (or library) to use, relative to the archive's root.
    pub program: &'static str,
}

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// A transform runs while the user carries on; it shouldn't make the rest of the PC stutter.
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

impl Tool {
    fn home(&self) -> io::Result<PathBuf> {
        Ok(root()?.join("tools").join(self.dir))
    }

    /// Where the program is once the tool is here.
    pub fn program(&self) -> io::Result<PathBuf> {
        Ok(self.home()?.join(self.program))
    }

    pub fn status(&self) -> Status {
        match self.program() {
            Ok(p) if p.is_file() => Status::Ready,
            _ if self.archive.status() == Status::Downloading => Status::Downloading,
            _ => Status::Missing,
        }
    }

    /// The program, downloading and unpacking the tool first if it isn't here yet. Blocking.
    pub fn get(&self, progress: &dyn Fn(f32), cancel: &AtomicBool) -> io::Result<PathBuf> {
        let program = self.program()?;
        if program.is_file() {
            return Ok(program);
        }
        let archive = self.archive.download(progress, cancel)?;
        let home = self.home()?;
        // Unpacked beside its final place and renamed in, so a half-unpacked tool never counts.
        let staging = home.with_extension("unpacking");
        let _ = fs::remove_dir_all(&staging);
        let unpacked = unzip(&archive, &staging, cancel);
        if let Err(err) = unpacked {
            let _ = fs::remove_dir_all(&staging);
            return Err(err);
        }
        let _ = fs::remove_dir_all(&home);
        fs::rename(&staging, &home)?;
        let _ = fs::remove_file(&archive);
        if !program.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} isn't where it should be", self.program),
            ));
        }
        Ok(program)
    }
}

/// Unpack every entry of a zip under `to`, refusing names that would land outside it.
fn unzip(archive: &Path, to: &Path, cancel: &AtomicBool) -> io::Result<()> {
    let mut zip = zip::ZipArchive::new(File::open(archive)?).map_err(io::Error::other)?;
    for i in 0..zip.len() {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let mut entry = zip.by_index(i).map_err(io::Error::other)?;
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let path = to.join(name);
        if entry.is_dir() {
            fs::create_dir_all(&path)?;
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        io::copy(&mut entry, &mut File::create(&path)?)?;
    }
    Ok(())
}

/// Run `program` to the end, without a window and below normal priority, handing each line it
/// writes to stdout to `line` (FFmpeg's `-progress pipe:1` reports there). Setting `cancel`
/// ends the process. A failure carries the last thing it wrote to stderr.
pub fn run(
    program: &Path,
    args: &[&std::ffi::OsStr],
    cancel: &AtomicBool,
    line: &mut dyn FnMut(&str),
) -> io::Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS)
        .spawn()?;
    let stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    // The process may print nothing for a long while, so a watcher ends it on cancel rather
    // than the reader, which is blocked on its output. It only runs while a transform does.
    let watcher = std::thread::scope(|scope| {
        let watcher = scope.spawn(|| {
            loop {
                if let Some(status) = child.try_wait()? {
                    return Ok(status);
                }
                if cancel.load(Ordering::Relaxed) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(io::Error::from(io::ErrorKind::Interrupted));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        for text in BufReader::new(stdout).lines() {
            match text {
                Ok(text) => line(&text),
                Err(_) => break,
            }
        }
        watcher.join().expect("watcher panicked")
    });
    let errors = errors.join().unwrap_or_default();
    let status = watcher?;
    if status.success() {
        return Ok(());
    }
    let name = program.file_stem().unwrap_or_default().to_string_lossy();
    crate::log(&format!("{name} failed ({status}): {errors}"));
    let last = errors.lines().rev().find(|l| !l.trim().is_empty());
    Err(io::Error::other(match last {
        Some(last) => format!("{name}: {}", last.trim()),
        None => format!("{name} stopped with {status}"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_runs_without_a_window_and_reports_its_lines() {
        let cmd = Path::new(r"C:\Windows\System32\cmd.exe");
        let mut lines = Vec::new();
        let args = ["/c", "echo one&& echo two"].map(std::ffi::OsStr::new);
        run(cmd, &args, &AtomicBool::new(false), &mut |l| {
            lines.push(l.trim().to_owned())
        })
        .unwrap();
        assert_eq!(lines, ["one", "two"]);

        let args = ["/c", "echo broken 1>&2 && exit 3"].map(std::ffi::OsStr::new);
        let err = run(cmd, &args, &AtomicBool::new(false), &mut |_| {}).unwrap_err();
        assert_eq!(err.to_string(), "cmd: broken");
    }

    #[test]
    fn cancel_ends_a_tool_that_prints_nothing() {
        let cancel = AtomicBool::new(true);
        let ping = Path::new(r"C:\Windows\System32\ping.exe");
        let args = ["-n", "30", "127.0.0.1"].map(std::ffi::OsStr::new);
        let err = run(ping, &args, &cancel, &mut |_| {}).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
    }
}
