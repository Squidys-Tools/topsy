//! A glass window and its shape: where it sits (hanging from the top edge as the notch, or
//! floating where a jiggle opened it, as the basket), how big it is, and what it is filled with.
//! The OS window is always exactly the shape (docs/internals/notch.md, glass.md).

mod paint;

use gpui::{App, Pixels, Size};
use windows::Win32::Foundation::HWND;

use crate::backdrop::Backdrop;
use crate::model::notch::Notch;
use crate::{Island, log, win32};

/// The notch's body when it peeks down from the top edge, and when it holds the tray.
pub const PEEK: (f32, f32) = (200.0, 32.0);
pub const TRAY: (f32, f32) = (400.0, 118.0);
/// The page capsule under the open tray, and the strip of window it hangs in: Droppy's, a gap
/// below the notch.
pub const CAPSULE: (f32, f32) = (94.0, 32.0);
pub const STRIP: f32 = 8.0 + CAPSULE.1;
/// The capsule's fill and edge on the glass.
const CAPSULE_FILL: u32 = 0x000000b3;
const CAPSULE_EDGE: u32 = 0xffffff24;

/// DWM's fixed window corner, which clips the window-wide acrylic of [`Glass::Acrylic`].
const DWM_RADIUS: f32 = 8.0;
/// The basket's corners, rounded all round.
const FLOAT_RADIUS: f32 = 26.0;

/// The acrylic fallback's own tint (0xRRGGBBAA).
pub const ACRYLIC_TINT: u32 = 0x0c101c8c;
/// Droppy's recipe, measured from its screenshots: solid black for the top 62% of the notch,
/// then fading to about 62% black at the bottom edge, so the backdrop's own colour shows
/// through, darkened but not greyed. The basket is filled the same way, to a darker floor.
const BLACK_UNTIL: f32 = 0.62;
const GLASS_FLOOR: u32 = 0x0000009f;
/// The basket's floor, sampled from Droppy's: about 88% black at the bottom edge.
pub const BASKET_FLOOR: u32 = 0x000000e0;
/// A light rim along the bottom edge, fading out up the sides where the notch turns black.
const RIM: u32 = 0xffffff38;

/// How the notch is filled. `TOPSY_GLASS` can force one, for A/B comparison and for the
/// automated blur check (`tint` and `clear` are its controls).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glass {
    /// The compositor's blur of whatever is behind the notch, in its own shape ([`Backdrop`]),
    /// under Droppy's black-to-glass fill.
    Blur,
    /// Window-wide acrylic, which only takes DWM's fixed corner. The fallback where the
    /// compositor glass can't start.
    Acrylic,
    /// The same fill with no blur behind it: the notch without its blur.
    Tint,
    /// Opaque black.
    Solid,
    /// Fully transparent, no blur.
    Clear,
}

impl Glass {
    /// The window-wide acrylic can't leave the ears' corners clear, so it goes without them.
    fn has_ears(self) -> bool {
        self != Glass::Acrylic
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Folded up past the top edge: nothing drawn, nothing to click.
    Hidden,
    /// The notch, showing what is on the shelf.
    Peek,
    Tray,
}

impl Mode {
    /// The notch body's size; hidden keeps the peek's width so it slides straight down.
    pub fn size(self) -> (f32, f32) {
        match self {
            Mode::Hidden => (PEEK.0, 0.0),
            Mode::Peek => PEEK,
            Mode::Tray => TRAY,
        }
    }
}

/// Where the window sits.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Place {
    /// Hanging from the top edge of its monitor.
    Notch,
    /// The basket, centred on `center` (physical screen pixels) on a monitor of `scale`.
    Floating { center: (i32, i32), scale: f32 },
}

/// A window's origin and size in physical pixels, and whether it was hidden.
type Applied = ((i32, i32), (i32, i32), bool);

pub struct Frame {
    hwnd: Option<HWND>,
    glass: Glass,
    /// The "Black notch" setting: draw [`Glass::Solid`] whatever the glass is.
    black: bool,
    /// The compositor glass, reshaped with the window ([`Glass::Blur`] only).
    backdrop: Option<Backdrop>,
    /// Where the notch hangs: horizontal centre and top edge of its monitor, in physical
    /// screen pixels. Every resize keeps the window centred on this point.
    pub anchor: Option<(i32, i32)>,
    /// The notch monitor's scale factor.
    scale: f32,
    place: Place,
    /// The scale GPUI last drew at; it changes when the window lands on a monitor of another DPI.
    drawn_scale: f32,
    /// Last requested from the OS, to skip redundant calls.
    applied: Option<Applied>,
    /// How dark the glass is at the bottom edge (0xRRGGBBAA).
    floor: u32,
    /// The capsule strip each recent window height (physical) was made with. The window takes
    /// its new size a frame after it is asked for, so a frame is split by the size it has.
    strips: Vec<(i32, f32)>,
}

impl Frame {
    pub fn new(
        hwnd: Option<HWND>,
        scale: f32,
        glass: Glass,
        black: bool,
        backdrop: Option<Backdrop>,
    ) -> Self {
        Self {
            hwnd,
            glass,
            black,
            backdrop,
            anchor: hwnd.and_then(win32::monitor_top_center),
            scale,
            place: Place::Notch,
            drawn_scale: scale,
            applied: None,
            floor: GLASS_FLOOR,
            strips: Vec::new(),
        }
    }

    /// The same glass, floating: the basket's window, placed with [`Frame::float_at`].
    pub fn floating(mut self) -> Self {
        self.anchor = None;
        self.place = Place::Floating {
            center: (0, 0),
            scale: self.scale,
        };
        self.floor = BASKET_FLOOR;
        self
    }

    /// Centre the floating window on `center` (physical screen pixels), on a monitor of `scale`.
    pub fn float_at(&mut self, center: (i32, i32), scale: f32) {
        self.place = Place::Floating { center, scale };
        self.applied = None;
    }

    /// Where the floating window is centred, in physical screen pixels.
    pub fn center(&self) -> Option<(i32, i32)> {
        match self.place {
            Place::Floating { center, .. } => Some(center),
            Place::Notch => None,
        }
    }

    /// The window was moved by Windows (its handle dragged as a caption): float from where it is
    /// now, rather than snapping back on the next frame. Only a move counts, at the size last
    /// asked for: a resize still on its way would read as one.
    pub fn follow_window(&mut self) {
        let (Some(hwnd), Some((origin, size, false)), Place::Floating { scale, .. }) =
            (self.hwnd, self.applied, self.place)
        else {
            return;
        };
        let Some(r) = win32::window_rect(hwnd) else {
            return;
        };
        let (left, top) = (r.left, r.top);
        if (r.right - left, r.bottom - top) == size && (left, top) != origin {
            self.place = Place::Floating {
                center: (left + size.0 / 2, top + size.1 / 2),
                scale,
            };
            self.applied = Some(((left, top), size, false));
        }
    }

    /// What the notch is drawn with right now.
    fn look(&self) -> Glass {
        if self.black { Glass::Solid } else { self.glass }
    }

    fn is_floating(&self) -> bool {
        matches!(self.place, Place::Floating { .. })
    }

    pub fn set_black(&mut self, black: bool, cx: &mut App) {
        self.black = black;
        // Reshape (or empty) the glass on the next frame.
        self.applied = None;
        if let (Glass::Acrylic, Some(hwnd)) = (self.glass, self.hwnd) {
            cx.spawn(async move |_| win32::set_acrylic(hwnd, (!black).then_some(ACRYLIC_TINT)))
                .detach();
        }
    }

    /// The notch in a window of `viewport`'s size, and the capsule strip under it.
    pub fn outline(&self, viewport: Size<Pixels>) -> (Notch, f32) {
        let ears = !self.is_floating() && self.look().has_ears();
        let height = f32::from(viewport.height);
        let tall = (height * self.drawn_scale).round() as i32;
        let strip = (self.strips.iter().rev())
            .find(|(made, _)| *made == tall)
            .or(self.strips.last())
            .map_or(0.0, |(_, strip)| *strip)
            .min(height);
        let notch = Notch::fitting(viewport.width.into(), height - strip, ears);
        (notch, strip)
    }

    fn float_radius(&self, height: f32) -> f32 {
        if self.look() == Glass::Acrylic {
            DWM_RADIUS
        } else {
            FLOAT_RADIUS.min(height / 2.0)
        }
    }

    /// Make the OS window exactly the shape for a body of `width` x `height` (logical pixels),
    /// hanging from the anchor or centred on the basket's point.
    ///
    /// The window is resized every frame as the spring moves, and its region and the glass's
    /// clip are reshaped with it. Hidden, it keeps a 1 px window (GPUI can't draw into an empty
    /// one) whose region is empty, so nothing shows and nothing takes clicks.
    ///
    /// The resize runs in a foreground task rather than here: this is called from `render`,
    /// and a resize mid-draw would reach GPUI's window handler where it cannot update the
    /// window (see [`win32::move_window`]).
    /// `strip` is the room under the notch for the page capsule, as it springs out.
    pub fn sync(&mut self, width: f32, height: f32, strip: f32, drawn_scale: f32, cx: &mut App) {
        if drawn_scale != self.drawn_scale {
            // GPUI has moved the window to the size Windows suggested for the new DPI.
            self.drawn_scale = drawn_scale;
            self.applied = None;
        }
        let Some(hwnd) = self.hwnd else {
            return;
        };
        let (s, floating) = match self.place {
            Place::Notch => (self.scale, false),
            Place::Floating { scale, .. } => (scale, true),
        };
        let notch = Notch::new(width.max(1.0), height, !floating && self.look().has_ears());
        let px = |v: f32| (v * s).round() as i32;
        let (w, h) = notch.window_size();
        let strip = if notch.is_hidden() || floating {
            0.0
        } else {
            strip.max(0.0)
        };
        let size = (px(w).max(1), px(h + strip).max(1));
        if self
            .strips
            .last()
            .is_none_or(|last| *last != (size.1, strip))
        {
            self.strips.push((size.1, strip));
            let stale = self.strips.len().saturating_sub(8);
            self.strips.drain(..stale);
        }
        // The capsule springs out from under the notch: its glass is whole, its clicks only
        // below the notch.
        let capsule = (strip >= 0.5).then(|| {
            let top = h + strip - CAPSULE.1;
            win32::Capsule {
                left: px((w - CAPSULE.0) / 2.0),
                top: px(top.max(h)),
                width: px(CAPSULE.0),
                height: px(CAPSULE.1 - (h - top).max(0.0)),
            }
        });
        let capsule_glass = (strip >= 0.5).then(|| {
            let top = (h + strip - CAPSULE.1) * s;
            ((w - CAPSULE.0) / 2.0 * s, top, CAPSULE.0 * s, CAPSULE.1 * s)
        });
        let origin = match self.place {
            Place::Notch => match self.anchor {
                Some((center_x, top)) => (center_x - size.0 / 2, top),
                None => return,
            },
            Place::Floating { center, .. } => (center.0 - size.0 / 2, center.1 - size.1 / 2),
        };
        let hidden = notch.is_hidden();
        if self.applied == Some((origin, size, hidden)) {
            return;
        }
        let appearing = !hidden && self.applied.is_none_or(|(_, _, was_hidden)| was_hidden);
        self.applied = Some((origin, size, hidden));
        let radius = if floating {
            self.float_radius(notch.height)
        } else {
            notch.radius
        };
        let clip = if hidden {
            win32::Clip::Nothing
        } else if self.look() == Glass::Acrylic {
            win32::Clip::Everything
        } else if floating {
            win32::Clip::Rounded {
                width: size.0,
                height: size.1,
                radius: px(radius),
            }
        } else {
            win32::Clip::Notch {
                width: size.0,
                height: px(h),
                ear_slot: px(notch.ear_slot),
                ear: px(notch.ear),
                radius: px(radius),
                capsule,
            }
        };
        let shape = self.backdrop.as_ref().map(Backdrop::shape);
        let capsule_shape = self.backdrop.as_ref().map(Backdrop::capsule);
        let r = radius * s;
        // No blur shows as a glass of no height.
        let blur = !hidden && self.look() == Glass::Blur;
        let tall = |h: f32| if blur { h } else { 0.0 };
        // The notch's rounded top corners sit above the window, which squares them off.
        let glass = if floating {
            (0.0, 0.0, size.0 as f32, tall(size.1 as f32), r)
        } else {
            let left = notch.ear_slot * s;
            (left, -r, notch.body_width * s, tall(h * s + r), r)
        };
        cx.spawn(async move |_| {
            win32::move_window(hwnd, origin.0, origin.1, size.0, size.1, appearing);
            win32::set_clip(hwnd, clip);
            if let Some(shape) = shape {
                shape.set(glass.0, glass.1, glass.2, glass.3, glass.4);
            }
            if let Some(shape) = capsule_shape {
                let (x, y, w, h) = capsule_glass.filter(|_| blur).unwrap_or_default();
                shape.set(x, y, w, h, h / 2.0);
            }
        })
        .detach();
    }
}

impl Island {
    /// A fullscreen video, game or slideshow has the screen: the hook's gestures stay out of its
    /// way. Read at the moment of the gesture rather than tracked, so nothing runs meanwhile.
    pub(crate) fn fullscreen(&self) -> bool {
        fullscreen(self.hwnd)
    }
}

/// Whether a fullscreen app has the screen under the cursor; `own` is the asking window.
pub(crate) fn fullscreen(own: Option<HWND>) -> bool {
    let fullscreen = own.is_some_and(win32::fullscreen_over);
    if fullscreen {
        log("fullscreen app in front: staying hidden");
    }
    fullscreen
}
