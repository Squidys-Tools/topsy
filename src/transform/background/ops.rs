//! The few tensor operations BiRefNet needs, on plain `f32` buffers. Feature maps are one image,
//! channels first (`c × h × w`); token sequences are row-major `len × dim`.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

use gemm::Parallelism;
use rayon::prelude::*;

/// Upper bound on one im2col buffer, in floats, so a 7×7 convolution at 256² doesn't need a
/// gigabyte at once.
const COLS_BUDGET: usize = 1 << 22;

pub struct Map {
    pub c: usize,
    pub h: usize,
    pub w: usize,
    pub data: Vec<f32>,
}

impl Map {
    pub fn zeros(c: usize, h: usize, w: usize) -> Self {
        Self {
            c,
            h,
            w,
            data: vec![0.0; c * h * w],
        }
    }

    pub fn plane(&self, c: usize) -> &[f32] {
        let n = self.h * self.w;
        &self.data[c * n..(c + 1) * n]
    }
}

/// A matrix operand: `at(i, j) = data[i * rs + j * cs]`.
#[derive(Clone, Copy)]
pub struct Mat<'a> {
    pub data: &'a [f32],
    pub rs: usize,
    pub cs: usize,
}

impl<'a> Mat<'a> {
    pub fn rows(data: &'a [f32], cols: usize) -> Self {
        Self {
            data,
            rs: cols,
            cs: 1,
        }
    }

    /// The transpose of a row-major `rows × cols` matrix, as Linear weights are used.
    pub fn transposed(data: &'a [f32], cols: usize) -> Self {
        Self {
            data,
            rs: 1,
            cs: cols,
        }
    }
}

/// `dst[m × n] (+)= a[m × k] · b[k × n]`, with `dst` row-major at row stride `dst_rs`.
#[allow(clippy::too_many_arguments)]
pub fn matmul(
    m: usize,
    n: usize,
    k: usize,
    dst: &mut [f32],
    dst_rs: usize,
    accumulate: bool,
    a: Mat,
    b: Mat,
    parallel: bool,
) {
    if m == 0 || n == 0 {
        return;
    }
    assert!(dst.len() >= (m - 1) * dst_rs + n);
    assert!(a.data.len() > (m - 1) * a.rs + (k.max(1) - 1) * a.cs);
    assert!(b.data.len() > (k.max(1) - 1) * b.rs + (n - 1) * b.cs);
    let parallelism = if parallel {
        Parallelism::Rayon(0)
    } else {
        Parallelism::None
    };
    // SAFETY: the asserts above keep every index the kernel touches inside the slices.
    unsafe {
        gemm::gemm(
            m,
            n,
            k,
            dst.as_mut_ptr(),
            1,
            dst_rs as isize,
            accumulate,
            a.data.as_ptr(),
            a.cs as isize,
            a.rs as isize,
            b.data.as_ptr(),
            b.cs as isize,
            b.rs as isize,
            1.0,
            1.0,
            false,
            false,
            false,
            parallelism,
        );
    }
}

pub fn check(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(io::ErrorKind::Interrupted.into())
    } else {
        Ok(())
    }
}

pub struct Conv {
    pub out: usize,
    pub inp: usize,
    pub k: usize,
    pub stride: usize,
    pub pad: usize,
    /// `out × (inp · k · k)`, row-major.
    pub weight: Vec<f32>,
    pub bias: Vec<f32>,
}

impl Conv {
    pub fn out_size(&self, n: usize) -> usize {
        (n + 2 * self.pad - self.k) / self.stride + 1
    }

    /// Folds an inference-time BatchNorm that follows this convolution into it.
    pub fn fold_bn(&mut self, gamma: &[f32], beta: &[f32], mean: &[f32], var: &[f32]) {
        let row = self.weight.len() / self.out;
        for o in 0..self.out {
            let s = gamma[o] / (var[o] + 1e-5).sqrt();
            self.weight[o * row..(o + 1) * row]
                .iter_mut()
                .for_each(|w| *w *= s);
            self.bias[o] = (self.bias[o] - mean[o]) * s + beta[o];
        }
    }

    pub fn forward(&self, x: &Map, cancel: &AtomicBool) -> io::Result<Map> {
        let mut out = Map::zeros(self.out, self.out_size(x.h), self.out_size(x.w));
        self.forward_into(x, &mut out.data, false, cancel)?;
        add_bias(&mut out, &self.bias);
        Ok(out)
    }

    /// Writes (or adds) the convolution without its bias into `dst`, `out × positions`.
    pub fn forward_into(
        &self,
        x: &Map,
        dst: &mut [f32],
        accumulate: bool,
        cancel: &AtomicBool,
    ) -> io::Result<()> {
        assert_eq!(x.c, self.inp);
        let (ho, wo) = (self.out_size(x.h), self.out_size(x.w));
        let positions = ho * wo;
        let kk = self.inp * self.k * self.k;
        let weight = Mat::rows(&self.weight, kk);
        if self.k == 1 && self.stride == 1 && self.pad == 0 {
            let input = Mat::rows(&x.data, positions);
            matmul(
                self.out, positions, kk, dst, positions, accumulate, weight, input, true,
            );
            return check(cancel);
        }
        let chunk = (COLS_BUDGET / kk).max(64).min(positions);
        let mut cols = vec![0.0f32; kk * chunk];
        for p0 in (0..positions).step_by(chunk) {
            let n = chunk.min(positions - p0);
            let cols = &mut cols[..kk * n];
            cols.par_chunks_mut(n).enumerate().for_each(|(row, line)| {
                let (c, tap) = (row / (self.k * self.k), row % (self.k * self.k));
                let (ky, kx) = (tap / self.k, tap % self.k);
                let plane = x.plane(c);
                let (mut oy, mut ox) = (p0 / wo, p0 % wo);
                for v in line.iter_mut() {
                    let iy = (oy * self.stride + ky) as isize - self.pad as isize;
                    let ix = (ox * self.stride + kx) as isize - self.pad as isize;
                    *v = if iy >= 0 && ix >= 0 && (iy as usize) < x.h && (ix as usize) < x.w {
                        plane[iy as usize * x.w + ix as usize]
                    } else {
                        0.0
                    };
                    ox += 1;
                    if ox == wo {
                        ox = 0;
                        oy += 1;
                    }
                }
            });
            let cols = Mat::rows(cols, n);
            matmul(
                self.out,
                n,
                kk,
                &mut dst[p0..],
                positions,
                accumulate,
                weight,
                cols,
                true,
            );
            check(cancel)?;
        }
        Ok(())
    }
}

pub fn add_bias(x: &mut Map, bias: &[f32]) {
    let n = x.h * x.w;
    x.data
        .par_chunks_mut(n)
        .zip(bias)
        .for_each(|(plane, b)| plane.iter_mut().for_each(|v| *v += b));
}

pub fn relu(x: &mut Map) {
    x.data.par_iter_mut().for_each(|v| *v = v.max(0.0));
}

pub fn sigmoid(v: f32) -> f32 {
    1.0 / (1.0 + (-v).exp())
}

/// Stacks maps of the same size along channels.
pub fn concat(maps: &[&Map]) -> Map {
    let (h, w) = (maps[0].h, maps[0].w);
    let mut data = Vec::with_capacity(maps.iter().map(|m| m.data.len()).sum());
    for m in maps {
        assert_eq!((m.h, m.w), (h, w));
        data.extend_from_slice(&m.data);
    }
    Map {
        c: data.len() / (h * w),
        h,
        w,
        data,
    }
}

/// Bilinear resize with `align_corners=True`, as PyTorch's `F.interpolate` in BiRefNet.
pub fn resize(x: &Map, h: usize, w: usize) -> Map {
    if (h, w) == (x.h, x.w) {
        return Map {
            c: x.c,
            h,
            w,
            data: x.data.clone(),
        };
    }
    let axis = |from: usize, to: usize| -> Vec<(usize, usize, f32)> {
        let scale = if to > 1 {
            (from - 1) as f32 / (to - 1) as f32
        } else {
            0.0
        };
        (0..to)
            .map(|i| {
                let s = i as f32 * scale;
                let lo = (s.floor() as usize).min(from - 1);
                let hi = (lo + 1).min(from - 1);
                (lo, hi, s - lo as f32)
            })
            .collect()
    };
    let (ys, xs) = (axis(x.h, h), axis(x.w, w));
    let mut out = Map::zeros(x.c, h, w);
    out.data
        .par_chunks_mut(h * w)
        .enumerate()
        .for_each(|(c, dst)| {
            let src = x.plane(c);
            for (y, &(y0, y1, fy)) in ys.iter().enumerate() {
                let (r0, r1) = (&src[y0 * x.w..][..x.w], &src[y1 * x.w..][..x.w]);
                for (xx, &(x0, x1, fx)) in xs.iter().enumerate() {
                    let top = r0[x0] + (r0[x1] - r0[x0]) * fx;
                    let bottom = r1[x0] + (r1[x1] - r1[x0]) * fx;
                    dst[y * w + xx] = top + (bottom - top) * fy;
                }
            }
        });
    out
}

/// Cuts the image into `grid × grid` tiles of `size²` and stacks them as channels, einops'
/// `b c (hg h) (wg w) -> b (c hg wg) h w`.
pub fn tiles(x: &Map, size: usize) -> Map {
    let grid = x.h / size;
    let mut out = Map::zeros(x.c * grid * grid, size, size);
    out.data
        .par_chunks_mut(size * size)
        .enumerate()
        .for_each(|(ch, dst)| {
            let (c, gy, gx) = (ch / (grid * grid), ch / grid % grid, ch % grid);
            let src = x.plane(c);
            for y in 0..size {
                let row = &src[(gy * size + y) * x.w + gx * size..][..size];
                dst[y * size..(y + 1) * size].copy_from_slice(row);
            }
        });
    out
}

/// LayerNorm over the last dimension of `len × dim` tokens.
pub fn layer_norm(x: &[f32], dim: usize, gamma: &[f32], beta: &[f32]) -> Vec<f32> {
    let mut out = vec![0.0; x.len()];
    out.par_chunks_mut(dim)
        .zip(x.par_chunks(dim))
        .for_each(|(o, t)| {
            let mean = t.iter().sum::<f32>() / dim as f32;
            let var = t.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / dim as f32;
            let inv = 1.0 / (var + 1e-5).sqrt();
            for i in 0..dim {
                o[i] = (t[i] - mean) * inv * gamma[i] + beta[i];
            }
        });
    out
}

pub struct Linear {
    pub out: usize,
    pub inp: usize,
    pub weight: Vec<f32>,
    pub bias: Option<Vec<f32>>,
}

impl Linear {
    pub fn forward(&self, x: &[f32]) -> Vec<f32> {
        let len = x.len() / self.inp;
        let mut out = vec![0.0; len * self.out];
        let (a, b) = (
            Mat::rows(x, self.inp),
            Mat::transposed(&self.weight, self.inp),
        );
        matmul(
            len, self.out, self.inp, &mut out, self.out, false, a, b, true,
        );
        if let Some(bias) = &self.bias {
            out.par_chunks_mut(self.out)
                .for_each(|row| row.iter_mut().zip(bias).for_each(|(v, b)| *v += b));
        }
        out
    }
}

/// GELU with the exact error function, as `nn.GELU()`.
pub fn gelu(x: &mut [f32]) {
    x.par_iter_mut()
        .for_each(|v| *v = 0.5 * *v * (1.0 + erf(*v * std::f32::consts::FRAC_1_SQRT_2)));
}

/// Abramowitz and Stegun 7.1.26, within 1.5e-7 of erf: below f32's resolution near 1.
fn erf(x: f32) -> f32 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs() as f64);
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let y = 1.0 - poly * (-(x as f64) * x as f64).exp();
    (y as f32).copysign(x)
}

pub fn softmax(row: &mut [f32]) {
    let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut sum = 0.0;
    for v in row.iter_mut() {
        *v = (*v - max).exp();
        sum += *v;
    }
    row.iter_mut().for_each(|v| *v /= sum);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv_naive(x: &Map, conv: &Conv) -> Vec<f32> {
        let (ho, wo) = (conv.out_size(x.h), conv.out_size(x.w));
        let mut out = vec![0.0; conv.out * ho * wo];
        for o in 0..conv.out {
            for oy in 0..ho {
                for ox in 0..wo {
                    let mut acc = conv.bias[o];
                    for c in 0..conv.inp {
                        for ky in 0..conv.k {
                            for kx in 0..conv.k {
                                let iy = (oy * conv.stride + ky) as isize - conv.pad as isize;
                                let ix = (ox * conv.stride + kx) as isize - conv.pad as isize;
                                if iy < 0 || ix < 0 || iy >= x.h as isize || ix >= x.w as isize {
                                    continue;
                                }
                                let w =
                                    conv.weight[((o * conv.inp + c) * conv.k + ky) * conv.k + kx];
                                acc += w * x.data[(c * x.h + iy as usize) * x.w + ix as usize];
                            }
                        }
                    }
                    out[(o * ho + oy) * wo + ox] = acc;
                }
            }
        }
        out
    }

    fn ramp(n: usize, seed: f32) -> Vec<f32> {
        (0..n)
            .map(|i| (i as f32 * 0.37 + seed).sin() * 0.5)
            .collect()
    }

    #[test]
    fn convolution_matches_the_definition() {
        let x = Map {
            c: 3,
            h: 9,
            w: 11,
            data: ramp(3 * 9 * 11, 0.1),
        };
        for (k, stride, pad) in [(3, 1, 1), (4, 4, 0), (1, 1, 0), (7, 1, 3)] {
            let conv = Conv {
                out: 5,
                inp: 3,
                k,
                stride,
                pad,
                weight: ramp(5 * 3 * k * k, 2.0),
                bias: ramp(5, 4.0),
            };
            let got = conv.forward(&x, &AtomicBool::new(false)).unwrap();
            for (a, b) in got.data.iter().zip(conv_naive(&x, &conv)) {
                assert!((a - b).abs() < 1e-4, "k{k}: {a} vs {b}");
            }
        }
    }

    #[test]
    fn align_corners_resize_keeps_the_corners_and_interpolates_between() {
        let x = Map {
            c: 1,
            h: 2,
            w: 2,
            data: vec![0.0, 1.0, 2.0, 3.0],
        };
        let y = resize(&x, 3, 3);
        assert_eq!(y.data, vec![0.0, 0.5, 1.0, 1.0, 1.5, 2.0, 2.0, 2.5, 3.0]);
    }

    #[test]
    fn tiles_stack_grid_cells_channel_major() {
        let x = Map {
            c: 1,
            h: 4,
            w: 4,
            data: (0..16).map(|v| v as f32).collect(),
        };
        let t = tiles(&x, 2);
        assert_eq!((t.c, t.h, t.w), (4, 2, 2));
        assert_eq!(t.plane(1), &[2.0, 3.0, 6.0, 7.0]);
        assert_eq!(t.plane(2), &[8.0, 9.0, 12.0, 13.0]);
    }

    #[test]
    fn erf_is_close_to_reference_values() {
        for (x, want) in [
            (0.0, 0.0),
            (0.5, 0.520_499_9),
            (1.0, 0.842_700_8),
            (-2.0, -0.995_322_3),
        ] {
            assert!((erf(x) - want).abs() < 1e-6, "{x}");
        }
    }
}
