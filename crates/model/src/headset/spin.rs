//! A headset's turntable: [`FRAMES`] renders a full turn apart, starting from the hero view,
//! packed [`COLUMNS`] to a row of [`FRAME`] px squares (`spin.png`, made by `cargo devices
//! render`). It plays once as the headset connects, easing out onto the hero view.

use std::time::Duration;

pub const FRAMES: usize = 24;
pub const FRAME: u32 = 128;
pub const COLUMNS: usize = 6;
/// How long the turn takes.
pub const SPIN: Duration = Duration::from_millis(1400);

/// The frame to show `elapsed` into the spin, and whether it is still turning.
pub fn frame_at(elapsed: Duration) -> (usize, bool) {
    if elapsed >= SPIN {
        return (0, false);
    }
    let t = elapsed.as_secs_f32() / SPIN.as_secs_f32();
    let eased = 1.0 - (1.0 - t).powi(3);
    ((eased * FRAMES as f32).round() as usize % FRAMES, true)
}

/// Where frame `index` starts in the packed grid, in pixels.
pub fn cell(index: usize) -> (u32, u32) {
    let (column, row) = (index % COLUMNS, index / COLUMNS);
    (column as u32 * FRAME, row as u32 * FRAME)
}

/// The packed grid's size.
pub fn grid() -> (u32, u32) {
    (
        COLUMNS as u32 * FRAME,
        FRAMES.div_ceil(COLUMNS) as u32 * FRAME,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_turn_easing_out_onto_the_hero_view() {
        assert_eq!(frame_at(Duration::ZERO), (0, true));
        assert_eq!(frame_at(SPIN), (0, false));
        assert_eq!(frame_at(SPIN * 3), (0, false));
        let frames: Vec<usize> = (0..140)
            .map(|ms| frame_at(Duration::from_millis(ms * 10)).0)
            .collect();
        let mut seen: Vec<usize> = frames.clone();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), FRAMES, "every view shows once on the way round");
        let forward = frames.windows(2).all(|w| w[1] >= w[0] || w[1] == 0);
        assert!(forward, "it only turns one way");
        let early = frame_at(SPIN / 4).0;
        assert!(early > FRAMES / 3, "it starts fast and slows down: {early}");
    }

    #[test]
    fn frames_pack_six_to_a_row() {
        assert_eq!(cell(0), (0, 0));
        assert_eq!(cell(7), (FRAME, FRAME));
        assert_eq!(grid(), (6 * FRAME, 4 * FRAME));
    }
}
