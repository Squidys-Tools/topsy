//! The rules of the island, with no Win32, no GPUI, no clock and no environment: each takes
//! time and input as arguments, so its tests drive them directly. `scripts/check-structure.ps1`
//! holds this folder to that.

pub mod edge;
pub mod jiggle;
pub mod media;
pub mod notch;
pub mod shelf;
pub mod spring;
pub mod tray_cause;
