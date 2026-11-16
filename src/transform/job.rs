//! Running the chosen action: a snapshot of the files and options goes to a worker thread,
//! which reports progress back to the panel and leaves every result beside its original under
//! a free name. A failed or cancelled step deletes what it had started, so nothing
//! half-written is left behind. Results go on the shelf; renamed chips follow their files.

use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{AsyncApp, Context};
use windows::Win32::Storage::FileSystem::{MOVE_FILE_FLAGS, MoveFileExW};
use windows::Win32::System::Com::COINIT_MULTITHREADED;

use super::engines::{self, Work};
use super::{Panel, State};
use crate::model::transform::{Action, Kind, combined_paths, name_problem, rename_targets, size};
use crate::{Island, archive, log, win32};

/// One file going in.
#[derive(Clone)]
pub struct Input {
    pub path: PathBuf,
    pub kind: Kind,
}

enum Report {
    Step { label: String, progress: f32 },
    Done(io::Result<Outcome>),
}

#[derive(Default)]
pub struct Outcome {
    pub outputs: Vec<PathBuf>,
    pub renamed: Vec<(PathBuf, PathBuf)>,
}

/// Claim the first free name: `create_new` makes it atomically, so a file that appears there
/// meanwhile is never overwritten.
pub fn reserve(candidates: impl Iterator<Item = PathBuf>) -> io::Result<PathBuf> {
    for path in candidates {
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(path),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "No free name for the result",
    ))
}

/// Run `make` into a reserved `out`, deleting `out` again if it fails.
pub fn into(out: PathBuf, make: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<PathBuf> {
    match make(&out) {
        Ok(()) => Ok(out),
        Err(err) => {
            let _ = fs::remove_file(&out);
            Err(err)
        }
    }
}

fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "Cancelled")
}

impl Panel {
    /// The run button's label, and whether it can run with the options as they are.
    pub fn button_label(&self, action: Action) -> (String, bool) {
        let n = self.files.len();
        let o = &self.options;
        match action {
            Action::Rename => {
                let label = if n == 1 {
                    "Rename".into()
                } else {
                    format!("Rename {n} files")
                };
                (label, name_problem(&o.rename).is_none())
            }
            Action::Zip => ("Zip".into(), name_problem(&o.zip_name).is_none()),
            Action::CombinePdf => (format!("Combine {n} into PDF"), n > 0),
            Action::Resize if o.resize_by == 2 => (
                "Resize".into(),
                o.width.parse::<u32>().is_ok_and(|w| w > 0)
                    || o.height.parse::<u32>().is_ok_and(|h| h > 0),
            ),
            _ => engines::button_label(self, action),
        }
    }

    /// Run the chosen action on every file.
    pub fn start(&mut self, cx: &mut Context<Self>) {
        let Some(action) = self.action else { return };
        if self.running() || !self.button_label(action).1 || self.files.is_empty() {
            return;
        }
        let inputs: Vec<Input> = (self.files.iter())
            .map(|f| Input {
                path: f.path.clone(),
                kind: f.kind,
            })
            .collect();
        let work = match action {
            Action::Zip => Work::Zip(self.options.zip_name.trim().to_owned()),
            Action::Rename => Work::Rename(rename_targets(&self.paths(), &self.options.rename, 1)),
            _ => match engines::work(self, action) {
                Ok(work) => work,
                Err(message) => {
                    self.state = State::Failed(message);
                    return cx.notify();
                }
            },
        };
        log(&format!("transform {action:?} on {} files", inputs.len()));
        let cancel = Arc::new(AtomicBool::new(false));
        self.state = State::Running {
            label: format!("{}…", engines::verb(action)),
            progress: 0.0,
            cancel: cancel.clone(),
        };
        cx.notify();
        let (tx, rx) = async_channel::unbounded();
        let spawned = std::thread::Builder::new()
            .name("topsy-transform".into())
            .spawn(move || {
                let report = |label: String, progress: f32| {
                    let _ = tx.try_send(Report::Step { label, progress });
                };
                let result = win32::with_com(COINIT_MULTITHREADED, || {
                    run(action, &work, &inputs, &report, &cancel)
                });
                let _ = tx.send_blocking(Report::Done(result));
            });
        if let Err(err) = spawned {
            self.state = State::Failed(format!("Could not start: {err}"));
            return cx.notify();
        }
        cx.spawn(async move |this, cx: &mut AsyncApp| {
            while let Ok(report) = rx.recv().await {
                let live = this.update(cx, |panel, cx| panel.reported(action, report, cx));
                if live.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn reported(&mut self, action: Action, report: Report, cx: &mut Context<Self>) {
        match report {
            Report::Step { label, progress } => {
                if let State::Running {
                    label: shown,
                    progress: at,
                    ..
                } = &mut self.state
                {
                    *shown = label;
                    *at = progress;
                }
            }
            Report::Done(Ok(outcome)) => {
                let before: u64 = self.files.iter().map(|f| f.bytes).sum();
                let after: u64 = (outcome.outputs.iter())
                    .filter_map(|p| fs::metadata(p).ok())
                    .map(|m| m.len())
                    .sum();
                let message = summary(action, &outcome, before, after);
                log(&format!("transform done: {message}"));
                if !outcome.renamed.is_empty() {
                    self.follow_renames(&outcome.renamed, cx);
                }
                self.shelve(outcome.outputs.clone(), cx);
                self.state = State::Done {
                    outputs: if outcome.outputs.is_empty() {
                        (outcome.renamed.iter()).map(|(_, to)| to.clone()).collect()
                    } else {
                        outcome.outputs
                    },
                    message,
                };
            }
            Report::Done(Err(err)) => {
                log(&format!("transform failed: {err}"));
                self.state = if err.kind() == io::ErrorKind::Interrupted {
                    State::Ready
                } else {
                    State::Failed(err.to_string())
                };
            }
        }
        cx.notify();
    }

    /// Renamed files: the panel and the shelf follow them.
    fn follow_renames(&mut self, renamed: &[(PathBuf, PathBuf)], cx: &mut Context<Self>) {
        for (from, to) in renamed {
            if let Some(file) = self.files.iter_mut().find(|f| &f.path == from) {
                file.path = to.clone();
            }
        }
        let renamed = renamed.to_vec();
        if let Some(island) = self.island.upgrade() {
            island.update(cx, |island: &mut Island, cx| {
                let mut moved = false;
                for (from, to) in renamed {
                    if island.shelf.rename(&from, to.clone()) {
                        if let Some(icon) = island.icons.remove(&from) {
                            island.icons.insert(to, icon);
                        }
                        moved = true;
                    }
                }
                if moved {
                    island.shelf_changed(cx);
                }
            });
        }
    }
}

/// What the panel says when it is done: `Saved 3.1 MB (52%)`, `3 files renamed`.
fn summary(action: Action, outcome: &Outcome, before: u64, after: u64) -> String {
    let n = outcome.outputs.len();
    match action {
        Action::Rename => match outcome.renamed.len() {
            1 => "Renamed".into(),
            n => format!("{n} files renamed"),
        },
        Action::Compress if after < before && before > 0 => {
            let saved = before - after;
            format!(
                "Saved {} ({:.0}%)",
                size(saved),
                saved as f64 * 100.0 / before as f64
            )
        }
        Action::Compress => "Already as small as it gets".into(),
        _ if n == 1 => format!("Done · {}", size(after)),
        _ => format!("{n} files · {}", size(after)),
    }
}

fn run(
    action: Action,
    work: &Work,
    inputs: &[Input],
    report: &dyn Fn(String, f32),
    cancel: &AtomicBool,
) -> io::Result<Outcome> {
    let mut outcome = Outcome::default();
    match work {
        Work::Zip(name) => {
            report("Zipping…".into(), 0.0);
            let sources: Vec<PathBuf> = inputs.iter().map(|i| i.path.clone()).collect();
            let candidates = combined_paths(&sources, name, "zip")
                .ok_or_else(|| io::Error::other("No folder to put the zip in"))?;
            let out = reserve(candidates)?;
            outcome.outputs.push(into(out, |out| {
                archive::compress_files(&sources, out, cancel)
            })?);
        }
        Work::Rename(names) => {
            outcome.renamed = rename(inputs, names)?;
        }
        _ => {
            engines::prepare(work, inputs, report, cancel)?;
            let total = inputs.len() as f32;
            let verb = engines::verb(action);
            if action.combines() {
                outcome.outputs = engines::run_all(work, inputs, report, cancel)?;
            } else {
                for (i, input) in inputs.iter().enumerate() {
                    if cancel.load(Ordering::Relaxed) {
                        return Err(cancelled());
                    }
                    let label = if inputs.len() == 1 {
                        format!("{verb}…")
                    } else {
                        format!("{verb} {} of {}…", i + 1, inputs.len())
                    };
                    report(label.clone(), i as f32 / total);
                    let step = |p: f32| report(label.clone(), (i as f32 + p) / total);
                    let made = engines::run_one(work, input, &step, cancel);
                    match made {
                        Ok(paths) => outcome.outputs.extend(paths),
                        // Results already made stay, and so does what is on the shelf.
                        Err(err) if outcome.outputs.is_empty() => return Err(err),
                        Err(err) => {
                            log(&format!("transform step failed: {err}"));
                            return Err(io::Error::new(
                                err.kind(),
                                format!("{err} ({} of {} done)", i, inputs.len()),
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(outcome)
}

/// Rename each file in place. Every new name is checked first, so a clash stops the whole
/// rename before anything moves; only a change of case may land on the file's own name.
fn rename(inputs: &[Input], names: &[String]) -> io::Result<Vec<(PathBuf, PathBuf)>> {
    let mut moves = Vec::new();
    for (input, name) in inputs.iter().zip(names) {
        let parent = input
            .path
            .parent()
            .ok_or_else(|| io::Error::other("No folder"))?;
        let to = parent.join(name);
        let same =
            to.to_string_lossy().to_lowercase() == input.path.to_string_lossy().to_lowercase();
        if to.exists() && !same {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{name} already exists"),
            ));
        }
        if to != input.path {
            moves.push((input.path.clone(), to));
        }
    }
    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (from, to) in moves {
        if let Err(err) = move_new(&from, &to) {
            // Put back what already moved, so a half-done rename doesn't scatter names.
            for (from, to) in done.iter().rev() {
                let _ = move_new(to, from);
            }
            return Err(err);
        }
        done.push((from, to));
    }
    Ok(done)
}

/// Move a file to a name that must still be free: unlike `fs::rename`, never replaces a file
/// that appeared there since the check.
fn move_new(from: &Path, to: &Path) -> io::Result<()> {
    let wide = |p: &Path| windows::core::HSTRING::from(p.as_os_str());
    unsafe { MoveFileExW(&wide(from), &wide(to), MOVE_FILE_FLAGS(0)) }.map_err(io::Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("topsy-rename-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn inputs(dir: &Path, names: &[&str]) -> Vec<Input> {
        names
            .iter()
            .map(|n| {
                let path = dir.join(n);
                fs::write(&path, n).unwrap();
                Input {
                    path,
                    kind: Kind::Image,
                }
            })
            .collect()
    }

    #[test]
    fn a_rename_that_would_land_on_another_file_moves_nothing() {
        let dir = scratch("clash");
        let files = inputs(&dir, &["a.png", "b.png"]);
        fs::write(dir.join("Trip 02.png"), "theirs").unwrap();
        let names = ["Trip 01.png".to_string(), "Trip 02.png".to_string()];
        assert!(rename(&files, &names).is_err());
        assert!(dir.join("a.png").exists() && dir.join("b.png").exists());
        assert_eq!(
            fs::read_to_string(dir.join("Trip 02.png")).unwrap(),
            "theirs"
        );

        // A file that appears after the check is never replaced either.
        assert!(move_new(&dir.join("a.png"), &dir.join("Trip 02.png")).is_err());
        assert_eq!(
            fs::read_to_string(dir.join("Trip 02.png")).unwrap(),
            "theirs"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rename_may_change_only_the_case() {
        let dir = scratch("case");
        let files = inputs(&dir, &["trip.png"]);
        let moved = rename(&files, &["Trip.png".to_string()]).unwrap();
        assert_eq!(moved, [(dir.join("trip.png"), dir.join("Trip.png"))]);
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["Trip.png"]);
        let _ = fs::remove_dir_all(&dir);
    }
}
