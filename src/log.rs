//! What topsy says on stderr: the event trace the live checks read, and failures nobody else
//! hears about.

use std::fmt::Debug;
use std::io;
use std::panic::Location;

use crate::config;

/// Event trace for scripted tests; silent unless TOPSY_LOG is set.
pub fn log(message: &str) {
    if config::get().log {
        eprintln!("topsy: {message}");
    }
}

/// For a fallible call whose failure changes nothing for the caller: the failure is reported
/// with where it happened, instead of dropped with `let _ =`.
pub trait LogErr<T> {
    fn log_err(self) -> Option<T>;
}

impl<T, E: Debug> LogErr<T> for Result<T, E> {
    #[track_caller]
    fn log_err(self) -> Option<T> {
        match self {
            Ok(value) => Some(value),
            Err(err) => {
                let at = Location::caller();
                eprintln!("topsy: {}:{}: {err:?}", at.file(), at.line());
                None
            }
        }
    }
}

/// Removing what is already gone is not a failure.
pub trait MissingOk {
    fn missing_ok(self) -> Self;
}

impl MissingOk for io::Result<()> {
    fn missing_ok(self) -> Self {
        match self {
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}
