//! Where the lens glass reads the screen from.

use super::float::Rect;

/// How far the open notch can reach, in logical pixels: the tray with its ears and the spring's
/// overshoot, plus the margin the bevel bends in from outside the edge.
const REACH: (f32, f32) = (720.0, 260.0);

/// The part of the screen the glass reads, for a notch hanging from `top` and centred on
/// `center_x`, on `monitor` at `scale`. It holds every size the open notch springs through, so
/// one read serves the whole opening.
pub fn source(center_x: i32, top: i32, scale: f32, monitor: Rect) -> Rect {
    let width = (REACH.0 * scale).round() as i32;
    let height = (REACH.1 * scale).round() as i32;
    Rect {
        left: (center_x - width / 2).max(monitor.left),
        top: top.max(monitor.top),
        right: (center_x - width / 2 + width).min(monitor.right),
        bottom: (top + height).min(monitor.bottom),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITOR: Rect = Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };

    #[test]
    fn the_source_is_centred_under_the_notch_and_scales() {
        let at_100 = source(960, 0, 1.0, MONITOR);
        assert_eq!((at_100.left, at_100.right, at_100.bottom), (600, 1320, 260));
        let at_150 = source(960, 0, 1.5, MONITOR);
        assert_eq!((at_150.right - at_150.left, at_150.bottom), (1080, 390));
    }

    #[test]
    fn the_source_stays_on_its_monitor() {
        let second = Rect {
            left: 1920,
            top: 0,
            right: 2420,
            bottom: 200,
        };
        let s = source(2170, 0, 1.0, second);
        assert_eq!(s, second);
    }
}
