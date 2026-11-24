//! Placing a floating frame: the basket, the floating player, the tray icon's menu and the
//! settings window.

use gpui::App;

use super::{BASKET_FLOOR, Frame, MENU_FLOOR, MENU_RADIUS, PANEL_RADIUS, Place};
use crate::win32;

impl Frame {
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

    /// Floating, evenly glazed, with a menu's corners: the tray icon's menu.
    pub fn menu(self) -> Self {
        let mut frame = self.floating();
        frame.floor = MENU_FLOOR;
        frame.radius = MENU_RADIUS;
        frame.even = true;
        frame
    }

    /// Floating and evenly glazed like the menu, with rounder corners and the basket's darker
    /// floor, since it holds lines of small text: the settings window.
    pub fn panel(self) -> Self {
        let mut frame = self.menu();
        frame.radius = PANEL_RADIUS;
        frame.floor = BASKET_FLOOR;
        frame
    }

    /// A notch of its own hanging from the top edge, glazed down to the basket's darker floor so
    /// the text low in it reads over any backdrop: the clipboard palette.
    pub fn hanging(mut self) -> Self {
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
    /// now, rather than snapping back on the next frame, and move the glass's view of what is
    /// behind with it. Only a move counts, at the size last asked for: a resize still on its way
    /// would read as one.
    pub fn follow_window(&mut self, cx: &mut App) {
        let (Some(hwnd), Some((origin, size, false, notch, open)), Place::Floating { scale, .. }) =
            (self.hwnd, self.applied, self.place)
        else {
            return;
        };
        let Some(r) = win32::window_rect(hwnd) else {
            return;
        };
        let moved_to = (r.left, r.top);
        if (r.right - r.left, r.bottom - r.top) != size || moved_to == origin {
            return;
        }
        self.place = Place::Floating {
            center: (moved_to.0 + size.0 / 2, moved_to.1 + size.1 / 2),
            scale,
        };
        self.applied = Some((moved_to, size, false, notch, open));
        if let (Some(backdrop), Some(glass)) = (&self.backdrop, self.glass_outline) {
            let shape = backdrop.shape();
            cx.spawn(async move |_| shape.set(moved_to, size, scale, glass, open))
                .detach();
        }
    }
}
