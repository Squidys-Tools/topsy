//! Swin-T, BiRefNet-lite's backbone: patch embedding, four stages of shifted-window attention,
//! and a LayerNorm on each stage's output.

use std::io;
use std::sync::atomic::AtomicBool;

use rayon::prelude::*;

use super::ops::{Conv, Linear, Map, Mat, check, gelu, layer_norm, matmul, softmax};
use super::weights::Weights;

const WINDOW: usize = 7;
const TOKENS: usize = WINDOW * WINDOW;
const SHIFT: usize = WINDOW / 2;
const DEPTHS: [usize; 4] = [2, 2, 6, 2];
const HEADS: [usize; 4] = [3, 6, 12, 24];
const EMBED: usize = 96;

struct Norm {
    gamma: Vec<f32>,
    beta: Vec<f32>,
}

impl Norm {
    fn load(w: &mut Weights, prefix: &str) -> io::Result<Self> {
        Ok(Self {
            gamma: w.vec(&format!("{prefix}.weight"))?,
            beta: w.vec(&format!("{prefix}.bias"))?,
        })
    }

    fn apply(&self, x: &[f32]) -> Vec<f32> {
        layer_norm(x, self.gamma.len(), &self.gamma, &self.beta)
    }
}

struct Block {
    norm1: Norm,
    qkv: Linear,
    /// `heads × 49 × 49`, gathered from the relative position table once.
    bias: Vec<f32>,
    proj: Linear,
    norm2: Norm,
    fc1: Linear,
    fc2: Linear,
}

struct Stage {
    dim: usize,
    heads: usize,
    blocks: Vec<Block>,
    merge: Option<(Norm, Linear)>,
    out_norm: Norm,
}

pub struct Swin {
    embed: Conv,
    embed_norm: Norm,
    stages: Vec<Stage>,
}

fn relative_bias(table: &[f32], heads: usize) -> Vec<f32> {
    let mut bias = vec![0.0; heads * TOKENS * TOKENS];
    for i in 0..TOKENS {
        for j in 0..TOKENS {
            let dy = (i / WINDOW) as isize - (j / WINDOW) as isize + WINDOW as isize - 1;
            let dx = (i % WINDOW) as isize - (j % WINDOW) as isize + WINDOW as isize - 1;
            let index = dy as usize * (2 * WINDOW - 1) + dx as usize;
            for h in 0..heads {
                bias[(h * TOKENS + i) * TOKENS + j] = table[index * heads + h];
            }
        }
    }
    bias
}

impl Swin {
    pub fn load(w: &mut Weights) -> io::Result<Self> {
        let embed = w.conv("bb.patch_embed.proj", 4, 0)?;
        let embed_norm = Norm::load(w, "bb.patch_embed.norm")?;
        let mut stages = Vec::new();
        for (s, (&depth, &heads)) in DEPTHS.iter().zip(&HEADS).enumerate() {
            let mut blocks = Vec::new();
            for b in 0..depth {
                let p = format!("bb.layers.{s}.blocks.{b}");
                blocks.push(Block {
                    norm1: Norm::load(w, &format!("{p}.norm1"))?,
                    qkv: w.linear(&format!("{p}.attn.qkv"), true)?,
                    bias: relative_bias(
                        &w.vec(&format!("{p}.attn.relative_position_bias_table"))?,
                        heads,
                    ),
                    proj: w.linear(&format!("{p}.attn.proj"), true)?,
                    norm2: Norm::load(w, &format!("{p}.norm2"))?,
                    fc1: w.linear(&format!("{p}.mlp.fc1"), true)?,
                    fc2: w.linear(&format!("{p}.mlp.fc2"), true)?,
                });
            }
            let merge = if s < 3 {
                let p = format!("bb.layers.{s}.downsample");
                Some((
                    Norm::load(w, &format!("{p}.norm"))?,
                    w.linear(&format!("{p}.reduction"), false)?,
                ))
            } else {
                None
            };
            let out_norm = Norm::load(w, &format!("bb.norm{s}"))?;
            stages.push(Stage {
                dim: EMBED << s,
                heads,
                blocks,
                merge,
                out_norm,
            });
        }
        Ok(Self {
            embed,
            embed_norm,
            stages,
        })
    }

    /// The four stage outputs, channels first, at strides 4, 8, 16 and 32.
    pub fn forward(
        &self,
        image: &Map,
        cancel: &AtomicBool,
        step: &mut dyn FnMut(),
    ) -> io::Result<Vec<Map>> {
        let x = self.embed.forward(image, cancel)?;
        let (mut h, mut w) = (x.h, x.w);
        let mut tokens = self.embed_norm.apply(&transpose(&x.data, EMBED, h * w));
        let mut outs = Vec::new();
        for stage in &self.stages {
            for (i, block) in stage.blocks.iter().enumerate() {
                let shift = if i % 2 == 1 { SHIFT } else { 0 };
                block.forward(&mut tokens, h, w, stage.dim, stage.heads, shift);
                check(cancel)?;
                step();
            }
            let normed = stage.out_norm.apply(&tokens);
            outs.push(Map {
                c: stage.dim,
                h,
                w,
                data: transpose(&normed, h * w, stage.dim),
            });
            if let Some((norm, reduce)) = &stage.merge {
                tokens = reduce.forward(&norm.apply(&merge(&tokens, h, w, stage.dim)));
                (h, w) = (h / 2, w / 2);
            }
        }
        Ok(outs)
    }
}

/// Row-major `rows × cols` to `cols × rows`.
fn transpose(x: &[f32], rows: usize, cols: usize) -> Vec<f32> {
    let mut out = vec![0.0; x.len()];
    out.par_chunks_mut(rows).enumerate().for_each(|(c, line)| {
        for (r, v) in line.iter_mut().enumerate() {
            *v = x[r * cols + c];
        }
    });
    out
}

/// PatchMerging's gather: each 2×2 neighbourhood becomes one token of `[x0, x1, x2, x3]`, with
/// x1 the pixel below and x2 the one to the right.
fn merge(x: &[f32], h: usize, w: usize, dim: usize) -> Vec<f32> {
    let (h2, w2) = (h.div_ceil(2), w.div_ceil(2));
    let mut out = vec![0.0; h2 * w2 * 4 * dim];
    out.par_chunks_mut(4 * dim)
        .enumerate()
        .for_each(|(t, dst)| {
            let (r, c) = (t / w2 * 2, t % w2 * 2);
            for (q, (dr, dc)) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
                if r + dr < h && c + dc < w {
                    let src = ((r + dr) * w + c + dc) * dim;
                    dst[q * dim..(q + 1) * dim].copy_from_slice(&x[src..src + dim]);
                }
            }
        });
    out
}

/// The region of a padded row or column for the shifted-window mask: 0, 1 or 2.
fn region(i: usize, padded: usize) -> usize {
    if i < padded - WINDOW {
        0
    } else if i < padded - SHIFT {
        1
    } else {
        2
    }
}

impl Block {
    fn forward(&self, x: &mut [f32], h: usize, w: usize, dim: usize, heads: usize, shift: usize) {
        let (hp, wp) = (h.div_ceil(WINDOW) * WINDOW, w.div_ceil(WINDOW) * WINDOW);
        let across = wp / WINDOW;
        let windows = (hp / WINDOW) * across;
        // Padded, rolled grid cell of each window token and the source token it reads, if any.
        let cell = |win: usize, t: usize| {
            let r = win / across * WINDOW + t / WINDOW;
            let c = win % across * WINDOW + t % WINDOW;
            let (sr, sc) = ((r + shift) % hp, (c + shift) % wp);
            (r, c, (sr < h && sc < w).then_some(sr * w + sc))
        };
        let normed = self.norm1.apply(x);
        let mut grid = vec![0.0; windows * TOKENS * dim];
        grid.par_chunks_mut(TOKENS * dim)
            .enumerate()
            .for_each(|(win, dst)| {
                for t in 0..TOKENS {
                    if let Some(src) = cell(win, t).2 {
                        dst[t * dim..(t + 1) * dim]
                            .copy_from_slice(&normed[src * dim..(src + 1) * dim]);
                    }
                }
            });
        drop(normed);
        let qkv = self.qkv.forward(&grid);
        let hd = dim / heads;
        let scale = (hd as f32).powf(-0.5);
        grid.par_chunks_mut(TOKENS * dim)
            .enumerate()
            .for_each(|(win, out)| {
                let rows = &qkv[win * TOKENS * 3 * dim..(win + 1) * TOKENS * 3 * dim];
                let ids: Vec<usize> = (0..TOKENS)
                    .map(|t| {
                        let (r, c, _) = cell(win, t);
                        region(r, hp) * 3 + region(c, wp)
                    })
                    .collect();
                let mut scores = vec![0.0f32; TOKENS * TOKENS];
                for head in 0..heads {
                    let q = Mat {
                        data: &rows[head * hd..],
                        rs: 3 * dim,
                        cs: 1,
                    };
                    let k = Mat {
                        data: &rows[dim + head * hd..],
                        rs: 1,
                        cs: 3 * dim,
                    };
                    let v = Mat {
                        data: &rows[2 * dim + head * hd..],
                        rs: 3 * dim,
                        cs: 1,
                    };
                    matmul(TOKENS, TOKENS, hd, &mut scores, TOKENS, false, q, k, false);
                    let bias = &self.bias[head * TOKENS * TOKENS..(head + 1) * TOKENS * TOKENS];
                    for i in 0..TOKENS {
                        let row = &mut scores[i * TOKENS..(i + 1) * TOKENS];
                        for j in 0..TOKENS {
                            row[j] = row[j] * scale + bias[i * TOKENS + j];
                            if shift > 0 && ids[i] != ids[j] {
                                row[j] -= 100.0;
                            }
                        }
                        softmax(row);
                    }
                    let probs = Mat::rows(&scores, TOKENS);
                    matmul(
                        TOKENS,
                        hd,
                        TOKENS,
                        &mut out[head * hd..],
                        dim,
                        false,
                        probs,
                        v,
                        false,
                    );
                }
            });
        drop(qkv);
        let attended = self.proj.forward(&grid);
        drop(grid);
        for win in 0..windows {
            for t in 0..TOKENS {
                let (r, c, _) = cell(win, t);
                let (or, oc) = ((r + shift) % hp, (c + shift) % wp);
                if or < h && oc < w {
                    let (dst, src) = ((or * w + oc) * dim, (win * TOKENS + t) * dim);
                    x[dst..dst + dim]
                        .iter_mut()
                        .zip(&attended[src..src + dim])
                        .for_each(|(a, b)| *a += b);
                }
            }
        }
        drop(attended);
        let mut hidden = self.fc1.forward(&self.norm2.apply(x));
        gelu(&mut hidden);
        let mlp = self.fc2.forward(&hidden);
        x.par_iter_mut().zip(&mlp).for_each(|(a, b)| *a += b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_bias_reads_the_table_at_the_offset_between_tokens() {
        let table: Vec<f32> = (0..169).map(|v| v as f32).collect();
        let bias = relative_bias(&table, 1);
        // Same token: offset (0, 0), the table's centre.
        assert_eq!(bias[0], 84.0);
        // Token 0 against token 48 (six down, six right): offset (-6, -6), the first entry.
        assert_eq!(bias[48], 0.0);
        assert_eq!(bias[48 * TOKENS], 168.0);
    }

    #[test]
    fn merge_orders_the_neighbourhood_as_patch_merging() {
        let x: Vec<f32> = (0..4).map(|v| v as f32).collect();
        assert_eq!(merge(&x, 2, 2, 1), vec![0.0, 2.0, 1.0, 3.0]);
    }

    #[test]
    fn shifted_regions_split_the_last_window() {
        let ids: Vec<usize> = (0..14).map(|i| region(i, 14)).collect();
        assert_eq!(ids, vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2]);
    }
}
