//! A baseline JPEG encoder: JFIF YCbCr, 4:2:0 below quality 90 and 4:4:4 from there (as
//! Photoshop does), the IJG quality scale everyone's sliders mean, and tables optimised per
//! image. The `image` crate's encoder writes 4:4:4 with stock tables, about half again bigger.

use std::io;

use super::coeffs::{Component, Jpeg};
use super::markers::{APP0, APP1, APP2, exif_segment, icc_segments};
use super::write::write;

/// T.81 Annex K, natural order.
const LUMA: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56,
    14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113,
    92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];
const CHROMA: [u16; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99,
    47, 66, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];

/// The IJG scaling of a standard table for `quality` 1..=100.
fn scaled(base: &[u16; 64], quality: u8) -> [u16; 64] {
    let q = u32::from(quality.clamp(1, 100));
    let scale = if q < 50 { 5000 / q } else { 200 - 2 * q };
    std::array::from_fn(|i| ((u32::from(base[i]) * scale + 50) / 100).clamp(1, 255) as u16)
}

/// The IJG quality a luminance table was most likely made with, for re-encoding a JPEG at the
/// quality it already had.
pub fn estimate_quality(luma: &[u16; 64]) -> u8 {
    let distance = |q: u8| -> u32 {
        scaled(&LUMA, q)
            .iter()
            .zip(luma)
            .map(|(&a, &b)| u32::from(a.abs_diff(b)))
            .sum()
    };
    (1..=100).min_by_key(|&q| distance(q)).unwrap_or(75)
}

pub struct Pixels<'a> {
    pub width: usize,
    pub height: usize,
    /// 1 (grey) or 3 (RGB) bytes per pixel.
    pub channels: usize,
    pub data: &'a [u8],
}

pub struct Metadata<'a> {
    pub exif: Option<&'a [u8]>,
    pub icc: Option<&'a [u8]>,
}

pub fn encode(px: &Pixels, quality: u8, meta: &Metadata) -> io::Result<Vec<u8>> {
    if px.width == 0 || px.height == 0 || px.width > 65535 || px.height > 65535 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "JPEG can't be larger than 65535 pixels a side",
        ));
    }
    let subsample = quality < 90 && px.channels == 3;
    let mut jpeg = Jpeg {
        width: px.width,
        height: px.height,
        components: Vec::new(),
        qtables: [
            scaled(&LUMA, quality),
            scaled(&CHROMA, quality),
            [1; 64],
            [1; 64],
        ],
        extras: Vec::new(),
    };
    jpeg.extras
        .push((APP0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0".to_vec()));
    if let Some(seg) = meta.exif.and_then(exif_segment) {
        jpeg.extras.push((APP1, seg));
    }
    for seg in meta.icc.map(icc_segments).unwrap_or_default() {
        jpeg.extras.push((APP2, seg));
    }
    let f = if subsample { 2 } else { 1 };
    jpeg.components.push(Component {
        id: 1,
        h: f,
        v: f,
        tq: 0,
        bw: 0,
        bh: 0,
        blocks: Vec::new(),
    });
    if px.channels == 3 {
        for id in [2, 3] {
            jpeg.components.push(Component {
                id,
                h: 1,
                v: 1,
                tq: 1,
                bw: 0,
                bh: 0,
                blocks: Vec::new(),
            });
        }
    }
    jpeg.allocate();
    let planes = planes(px);
    let dct = Dct::new();
    for (ci, plane) in planes.iter().enumerate() {
        let sub = if ci > 0 && subsample { 2 } else { 1 };
        let comp = &mut jpeg.components[ci];
        let q = jpeg.qtables[comp.tq as usize];
        let (pw, ph) = (px.width.div_ceil(sub), px.height.div_ceil(sub));
        for by in 0..comp.bh {
            for bx in 0..comp.bw {
                let mut block = [0f32; 64];
                for y in 0..8 {
                    for x in 0..8 {
                        let sx = (bx * 8 + x).min(pw - 1);
                        let sy = (by * 8 + y).min(ph - 1);
                        block[y * 8 + x] = sample(plane, px.width, sub, sx, sy) - 128.0;
                    }
                }
                let coeffs = dct.forward(&block);
                comp.blocks[by * comp.bw + bx] =
                    std::array::from_fn(|i| (coeffs[i] / f32::from(q[i])).round() as i16);
            }
        }
    }
    write(&jpeg)
}

/// Full-resolution Y (or grey), Cb and Cr.
fn planes(px: &Pixels) -> Vec<Vec<f32>> {
    let n = px.width * px.height;
    if px.channels == 1 {
        return vec![px.data[..n].iter().map(|&v| f32::from(v)).collect()];
    }
    let mut y = Vec::with_capacity(n);
    let mut cb = Vec::with_capacity(n);
    let mut cr = Vec::with_capacity(n);
    for p in px.data[..n * 3].as_chunks::<3>().0 {
        let (r, g, b) = (f32::from(p[0]), f32::from(p[1]), f32::from(p[2]));
        y.push(0.299 * r + 0.587 * g + 0.114 * b);
        cb.push(-0.168_736 * r - 0.331_264 * g + 0.5 * b + 128.0);
        cr.push(0.5 * r - 0.418_688 * g - 0.081_312 * b + 128.0);
    }
    vec![y, cb, cr]
}

/// The plane's value at (x, y) of its `sub`-sampled grid: the mean of the source pixels it
/// covers.
fn sample(plane: &[f32], width: usize, sub: usize, x: usize, y: usize) -> f32 {
    if sub == 1 {
        return plane[y * width + x];
    }
    let height = plane.len() / width;
    let (x0, y0) = (x * 2, y * 2);
    let (x1, y1) = ((x0 + 1).min(width - 1), (y0 + 1).min(height - 1));
    (plane[y0 * width + x0]
        + plane[y0 * width + x1]
        + plane[y1 * width + x0]
        + plane[y1 * width + x1])
        / 4.0
}

/// The separable 8x8 DCT-II, scaled as T.81 A.3.3 defines it.
pub struct Dct {
    basis: [[f32; 8]; 8],
}

impl Dct {
    pub fn new() -> Self {
        let basis = std::array::from_fn(|u| {
            std::array::from_fn(|x| {
                let c = if u == 0 {
                    std::f32::consts::FRAC_1_SQRT_2
                } else {
                    1.0
                };
                0.5 * c * (((2 * x + 1) as f32 * u as f32 * std::f32::consts::PI) / 16.0).cos()
            })
        });
        Self { basis }
    }

    /// Basis function `u` at sample `x`.
    pub fn at(&self, u: usize, x: usize) -> f32 {
        self.basis[u][x]
    }

    pub fn forward(&self, block: &[f32; 64]) -> [f32; 64] {
        let mut rows = [0f32; 64];
        for y in 0..8 {
            for u in 0..8 {
                rows[y * 8 + u] = (0..8).map(|x| self.basis[u][x] * block[y * 8 + x]).sum();
            }
        }
        let mut out = [0f32; 64];
        for v in 0..8 {
            for u in 0..8 {
                out[v * 8 + u] = (0..8).map(|y| self.basis[v][y] * rows[y * 8 + u]).sum();
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_scaling_matches_ijg_and_estimates_back() {
        assert_eq!(scaled(&LUMA, 50), LUMA);
        assert_eq!(scaled(&LUMA, 100), [1; 64]);
        assert_eq!(scaled(&LUMA, 75)[0], 8);
        for q in [10, 30, 50, 75, 85, 92] {
            let est = estimate_quality(&scaled(&LUMA, q));
            assert!(est.abs_diff(q) <= 1, "{q} came back as {est}");
        }
    }

    #[test]
    fn dct_of_a_flat_block_is_all_dc() {
        let d = Dct::new();
        let out = d.forward(&[10.0; 64]);
        assert!((out[0] - 80.0).abs() < 1e-3);
        assert!(out[1..].iter().all(|v| v.abs() < 1e-3));
    }
}
