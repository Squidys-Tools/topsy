//! Progress for the Transform engines: reported at most about 30 times a second, and
//! `ErrorKind::Interrupted` between steps once `cancel` is set.

use std::cell::Cell;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::tools::Tool;
use model::download::ModelStatus;

/// Throttles progress reports and checks for a cancel.
pub struct Steps<'a> {
    report: &'a dyn Fn(f32),
    cancel: &'a AtomicBool,
    last: Cell<Option<Instant>>,
    /// The share of the bar already spent on downloading tools; the work fills the rest.
    fetched: Cell<f32>,
}

impl<'a> Steps<'a> {
    pub fn new(report: &'a dyn Fn(f32), cancel: &'a AtomicBool) -> Self {
        Self {
            report,
            cancel,
            last: Cell::new(None),
            fetched: Cell::new(0.0),
        }
    }

    /// Reports `done` (0..=1) unless one went out in the last 33 ms, then fails if cancelled.
    pub fn at(&self, done: f32) -> io::Result<()> {
        let base = self.fetched.get();
        self.raw(base + (1.0 - base) * done.clamp(0.0, 1.0))
    }

    fn raw(&self, done: f32) -> io::Result<()> {
        let now = Instant::now();
        let due = self
            .last
            .get()
            .is_none_or(|t| now.duration_since(t) >= Duration::from_millis(33));
        if due || done >= 1.0 {
            self.last.set(Some(now));
            (self.report)(done.clamp(0.0, 1.0));
        }
        self.check()
    }

    pub fn cancel(&self) -> &'a AtomicBool {
        self.cancel
    }

    pub fn check(&self) -> io::Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(io::Error::new(io::ErrorKind::Interrupted, "Cancelled"))
        } else {
            Ok(())
        }
    }

    /// The tool's program, downloading it first if it isn't here. Call before any `at`: a
    /// download takes the next 40 % of the bar.
    pub fn tool(
        &self,
        tool: &Tool,
        keep: &dyn Fn(&std::path::Path) -> bool,
    ) -> io::Result<PathBuf> {
        if tool.status() == ModelStatus::Ready {
            return tool.program();
        }
        let from = self.fetched.get();
        let span = (1.0 - from) * 0.4;
        let program = tool.get_with(keep, &|p| drop(self.raw(from + span * p)), self.cancel)?;
        self.fetched.set(from + span);
        Ok(program)
    }
}
