//! Progress for the Transform engines: reported at most about 30 times a second, and
//! `ErrorKind::Interrupted` between steps once `cancel` is set.

use std::cell::Cell;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Throttles progress reports and checks for a cancel.
pub struct Steps<'a> {
    report: &'a dyn Fn(f32),
    cancel: &'a AtomicBool,
    last: Cell<Option<Instant>>,
}

impl<'a> Steps<'a> {
    pub fn new(report: &'a dyn Fn(f32), cancel: &'a AtomicBool) -> Self {
        Self {
            report,
            cancel,
            last: Cell::new(None),
        }
    }

    /// Reports `done` (0..=1) unless one went out in the last 33 ms, then fails if cancelled.
    pub fn at(&self, done: f32) -> io::Result<()> {
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

    pub fn check(&self) -> io::Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(io::Error::new(io::ErrorKind::Interrupted, "Cancelled"))
        } else {
            Ok(())
        }
    }
}
