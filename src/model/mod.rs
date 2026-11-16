//! The rules of the island, with no Win32, no GPUI, no clock and no environment: each takes
//! time and input as arguments, so its tests drive them directly. `scripts/check-structure.ps1`
//! holds this folder to that.

pub mod archive;
pub mod clip_kind;
pub mod clipboard;
pub mod dib;
pub mod edge;
#[allow(dead_code)] // Transform, until the panel calls it.
pub mod exif;
pub mod favicon;
pub mod float;
#[allow(dead_code)]
pub mod heif;
pub mod highlight;
pub mod hud;
#[allow(dead_code)]
pub mod image_geometry;
#[allow(dead_code)]
pub mod image_ops;
pub mod jiggle;
#[allow(dead_code)]
pub mod jpeg;
pub mod levels;
pub mod media;
pub mod notch;
#[allow(dead_code)]
pub mod orientation;
#[allow(dead_code)]
pub mod pdf;
// Drawn by build.rs into the exe's icon; here only for its tests.
#[cfg(test)]
pub mod pill;
#[allow(dead_code)]
pub mod png;
#[allow(dead_code)]
pub mod png_opt;
#[allow(dead_code)]
pub mod quantize;
pub mod shelf;
#[allow(dead_code)]
pub mod size_search;
pub mod spring;
pub mod tray_cause;
#[allow(dead_code)]
pub mod webp;
