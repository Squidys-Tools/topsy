//! The glass behind the notch, drawn under GPUI's content in any shape we ask for
//! (docs/internals/glass.md).
//!
//! [`Live`] is DWM's live copy of what is behind the window, blurred and saturated by amounts we
//! choose. Where that is refused, [`Host`] is the system's host backdrop, whose blur is fixed.
//! The open notch and the floating player can also have a [`Lens`]: our own shader, which bends
//! what is behind it.

mod host;
mod lens;
mod live;

use windows::Win32::Foundation::HWND;

use self::host::{Host, HostShape};
use self::lens::{Holder, Lens};
use self::live::{Live, LiveShape};
use crate::log;

/// How much more saturated the glass is than the desktop behind it, as Droppy's glass borrows the
/// colour behind it rather than greying it.
const SATURATION: f32 = 1.45;
/// The blur's standard deviation in logical pixels: light enough that what is behind stays
/// recognisable through the glass, as it does through Droppy's.
const BLUR: f32 = 4.0;

/// Which compositor glass to try.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// The live copy, falling back to the host backdrop.
    Live,
    /// Only the host backdrop.
    Host,
}

enum Compositor {
    Live(Live),
    Host(Host),
}

pub struct Backdrop {
    compositor: Compositor,
    lens: Option<Lens>,
}

impl Backdrop {
    /// Put the glass behind `hwnd`'s content. Fails where neither kind can start (before
    /// Windows 11), where the caller falls back to window-wide acrylic.
    pub fn new(hwnd: HWND, kind: Kind, cx: &mut gpui::App) -> windows::core::Result<Self> {
        let compositor = match kind {
            Kind::Live => match Live::new(hwnd, cx) {
                Ok(live) => Compositor::Live(live),
                Err(e) => {
                    log(&format!("live glass unavailable ({e}); host backdrop"));
                    Compositor::Host(Host::new(hwnd, cx)?)
                }
            },
            Kind::Host => Compositor::Host(Host::new(hwnd, cx)?),
        };
        Ok(Self {
            compositor,
            lens: None,
        })
    }

    /// Give the open notch the lens glass, following the screen if `live`. Without it, the
    /// compositor glass shows there too.
    pub fn with_lens(mut self, hwnd: HWND, live: bool, cx: &mut gpui::App) -> Self {
        let shared = match &self.compositor {
            Compositor::Live(_) => live::made(cx),
            Compositor::Host(_) => None,
        };
        match Lens::new(hwnd, Holder::Notch, live, shared, cx) {
            Ok(lens) => self.lens = Some(lens),
            Err(e) => log(&format!("lens glass unavailable ({e})")),
        }
        self
    }

    /// Give a floating window of at most `reach` (logical pixels) the lens glass, which always
    /// follows the screen, so only over the live glass.
    pub fn with_floating_lens(mut self, hwnd: HWND, reach: (f32, f32), cx: &mut gpui::App) -> Self {
        let Compositor::Live(_) = &self.compositor else {
            return self;
        };
        let shared = live::made(cx);
        match Lens::new(hwnd, Holder::Floating { reach }, true, shared, cx) {
            Ok(lens) => self.lens = Some(lens),
            Err(e) => log(&format!("floating lens glass unavailable ({e})")),
        }
        self
    }

    /// Whether the pointer is over a floating window: its lens follows the screen faster.
    pub fn set_lens_active(&self, active: bool) {
        if let Some(lens) = &self.lens {
            lens.set_active(active);
        }
    }

    pub fn has_lens(&self) -> bool {
        self.lens.is_some()
    }

    /// The *Live glass* setting.
    pub fn set_live_lens(&self, live: bool) {
        if let Some(lens) = &self.lens {
            lens.set_live(live);
        }
    }

    /// The handle to reshape the glass from a task; cheap to clone.
    pub fn shape(&self) -> Shape {
        Shape {
            compositor: match &self.compositor {
                Compositor::Live(live) => CompositorShape::Live(live.shape()),
                Compositor::Host(host) => CompositorShape::Host(host.shape()),
            },
            lens: self.lens.clone(),
        }
    }

    /// The page capsule's pane, the same kind of handle. The lens is the window's alone.
    pub fn capsule(&self) -> Shape {
        Shape {
            compositor: match &self.compositor {
                Compositor::Live(live) => CompositorShape::Live(live.capsule()),
                Compositor::Host(host) => CompositorShape::Host(host.capsule()),
            },
            lens: None,
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
enum CompositorShape {
    Live(LiveShape),
    Host(HostShape),
}

#[derive(Clone)]
pub struct Shape {
    compositor: CompositorShape,
    lens: Option<Lens>,
}

impl Shape {
    /// Shape the glass of a window whose top-left corner is at `origin` and whose size is `size`
    /// (physical screen pixels), on a monitor of `scale`. The lens takes the glass while `open`.
    pub fn set(
        &self,
        origin: (i32, i32),
        size: (i32, i32),
        scale: f32,
        outline: Outline,
        open: bool,
    ) {
        let lensed = open
            && outline.height > 0.0
            && self
                .lens
                .as_ref()
                .is_some_and(|lens| lens.show(origin, size, scale, outline));
        if !lensed && let Some(lens) = &self.lens {
            lens.hide();
        }
        // The lens is under the island; the compositor glass would hide it.
        let outline = if lensed {
            Outline {
                height: 0.0,
                ..outline
            }
        } else {
            outline
        };
        match &self.compositor {
            CompositorShape::Live(live) => live.set(origin, scale, outline),
            CompositorShape::Host(host) => host.set(outline),
        }
    }

    /// The window has been raised: put the notch's lens back under it.
    pub fn restack(&self) {
        if let Some(lens) = &self.lens {
            lens.restack();
        }
    }
}
