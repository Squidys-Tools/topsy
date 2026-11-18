//! topsy's icon, topsy-turvy: a rounded tile with the glass notch hanging into its dark top half,
//! and the same notch upside down in ink, rising into its light bottom half. Drawn in code so
//! there is no binary asset; `build.rs` renders it at each size into the exe's icon resource.

type Rgb = [f32; 3];

const NIGHT: (Rgb, Rgb) = ([44.0, 48.0, 72.0], [16.0, 18.0, 25.0]);
const DAY: Rgb = [233.0, 236.0, 255.0];
const GLASS: (Rgb, Rgb) = ([154.0, 166.0, 255.0], [87.0, 99.0, 224.0]);
const INK: (Rgb, Rgb) = ([37.0, 38.0, 44.0], [5.0, 5.0, 6.0]);

/// Samples per pixel along each axis.
const SUPERSAMPLE: usize = 4;

/// The shape is laid out on a 256-unit grid; the tile spans 16..240 with corners of 56.
const GRID: f32 = 256.0;
const TILE: (f32, f32, f32) = (16.0, 240.0, 56.0);

/// A notch hanging from the tile's top edge: its sides, depth, bottom corner radius, and the
/// concave flare where it meets the edge.
struct Notch {
    left: f32,
    right: f32,
    depth: f32,
    radius: f32,
    flare: f32,
}

/// Small icons get a bigger notch so it still reads in the tray.
fn notch_for(size: usize) -> Notch {
    if size <= 24 {
        Notch {
            left: 66.0,
            right: 190.0,
            depth: 82.0,
            radius: 38.0,
            flare: 18.0,
        }
    } else {
        Notch {
            left: 82.0,
            right: 174.0,
            depth: 66.0,
            radius: 30.0,
            flare: 16.0,
        }
    }
}

impl Notch {
    fn contains(&self, x: f32, y: f32) -> bool {
        let (top, bottom) = (TILE.0, TILE.0 + self.depth);
        if y < top || y > bottom {
            return false;
        }
        let (centre, half) = (
            (self.left + self.right) / 2.0,
            (self.right - self.left) / 2.0,
        );
        let dx = (x - centre).abs();
        if dx <= half {
            let corner_y = bottom - self.radius;
            let corner_x = half - self.radius;
            return y <= corner_y
                || dx <= corner_x
                || (dx - corner_x).powi(2) + (y - corner_y).powi(2) <= self.radius.powi(2);
        }
        let out = dx - half;
        let into = y - top;
        out <= self.flare
            && into <= self.flare
            && (out - self.flare).powi(2) + (into - self.flare).powi(2) >= self.flare.powi(2)
    }
}

fn in_tile(x: f32, y: f32) -> bool {
    let (low, high, radius) = TILE;
    let centre = (low + high) / 2.0;
    let flat = (high - low) / 2.0 - radius;
    let dx = ((x - centre).abs() - flat).max(0.0);
    let dy = ((y - centre).abs() - flat).max(0.0);
    dx * dx + dy * dy <= radius * radius
}

fn mix((from, to): (Rgb, Rgb), t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| from[i] + (to[i] - from[i]) * t)
}

/// The colour at a point on the grid, or `None` outside the tile.
fn colour(notch: &Notch, x: f32, y: f32) -> Option<Rgb> {
    if !in_tile(x, y) {
        return None;
    }
    Some(if notch.contains(x, y) {
        mix(GLASS, (y - TILE.0) / notch.depth)
    } else if notch.contains(GRID - x, GRID - y) {
        mix(INK, (GRID - y - TILE.0) / notch.depth)
    } else if y < GRID / 2.0 {
        mix(NIGHT, y / (GRID / 2.0))
    } else {
        DAY
    })
}

/// `size`×`size` BGRA pixels, top-down, straight alpha.
pub fn app_icon(size: usize) -> Vec<u8> {
    let notch = notch_for(size);
    let step = GRID / (size * SUPERSAMPLE) as f32;
    let mut pixels = Vec::with_capacity(size * size * 4);
    for row in 0..size {
        for column in 0..size {
            let (mut sum, mut hits) = ([0.0f32; 3], 0usize);
            for sy in 0..SUPERSAMPLE {
                for sx in 0..SUPERSAMPLE {
                    let x = ((column * SUPERSAMPLE + sx) as f32 + 0.5) * step;
                    let y = ((row * SUPERSAMPLE + sy) as f32 + 0.5) * step;
                    if let Some(rgb) = colour(&notch, x, y) {
                        (0..3).for_each(|i| sum[i] += rgb[i]);
                        hits += 1;
                    }
                }
            }
            let [r, g, b] = sum.map(|c| (c / hits.max(1) as f32).round() as u8);
            let alpha = (hits * 255) as f32 / (SUPERSAMPLE * SUPERSAMPLE) as f32;
            pixels.extend_from_slice(&[b, g, r, alpha.round() as u8]);
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (red, green, blue, alpha) at a fraction of the way across and down.
    fn at(pixels: &[u8], size: usize, fx: f32, fy: f32) -> [u8; 4] {
        let (x, y) = ((fx * size as f32) as usize, (fy * size as f32) as usize);
        let i = (y * size + x) * 4;
        [pixels[i + 2], pixels[i + 1], pixels[i], pixels[i + 3]]
    }

    #[test]
    fn is_a_glass_notch_on_dark_over_an_ink_notch_on_light_at_every_size() {
        for size in [16, 20, 24, 32, 48, 256] {
            let p = app_icon(size);
            assert_eq!(p.len(), size * size * 4);
            assert_eq!(at(&p, size, 0.0, 0.0)[3], 0, "{size}: clear in the corner");

            let [r, _, b, a] = at(&p, size, 0.5, 0.2);
            assert!(
                a == 255 && b > 200 && b > r + 60,
                "{size}: glass notch at the top"
            );
            let [r, g, b, a] = at(&p, size, 0.2, 0.3);
            assert!(a == 255 && r.max(g).max(b) < 80, "{size}: dark beside it");
            let [r, g, b, a] = at(&p, size, 0.2, 0.7);
            assert!(a == 255 && r.min(g).min(b) > 200, "{size}: light below");
            let [r, g, b, a] = at(&p, size, 0.5, 0.8);
            assert!(
                a == 255 && r.max(g).max(b) < 60,
                "{size}: ink notch at the bottom"
            );
        }
    }

    #[test]
    fn is_mirror_symmetric_and_the_same_shape_upside_down() {
        for size in [16, 32] {
            let p = app_icon(size);
            let pixel = |x: usize, y: usize| &p[(y * size + x) * 4..][..4];
            for y in 0..size {
                for x in 0..size {
                    let (fx, fy) = (size - 1 - x, size - 1 - y);
                    assert_eq!(pixel(x, y), pixel(fx, y), "{size}: row {y}, column {x}");
                    assert_eq!(
                        pixel(x, y)[3],
                        pixel(fx, fy)[3],
                        "{size}: ({x}, {y}) turned"
                    );
                }
            }
        }
    }
}
