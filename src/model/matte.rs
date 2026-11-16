//! The arithmetic around the background model: the network's input, and turning its
//! low-resolution mask into a clean full-size matte with colours that don't carry the old
//! background at the edges.

#![allow(dead_code)]

use rayon::prelude::*;

const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

/// Interleaved 8-bit RGB to ImageNet-normalised planes, `3 × len`.
pub fn normalize(rgb: &[u8]) -> Vec<f32> {
    let len = rgb.len() / 3;
    let mut out = vec![0.0; 3 * len];
    for (c, plane) in out.chunks_mut(len).enumerate() {
        for (i, v) in plane.iter_mut().enumerate() {
            *v = (rgb[i * 3 + c] as f32 / 255.0 - MEAN[c]) / STD[c];
        }
    }
    out
}

pub fn sigmoid(logits: &[f32]) -> Vec<f32> {
    logits.iter().map(|v| 1.0 / (1.0 + (-v).exp())).collect()
}

/// Bilinear resize of one plane with pixel-centre alignment (as image editors and PIL do).
pub fn upscale(src: &[f32], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<f32> {
    let axis = |from: usize, to: usize| -> Vec<(usize, usize, f32)> {
        let scale = from as f32 / to as f32;
        (0..to)
            .map(|i| {
                let s = ((i as f32 + 0.5) * scale - 0.5).clamp(0.0, (from - 1) as f32);
                let lo = s.floor() as usize;
                (lo, (lo + 1).min(from - 1), s - lo as f32)
            })
            .collect()
    };
    let (xs, ys) = (axis(sw, dw), axis(sh, dh));
    let mut out = vec![0.0; dw * dh];
    out.par_chunks_mut(dw)
        .zip(ys.par_iter())
        .for_each(|(row, &(y0, y1, fy))| {
            let (r0, r1) = (&src[y0 * sw..][..sw], &src[y1 * sw..][..sw]);
            for (v, &(x0, x1, fx)) in row.iter_mut().zip(&xs) {
                let top = r0[x0] + (r0[x1] - r0[x0]) * fx;
                let bottom = r1[x0] + (r1[x1] - r1[x0]) * fx;
                *v = top + (bottom - top) * fy;
            }
        });
    out
}

/// Mean over the `(2r+1)²` window around each pixel, shrunk at the borders to what's inside.
/// Two running sums, so the cost doesn't grow with `r`.
pub fn box_mean(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut rows = vec![0.0; w * h];
    rows.par_chunks_mut(w)
        .zip(src.par_chunks(w))
        .for_each(|(dst, line)| running_mean(line, dst, r));
    let mut out = vec![0.0; w * h];
    // Columns in bands of 64, so each pass walks memory row by row.
    let band = 64;
    let bands: Vec<(usize, Vec<f32>)> = (0..w.div_ceil(band))
        .into_par_iter()
        .map(|b| {
            let (x0, x1) = (b * band, ((b + 1) * band).min(w));
            let width = x1 - x0;
            let mut col = vec![0.0; h];
            let mut res = vec![0.0; h];
            let mut block = vec![0.0; width * h];
            for x in x0..x1 {
                for y in 0..h {
                    col[y] = rows[y * w + x];
                }
                running_mean(&col, &mut res, r);
                for y in 0..h {
                    block[y * width + x - x0] = res[y];
                }
            }
            (x0, block)
        })
        .collect();
    for (x0, block) in bands {
        let width = block.len() / h;
        for y in 0..h {
            out[y * w + x0..][..width].copy_from_slice(&block[y * width..][..width]);
        }
    }
    out
}

fn running_mean(src: &[f32], dst: &mut [f32], r: usize) {
    let n = src.len();
    let mut sum = 0.0f64;
    let (mut lo, mut hi) = (0usize, 0usize);
    for (i, d) in dst.iter_mut().enumerate() {
        let (want_lo, want_hi) = (i.saturating_sub(r), (i + r + 1).min(n));
        while hi < want_hi {
            sum += src[hi] as f64;
            hi += 1;
        }
        while lo < want_lo {
            sum -= src[lo] as f64;
            lo += 1;
        }
        *d = (sum / (hi - lo) as f64) as f32;
    }
}

/// He et al.'s guided filter: pulls a soft, upscaled mask onto the edges of the full-size
/// image `guide` (grey, 0..1). `eps` sets how strong an edge must be to be followed.
pub fn guided_filter(guide: &[f32], p: &[f32], w: usize, h: usize, r: usize, eps: f32) -> Vec<f32> {
    let mul =
        |a: &[f32], b: &[f32]| -> Vec<f32> { a.par_iter().zip(b).map(|(x, y)| x * y).collect() };
    let mean_i = box_mean(guide, w, h, r);
    let mean_p = box_mean(p, w, h, r);
    let corr_ip = box_mean(&mul(guide, p), w, h, r);
    let corr_ii = box_mean(&mul(guide, guide), w, h, r);
    let mut a = vec![0.0; w * h];
    let mut b = vec![0.0; w * h];
    a.par_iter_mut()
        .zip(b.par_iter_mut())
        .enumerate()
        .for_each(|(i, (a, b))| {
            let var = corr_ii[i] - mean_i[i] * mean_i[i];
            let cov = corr_ip[i] - mean_i[i] * mean_p[i];
            *a = cov / (var + eps);
            *b = mean_p[i] - *a * mean_i[i];
        });
    drop((mean_p, corr_ip, corr_ii, mean_i));
    let (mean_a, mean_b) = (box_mean(&a, w, h, r), box_mean(&b, w, h, r));
    guide
        .par_iter()
        .zip(mean_a.par_iter().zip(&mean_b))
        .map(|(g, (a, b))| (a * g + b).clamp(0.0, 1.0))
        .collect()
}

/// Snaps the near-certain ends of a matte to fully clear or fully solid, keeping the soft
/// band in between, so flat areas don't come out faintly see-through.
pub fn tidy(alpha: &mut [f32]) {
    const LO: f32 = 0.02;
    const HI: f32 = 0.98;
    alpha
        .par_iter_mut()
        .for_each(|a| *a = ((*a - LO) / (HI - LO)).clamp(0.0, 1.0));
}

/// One pass of Forte and Pitié's blur fusion for one colour plane: estimates the foreground
/// and background colours behind each pixel from their neighbourhood, weighted by alpha.
fn blur_fusion(
    image: &[f32],
    fg: &[f32],
    bg: &[f32],
    alpha: &[f32],
    blurred_alpha: &[f32],
    (w, h): (usize, usize),
    r: usize,
) -> (Vec<f32>, Vec<f32>) {
    let fa: Vec<f32> = fg.par_iter().zip(alpha).map(|(f, a)| f * a).collect();
    let ba: Vec<f32> = bg
        .par_iter()
        .zip(alpha)
        .map(|(b, a)| b * (1.0 - a))
        .collect();
    let blurred_fa = box_mean(&fa, w, h, r);
    drop(fa);
    let blurred_ba = box_mean(&ba, w, h, r);
    drop(ba);
    let mut f = vec![0.0; w * h];
    let mut b = vec![0.0; w * h];
    f.par_iter_mut()
        .zip(b.par_iter_mut())
        .enumerate()
        .for_each(|(i, (f, b))| {
            let bf = blurred_fa[i] / (blurred_alpha[i] + 1e-5);
            let bb = blurred_ba[i] / ((1.0 - blurred_alpha[i]) + 1e-5);
            let a = alpha[i];
            *f = (bf + a * (image[i] - a * bf - (1.0 - a) * bb)).clamp(0.0, 1.0);
            *b = bb;
        });
    (f, b)
}

/// The foreground colour of one plane (0..1) under `alpha`: BiRefNet's own `refine_foreground`,
/// a wide pass then a narrow one, so a cut-out's edges keep the subject's colour instead of a
/// rim of the old background. `scale` widens the radii for images larger than 1024².
pub fn foreground(image: &[f32], alpha: &[f32], w: usize, h: usize, scale: f32) -> Vec<f32> {
    let wide = ((45.0 * scale) as usize).max(1);
    let narrow = ((3.0 * scale) as usize).max(1);
    let blurred_alpha = box_mean(alpha, w, h, wide);
    let (f, b) = blur_fusion(image, image, image, alpha, &blurred_alpha, (w, h), wide);
    let blurred_alpha = box_mean(alpha, w, h, narrow);
    let b = box_mean(&b, w, h, narrow);
    blur_fusion(image, &f, &b, alpha, &blurred_alpha, (w, h), narrow).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_uses_imagenet_statistics_per_plane() {
        let out = normalize(&[255, 0, 128, 0, 255, 128]);
        assert!((out[0] - (1.0 - 0.485) / 0.229).abs() < 1e-5);
        assert!((out[1] - (-0.485 / 0.229)).abs() < 1e-5);
        assert!((out[2] - (-0.456 / 0.224)).abs() < 1e-5);
        assert!((out[5] - (128.0 / 255.0 - 0.406) / 0.225).abs() < 1e-5);
    }

    #[test]
    fn upscale_keeps_flat_areas_and_ramps_between_samples() {
        let out = upscale(&[0.0, 1.0], 2, 1, 4, 1);
        assert_eq!(out, vec![0.0, 0.25, 0.75, 1.0]);
        assert!(
            upscale(&[0.5; 4], 2, 2, 7, 5)
                .iter()
                .all(|v| (v - 0.5).abs() < 1e-6)
        );
    }

    #[test]
    fn box_mean_matches_a_direct_average_with_shrunk_borders() {
        let (w, h) = (70, 5);
        let src: Vec<f32> = (0..w * h).map(|i| (i * 7 % 13) as f32).collect();
        let got = box_mean(&src, w, h, 2);
        for y in 0..h {
            for x in 0..w {
                let (mut sum, mut n) = (0.0, 0.0);
                for yy in y.saturating_sub(2)..(y + 3).min(h) {
                    for xx in x.saturating_sub(2)..(x + 3).min(w) {
                        sum += src[yy * w + xx];
                        n += 1.0;
                    }
                }
                assert!((got[y * w + x] - sum / n).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn guided_filter_snaps_a_blurry_mask_to_the_guide_edge() {
        let (w, h) = (40, 4);
        let guide: Vec<f32> = (0..w * h)
            .map(|i| if i % w < 20 { 0.0 } else { 1.0 })
            .collect();
        // A mask that ramps across eight pixels around the edge.
        let mask: Vec<f32> = (0..w * h)
            .map(|i| ((i % w) as f32 - 16.0).clamp(0.0, 8.0) / 8.0)
            .collect();
        let out = guided_filter(&guide, &mask, w, h, 4, 1e-4);
        assert!(out[17] < 0.2 && out[22] > 0.8, "{:?}", &out[..w]);
        assert!(out[17] < mask[17] && out[22] > mask[22]);
    }

    #[test]
    fn tidy_clears_the_near_certain_ends_and_keeps_the_middle() {
        let mut a = [0.01, 0.5, 0.99];
        tidy(&mut a);
        assert_eq!(a[0], 0.0);
        assert!((a[1] - 0.5).abs() < 1e-6);
        assert_eq!(a[2], 1.0);
    }

    #[test]
    fn foreground_drops_the_background_tint_at_a_soft_edge() {
        // Red subject on a green background; the 0.5-alpha band is a 50/50 blend of both, so
        // the colour as captured there is 0.5 red, 0.5 green.
        let (w, h) = (60, 8);
        let alpha: Vec<f32> = (0..w * h)
            .map(|i| match i % w {
                x if x < 28 => 1.0,
                28..=31 => 0.5,
                _ => 0.0,
            })
            .collect();
        let red: Vec<f32> = alpha.clone();
        let green: Vec<f32> = alpha.iter().map(|a| 1.0 - a).collect();
        let r = foreground(&red, &alpha, w, h, 0.2);
        let g = foreground(&green, &alpha, w, h, 0.2);
        let edge = 3 * w + 30;
        assert!(
            r[edge] > 0.85 && g[edge] < 0.15,
            "r {} g {}",
            r[edge],
            g[edge]
        );
    }
}
