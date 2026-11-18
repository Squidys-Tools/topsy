//! A title too long for its line, read by scrolling it side to side: it rests at the start,
//! glides to the end, rests, and glides back.

use std::time::Duration;

/// Logical pixels a second.
const SPEED: f32 = 30.0;
const REST: Duration = Duration::from_millis(2000);
/// While it glides, how often it is redrawn.
pub const FRAME: Duration = Duration::from_millis(16);

/// How far to scroll a title that overflows its line by `overflow` pixels, `since` it was
/// first shown, and when to look again: a frame while it glides, the rest of a rest otherwise,
/// so a resting title books no redraws. `None` when it fits.
pub fn at(overflow: f32, since: Duration) -> Option<(f32, Duration)> {
    if overflow < 1.0 {
        return None;
    }
    let rest = REST.as_secs_f32();
    let glide = overflow / SPEED;
    let t = since.as_secs_f32() % (2.0 * (rest + glide));
    let wait = |until: f32| Duration::from_secs_f32(until - t).max(FRAME);
    Some(if t < rest {
        (0.0, wait(rest))
    } else if t < rest + glide {
        ((t - rest) * SPEED, FRAME)
    } else if t < 2.0 * rest + glide {
        (overflow, wait(2.0 * rest + glide))
    } else {
        (overflow - (t - 2.0 * rest - glide) * SPEED, FRAME)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: f32) -> Duration {
        Duration::from_secs_f32(s)
    }

    #[test]
    fn a_title_that_fits_stays_put() {
        assert_eq!(at(0.0, secs(5.0)), None);
        assert_eq!(at(0.5, secs(5.0)), None);
    }

    #[test]
    fn it_rests_glides_to_the_end_rests_and_glides_back() {
        // 60 px over: two seconds to glide each way, a cycle of eight.
        let offset = |s| at(60.0, secs(s)).unwrap().0;
        assert_eq!(offset(1.0), 0.0);
        assert!((offset(3.0) - 30.0).abs() < 0.01);
        assert_eq!(offset(5.0), 60.0);
        assert!((offset(7.0) - 30.0).abs() < 0.01);
        assert_eq!(offset(8.5), 0.0);
    }

    #[test]
    fn a_rest_books_one_wake_and_a_glide_every_frame() {
        assert!((at(60.0, secs(0.5)).unwrap().1.as_secs_f32() - 1.5).abs() < 0.01);
        assert_eq!(at(60.0, secs(3.0)).unwrap().1, FRAME);
        assert!((at(60.0, secs(4.5)).unwrap().1.as_secs_f32() - 1.5).abs() < 0.01);
    }
}
