//! The pictures inside a PDF that Compress can re-encode beyond plain grey and RGB.

use super::jpeg::markers::{self, APP14};

/// An indexed picture's samples through its palette: `base` bytes (1 grey, 3 RGB) per entry. An
/// index past the palette's end reads as its last entry, as viewers do. `None` for a palette
/// with no whole entry.
pub fn through_palette(indices: &[u8], palette: &[u8], base: usize) -> Option<Vec<u8>> {
    let entries = palette.len().checked_div(base)?;
    let last = entries.checked_sub(1)?;
    let mut out = Vec::with_capacity(indices.len() * base);
    for &i in indices {
        let at = usize::from(i).min(last) * base;
        out.extend_from_slice(&palette[at..at + base]);
    }
    Some(out)
}

/// Whether a CMYK JPEG is stored the way Photoshop, InDesign and ImageMagick write it: with
/// Adobe's marker, its samples inverted, and the PDF's `/Decode [1 0 1 0 1 0 1 0]` undoing that.
/// ImageMagick reads and writes exactly that pairing, so only these are re-encoded.
pub fn adobe_cmyk(decode: Option<&[f64]>, jpeg: &[u8]) -> bool {
    decode == Some(&[1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0])
        && markers::segments(jpeg).is_ok_and(|s| {
            s.iter()
                .any(|s| s.marker == APP14 && s.data.starts_with(b"Adobe"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jpeg::markers::{SOI, write_segment};

    #[test]
    fn a_palette_turns_indices_into_colours_and_clamps_the_stray() {
        let palette = [255, 0, 0, 0, 255, 0, 0, 0, 255];
        assert_eq!(
            through_palette(&[2, 0, 1, 9], &palette, 3).unwrap(),
            [0, 0, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255]
        );
        assert_eq!(through_palette(&[1, 0], &[10, 200], 1).unwrap(), [200, 10]);
        assert!(
            through_palette(&[0], &[1, 2], 3).is_none(),
            "no whole entry"
        );
        assert!(through_palette(&[0], &[1], 0).is_none());
    }

    #[test]
    fn only_the_adobe_pairing_of_marker_and_decode_counts() {
        let invert = [1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0];
        let mut adobe = vec![0xFF, SOI];
        write_segment(&mut adobe, APP14, b"Adobe\0\x64\0\0\0\0\x02");
        adobe.extend_from_slice(&[0xFF, markers::EOI]);
        let plain = [0xFF, SOI, 0xFF, markers::EOI];
        assert!(adobe_cmyk(Some(&invert), &adobe));
        assert!(!adobe_cmyk(None, &adobe), "inverted samples nobody undoes");
        assert!(!adobe_cmyk(Some(&invert), &plain), "an undo of nothing");
        assert!(!adobe_cmyk(Some(&invert), b"not a jpeg"));
    }
}
