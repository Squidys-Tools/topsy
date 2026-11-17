//! Running the chosen action: a snapshot of the files and options goes to a worker thread,
//! which leaves every result beside its original under a free name. The panel steps aside for
//! result cards (`cards`) that show each file's progress and what it made. A failed or cancelled
//! run deletes what it had made, so nothing half-done is left behind, and brings the panel
//! back. Results go on the shelf; renamed chips follow their files.

use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{AsyncApp, Context};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::COINIT_MULTITHREADED;

use super::cards::{self, Cards};
use super::engines::{self, Work};
use super::{Open, Panel, State, image, video};
use crate::model::transform::{Action, Kind, combined_paths, name_problem, rename_targets};
use crate::model::transform_card::{Made, Sides};
use crate::{Island, archive, log, win32};

/// One file going in.
#[derive(Clone)]
pub struct Input {
    pub path: PathBuf,
    pub kind: Kind,
}

enum Report {
    /// Card `index` is working; `label` replaces the action's own word, as while a tool is
    /// fetched.
    Step {
        index: usize,
        label: Option<String>,
        progress: f32,
    },
    Made {
        index: usize,
        made: Made,
        output: PathBuf,
    },
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
        self.field = None;
        self.state = State::Running {
            cancel: cancel.clone(),
        };
        cx.notify();
        let (tx, rx) = async_channel::unbounded();
        let sources = self.paths();
        let worker_cancel = cancel.clone();
        let spawned = std::thread::Builder::new()
            .name("topsy-transform".into())
            .spawn(move || {
                let tell = |report: Report| {
                    let _ = tx.try_send(report);
                };
                let result = win32::with_com(COINIT_MULTITHREADED, || {
                    run(action, &work, &inputs, &tell, &worker_cancel)
                });
                let _ = tx.send_blocking(Report::Done(result));
            });
        if let Err(err) = spawned {
            self.state = State::Failed(format!("Could not start: {err}"));
            return cx.notify();
        }
        let hwnd = self.hwnd;
        cx.spawn(async move |this, cx: &mut AsyncApp| {
            // The panel steps aside for the cards, where it stood.
            // Hidden first, so the focus goes back to what was under it before the cards
            // note who has it; shown again if they can't open.
            let anchor = win32::window_rect(HWND(hwnd as *mut _));
            win32::set_visible(HWND(hwnd as *mut _), false);
            let opened = anchor.and_then(|anchor| {
                cx.update(|cx| cards::open(cx, anchor, action, sources, cancel))
                    .ok()
                    .flatten()
            });
            if opened.is_none() {
                win32::set_visible(HWND(hwnd as *mut _), true);
            }
            if this.update(cx, |panel, _| panel.cards = opened).is_err() {
                return;
            }
            while let Ok(report) = rx.recv().await {
                let live = this.update(cx, |panel, cx| panel.reported(report, cx));
                if live.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn reported(&mut self, report: Report, cx: &mut Context<Self>) {
        let cards = self.cards;
        let with_cards = |cx: &mut Context<Self>, f: &dyn Fn(&mut Cards, &mut Context<Cards>)| {
            if let Some(cards) = cards {
                cards.update(cx, |cards, _, cx| f(cards, cx)).ok();
            }
        };
        match report {
            Report::Step {
                index,
                label,
                progress,
            } => return with_cards(cx, &|c, cx| c.step(index, label.clone(), progress, cx)),
            Report::Made {
                index,
                made,
                output,
            } => return with_cards(cx, &|c, cx| c.made(index, made.clone(), output.clone(), cx)),
            Report::Done(Ok(outcome)) => {
                log(&format!(
                    "transform done: {} made, {} renamed",
                    outcome.outputs.len(),
                    outcome.renamed.len()
                ));
                if !outcome.renamed.is_empty() {
                    self.follow_renames(&outcome.renamed, cx);
                }
                self.shelve(outcome.outputs, cx);
                with_cards(cx, &|c, cx| c.finish(cx));
                self.state = State::Ready;
                // Done: the cards have it from here.
                if let Some(open) = cx.try_global::<Open>() {
                    let handle: gpui::AnyWindowHandle = open.0.into();
                    cx.defer(move |cx| {
                        handle
                            .update(cx, |_, window, _| window.remove_window())
                            .ok();
                    });
                }
            }
            Report::Done(Err(err)) => {
                log(&format!("transform failed: {err}"));
                with_cards(cx, &|c, cx| c.close(cx));
                self.cards = None;
                // A stop was just clicked, so the panel may take the focus back; a failure
                // can land while the user is elsewhere, so it only shows.
                let stopped = err.kind() == io::ErrorKind::Interrupted;
                self.state = if stopped {
                    State::Ready
                } else {
                    State::Failed(err.to_string())
                };
                let hwnd = self.hwnd;
                cx.spawn(async move |_, _: &mut AsyncApp| {
                    win32::set_visible(HWND(hwnd as *mut _), true);
                    if stopped {
                        win32::bring_to_front(HWND(hwnd as *mut _));
                    }
                })
                .detach();
            }
        }
        cx.notify();
    }

    /// Renamed files: the panel, the shelf and the basket follow them.
    fn follow_renames(&mut self, renamed: &[(PathBuf, PathBuf)], cx: &mut Context<Self>) {
        for (from, to) in renamed {
            if let Some(file) = self.files.iter_mut().find(|f| &f.path == from) {
                file.path = to.clone();
            }
        }
        if let Some(island) = self.island.upgrade() {
            let basket = island.update(cx, |island: &mut Island, cx| {
                island.follow_renames(renamed, cx);
                island.basket.clone()
            });
            if let Some(basket) = basket {
                basket.update(cx, |basket, cx| basket.follow_renames(renamed, cx));
            }
        }
    }
}

fn run(
    action: Action,
    work: &Work,
    inputs: &[Input],
    tell: &dyn Fn(Report),
    cancel: &AtomicBool,
) -> io::Result<Outcome> {
    let step = |index: usize, label: Option<String>, progress: f32| {
        tell(Report::Step {
            index,
            label,
            progress,
        })
    };
    let mut outcome = Outcome::default();
    match work {
        Work::Zip(name) => {
            step(0, None, 0.0);
            let sources: Vec<PathBuf> = inputs.iter().map(|i| i.path.clone()).collect();
            let candidates = combined_paths(&sources, name, "zip")
                .ok_or_else(|| io::Error::other("No folder to put the zip in"))?;
            let out = reserve(candidates)?;
            outcome.outputs.push(into(out, |out| {
                archive::compress_files(&sources, out, cancel)
            })?);
            tell(card(0, inputs, &outcome.outputs));
        }
        Work::Rename(names) => {
            step(0, None, 0.0);
            outcome.renamed = rename(inputs, names)?;
            if let (Some(input), Some(name)) = (inputs.first(), names.first()) {
                tell(Report::Made {
                    index: 0,
                    made: Made {
                        bytes: None,
                        sides: (None, None),
                        name: name.clone(),
                        renamed: outcome.renamed.len(),
                    },
                    output: input.path.with_file_name(name),
                });
            }
        }
        _ => {
            let note = |label: String, p: f32| step(0, Some(label), p);
            engines::prepare(work, inputs, &note, cancel)?;
            if action.combines() {
                outcome.outputs = engines::run_all(work, inputs, &note, cancel)?;
                tell(card(0, inputs, &outcome.outputs));
            } else {
                for (i, input) in inputs.iter().enumerate() {
                    step(i, None, 0.0);
                    let progress = |p: f32| step(i, None, p);
                    let made = match cancel.load(Ordering::Relaxed) {
                        true => Err(cancelled()),
                        false => engines::run_one(work, input, &progress, cancel),
                    };
                    match made {
                        Ok(paths) => {
                            tell(card(i, std::slice::from_ref(input), &paths));
                            outcome.outputs.extend(paths);
                        }
                        // All or nothing: what the earlier files made goes too.
                        Err(err) => {
                            for path in &outcome.outputs {
                                let _ = fs::remove_file(path);
                            }
                            if inputs.len() == 1 || err.kind() == io::ErrorKind::Interrupted {
                                return Err(err);
                            }
                            let name = input.path.file_name().unwrap_or_default();
                            return Err(io::Error::new(
                                err.kind(),
                                format!("{}: {err}", name.to_string_lossy()),
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(outcome)
}

/// What card `index` shows for `outputs` made from `inputs`: sizes, and a picture's sides when
/// one file became one.
fn card(index: usize, inputs: &[Input], outputs: &[PathBuf]) -> Report {
    let bytes = |p: &Path| fs::metadata(p).map_or(0, |m| m.len());
    let before = inputs.iter().map(|i| bytes(&i.path)).sum();
    let after = outputs.iter().map(|p| bytes(p)).sum();
    let output = (outputs.first().cloned())
        .or_else(|| inputs.first().map(|i| i.path.clone()))
        .unwrap_or_default();
    let sides = match (inputs, outputs) {
        ([input], [out]) => (sides(&input.path, input.kind), sides(out, Kind::of(out))),
        _ => (None, None),
    };
    let name = output.file_name().unwrap_or_default();
    Report::Made {
        index,
        made: Made {
            bytes: Some((before, after)),
            sides,
            name: name.to_string_lossy().into_owned(),
            renamed: 0,
        },
        output,
    }
}

fn sides(path: &Path, kind: Kind) -> Option<Sides> {
    match kind {
        Kind::Image => (image::probe(path).ok())
            .filter(|i| i.width > 0)
            .map(|i| (i.width, i.height)),
        Kind::Video => (video::probe(path).ok()?.video).map(|v| (v.width, v.height)),
        _ => None,
    }
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
        if let Err(err) = win32::move_new(&from, &to) {
            // Put back what already moved, so a half-done rename doesn't scatter names.
            for (from, to) in done.iter().rev() {
                let _ = win32::move_new(to, from);
            }
            return Err(err);
        }
        done.push((from, to));
    }
    Ok(done)
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
        assert!(win32::move_new(&dir.join("a.png"), &dir.join("Trip 02.png")).is_err());
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
