//! A PNG writer that makes the smallest lossless file it can, as oxipng would (which needs a C
//! deflate, so isn't an option): the narrowest colour type and bit depth the pixels allow,
//! then the best of a few filter strategies through zlib at its strongest.

use std::io::Write;

use super::png::write_chunk;
use super::quantize::Indexed;

/// Straight RGBA8 pixels.
pub struct Rgba<'a> {
    pub width: usize,
    pub height: usize,
    pub data: &'a [u8],
}

struct Layout {
    color_type: u8,
    depth: u8,
    palette: Vec<[u8; 4]>,
    /// Packed samples per row, without filter bytes.
    rows: Vec<Vec<u8>>,
    /// Bytes per complete pixel, for the filters (at least 1).
    bpp: usize,
}

/// The smallest PNG for these pixels.
pub fn encode(px: &Rgba) -> Vec<u8> {
    write(&reduce(px), px.width, px.height)
}

/// A palette PNG from quantised pixels.
pub fn encode_indexed(q: &Indexed, width: usize, height: usize) -> Vec<u8> {
    write(
        &palette_layout(q.palette.clone(), &q.indices, width, height),
        width,
        height,
    )
}

/// 8-bit samples (`channels` per pixel) filtered row by row as PNG does and deflated: a PDF
/// image stream with `/Predictor 15`.
pub fn deflate_predicted(samples: &[u8], width: usize, channels: usize) -> Vec<u8> {
    let rows = samples
        .chunks(width * channels)
        .map(<[u8]>::to_vec)
        .collect();
    deflate(&filtered(
        &Layout {
            color_type: 2,
            depth: 8,
            palette: Vec::new(),
            rows,
            bpp: channels,
        },
        None,
    ))
}

fn reduce(px: &Rgba) -> Layout {
    let pixels = || px.data.as_chunks::<4>().0.iter();
    let opaque = pixels().all(|p| p[3] == 255);
    let grey = pixels().all(|p| p[0] == p[1] && p[1] == p[2]);
    let mut palette: Vec<[u8; 4]> = Vec::new();
    let mut index = std::collections::HashMap::new();
    for p in pixels() {
        let c = [p[0], p[1], p[2], p[3]];
        if let std::collections::hash_map::Entry::Vacant(e) = index.entry(c) {
            if palette.len() == 256 {
                palette.clear();
                break;
            }
            e.insert(palette.len() as u8);
            palette.push(c);
        }
    }
    let paletted = !palette.is_empty();
    // A palette pays when it beats the direct form's bytes per pixel.
    let direct_bytes = match (grey, opaque) {
        (true, true) => 1,
        (true, false) => 2,
        (false, true) => 3,
        (false, false) => 4,
    };
    if grey && opaque {
        let depth = grey_depth(pixels().map(|p| p[0]));
        if !paletted || depth <= palette_depth(palette.len()) {
            let rows = (0..px.height)
                .map(|y| {
                    pack(
                        px.data[y * px.width * 4..(y + 1) * px.width * 4]
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .map(|p| p[0] >> (8 - depth)),
                        depth,
                    )
                })
                .collect();
            return Layout {
                color_type: 0,
                depth,
                palette: Vec::new(),
                rows,
                bpp: 1,
            };
        }
    }
    if paletted && (direct_bytes > 1 || palette.len() <= 16) {
        // Transparent entries first keeps tRNS short.
        palette.sort_by_key(|c| c[3] == 255);
        let lookup: std::collections::HashMap<[u8; 4], u8> = palette
            .iter()
            .enumerate()
            .map(|(i, c)| (*c, i as u8))
            .collect();
        let indices: Vec<u8> = pixels()
            .map(|p| lookup[&[p[0], p[1], p[2], p[3]]])
            .collect();
        return palette_layout(palette, &indices, px.width, px.height);
    }
    let (color_type, channels): (u8, &[usize]) = match (grey, opaque) {
        (true, true) => (0, &[0]),
        (true, false) => (4, &[0, 3]),
        (false, true) => (2, &[0, 1, 2]),
        (false, false) => (6, &[0, 1, 2, 3]),
    };
    let rows = (0..px.height)
        .map(|y| {
            px.data[y * px.width * 4..(y + 1) * px.width * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| channels.iter().map(move |&c| p[c]))
                .collect()
        })
        .collect();
    Layout {
        color_type,
        depth: 8,
        palette: Vec::new(),
        rows,
        bpp: channels.len(),
    }
}

fn palette_layout(palette: Vec<[u8; 4]>, indices: &[u8], width: usize, height: usize) -> Layout {
    let depth = palette_depth(palette.len());
    let rows = (0..height)
        .map(|y| pack(indices[y * width..(y + 1) * width].iter().copied(), depth))
        .collect();
    Layout {
        color_type: 3,
        depth,
        palette,
        rows,
        bpp: 1,
    }
}

fn palette_depth(n: usize) -> u8 {
    match n {
        0..=2 => 1,
        3..=4 => 2,
        5..=16 => 4,
        _ => 8,
    }
}

/// The lowest grey depth that holds every value exactly (1, 2 and 4 bits scale up by
/// replication).
fn grey_depth(values: impl Iterator<Item = u8> + Clone) -> u8 {
    for depth in [1u8, 2, 4] {
        let levels = (1u16 << depth) - 1;
        let step = 255 / levels;
        if values.clone().all(|v| u16::from(v) % step == 0) {
            return depth;
        }
    }
    8
}

fn pack(samples: impl Iterator<Item = u8>, depth: u8) -> Vec<u8> {
    if depth == 8 {
        return samples.collect();
    }
    let per = 8 / depth as usize;
    let mut out = Vec::new();
    let mut acc = 0u8;
    let mut n = 0;
    for s in samples {
        acc |= (s & ((1 << depth) - 1)) << (8 - depth * (n as u8 + 1));
        n += 1;
        if n == per {
            out.push(acc);
            acc = 0;
            n = 0;
        }
    }
    if n > 0 {
        out.push(acc);
    }
    out
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let (pa, pb, pc) = (
        (p - i16::from(a)).abs(),
        (p - i16::from(b)).abs(),
        (p - i16::from(c)).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn filter(kind: u8, row: &[u8], prev: &[u8], bpp: usize, out: &mut Vec<u8>) {
    out.push(kind);
    for i in 0..row.len() {
        let a = if i >= bpp { row[i - bpp] } else { 0 };
        let b = prev[i];
        let c = if i >= bpp { prev[i - bpp] } else { 0 };
        out.push(match kind {
            0 => row[i],
            1 => row[i].wrapping_sub(a),
            2 => row[i].wrapping_sub(b),
            3 => row[i].wrapping_sub(((u16::from(a) + u16::from(b)) / 2) as u8),
            _ => row[i].wrapping_sub(paeth(a, b, c)),
        });
    }
}

/// The filtered image data, each row's filter chosen by `pick` (a fixed type, or the one with
/// the smallest sum of absolute differences when `None`).
fn filtered(layout: &Layout, pick: Option<u8>) -> Vec<u8> {
    let width = layout.rows.first().map_or(0, Vec::len);
    let mut prev = vec![0u8; width];
    let mut out = Vec::with_capacity((width + 1) * layout.rows.len());
    let mut trial = Vec::with_capacity(width + 1);
    for row in &layout.rows {
        let kind = pick.unwrap_or_else(|| {
            (0..5)
                .min_by_key(|&k| {
                    trial.clear();
                    filter(k, row, &prev, layout.bpp, &mut trial);
                    trial[1..]
                        .iter()
                        .map(|&v| u32::from((v as i8).unsigned_abs()))
                        .sum::<u32>()
                })
                .unwrap_or(0)
        });
        filter(kind, row, &prev, layout.bpp, &mut out);
        prev.clone_from(row);
    }
    out
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    let _ = z.write_all(data);
    z.finish().unwrap_or_default()
}

fn write(layout: &Layout, width: usize, height: usize) -> Vec<u8> {
    // Palettes and low depths compress best unfiltered; everything else is worth a search.
    // Past a few megapixels the search costs seconds for a percent or two.
    let large = layout.rows.len() * layout.rows.first().map_or(0, Vec::len) > 8_000_000;
    let strategies: &[Option<u8>] = if large {
        &[None]
    } else if layout.color_type == 3 || layout.depth < 8 {
        &[Some(0), None]
    } else {
        &[None, Some(4), Some(0)]
    };
    let idat = strategies
        .iter()
        .map(|&s| deflate(&filtered(layout, s)))
        .min_by_key(Vec::len)
        .unwrap_or_default();
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(height as u32).to_be_bytes());
    ihdr.extend_from_slice(&[layout.depth, layout.color_type, 0, 0, 0]);
    write_chunk(&mut out, b"IHDR", &ihdr);
    if layout.color_type == 3 {
        let plte: Vec<u8> = layout
            .palette
            .iter()
            .flat_map(|c| [c[0], c[1], c[2]])
            .collect();
        write_chunk(&mut out, b"PLTE", &plte);
        let alphas: Vec<u8> = layout.palette.iter().map(|c| c[3]).collect();
        let used = alphas.iter().rposition(|&a| a != 255).map_or(0, |i| i + 1);
        if used > 0 {
            write_chunk(&mut out, b"tRNS", &alphas[..used]);
        }
    }
    write_chunk(&mut out, b"IDAT", &idat);
    write_chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(data: &[u8], w: usize, h: usize) -> (Vec<u8>, image::RgbaImage) {
        let png = encode(&Rgba {
            width: w,
            height: h,
            data,
        });
        let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
            .unwrap()
            .to_rgba8();
        assert_eq!(img.as_raw(), data, "{w}x{h} must round-trip exactly");
        (png, img)
    }

    fn color_type(png: &[u8]) -> (u8, u8) {
        (png[24], png[25])
    }

    #[test]
    fn every_reduction_is_lossless() {
        let (w, h) = (37, 23);
        let photo: Vec<u8> = (0..w * h)
            .flat_map(|i| [(i % 251) as u8, (i / 251 * 60) as u8, (i * 3) as u8, 255])
            .collect();
        let (png, _) = round_trip(&photo, w, h);
        assert_eq!(color_type(&png), (8, 2), "opaque colour drops alpha");
        let alpha: Vec<u8> = (0..w * h)
            .flat_map(|i| [(i % 251) as u8, (i / 251 * 60) as u8, 9, (i * 5) as u8])
            .collect();
        assert_eq!(color_type(&round_trip(&alpha, w, h).0), (8, 6));
        let grey: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                let v = (i * 11) as u8;
                [v, v, v, 255]
            })
            .collect();
        assert_eq!(color_type(&round_trip(&grey, w, h).0), (8, 0));
        let bw: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                let v = if i % 3 == 0 { 255 } else { 0 };
                [v, v, v, 255]
            })
            .collect();
        assert_eq!(color_type(&round_trip(&bw, w, h).0), (1, 0));
        let few: Vec<u8> = (0..w * h)
            .flat_map(|i| [[255, 0, 0, 255], [0, 0, 255, 0], [9, 9, 200, 128]][i % 3])
            .collect();
        assert_eq!(
            color_type(&round_trip(&few, w, h).0),
            (2, 3),
            "three colours, 2-bit palette"
        );
        let one = [1u8, 2, 3, 4];
        round_trip(&one, 1, 1);
    }

    #[test]
    fn smaller_than_a_plain_encoder() {
        let (w, h) = (128, 96);
        let data: Vec<u8> = (0..w * h)
            .flat_map(|i| [((i % w) * 2) as u8, ((i / w) * 2) as u8, 64, 255])
            .collect();
        let mine = encode(&Rgba {
            width: w,
            height: h,
            data: &data,
        });
        let mut plain = Vec::new();
        image::RgbaImage::from_raw(w as u32, h as u32, data)
            .unwrap()
            .write_to(
                &mut std::io::Cursor::new(&mut plain),
                image::ImageFormat::Png,
            )
            .unwrap();
        assert!(
            mine.len() < plain.len(),
            "{} vs {}",
            mine.len(),
            plain.len()
        );
    }

    #[test]
    fn indexed_output_decodes_to_its_palette() {
        let q = Indexed {
            palette: vec![[0, 0, 0, 0], [255, 0, 0, 255], [0, 255, 0, 200]],
            indices: vec![0, 1, 2, 1, 0, 2],
        };
        let png = encode_indexed(&q, 3, 2);
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        let want: Vec<u8> = q
            .indices
            .iter()
            .flat_map(|&i| q.palette[i as usize])
            .collect();
        assert_eq!(img.as_raw(), &want);
    }
}
