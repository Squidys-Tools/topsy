//! The rules of the island, with no Win32, no GPUI, no clock and no environment: each takes
//! time and input as arguments, so its tests drive them directly. `scripts/check-structure.ps1`
//! holds this folder to that.

pub mod archive;
pub mod clip_kind;
pub mod clipboard;
pub mod dib;
pub mod edge;
pub mod favicon;
pub mod float;
pub mod highlight;
pub mod hud;
pub mod jiggle;
pub mod levels;
pub mod media;
pub mod notch;
// Drawn by build.rs into the exe's icon; here only for its tests.
#[cfg(test)]
pub mod pill;
pub mod shelf;
pub mod spring;
pub mod transform;
pub mod tray_cause;
