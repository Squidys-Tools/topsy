//! BiRefNet-lite (Zheng et al. 2024, MIT): Swin-T run at full and half size, a squeeze block,
//! and a four-level decoder whose blocks attend through deformable ASPP and whose gates come
//! from gradient-attention heads. Mirrors `birefnet.py` at the pinned revision, inference only.

use std::io;
use std::sync::atomic::AtomicBool;

use rayon::prelude::*;

use super::deform::DeformConv;
use super::ops::{Conv, Map, add_bias, check, concat, relu, resize, sigmoid, tiles};
use super::swin::Swin;
use super::weights::Weights;

/// The network's input and output side.
pub const SIZE: usize = 1024;

struct Aspp {
    branches: Vec<DeformConv>,
    pool: Conv,
    /// 1×1 over the five 256-channel branches, BatchNorm folded in.
    project: Conv,
}

struct DecBlock {
    conv_in: Conv,
    aspp: Aspp,
    conv_out: Conv,
}

struct Gate {
    conv: Conv,
    attn: Conv,
}

struct Level {
    input: (Conv, Conv),
    block: DecBlock,
    lateral: Option<Conv>,
    gate: Option<Gate>,
}

pub struct BiRefNet {
    swin: Swin,
    squeeze: DecBlock,
    /// From the coarsest (stride 32) to the finest (stride 4).
    levels: Vec<Level>,
    /// `ipt_blk1`'s first convolution, over the full-size image.
    image_conv: Conv,
    /// `ipt_blk1`'s second convolution composed with its share of `conv_out1`.
    image_head: Conv,
    /// `conv_out1`'s share for the decoder's 96 channels, applied before the final upsample.
    decoder_head: Conv,
}

impl Aspp {
    fn load(w: &mut Weights, p: &str) -> io::Result<Self> {
        let mut branches =
            vec![w.deform(&format!("{p}.aspp1.atrous_conv"), &format!("{p}.aspp1.bn"))?];
        for i in 0..3 {
            let q = format!("{p}.aspp_deforms.{i}");
            branches.push(w.deform(&format!("{q}.atrous_conv"), &format!("{q}.bn"))?);
        }
        let pool = w.conv_bn(
            &format!("{p}.global_avg_pool.1"),
            &format!("{p}.global_avg_pool.2"),
        )?;
        let project = w.conv_bn(&format!("{p}.conv1"), &format!("{p}.bn1"))?;
        Ok(Self {
            branches,
            pool,
            project,
        })
    }

    /// Projects each branch straight into the output, so the 1280-channel concatenation never
    /// exists; the pooled branch is constant over the image and becomes part of the bias.
    fn forward(&self, x: &Map, cancel: &AtomicBool) -> io::Result<Map> {
        let width = self.pool.out;
        let mut out = Map::zeros(self.project.out, x.h, x.w);
        for (i, branch) in self.branches.iter().enumerate() {
            let mut b = branch.forward(x, cancel)?;
            relu(&mut b);
            let part = slice_inputs(&self.project, i * width, width);
            part.forward_into(&b, &mut out.data, i > 0, cancel)?;
        }
        let n = (x.h * x.w) as f32;
        let mean: Vec<f32> = x
            .data
            .par_chunks(x.h * x.w)
            .map(|p| p.iter().sum::<f32>() / n)
            .collect();
        let pooled = self.pool.forward(
            &Map {
                c: x.c,
                h: 1,
                w: 1,
                data: mean,
            },
            cancel,
        )?;
        let pooled = Map {
            data: pooled.data.iter().map(|v| v.max(0.0)).collect(),
            ..pooled
        };
        let mut bias = slice_inputs(&self.project, 4 * width, width)
            .forward(&pooled, cancel)?
            .data;
        bias.iter_mut()
            .zip(&self.project.bias)
            .for_each(|(b, p)| *b += p);
        add_bias(&mut out, &bias);
        relu(&mut out);
        Ok(out)
    }
}

/// The 1×1 convolution restricted to input channels `from..from + len`, without bias.
fn slice_inputs(conv: &Conv, from: usize, len: usize) -> Conv {
    let weight = (0..conv.out)
        .flat_map(|o| conv.weight[o * conv.inp + from..][..len].iter().copied())
        .collect();
    Conv {
        out: conv.out,
        inp: len,
        k: 1,
        stride: 1,
        pad: 0,
        weight,
        bias: vec![0.0; conv.out],
    }
}

impl DecBlock {
    fn load(w: &mut Weights, p: &str) -> io::Result<Self> {
        Ok(Self {
            conv_in: w.conv_bn(&format!("{p}.conv_in"), &format!("{p}.bn_in"))?,
            aspp: Aspp::load(w, &format!("{p}.dec_att"))?,
            conv_out: w.conv_bn(&format!("{p}.conv_out"), &format!("{p}.bn_out"))?,
        })
    }

    fn forward(&self, x: &Map, cancel: &AtomicBool) -> io::Result<Map> {
        let mut y = self.conv_in.forward(x, cancel)?;
        relu(&mut y);
        let y = self.aspp.forward(&y, cancel)?;
        self.conv_out.forward(&y, cancel)
    }
}

fn simple_convs(w: &mut Weights, p: &str) -> io::Result<(Conv, Conv)> {
    let a = w.conv(&format!("{p}.conv1"), 1, 1)?;
    let b = w.conv(&format!("{p}.conv_out"), 1, 1)?;
    Ok((a, b))
}

fn run_pair(pair: &(Conv, Conv), x: &Map, cancel: &AtomicBool) -> io::Result<Map> {
    pair.1.forward(&pair.0.forward(x, cancel)?, cancel)
}

impl BiRefNet {
    pub fn load(w: &mut Weights) -> io::Result<Self> {
        let swin = Swin::load(w)?;
        let squeeze = DecBlock::load(w, "squeeze_module.0")?;
        let mut levels = Vec::new();
        for (n, ipt) in [(4, 5), (3, 4), (2, 3), (1, 2)] {
            let d = "decoder";
            let lateral = if n > 1 {
                Some(w.conv(&format!("{d}.lateral_block{n}.conv"), 1, 0)?)
            } else {
                None
            };
            let gate = if n > 1 {
                Some(Gate {
                    conv: w.conv_bn(
                        &format!("{d}.gdt_convs_{n}.0"),
                        &format!("{d}.gdt_convs_{n}.1"),
                    )?,
                    attn: w.conv(&format!("{d}.gdt_convs_attn_{n}.0"), 1, 0)?,
                })
            } else {
                None
            };
            levels.push(Level {
                input: simple_convs(w, &format!("{d}.ipt_blk{ipt}"))?,
                block: DecBlock::load(w, &format!("{d}.decoder_block{n}"))?,
                lateral,
                gate,
            });
        }
        let (image_conv, side) = simple_convs(w, "decoder.ipt_blk1")?;
        let out = w.conv("decoder.conv_out1.0", 1, 0)?;
        let width = out.inp - side.out;
        let decoder_head = Conv {
            out: 1,
            inp: width,
            k: 1,
            stride: 1,
            pad: 0,
            weight: out.weight[..width].to_vec(),
            bias: out.bias,
        };
        // conv_out1 is linear over its channels, so its part for the side branch composes with
        // the convolution before it: one 3×3 to one channel instead of 24 at full size.
        let mix = &out.weight[width..];
        let taps = side.inp * side.k * side.k;
        let mut weight = vec![0.0; taps];
        for (k, m) in mix.iter().enumerate() {
            weight
                .iter_mut()
                .zip(&side.weight[k * taps..][..taps])
                .for_each(|(a, b)| *a += m * b);
        }
        let bias = vec![mix.iter().zip(&side.bias).map(|(m, b)| m * b).sum()];
        let image_head = Conv {
            out: 1,
            weight,
            bias,
            ..side
        };
        Ok(Self {
            swin,
            squeeze,
            levels,
            image_conv,
            image_head,
            decoder_head,
        })
    }

    /// Logits of the foreground mask, `SIZE × SIZE`, for a normalised `3 × SIZE × SIZE` image.
    pub fn forward(
        &self,
        x: &Map,
        cancel: &AtomicBool,
        progress: &dyn Fn(f32),
    ) -> io::Result<Vec<f32>> {
        // 24 Swin blocks over two passes, then the squeeze block and four decoder levels.
        const STEPS: f32 = 24.0 + 5.0;
        let mut done = 0.0;
        let mut step = || {
            done += 1.0;
            progress(done / STEPS);
        };
        let full = self.swin.forward(x, cancel, &mut step)?;
        let half = self
            .swin
            .forward(&resize(x, SIZE / 2, SIZE / 2), cancel, &mut step)?;
        let mut feats: Vec<Map> = full
            .iter()
            .zip(&half)
            .map(|(f, h)| concat(&[f, &resize(h, f.h, f.w)]))
            .collect();
        let top = &feats[3];
        let context: Vec<Map> = feats[..3].iter().map(|f| resize(f, top.h, top.w)).collect();
        let joined = concat(&[&context[0], &context[1], &context[2], top]);
        drop(context);
        feats[3] = self.squeeze.forward(&joined, cancel)?;
        drop(joined);
        step();

        let mut p = feats.pop().unwrap();
        for level in &self.levels {
            let side = run_pair(&level.input, &tiles(x, p.h), cancel)?;
            p = level.block.forward(&concat(&[&p, &side]), cancel)?;
            if let Some(gate) = &level.gate {
                let mut g = gate.conv.forward(&p, cancel)?;
                relu(&mut g);
                let attn = gate.attn.forward(&g, cancel)?;
                let plane = p.h * p.w;
                p.data.par_chunks_mut(plane).for_each(|c| {
                    c.iter_mut()
                        .zip(&attn.data)
                        .for_each(|(v, a)| *v *= sigmoid(*a))
                });
            }
            if let (Some(lateral), Some(skip)) = (&level.lateral, feats.pop()) {
                let mut up = resize(&p, skip.h, skip.w);
                let lat = lateral.forward(&skip, cancel)?;
                up.data
                    .par_iter_mut()
                    .zip(&lat.data)
                    .for_each(|(a, b)| *a += b);
                p = up;
            }
            check(cancel)?;
            step();
        }
        // conv_out1 is 1×1 and the upsample is linear per channel, so projecting to one channel
        // first gives the same logits without a 96-channel map at full size.
        let coarse = self.decoder_head.forward(&p, cancel)?;
        let mut logits = resize(&coarse, SIZE, SIZE);
        let side = self
            .image_head
            .forward(&self.image_conv.forward(x, cancel)?, cancel)?;
        logits
            .data
            .par_iter_mut()
            .zip(&side.data)
            .for_each(|(a, b)| *a += b);
        Ok(logits.data)
    }
}
