//! Where the lens glass reads the screen from.

use super::float::Rect;

/// How far the open notch can reach, in logical pixels: its Home page (the player over a row of
/// every widget) with its ears and the spring's overshoot, plus the margin the bevel bends in from
/// outside the edge.
const REACH: (f32, f32) = (1000.0, 400.0);

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

/// The part of the screen a floating window's glass reads: `reach` (physical pixels) centred on
/// `center`, moved whole onto `monitor` where it fits there. Its size never changes, so a move
/// only points the live copy somewhere else.
pub fn around(center: (i32, i32), reach: (i32, i32), monitor: Rect) -> Rect {
    let start = |center: i32, length: i32, low: i32, high: i32| {
        let start = center - length / 2;
        if high - low <= length {
            low
        } else {
            start.clamp(low, high - length)
        }
    };
    let left = start(center.0, reach.0, monitor.left, monitor.right);
    let top = start(center.1, reach.1, monitor.top, monitor.bottom);
    Rect {
        left,
        top,
        right: left + reach.0,
        bottom: top + reach.1,
    }
}

/// Whether `source` holds a window at `origin` of `size`, with `margin` to spare all round for
/// what the bevel bends in from beyond its edge.
pub fn holds(source: Rect, origin: (i32, i32), size: (i32, i32), margin: i32) -> bool {
    origin.0 - margin >= source.left
        && origin.1 - margin >= source.top
        && origin.0 + size.0 + margin <= source.right
        && origin.1 + size.1 + margin <= source.bottom
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
        assert_eq!((at_100.left, at_100.right, at_100.bottom), (460, 1460, 400));
        let at_150 = source(960, 0, 1.5, MONITOR);
        assert_eq!((at_150.right - at_150.left, at_150.bottom), (1500, 600));
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

    #[test]
    fn a_floating_source_is_centred_and_moved_whole_onto_its_monitor() {
        let middle = around((960, 540), (600, 400), MONITOR);
        assert_eq!(
            middle,
            Rect {
                left: 660,
                top: 340,
                right: 1260,
                bottom: 740
            }
        );
        let corner = around((1900, 20), (600, 400), MONITOR);
        assert_eq!((corner.left, corner.top), (1320, 0));
        assert_eq!((corner.right, corner.bottom), (1920, 400));
        // Bigger than its monitor: the same size, from its corner.
        let small = Rect {
            left: 0,
            top: 0,
            right: 500,
            bottom: 300,
        };
        let over = around((250, 150), (600, 400), small);
        assert_eq!(
            (over.left, over.top, over.right, over.bottom),
            (0, 0, 600, 400)
        );
    }

    #[test]
    fn a_floating_source_holds_its_window_with_a_margin() {
        let source = around((960, 540), (600, 400), MONITOR);
        assert!(holds(source, (800, 440), (320, 200), 48));
        // Moved so the bevel's margin would read past the source.
        assert!(!holds(source, (660, 440), (320, 200), 48));
        assert!(!holds(source, (800, 600), (320, 200), 48));
    }
}
