//! Where the basket opens: centred under the pointer, moved only as far as it takes to stay
//! whole on the pointer's monitor. Physical screen pixels; no OS calls.

/// Room kept between the basket and the edge of the work area, in logical pixels.
pub const MARGIN: f32 = 8.0;

/// A screen rectangle: left and top inclusive, right and bottom exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// The centre of a panel of `size` opened at `cursor`, kept `margin` inside `work` (the monitor
/// less its taskbar). A panel whose window sits at `centre - size / 2` then lies wholly inside.
/// A work area too small for the panel centres it there.
pub fn center_under(cursor: (i32, i32), size: (i32, i32), work: Rect, margin: i32) -> (i32, i32) {
    let axis = |at: i32, len: i32, start: i32, end: i32| {
        let low = start + margin + len / 2;
        let high = end - margin - len + len / 2;
        if low > high {
            (start + end) / 2
        } else {
            at.clamp(low, high)
        }
    };
    (
        axis(cursor.0, size.0, work.left, work.right),
        axis(cursor.1, size.1, work.top, work.bottom),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1032,
    };

    fn origin(center: (i32, i32), size: (i32, i32)) -> (i32, i32) {
        (center.0 - size.0 / 2, center.1 - size.1 / 2)
    }

    #[test]
    fn opens_centred_on_the_pointer_when_there_is_room() {
        assert_eq!(center_under((900, 500), (400, 118), WORK, 8), (900, 500));
    }

    #[test]
    fn stays_whole_near_every_edge() {
        let size = (401, 119);
        for cursor in [
            (0, 0),
            (1919, 0),
            (0, 1031),
            (1919, 1031),
            (5, 600),
            (960, 1030),
        ] {
            let (x, y) = origin(center_under(cursor, size, WORK, 8), size);
            assert!(x >= 8 && x + size.0 <= 1920 - 8, "{cursor:?} -> x {x}");
            assert!(y >= 8 && y + size.1 <= 1032 - 8, "{cursor:?} -> y {y}");
        }
        // Only as far as needed: flush against the margin.
        let (x, y) = origin(center_under((1919, 1031), size, WORK, 8), size);
        assert_eq!((x + size.0, y + size.1), (1912, 1024));
    }

    #[test]
    fn keeps_to_a_monitor_left_of_or_above_the_primary() {
        let left = Rect {
            left: -2560,
            top: -400,
            right: 0,
            bottom: 1000,
        };
        let size = (800, 236);
        let (x, y) = origin(center_under((-1, -400), size, left, 16), size);
        assert_eq!((x + size.0, y), (-16, -384));
    }

    #[test]
    fn a_work_area_too_small_centres_it() {
        let tiny = Rect {
            left: 100,
            top: 0,
            right: 400,
            bottom: 100,
        };
        assert_eq!(center_under((0, 0), (400, 118), tiny, 8), (250, 50));
    }
}
