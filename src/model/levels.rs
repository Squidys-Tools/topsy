//! Now Playing's level bars: the output's peak meter, read a frame at a time, turned into a
//! few bar heights that move like Droppy's. And the colour they take from the track's art.

/// How many bars are drawn.
pub const BARS: usize = 5;
/// How often the meter is read while the bars are on screen. Each frame is a whole redraw of
/// the island, so about 12 a second keeps the bars to roughly 2.5–3% of a core (measured).
pub const FRAME_MS: u64 = 80;

/// The loudest recent peak forgets itself this much each frame (about 3 s to halve), so a quiet
/// track still fills the bars and a loud one doesn't pin them.
const GAIN_DECAY: f32 = 0.98;
/// Below this the output counts as silence, not as a very quiet track to amplify.
const GAIN_FLOOR: f32 = 0.02;
/// A bar's height with nothing playing: a dot, as in Droppy.
pub const REST: f32 = 0.18;
/// The outer bars stay a little shorter than the middle one.
const SHAPE: [f32; BARS] = [0.7, 0.9, 1.0, 0.9, 0.7];
/// Which frame's level each bar shows: the newest in the middle, older outwards, so a beat
/// ripples out from the centre.
const AGE: [usize; BARS] = [2, 1, 0, 1, 2];
/// How far a bar moves towards its target each frame, up and down.
const RISE: f32 = 0.75;
const FALL: f32 = 0.35;

#[derive(Clone, Debug, PartialEq)]
pub struct Levels {
    gain: f32,
    /// Normalised levels, newest first.
    recent: [f32; 3],
    bars: [f32; BARS],
}

impl Default for Levels {
    fn default() -> Self {
        Self {
            gain: GAIN_FLOOR,
            recent: [0.0; 3],
            bars: [REST; BARS],
        }
    }
}

impl Levels {
    /// Take one frame's peak (0 to 1, as the meter reads it) and move the bars.
    pub fn push(&mut self, peak: f32) {
        let peak = if peak.is_finite() {
            peak.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.gain = (self.gain * GAIN_DECAY).max(peak).max(GAIN_FLOOR);
        // The square root lifts the quiet passages, as hearing does.
        let level = if peak < GAIN_FLOOR {
            0.0
        } else {
            (peak / self.gain).sqrt()
        };
        self.recent.rotate_right(1);
        self.recent[0] = level;
        for (i, bar) in self.bars.iter_mut().enumerate() {
            let target = REST + (1.0 - REST) * self.recent[AGE[i]] * SHAPE[i];
            let step = if target > *bar { RISE } else { FALL };
            *bar += (target - *bar) * step;
        }
    }

    /// Each bar's height, from `REST` to 1.
    pub fn bars(&self) -> [f32; BARS] {
        self.bars
    }

    /// Back to dots, for when playback stops or the bars are put away.
    pub fn rest(&mut self) {
        *self = Self {
            gain: self.gain,
            ..Self::default()
        };
    }
}

/// The colour the bars take from the art: its pixels averaged with the vivid ones counting
/// most, then lifted until it reads on black. `None` for art with no colour to speak of.
/// `bgra` is 4 bytes a pixel, blue first.
pub fn accent(bgra: &[u8]) -> Option<[u8; 3]> {
    let (mut sum, mut weight) = ([0.0f32; 3], 0.0f32);
    for px in bgra.as_chunks::<4>().0 {
        let [b, g, r, a] = px.map(|v| v as f32 / 255.0);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let saturation = if max > 0.0 { (max - min) / max } else { 0.0 };
        let w = saturation * saturation * max * a;
        sum = [sum[0] + r * w, sum[1] + g * w, sum[2] + b * w];
        weight += w;
    }
    let count = (bgra.len() / 4).max(1) as f32;
    if weight / count < 0.02 {
        return None;
    }
    let mut rgb = sum.map(|c| c / weight);
    // Lift dark colours by scaling up to a bright top channel, keeping the hue.
    let top = rgb[0].max(rgb[1]).max(rgb[2]);
    if top < 0.85 {
        rgb = rgb.map(|c| c * 0.85 / top);
    }
    Some(rgb.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(levels: &mut Levels, peak: f32, frames: usize) {
        for _ in 0..frames {
            levels.push(peak);
        }
    }

    #[test]
    fn silence_leaves_the_bars_at_rest() {
        let mut levels = Levels::default();
        run(&mut levels, 0.0, 30);
        assert!(levels.bars().iter().all(|&b| (b - REST).abs() < 1e-4));
        run(&mut levels, 0.005, 30);
        assert!(levels.bars().iter().all(|&b| (b - REST).abs() < 1e-4));
    }

    #[test]
    fn a_quiet_track_fills_the_bars_as_a_loud_one_does() {
        let (mut quiet, mut loud) = (Levels::default(), Levels::default());
        run(&mut quiet, 0.08, 60);
        run(&mut loud, 0.9, 60);
        for (q, l) in quiet.bars().iter().zip(loud.bars()) {
            assert!((q - l).abs() < 0.02, "{q} vs {l}");
        }
        assert!(quiet.bars()[2] > 0.95);
    }

    #[test]
    fn a_beat_starts_in_the_middle_and_spreads_out() {
        let mut levels = Levels::default();
        run(&mut levels, 0.05, 60);
        run(&mut levels, 0.0, 40);
        levels.push(0.05);
        let b = levels.bars();
        assert!(b[2] > b[1] && b[1] - REST < 1e-3, "{b:?}");
        levels.push(0.0);
        let b = levels.bars();
        assert!(b[1] > REST + 0.1 && b[0] - REST < 1e-3, "{b:?}");
        assert_eq!(b[0], b[4]);
        assert_eq!(b[1], b[3]);
    }

    #[test]
    fn bars_rise_faster_than_they_fall() {
        let mut levels = Levels::default();
        run(&mut levels, 0.5, 60);
        run(&mut levels, 0.0, 3);
        let fallen = 1.0 - levels.bars()[2];
        let mut levels = Levels::default();
        run(&mut levels, 0.5, 3);
        let risen = levels.bars()[2] - REST;
        assert!(risen > fallen, "rose {risen}, fell {fallen}");
    }

    #[test]
    fn rest_keeps_the_gain_so_resuming_does_not_blare() {
        let mut levels = Levels::default();
        run(&mut levels, 0.9, 30);
        levels.rest();
        assert_eq!(levels.bars(), [REST; BARS]);
        levels.push(0.1);
        assert!(levels.bars()[2] < 0.5);
    }

    #[test]
    fn junk_from_the_meter_is_silence() {
        let mut levels = Levels::default();
        run(&mut levels, f32::NAN, 5);
        run(&mut levels, -1.0, 5);
        assert_eq!(levels.bars(), [REST; BARS]);
    }

    fn image(pixels: &[[u8; 3]]) -> Vec<u8> {
        pixels
            .iter()
            .flat_map(|&[r, g, b]| [b, g, r, 255])
            .collect()
    }

    #[test]
    fn accent_follows_the_vivid_part_of_the_art() {
        let mut art = vec![[128, 128, 128]; 90];
        art.extend([[200, 30, 40]; 10]);
        let [r, g, b] = accent(&image(&art)).unwrap();
        assert!(r > 200 && g < 80 && b < 80, "{r} {g} {b}");
    }

    #[test]
    fn dark_art_is_lifted_and_grey_art_has_no_accent() {
        let [r, g, b] = accent(&image(&[[20, 30, 90]; 4])).unwrap();
        assert!(b >= 210 && r < b && g < b, "{r} {g} {b}");
        assert_eq!(accent(&image(&[[90, 90, 90]; 4])), None);
        assert_eq!(accent(&[]), None);
    }
}
