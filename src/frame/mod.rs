//! A glass window and its shape: where it sits (hanging from the top edge as the notch, or
//! floating where a jiggle opened it, as the basket), how big it is, and what it is filled with.
//! The OS window is always exactly the shape (docs/internals/notch.md, glass.md).

mod float;
mod paint;
mod window;

pub(crate) use window::{Popup, Shape, open_glass, open_window};

use std::sync::atomic::{AtomicUsize, Ordering};

use gpui::{App, Pixels, Size};
use windows::Win32::Foundation::HWND;

use crate::backdrop::{self, Backdrop, Outline};
use crate::{log, win32};
use model::notch::Notch;

/// The notch's body when it peeks down from the top edge, and when it holds the tray: its least
/// width, and the shelf page's height. The shelf page is wider, to fit four chips.
pub const PEEK: (f32, f32) = (200.0, 32.0);
pub const TRAY: (f32, f32) = (400.0, 152.0);
pub const SHELF_WIDTH: f32 = 460.0;
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
/// The tray icon's menu's corners.
const MENU_RADIUS: f32 = 12.0;
/// The settings window's corners.
const PANEL_RADIUS: f32 = 20.0;

/// The acrylic fallback's own tint (0xRRGGBBAA).
pub const ACRYLIC_TINT: u32 = 0x0c101c8c;
/// Droppy's recipe, measured from its screenshots: solid black for the top 45% of the notch,
/// then fading to about 44% black at the bottom edge, so the backdrop's own colour shows
/// through, darkened but not greyed. The basket is filled the same way, to a darker floor.
const BLACK_UNTIL: f32 = 0.45;
const GLASS_FLOOR: u32 = 0x00000070;
/// The open notch's floor over the lens glass: lighter, so the bend shows. A floating window's is
/// darker, since its text sits low, over whatever is behind it.
const LENS_FLOOR: u32 = 0x00000040;
const FLOATING_LENS_FLOOR: u32 = 0x00000090;
/// The tray icon's menu: one even shade of glass, dark enough that it only just shows.
const MENU_FLOOR: u32 = 0x000000c4;
/// The basket's floor, sampled from Droppy's: about 88% black at the bottom edge.
pub const BASKET_FLOOR: u32 = 0x000000e0;
/// A light rim along the bottom edge, fading out up the sides where the notch turns black.
const RIM: u32 = 0xffffff38;

/// The window that stays in front of every other frame while it is open (the clipboard palette,
/// which hangs where the island does), as a raw handle; 0 for none. A frame that appears is
/// raised to just under it rather than over it.
static ON_TOP: AtomicUsize = AtomicUsize::new(0);

/// Keep `hwnd` in front of every other frame until [`release_top`].
pub(crate) fn keep_on_top(hwnd: HWND) {
    ON_TOP.store(hwnd.0 as usize, Ordering::Relaxed);
}

/// `hwnd` no longer stays in front; a newer one that took its place does.
pub(crate) fn release_top(hwnd: HWND) {
    ON_TOP
        .compare_exchange(hwnd.0 as usize, 0, Ordering::Relaxed, Ordering::Relaxed)
        .ok();
}

/// How the notch is filled. `TOPSY_GLASS` can force one, for A/B comparison and for the
/// automated blur check (`tint` and `clear` are its controls).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glass {
    /// The compositor's blur of whatever is behind the notch, in its own shape ([`Backdrop`]),
    /// under Droppy's black-to-glass fill.
    Blur,
    /// The same, held to the host backdrop and its fixed blur.
    HostBlur,
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
    /// The compositor glass to try, for the kinds drawn over one.
    pub fn backdrop(self) -> Option<backdrop::Kind> {
        match self {
            Glass::Blur => Some(backdrop::Kind::Live),
            Glass::HostBlur => Some(backdrop::Kind::Host),
            Glass::Acrylic | Glass::Tint | Glass::Solid | Glass::Clear => None,
        }
    }

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

/// A window's origin and size in physical pixels, whether it was hidden, the notch's height
/// within it (the rest is the capsule's strip), which shapes its region and glass, and whether
/// it was open.
type Applied = ((i32, i32), (i32, i32), bool, i32, bool);

pub struct Frame {
    hwnd: Option<HWND>,
    glass: Glass,
    /// The "Black notch" setting: draw [`Glass::Solid`] whatever the glass is.
    black: bool,
    /// The compositor glass, reshaped with the window (the kinds with a [`Glass::backdrop`]).
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
    /// The lens glass is behind the open notch or the floating window.
    lensed: bool,
    /// The glass's outline when it was last shaped, physical pixels: a move by Windows moves
    /// the glass's view of what is behind without reshaping it.
    glass_outline: Option<Outline>,
    /// The corners of a floating window.
    radius: f32,
    /// One shade of glass at the floor all the way down, without the black band at the top.
    even: bool,
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
            lensed: false,
            glass_outline: None,
            radius: FLOAT_RADIUS,
            even: false,
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

    /// The *Live glass* setting, for the open notch's lens.
    pub fn set_live_lens(&self, live: bool) {
        if let Some(backdrop) = &self.backdrop {
            backdrop.set_live_lens(live);
        }
    }

    /// Whether the pointer is over a floating window, whose lens then follows the screen faster.
    pub fn set_lens_active(&self, active: bool) {
        if let Some(backdrop) = &self.backdrop {
            backdrop.set_lens_active(active);
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
            self.radius.min(height / 2.0)
        }
    }

    /// Make the OS window exactly the shape for a body of `width` x `height` (logical pixels),
    /// hanging from the anchor or centred on the basket's point. The lens glass, if it has one,
    /// takes the glass while `open`.
    ///
    /// The window is resized every frame as the spring moves, and its region and the glass's
    /// clip are reshaped with it. Hidden, it keeps a 1 px window (GPUI can't draw into an empty
    /// one) whose region is empty, so nothing shows and nothing takes clicks.
    ///
    /// The resize runs in a foreground task rather than here: this is called from `render`,
    /// and a resize mid-draw would reach GPUI's window handler where it cannot update the
    /// window (see [`win32::move_window`]).
    /// `strip` is the room under the notch for the page capsule, as it springs out.
    pub fn sync(
        &mut self,
        width: f32,
        height: f32,
        strip: f32,
        drawn_scale: f32,
        open: bool,
        cx: &mut App,
    ) {
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
            Outline {
                x: (w - CAPSULE.0) / 2.0 * s,
                y: top,
                width: CAPSULE.0 * s,
                height: CAPSULE.1 * s,
                radius: CAPSULE.1 * s / 2.0,
            }
        });
        let origin = match self.place {
            Place::Notch => match self.anchor {
                Some((center_x, top)) => (center_x - size.0 / 2, top),
                None => return,
            },
            Place::Floating { center, .. } => (center.0 - size.0 / 2, center.1 - size.1 / 2),
        };
        let hidden = notch.is_hidden();
        let applied = (origin, size, hidden, px(h), open);
        if self.applied == Some(applied) {
            return;
        }
        let appearing = !hidden
            && self
                .applied
                .is_none_or(|(_, _, was_hidden, _, _)| was_hidden);
        self.applied = Some(applied);
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
        let blur = !hidden && self.look().backdrop().is_some();
        self.lensed = blur && open && self.backdrop.as_ref().is_some_and(Backdrop::has_lens);
        let tall = |h: f32| if blur { h } else { 0.0 };
        // The notch's rounded top corners sit above the window, which squares them off.
        let glass = if floating {
            Outline {
                x: 0.0,
                y: 0.0,
                width: size.0 as f32,
                height: tall(size.1 as f32),
                radius: r,
            }
        } else {
            Outline {
                x: notch.ear_slot * s,
                y: -r,
                width: notch.body_width * s,
                height: tall(h * s + r),
                radius: r,
            }
        };
        self.glass_outline = Some(glass);
        cx.spawn(async move |_| {
            // The lens reads the screen as it opens: before the window grows over it.
            if let Some(shape) = &shape {
                shape.set(origin, size, s, glass, open);
            }
            let on_top = ON_TOP.load(Ordering::Relaxed);
            if appearing && on_top != 0 && on_top != hwnd.0 as usize {
                let above = HWND(on_top as *mut _);
                win32::move_window_under(hwnd, origin.0, origin.1, size.0, size.1, above);
            } else {
                win32::move_window(hwnd, origin.0, origin.1, size.0, size.1, appearing);
            }
            win32::set_clip(hwnd, clip);
            if let Some(shape) = &shape {
                shape.restack();
            }
            if let Some(shape) = capsule_shape {
                let none = Outline::default();
                let capsule = capsule_glass.filter(|_| blur).unwrap_or(none);
                shape.set(origin, size, s, capsule, false);
            }
        })
        .detach();
    }
}

/// Whether a fullscreen app (a video, a game, a slideshow) has the screen under the cursor, so
/// the hook's gestures stay out of its way; `own` is the asking window. Read at the moment of the
/// gesture rather than tracked, so nothing runs meanwhile.
pub(crate) fn fullscreen(own: Option<HWND>) -> bool {
    let fullscreen = own.is_some_and(win32::fullscreen_over);
    if fullscreen {
        log("fullscreen app in front: staying hidden");
    }
    fullscreen
}
