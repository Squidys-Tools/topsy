//! The bridge from the panel's options to the engines that do the work.

use std::io;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use gpui::{Context, Div};

use super::Panel;
use super::job::Input;
use super::widgets::note;
use crate::model::transform::Action;

/// A snapshot of what to do, sent to the worker thread.
pub enum Work {
    Zip(String),
    Rename(Vec<String>),
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
    Ok(Work::Pending(action))
}

pub fn run_one(
    work: &Work,
    _input: &Input,
    _progress: &dyn Fn(f32),
    _cancel: &AtomicBool,
) -> io::Result<Vec<PathBuf>> {
    Err(io::Error::other(match work {
        Work::Pending(action) => format!("{} isn't ready yet", action.label()),
        _ => "Not a per-file action".into(),
    }))
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
        note("Runs on this PC; the picture never leaves it.")
    }
}
