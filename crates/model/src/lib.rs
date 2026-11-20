//! The rules of the island, with no Win32, no GPUI, no clock and no environment: each takes
//! time and input as arguments, so its tests drive them directly. The crate depends on neither
//! `gpui` nor `windows`; `scripts/check-structure.ps1` keeps the clock and the environment out.

pub mod activity;
// Drawn by build.rs into the exe's icon; here only for its tests.
#[cfg(test)]
pub mod app_icon;
pub mod archive;
pub mod basket;
pub mod browser_download;
pub mod calendar;
pub mod clamp;
pub mod clip_kind;
pub mod clipboard;
pub mod dib;
pub mod download;
pub mod edge;
pub mod exif;
pub mod favicon;
pub mod float;
pub mod heif;
pub mod highlight;
pub mod hud;
pub mod image_geometry;
pub mod image_ops;
pub mod jiggle;
pub mod jpeg;
pub mod lens;
pub mod levels;
pub mod magick;
pub mod marquee;
pub mod matte;
pub mod media;
pub mod mixer;
pub mod name_field;
pub mod notch;
pub mod orientation;
pub mod pager;
pub mod pdf;
pub mod pdf_image;
pub mod pdf_place;
pub mod pomodoro;
pub mod selection;
pub mod sessions;
pub mod shelf;
pub mod size_search;
pub mod spring;
pub mod stats;
pub mod transform;
pub mod transform_card;
pub mod tray_cause;
pub mod video;
pub mod weather;
pub mod widgets;
