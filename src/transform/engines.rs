//! The bridge from the panel's options to the engines that do the work.

use std::io;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use gpui::{Context, Div};

use super::Panel;
use super::background::{self, ModelStatus};
use super::job::{Input, into, reserve};
use super::widgets::note;
use crate::model::transform::{Action, result_paths};

/// A snapshot of what to do, sent to the worker thread.
pub enum Work {
    Zip(String),
    Rename(Vec<String>),
    Background,
    Pending(Action),
}

/// The present participle the progress line uses.
pub fn verb(action: Action) -> &'static str {
    match action {
        Action::Compress => "Compressing",
        Action::Convert => "Converting",
        Action::Resize => "Resizing",
        Action::Trim => "Trimming",
        Action::MakeGif => "Making GIF",
        Action::RemoveBackground => "Removing background",
        Action::Rotate => "Rotating",
        Action::RemoveMetadata => "Removing metadata",
        Action::CombinePdf => "Combining",
        Action::Zip => "Zipping",
        Action::Rename => "Renaming",
    }
}

pub fn button_label(_panel: &Panel, action: Action) -> (String, bool) {
    (action.label().into(), true)
}

pub fn work(_panel: &Panel, action: Action) -> Result<Work, String> {
    Ok(match action {
        Action::RemoveBackground => Work::Background,
        _ => Work::Pending(action),
    })
}

/// What has to happen once before the files: fetching the background model on first use.
pub fn prepare(work: &Work, progress: &dyn Fn(String, f32), cancel: &AtomicBool) -> io::Result<()> {
    if matches!(work, Work::Background) && background::model_status() != ModelStatus::Ready {
        background::download(&|p| progress("Getting the model…".into(), p), cancel)?;
    }
    Ok(())
}

/// A result beside `src`, `name (suffix).ext`, made by `make` and deleted again if it fails.
fn beside(
    src: &std::path::Path,
    action: Action,
    ext: &str,
    make: impl FnOnce(&std::path::Path) -> io::Result<()>,
) -> io::Result<Vec<PathBuf>> {
    let candidates =
        result_paths(src, action, ext).ok_or_else(|| io::Error::other("No folder to save in"))?;
    Ok(vec![into(reserve(candidates)?, make)?])
}

pub fn run_one(
    work: &Work,
    input: &Input,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<Vec<PathBuf>> {
    match work {
        Work::Background => beside(&input.path, Action::RemoveBackground, "png", |out| {
            background::remove(&input.path, out, progress, cancel)
        }),
        Work::Pending(action) => Err(io::Error::other(format!(
            "{} isn't ready yet",
            action.label()
        ))),
        _ => Err(io::Error::other("Not a per-file action")),
    }
}

pub fn run_all(
    work: &Work,
    inputs: &[Input],
    progress: &dyn Fn(String, f32),
    cancel: &AtomicBool,
) -> io::Result<Vec<PathBuf>> {
    let _ = (inputs, progress, cancel);
    run_one(work, &inputs[0], &|_| {}, cancel)
}

impl Panel {
    pub fn range_rows(&self, _action: Action, _cx: &mut Context<Self>) -> Vec<Div> {
        vec![note("Loading the clip…")]
    }

    pub fn background_row(&self, _cx: &mut Context<Self>) -> Div {
        note(match background::model_status() {
            ModelStatus::Ready => "Runs on this PC; the picture never leaves it.",
            _ => "The first run downloads the 178 MB model, once. After that it runs on this PC.",
        })
    }
}
