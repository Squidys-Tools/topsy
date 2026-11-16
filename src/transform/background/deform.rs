//! Modulated deformable convolution (DCNv2), as torchvision's `deform_conv2d` with one offset
//! group, stride 1 and no dilation: BiRefNet's decoder attention samples its input at learned
//! offsets.

use std::io;
use std::sync::atomic::AtomicBool;

use rayon::prelude::*;

use super::ops::{Conv, Map, Mat, check, matmul, sigmoid};

const COLS_BUDGET: usize = 1 << 22;

pub struct DeformConv {
    /// Predicts `2·k²` offsets (dy, dx per tap) then `k²` modulator logits, from one pass.
    pub offsets: Conv,
    /// The sampling convolution, `out × (inp · k²)`, with its bias (a folded BatchNorm).
    pub conv: Conv,
}

/// One bilinear sample: four source indices and their weights, already scaled by the modulator.
#[derive(Clone, Copy, Default)]
struct Sample {
    at: [u32; 4],
    w: [f32; 4],
}

fn sample(y: f32, x: f32, h: usize, w: usize, scale: f32) -> Sample {
    let mut s = Sample::default();
    if y <= -1.0 || x <= -1.0 || y >= h as f32 || x >= w as f32 {
        return s;
    }
    let (y0, x0) = (y.floor(), x.floor());
    let (ly, lx) = (y - y0, x - x0);
    let (y0, x0) = (y0 as isize, x0 as isize);
    let corners = [
        (y0, x0, (1.0 - ly) * (1.0 - lx)),
        (y0, x0 + 1, (1.0 - ly) * lx),
        (y0 + 1, x0, ly * (1.0 - lx)),
        (y0 + 1, x0 + 1, ly * lx),
    ];
    for (i, (cy, cx, cw)) in corners.into_iter().enumerate() {
        if cy >= 0 && cx >= 0 && (cy as usize) < h && (cx as usize) < w {
            s.at[i] = (cy as usize * w + cx as usize) as u32;
            s.w[i] = cw * scale;
        }
    }
    s
}

impl DeformConv {
    pub fn forward(&self, x: &Map, cancel: &AtomicBool) -> io::Result<Map> {
        let conv = &self.conv;
        let (k, pad) = (conv.k, conv.pad);
        let taps = k * k;
        let field = self.offsets.forward(x, cancel)?;
        let (h, w) = (x.h, x.w);
        let positions = h * w;
        let kk = conv.inp * taps;
        let mut out = Map::zeros(conv.out, h, w);
        let chunk = (COLS_BUDGET / kk).max(64).min(positions);
        let mut samples = vec![Sample::default(); taps * chunk];
        let mut cols = vec![0.0f32; kk * chunk];
        for p0 in (0..positions).step_by(chunk) {
            let n = chunk.min(positions - p0);
            samples[..taps * n]
                .par_chunks_mut(n)
                .enumerate()
                .for_each(|(tap, line)| {
                    let (dy, dx) = (field.plane(2 * tap), field.plane(2 * tap + 1));
                    let gate = field.plane(2 * taps + tap);
                    let (ky, kx) = (tap / k, tap % k);
                    for (i, s) in line.iter_mut().enumerate() {
                        let p = p0 + i;
                        let (oy, ox) = (p / w, p % w);
                        let y = (oy + ky) as f32 - pad as f32 + dy[p];
                        let xx = (ox + kx) as f32 - pad as f32 + dx[p];
                        *s = sample(y, xx, h, w, 2.0 * sigmoid(gate[p]));
                    }
                });
            let samples = &samples[..taps * n];
            cols[..kk * n]
                .par_chunks_mut(n)
                .enumerate()
                .for_each(|(row, line)| {
                    let (c, tap) = (row / taps, row % taps);
                    let plane = x.plane(c);
                    for (v, s) in line.iter_mut().zip(&samples[tap * n..(tap + 1) * n]) {
                        *v = s.w[0] * plane[s.at[0] as usize]
                            + s.w[1] * plane[s.at[1] as usize]
                            + s.w[2] * plane[s.at[2] as usize]
                            + s.w[3] * plane[s.at[3] as usize];
                    }
                });
            let weight = Mat::rows(&conv.weight, kk);
            let cols = Mat::rows(&cols[..kk * n], n);
            matmul(
                conv.out,
                n,
                kk,
                &mut out.data[p0..],
                positions,
                false,
                weight,
                cols,
                true,
            );
            check(cancel)?;
        }
        super::ops::add_bias(&mut out, &conv.bias);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(n: usize, seed: f32) -> Vec<f32> {
        (0..n)
            .map(|i| (i as f32 * 0.37 + seed).sin() * 0.5)
            .collect()
    }

    fn conv(out: usize, inp: usize, k: usize, weight: Vec<f32>) -> Conv {
        Conv {
            out,
            inp,
            k,
            stride: 1,
            pad: k / 2,
            weight,
            bias: vec![0.0; out],
        }
    }

    #[test]
    fn zero_offsets_and_a_neutral_gate_are_an_ordinary_convolution() {
        let x = Map {
            c: 2,
            h: 6,
            w: 5,
            data: ramp(60, 0.3),
        };
        let regular = conv(3, 2, 3, ramp(3 * 2 * 9, 1.0));
        // Zero weights and biases: no offsets, and 2·sigmoid(0) = 1.
        let dcn = DeformConv {
            offsets: conv(27, 2, 3, vec![0.0; 27 * 18]),
            conv: conv(3, 2, 3, regular.weight.clone()),
        };
        let cancel = AtomicBool::new(false);
        let want = regular.forward(&x, &cancel).unwrap();
        let got = dcn.forward(&x, &cancel).unwrap();
        for (a, b) in got.data.iter().zip(&want.data) {
            assert!((a - b).abs() < 1e-5);
        }
    }

    #[test]
    fn an_offset_of_one_pixel_reads_the_neighbour() {
        let x = Map {
            c: 1,
            h: 3,
            w: 3,
            data: (0..9).map(|v| v as f32).collect(),
        };
        let mut offsets = conv(3, 1, 1, vec![0.0; 3]);
        offsets.bias = vec![0.0, 1.0, 0.0];
        let dcn = DeformConv {
            offsets,
            conv: conv(1, 1, 1, vec![1.0]),
        };
        let got = dcn.forward(&x, &AtomicBool::new(false)).unwrap();
        // Each pixel takes its right neighbour; the last column samples past the edge, which
        // torchvision weights as zero for the missing corners.
        assert_eq!(got.data, vec![1.0, 2.0, 0.0, 4.0, 5.0, 0.0, 7.0, 8.0, 0.0]);
    }

    #[test]
    fn half_pixel_offsets_blend_bilinearly() {
        let s = sample(0.5, 0.25, 2, 2, 1.0);
        let plane = [0.0, 4.0, 8.0, 12.0];
        let v: f32 = (0..4).map(|i| s.w[i] * plane[s.at[i] as usize]).sum();
        assert!((v - 5.0).abs() < 1e-6);
    }
}
