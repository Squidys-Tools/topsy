//! The glass behind the notch, drawn by the system compositor under GPUI's content, in any shape
//! we ask for (docs/internals/glass.md).
//!
//! [`Live`] is DWM's live copy of what is behind the window, blurred and saturated by amounts we
//! choose. Where that is refused, [`Host`] is the system's host backdrop, whose blur is fixed.

mod host;
mod live;

use windows::Win32::Foundation::HWND;

use self::host::{Host, HostShape};
use self::live::{Live, LiveShape};
use crate::log;

/// How much more saturated the glass is than the desktop behind it, as Droppy's glass borrows the
/// colour behind it rather than greying it.
const SATURATION: f32 = 1.45;

/// Which compositor glass to try.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// The live copy, falling back to the host backdrop.
    Live,
    /// Only the host backdrop.
    Host,
}

pub enum Backdrop {
    Live(Live),
    Host(Host),
}

impl Backdrop {
    /// Put the glass behind `hwnd`'s content. Fails where neither kind can start (before
    /// Windows 11), where the caller falls back to window-wide acrylic.
    pub fn new(hwnd: HWND, kind: Kind, cx: &mut gpui::App) -> windows::core::Result<Self> {
        if kind == Kind::Live {
            match Live::new(hwnd, cx) {
                Ok(live) => return Ok(Backdrop::Live(live)),
                Err(e) => log(&format!("live glass unavailable ({e}); host backdrop")),
            }
        }
        Host::new(hwnd, cx).map(Backdrop::Host)
    }

    /// The handle to reshape the glass from a task; cheap to clone.
    pub fn shape(&self) -> Shape {
        match self {
            Backdrop::Live(live) => Shape::Live(live.shape()),
            Backdrop::Host(host) => Shape::Host(host.shape()),
        }
    }

    /// The page capsule's pane, the same kind of handle.
    pub fn capsule(&self) -> Shape {
        match self {
            Backdrop::Live(live) => Shape::Live(live.capsule()),
            Backdrop::Host(host) => Shape::Host(host.capsule()),
        }
    }
}

/// A rounded rectangle in physical pixels of the window's client area. Parts outside the window
/// are cut off with it, which is how the notch gets a square top: its rounded top corners sit
/// above the window.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Outline {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub radius: f32,
}

#[derive(Clone)]
pub enum Shape {
    Live(LiveShape),
    Host(HostShape),
}

impl Shape {
    /// Shape the glass of a window whose top-left corner is at `origin` (physical screen pixels),
    /// on a monitor of `scale`.
    pub fn set(&self, origin: (i32, i32), scale: f32, outline: Outline) {
        match self {
            Shape::Live(live) => live.set(origin, scale, outline),
            Shape::Host(host) => host.set(outline),
        }
    }
}
