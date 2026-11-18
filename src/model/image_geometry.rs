//! The shapes the image Transforms give a picture: Compress's largest side, Resize's sizes and
//! crops, and Rotate's turns.

use crate::model::orientation::Orientation;

/// Compress's "Largest side".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LargestSide {
    Original,
    Px(u32),
}

impl LargestSide {
    pub const PRESETS: [LargestSide; 8] = [
        LargestSide::Original,
        LargestSide::Px(3840),
        LargestSide::Px(2560),
        LargestSide::Px(1920),
        LargestSide::Px(1600),
        LargestSide::Px(1280),
        LargestSide::Px(1024),
        LargestSide::Px(640),
    ];

    pub fn fit(self, w: u32, h: u32) -> (u32, u32) {
        match self {
            LargestSide::Original => (w, h),
            LargestSide::Px(px) => fit_within(w, h, px),
        }
    }
}

/// x, y, width, height.
pub type Crop = (u32, u32, u32, u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resize {
    LargestSide(u32),
    Percent(u32),
    /// Exactly this size: `fill` crops to cover it, otherwise it fits inside.
    Exact {
        width: u32,
        height: u32,
        fill: bool,
    },
}

impl Resize {
    /// The size to scale to, then the centred crop to take from it, if any.
    pub fn plan(self, w: u32, h: u32) -> ((u32, u32), Option<Crop>) {
        match self {
            Resize::LargestSide(px) => {
                let s = f64::from(px) / f64::from(w.max(h));
                (scaled(w, h, s), None)
            }
            Resize::Percent(p) => (scaled(w, h, f64::from(p) / 100.0), None),
            Resize::Exact {
                width,
                height,
                fill,
            } => {
                let (sx, sy) = (
                    f64::from(width) / f64::from(w),
                    f64::from(height) / f64::from(h),
                );
                if !fill {
                    return (scaled(w, h, sx.min(sy)), None);
                }
                let (cw, ch) = scaled(w, h, sx.max(sy));
                let (cw, ch) = (cw.max(width), ch.max(height));
                (
                    (cw, ch),
                    Some(((cw - width) / 2, (ch - height) / 2, width, height)),
                )
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    Left,
    Right,
    Half,
    FlipHorizontal,
    FlipVertical,
}

impl Rotation {
    pub const ALL: [Rotation; 5] = [
        Rotation::Left,
        Rotation::Right,
        Rotation::Half,
        Rotation::FlipHorizontal,
        Rotation::FlipVertical,
    ];

    pub fn orientation(self) -> Orientation {
        match self {
            Rotation::Left => Orientation::Rotate270,
            Rotation::Right => Orientation::Rotate90,
            Rotation::Half => Orientation::Rotate180,
            Rotation::FlipHorizontal => Orientation::FlipHorizontal,
            Rotation::FlipVertical => Orientation::FlipVertical,
        }
    }
}

fn scaled(w: u32, h: u32, s: f64) -> (u32, u32) {
    let d = |v: u32| ((f64::from(v) * s).round() as u32).max(1);
    (d(w), d(h))
}

/// `w` x `h` shrunk to fit `max` on its longest side, never enlarged.
pub fn fit_within(w: u32, h: u32, max: u32) -> (u32, u32) {
    if w.max(h) <= max {
        (w, h)
    } else {
        scaled(w, h, f64::from(max) / f64::from(w.max(h)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_fit_never_enlarge_and_crop_to_fill() {
        assert_eq!(fit_within(4032, 3024, 1920), (1920, 1440));
        assert_eq!(fit_within(3024, 4032, 1920), (1440, 1920));
        assert_eq!(fit_within(800, 600, 1920), (800, 600));
        assert_eq!(fit_within(10000, 1, 100), (100, 1));
        assert_eq!(Resize::Percent(50).plan(101, 51).0, (51, 26));
        assert_eq!(Resize::LargestSide(3000).plan(1000, 500).0, (3000, 1500));
        assert_eq!(
            Resize::Exact {
                width: 500,
                height: 500,
                fill: false
            }
            .plan(1000, 500),
            ((500, 250), None)
        );
        let (size, crop) = Resize::Exact {
            width: 500,
            height: 500,
            fill: true,
        }
        .plan(1000, 500);
        assert_eq!(size, (1000, 500));
        assert_eq!(crop, Some((250, 0, 500, 500)));
    }

    #[test]
    fn rotations_name_their_turns() {
        assert_eq!(Rotation::Left.orientation(), Orientation::Rotate270);
        assert_eq!(Rotation::Right.orientation(), Orientation::Rotate90);
    }
}
