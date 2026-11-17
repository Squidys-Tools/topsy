//! What a Transform result card says once its file is done, after Droppy's: the size before and
//! after on the first line, then the picture's sides before and after, or the result's name when
//! a side isn't known (a PDF, a zip, a sound).

use super::transform::size;

/// Width and height in pixels, as shown.
pub type Sides = (u32, u32);

/// What one card's work made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Made {
    /// Bytes going in and coming out; `None` when nothing was written, as for a rename.
    pub bytes: Option<(u64, u64)>,
    pub sides: (Option<Sides>, Option<Sides>),
    /// The result's file name, or the renamed file's new one.
    pub name: String,
    /// Files renamed, for a rename's card.
    pub renamed: usize,
}

/// The card's big line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Headline {
    /// `3 MB → 811 KB`, drawn in two colours.
    Sizes(String, String),
    Text(String),
}

impl Headline {
    /// Its font size: `max`, or smaller so that it fits `width` with `gap` between the sizes and
    /// the arrow. It is set in a monospace font; Consolas advances 0.55 em, and 0.6 leaves room.
    pub fn font_size(&self, width: f32, gap: f32, max: f32) -> f32 {
        let (chars, gaps) = match self {
            Headline::Sizes(before, after) => (
                before.chars().count() + 1 + after.chars().count(),
                2.0 * gap,
            ),
            Headline::Text(text) => (text.chars().count(), 0.0),
        };
        ((width - gaps) / (chars.max(1) as f32 * 0.6)).min(max)
    }
}

impl Made {
    pub fn headline(&self) -> Headline {
        match self.bytes {
            Some((before, after)) => Headline::Sizes(size(before), size(after)),
            None if self.renamed > 1 => Headline::Text(format!("{} renamed", self.renamed)),
            None => Headline::Text("Renamed".into()),
        }
    }

    /// The small line: `4032×3024 → 1920×1440`, one size when they match, else the name.
    pub fn detail(&self) -> String {
        let sides = |(w, h): Sides| format!("{w}×{h}");
        match self.sides {
            (Some(a), Some(b)) if a == b => sides(a),
            (Some(a), Some(b)) => format!("{} → {}", sides(a), sides(b)),
            _ => self.name.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn made(bytes: Option<(u64, u64)>, sides: (Option<Sides>, Option<Sides>)) -> Made {
        Made {
            bytes,
            sides,
            name: "IMG_2041 (compressed).jpg".into(),
            renamed: 0,
        }
    }

    #[test]
    fn a_long_headline_shrinks_to_fit_and_a_short_one_keeps_its_size() {
        let short = Headline::Sizes("22 MB".into(), "12 MB".into());
        assert_eq!(short.font_size(180.0, 7.0, 18.0), 18.0);
        // What ran off the card: 463 bytes → 310 bytes at 18 px.
        let long = Headline::Sizes("999 bytes".into(), "999 bytes".into());
        let px = long.font_size(180.0, 7.0, 18.0);
        assert!(px < 18.0);
        assert!(19.0 * 0.55 * px + 14.0 < 180.0 * 0.95, "leaves headroom");
    }

    #[test]
    fn a_card_shows_sizes_then_sides_or_the_name() {
        let shrunk = made(
            Some((3_000_000, 811_000)),
            (Some((4032, 3024)), Some((1920, 1440))),
        );
        assert_eq!(
            shrunk.headline(),
            Headline::Sizes("3 MB".into(), "811 KB".into())
        );
        assert_eq!(shrunk.detail(), "4032×3024 → 1920×1440");

        let same = made(Some((10, 5)), (Some((800, 600)), Some((800, 600))));
        assert_eq!(same.detail(), "800×600");

        let pdf = made(Some((10, 5)), (Some((800, 600)), None));
        assert_eq!(pdf.detail(), "IMG_2041 (compressed).jpg");

        let mut renamed = made(None, (None, None));
        assert_eq!(renamed.headline(), Headline::Text("Renamed".into()));
        renamed.renamed = 3;
        assert_eq!(renamed.headline(), Headline::Text("3 renamed".into()));
    }
}
