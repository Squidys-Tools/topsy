//! Device-independent bitmaps, the clipboard's image format (`CF_DIB`, `CF_DIBV5`): a header,
//! maybe colour masks, then rows of pixels. Read into straight RGBA, 4 bytes a pixel, top row
//! first. 24- and 32-bit only, which is what apps put on the clipboard.

/// Past this many pixels a copied image isn't kept.
pub const MAX_PIXELS: u64 = 40_000_000;

const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;
const BI_ALPHABITFIELDS: u32 = 6;

pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub fn decode(dib: &[u8]) -> Option<Rgba> {
    let u32_at = |at: usize| Some(u32::from_le_bytes(dib.get(at..at + 4)?.try_into().ok()?));
    let u16_at = |at: usize| Some(u16::from_le_bytes(dib.get(at..at + 2)?.try_into().ok()?));
    let header = u32_at(0)? as usize;
    if header < 40 {
        return None;
    }
    let width = u32_at(4)? as i32;
    let height = u32_at(8)? as i32;
    let bits = u16_at(14)?;
    let compression = u32_at(16)?;
    let colors_used = u32_at(32)? as usize;
    if width <= 0 || height == 0 || !matches!(bits, 24 | 32) {
        return None;
    }
    let (w, h) = (width as u32, height.unsigned_abs());
    if w as u64 * h as u64 > MAX_PIXELS {
        return None;
    }
    // Masks follow a plain 40-byte header; later headers carry them inside.
    let (masks, after_masks) = match compression {
        BI_RGB => (None, header),
        BI_BITFIELDS | BI_ALPHABITFIELDS if bits == 32 => {
            let at = 40;
            let alpha = if header > 52 || compression == BI_ALPHABITFIELDS {
                u32_at(at + 12)?
            } else {
                0
            };
            let extra = match (header, compression) {
                (40, BI_BITFIELDS) => 12,
                (40, _) => 16,
                _ => 0,
            };
            (
                Some([u32_at(at)?, u32_at(at + 4)?, u32_at(at + 8)?, alpha]),
                header + extra,
            )
        }
        _ => return None,
    };
    let start = after_masks + colors_used * 4;
    let stride = (w as usize * bits as usize).div_ceil(32) * 4;
    let data = dib.get(start..start + stride * h as usize)?;
    let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
    for row in 0..h as usize {
        // Positive height is stored bottom row first.
        let src = if height > 0 {
            h as usize - 1 - row
        } else {
            row
        };
        let line = &data[src * stride..];
        for x in 0..w as usize {
            let px = match bits {
                24 => {
                    let p = &line[x * 3..x * 3 + 3];
                    [p[2], p[1], p[0], 255]
                }
                _ => {
                    let v = u32::from_le_bytes(line[x * 4..x * 4 + 4].try_into().ok()?);
                    match masks {
                        Some([r, g, b, a]) => [
                            channel(v, r),
                            channel(v, g),
                            channel(v, b),
                            if a == 0 { 255 } else { channel(v, a) },
                        ],
                        None => [(v >> 16) as u8, (v >> 8) as u8, v as u8, (v >> 24) as u8],
                    }
                }
            };
            pixels.extend_from_slice(&px);
        }
    }
    // Most 32-bit bitmaps leave alpha at 0: they are opaque.
    if bits == 32 && pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
        pixels
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .for_each(|p| p[3] = 255);
    }
    Some(Rgba {
        width: w,
        height: h,
        pixels,
    })
}

/// `CF_DIB` bytes for an image: a plain header and 32-bit rows, bottom row first, which every
/// app that takes a pasted bitmap can read.
pub fn encode(image: &Rgba) -> Vec<u8> {
    let mut dib = Vec::with_capacity(40 + image.pixels.len());
    dib.extend(40u32.to_le_bytes());
    dib.extend((image.width as i32).to_le_bytes());
    dib.extend((image.height as i32).to_le_bytes());
    dib.extend(1u16.to_le_bytes());
    dib.extend(32u16.to_le_bytes());
    dib.extend(BI_RGB.to_le_bytes());
    dib.extend((image.pixels.len() as u32).to_le_bytes());
    dib.resize(40, 0);
    let row = image.width as usize * 4;
    if row > 0 {
        for line in image.pixels.chunks_exact(row).rev() {
            for p in line.as_chunks::<4>().0 {
                dib.extend([p[2], p[1], p[0], p[3]]);
            }
        }
    }
    dib
}

fn channel(value: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let max = (mask >> shift) as u64;
    (((value & mask) >> shift) as u64 * 255 / max) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(size: u32, w: i32, h: i32, bits: u16, compression: u32) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend(size.to_le_bytes());
        v.extend(w.to_le_bytes());
        v.extend(h.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend(bits.to_le_bytes());
        v.extend(compression.to_le_bytes());
        v.resize(size as usize, 0);
        v
    }

    #[test]
    fn a_bottom_up_24_bit_bitmap_reads_top_row_first() {
        // 2x2: bottom row blue, red; top row green, white. Rows pad to 8 bytes.
        let mut dib = header(40, 2, 2, 24, BI_RGB);
        dib.extend([255, 0, 0, 0, 0, 255, 0, 0]);
        dib.extend([0, 255, 0, 255, 255, 255, 0, 0]);
        let img = decode(&dib).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(
            img.pixels,
            [
                0, 255, 0, 255, 255, 255, 255, 255, 0, 0, 255, 255, 255, 0, 0, 255
            ]
        );
    }

    #[test]
    fn a_32_bit_bitmap_with_no_alpha_is_opaque_and_top_down_stays_put() {
        let mut dib = header(40, 1, -2, 32, BI_RGB);
        dib.extend([1, 2, 3, 0, 4, 5, 6, 0]);
        let img = decode(&dib).unwrap();
        assert_eq!(img.pixels, [3, 2, 1, 255, 6, 5, 4, 255]);
    }

    #[test]
    fn a_v5_bitmap_keeps_its_alpha_through_its_masks() {
        let mut dib = header(124, 1, 1, 32, BI_BITFIELDS);
        for (i, mask) in [0x00ff_0000u32, 0x0000_ff00, 0x0000_00ff, 0xff00_0000]
            .iter()
            .enumerate()
        {
            dib[40 + i * 4..44 + i * 4].copy_from_slice(&mask.to_le_bytes());
        }
        dib.extend([30, 20, 10, 128]);
        let img = decode(&dib).unwrap();
        assert_eq!(img.pixels, [10, 20, 30, 128]);
        assert_eq!(channel(u32::MAX, u32::MAX), 255);
    }

    #[test]
    fn a_plain_header_with_bitfields_reads_the_masks_after_it() {
        let mut dib = header(40, 1, 1, 32, BI_BITFIELDS);
        dib.extend(0x0000_00ffu32.to_le_bytes());
        dib.extend(0x0000_ff00u32.to_le_bytes());
        dib.extend(0x00ff_0000u32.to_le_bytes());
        dib.extend([7, 8, 9, 0]);
        assert_eq!(decode(&dib).unwrap().pixels, [7, 8, 9, 255]);
    }

    #[test]
    fn truncated_paletted_and_huge_bitmaps_are_refused() {
        let mut short = header(40, 4, 4, 24, BI_RGB);
        short.extend([0; 10]);
        assert!(decode(&short).is_none());
        assert!(decode(&header(40, 1, 1, 8, BI_RGB)).is_none());
        assert!(decode(&header(40, 100_000, 100_000, 32, BI_RGB)).is_none());
        assert!(decode(&[1, 2, 3]).is_none());
    }

    #[test]
    fn an_encoded_image_reads_back_the_same() {
        let image = Rgba {
            width: 2,
            height: 2,
            pixels: vec![1, 2, 3, 255, 4, 5, 6, 128, 7, 8, 9, 255, 10, 11, 12, 0],
        };
        let back = decode(&encode(&image)).unwrap();
        assert_eq!((back.width, back.height), (2, 2));
        assert_eq!(back.pixels, image.pixels);
    }
}
