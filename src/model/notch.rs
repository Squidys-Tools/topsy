//! The notch's outline: a body hanging from the top edge of the screen, square at the top and
//! rounded at the bottom, with a small concave flare ("ear") either side where it meets the
//! edge, as on a MacBook. Pure geometry in logical pixels; no OS calls.

/// How far each ear reaches out from the body, at most (the open tray).
pub const EAR: f32 = 16.0;
/// Droppy's ears reach out about this share of the notch's height.
const EAR_SHARE: f32 = 0.14;
/// But never smaller than this, so the closed notch still flares into the edge.
const MIN_EAR: f32 = 6.0;
/// The bottom corners never round more than this (the open tray).
pub const MAX_RADIUS: f32 = 24.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Notch {
    pub body_width: f32,
    pub height: f32,
    /// Room left for an ear on each side of the body; zero when the glass can't draw them.
    pub ear_slot: f32,
    /// The ear's current size: it shrinks with a short notch so it never outgrows the body.
    pub ear: f32,
    /// Radius of the bottom corners.
    pub radius: f32,
}

impl Notch {
    pub fn new(body_width: f32, height: f32, ears: bool) -> Self {
        let height = height.max(0.0);
        let ear_slot = if ears { EAR } else { 0.0 };
        Self {
            body_width,
            height,
            ear_slot,
            ear: (height * EAR_SHARE)
                .max(MIN_EAR)
                .min(height / 2.0)
                .min(ear_slot),
            radius: (height * 0.375).min(MAX_RADIUS),
        }
    }

    /// The notch that fills a window of this size.
    pub fn fitting(window_width: f32, height: f32, ears: bool) -> Self {
        let slot = if ears { EAR } else { 0.0 };
        Self::new((window_width - 2.0 * slot).max(0.0), height, ears)
    }

    pub fn window_size(&self) -> (f32, f32) {
        (self.body_width + 2.0 * self.ear_slot, self.height)
    }

    /// Too short to show: nothing should draw or take clicks.
    pub fn is_hidden(&self) -> bool {
        self.height < 1.0
    }
}

/// The open glass is solid black down to this share of its height, as Droppy's is.
const BLACK_UNTIL: f32 = 0.45;

/// The tint over the glass of a body `h` tall on its way to `target`: solid black down to the
/// returned share of the body (never less than `at_least` pixels), then fading to the returned
/// colour at its foot, which reaches `floor` (0xRRGGBBAA) once open. The black is measured on
/// the height the body is growing to, so the glass rises out of it rather than it sliding down.
pub fn tint(h: f32, target: f32, at_least: f32, floor: u32) -> (f32, u32) {
    let h = h.max(1.0);
    let reach = target.max(h);
    let black_px = (reach * BLACK_UNTIL).max(at_least).min(reach);
    let glass_share = if reach > black_px {
        ((h - black_px) / (reach - black_px)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let alpha = 255.0 - glass_share * (255.0 - (floor & 0xff) as f32);
    (
        (black_px / h).min(1.0),
        (floor & 0xffffff00) | alpha.round() as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_glass_rises_out_of_the_black_as_the_tray_opens() {
        let floor = 0x00000070;
        assert_eq!(tint(32.0, 32.0, 32.0, floor), (1.0, 0x000000ff));
        let (until, bottom) = tint(118.0, 118.0, 32.0, floor);
        assert!((until - BLACK_UNTIL).abs() < 1e-6);
        assert_eq!(bottom, floor);
        // Halfway open: the black already reaches as far down as it will, the foot half clear.
        let (until, bottom) = tint(80.0, 118.0, 32.0, floor);
        assert!((until * 80.0 - 118.0 * BLACK_UNTIL).abs() < 1e-3);
        assert!(bottom & 0xff > 0x70 && bottom & 0xff < 0xff);
    }

    #[test]
    fn the_window_leaves_room_for_an_ear_each_side() {
        let n = Notch::new(200.0, 32.0, true);
        assert_eq!(n.window_size(), (200.0 + 2.0 * EAR, 32.0));
        assert_eq!(Notch::fitting(200.0 + 2.0 * EAR, 32.0, true), n);
        assert_eq!(Notch::new(200.0, 32.0, false).window_size(), (200.0, 32.0));
    }

    #[test]
    fn ears_and_corners_shrink_with_a_short_notch() {
        let sliding = Notch::new(200.0, 6.0, true);
        assert_eq!(sliding.ear, 3.0);
        assert_eq!(Notch::new(200.0, 32.0, true).ear, MIN_EAR);
        assert!(sliding.radius <= sliding.height / 2.0);
        let tray = Notch::new(400.0, 118.0, true);
        assert_eq!((tray.ear, tray.radius), (EAR, MAX_RADIUS));
    }

    #[test]
    fn a_spring_overshooting_past_closed_stays_hidden_and_sane() {
        let n = Notch::new(200.0, -3.0, true);
        assert!(n.is_hidden());
        assert_eq!((n.height, n.ear, n.radius), (0.0, 0.0, 0.0));
        assert!(!Notch::new(200.0, 1.0, true).is_hidden());
    }
}
