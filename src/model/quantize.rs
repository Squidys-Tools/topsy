//! Lossy palette reduction for PNG, the TinyPNG trick: median cut over a colour histogram,
//! refined by a few rounds of k-means, then mapped back with damped Floyd-Steinberg dithering.
//! Clustering happens on premultiplied colour, so every fully transparent pixel is one entry
//! and edges blend the way they will be drawn.

use std::collections::HashMap;

/// A palette (straight RGBA) and one index per pixel.
pub struct Indexed {
    pub palette: Vec<[u8; 4]>,
    pub indices: Vec<u8>,
}

fn premultiply(p: [u8; 4]) -> [f32; 4] {
    let a = f32::from(p[3]) / 255.0;
    [
        f32::from(p[0]) * a,
        f32::from(p[1]) * a,
        f32::from(p[2]) * a,
        f32::from(p[3]),
    ]
}

fn unpremultiply(c: [f32; 4]) -> [u8; 4] {
    let a = c[3].clamp(0.0, 255.0);
    if a < 0.5 {
        return [0, 0, 0, 0];
    }
    let f = 255.0 / a;
    let ch = |v: f32| (v * f).round().clamp(0.0, 255.0) as u8;
    [ch(c[0]), ch(c[1]), ch(c[2]), a.round() as u8]
}

fn distance(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    // Green counts most and alpha a bit more than any channel, roughly as the eye weighs them.
    const W: [f32; 4] = [0.5, 1.0, 0.25, 1.25];
    (0..4).map(|i| W[i] * (a[i] - b[i]) * (a[i] - b[i])).sum()
}

#[derive(Clone, Copy)]
struct Bucket {
    color: [f32; 4],
    count: f32,
}

/// Colours bucketed to 6 bits a channel, each bucket holding the mean of what fell in it.
fn histogram(rgba: &[u8]) -> Vec<Bucket> {
    let mut map: HashMap<u32, ([f64; 4], u32)> = HashMap::new();
    for p in rgba.as_chunks::<4>().0 {
        let px = [p[0], p[1], p[2], p[3]];
        let key = if px[3] == 0 {
            0
        } else {
            (u32::from(px[0] >> 2) << 18)
                | (u32::from(px[1] >> 2) << 12)
                | (u32::from(px[2] >> 2) << 6)
                | u32::from(px[3] >> 2)
                | 1 << 24
        };
        let c = premultiply(px);
        let e = map.entry(key).or_insert(([0.0; 4], 0));
        for (sum, v) in e.0.iter_mut().zip(c) {
            *sum += f64::from(v);
        }
        e.1 += 1;
    }
    map.into_values()
        .map(|(sum, n)| Bucket {
            color: std::array::from_fn(|i| (sum[i] / f64::from(n)) as f32),
            count: n as f32,
        })
        .collect()
}

fn mean(buckets: &[Bucket]) -> [f32; 4] {
    let total: f32 = buckets.iter().map(|b| b.count).sum();
    std::array::from_fn(|i| {
        buckets.iter().map(|b| b.color[i] * b.count).sum::<f32>() / total.max(1.0)
    })
}

fn median_cut(mut buckets: Vec<Bucket>, colors: usize) -> Vec<[f32; 4]> {
    let mut boxes: Vec<Vec<Bucket>> = vec![std::mem::take(&mut buckets)];
    while boxes.len() < colors {
        // Split the box with the most weighted spread along its widest channel.
        let mut pick = None;
        let mut best = 0f32;
        for (i, b) in boxes.iter().enumerate() {
            if b.len() < 2 {
                continue;
            }
            let (axis, range) = widest(b);
            let weight: f32 = b.iter().map(|x| x.count).sum();
            let score = range * weight.sqrt();
            if score > best {
                best = score;
                pick = Some((i, axis));
            }
        }
        let Some((i, axis)) = pick else { break };
        let mut b = boxes.swap_remove(i);
        b.sort_by(|x, y| x.color[axis].total_cmp(&y.color[axis]));
        let half: f32 = b.iter().map(|x| x.count).sum::<f32>() / 2.0;
        let mut acc = 0.0;
        let mut cut = 1;
        for (k, x) in b.iter().enumerate() {
            acc += x.count;
            if acc >= half {
                cut = (k + 1).clamp(1, b.len() - 1);
                break;
            }
        }
        let rest = b.split_off(cut);
        boxes.push(b);
        boxes.push(rest);
    }
    boxes.iter().map(|b| mean(b)).collect()
}

fn widest(b: &[Bucket]) -> (usize, f32) {
    (0..4)
        .map(|axis| {
            let (lo, hi) = b.iter().fold((f32::MAX, f32::MIN), |(lo, hi), x| {
                (lo.min(x.color[axis]), hi.max(x.color[axis]))
            });
            (axis, hi - lo)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((0, 0.0))
}

fn nearest(palette: &[[f32; 4]], c: &[f32; 4]) -> usize {
    let mut best = (0, f32::MAX);
    for (i, p) in palette.iter().enumerate() {
        let d = distance(p, c);
        if d < best.1 {
            best = (i, d);
        }
    }
    best.0
}

fn refine(buckets: &[Bucket], palette: &mut [[f32; 4]]) {
    for _ in 0..3 {
        let mut sums = vec![([0f32; 4], 0f32); palette.len()];
        for b in buckets {
            let i = nearest(palette, &b.color);
            for c in 0..4 {
                sums[i].0[c] += b.color[c] * b.count;
            }
            sums[i].1 += b.count;
        }
        for (p, (sum, n)) in palette.iter_mut().zip(sums) {
            if n > 0.0 {
                *p = std::array::from_fn(|c| sum[c] / n);
            }
        }
    }
}

/// `rgba` (`width` pixels a row) reduced to at most `colors` colours, or `None` if it already
/// has that few (an exact palette is then lossless, and the caller makes it).
pub fn quantize(rgba: &[u8], width: usize, colors: usize) -> Option<Indexed> {
    let buckets = histogram(rgba);
    if buckets.len() <= colors.min(256) && distinct_at_most(rgba, colors) {
        return None;
    }
    // Fully transparent pixels get an exact entry of their own, kept out of the clustering.
    let clear = buckets.iter().any(|b| b.color[3] == 0.0);
    let buckets: Vec<Bucket> = buckets.into_iter().filter(|b| b.color[3] > 0.0).collect();
    let mut palette = median_cut(buckets.clone(), colors.clamp(2, 256) - usize::from(clear));
    // k-means is buckets x colours a round; a photo's few hundred thousand buckets are thinned
    // to an even sample first.
    let step = buckets.len().div_ceil(20_000).max(1);
    let sample: Vec<Bucket> = buckets.iter().step_by(step).copied().collect();
    refine(&sample, &mut palette);
    if clear {
        palette.push([0.0; 4]);
    }
    let indices = dither(rgba, width, &palette);
    Some(Indexed {
        palette: palette.iter().map(|&c| unpremultiply(c)).collect(),
        indices,
    })
}

fn distinct_at_most(rgba: &[u8], n: usize) -> bool {
    let mut seen = std::collections::HashSet::new();
    rgba.as_chunks::<4>().0.iter().all(|p| {
        seen.insert([p[0], p[1], p[2], p[3]]);
        seen.len() <= n
    })
}

/// Maps each pixel to the palette, carrying three quarters of the error on (Floyd-Steinberg),
/// with nearest-colour lookups cached on a 5-bit grid.
fn dither(rgba: &[u8], width: usize, palette: &[[f32; 4]]) -> Vec<u8> {
    let height = rgba.len() / 4 / width;
    let mut cache = vec![0u16; 1 << 20];
    let mut lookup = |c: &[f32; 4]| -> usize {
        let q = |v: f32| (v.clamp(0.0, 255.0) as u32) >> 3;
        let key = (q(c[0]) << 15 | q(c[1]) << 10 | q(c[2]) << 5 | q(c[3])) as usize;
        if cache[key] == 0 {
            cache[key] = nearest(palette, c) as u16 + 1;
        }
        usize::from(cache[key] - 1)
    };
    let mut out = vec![0u8; width * height];
    let mut err = vec![[0f32; 4]; (width + 2) * 2];
    for y in 0..height {
        let (cur, next) = if y % 2 == 0 {
            (0, width + 2)
        } else {
            (width + 2, 0)
        };
        for e in &mut err[next..next + width + 2] {
            *e = [0.0; 4];
        }
        for x in 0..width {
            let i = (y * width + x) * 4;
            let px = premultiply([rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]);
            if px[3] == 0.0 {
                out[y * width + x] = lookup(&[0.0; 4]) as u8;
                continue;
            }
            let e = err[cur + x + 1];
            let want: [f32; 4] = std::array::from_fn(|c| px[c] + e[c]);
            let k = lookup(&want);
            out[y * width + x] = k as u8;
            let d: [f32; 4] = std::array::from_fn(|c| (want[c] - palette[k][c]) * 0.75);
            for c in 0..4 {
                err[cur + x + 2][c] += d[c] * 7.0 / 16.0;
                err[next + x][c] += d[c] * 3.0 / 16.0;
                err[next + x + 1][c] += d[c] * 5.0 / 16.0;
                err[next + x + 2][c] += d[c] / 16.0;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: usize, h: usize) -> Vec<u8> {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let a = if x < 4 { 0 } else { (y * 255 / h) as u8 };
                v.extend_from_slice(&[(x * 255 / w) as u8, 128, (y * 255 / h) as u8, a]);
            }
        }
        v
    }

    #[test]
    fn a_gradient_comes_back_close_with_the_palette_it_was_given() {
        let (w, h) = (96, 64);
        let src = gradient(w, h);
        let q = quantize(&src, w, 64).unwrap();
        assert!(q.palette.len() <= 64);
        assert_eq!(q.indices.len(), w * h);
        let mut err = 0f64;
        let mut n = 0f64;
        for (i, &k) in q.indices.iter().enumerate() {
            let (a, b) = (
                premultiply(src[i * 4..i * 4 + 4].try_into().unwrap()),
                premultiply(q.palette[k as usize]),
            );
            err += (0..4).map(|c| f64::from((a[c] - b[c]).abs())).sum::<f64>();
            n += 4.0;
        }
        assert!(err / n < 6.0, "mean error {}", err / n);
        // Fully transparent pixels share one fully transparent entry.
        let t = q.indices[0];
        assert_eq!(q.palette[t as usize][3], 0);
        assert!((0..h).all(|y| q.indices[y * w] == t));
    }

    #[test]
    fn few_colours_are_left_for_an_exact_palette() {
        let px: Vec<u8> = (0..100)
            .flat_map(|i| {
                if i % 3 == 0 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 128]
                }
            })
            .collect();
        assert!(quantize(&px, 10, 256).is_none());
    }

    #[test]
    fn premultiplied_round_trip() {
        for p in [
            [10, 200, 30, 255],
            [255, 255, 255, 1],
            [0, 0, 0, 0],
            [100, 50, 25, 128],
        ] {
            let back = unpremultiply(premultiply(p));
            assert!(
                (0..4).all(|c| back[c].abs_diff(p[c]) <= if p[3] < 4 { 255 } else { 2 }),
                "{p:?} -> {back:?}"
            );
        }
    }
}
