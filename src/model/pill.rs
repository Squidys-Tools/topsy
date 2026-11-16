//! topsy's icon: a glass-blue pill with a lighter top edge, like the island. Drawn in code so
//! there is no binary asset; `build.rs` renders it at each size into the exe's icon resource.

/// `size`×`size` BGRA pixels, top-down, straight alpha.
pub fn pill(size: usize) -> Vec<u8> {
    let s = size as f32 / 32.0;
    let (cx, cy) = (size as f32 / 2.0, size as f32 / 2.0);
    let (half_length, radius) = (6.0 * s, 8.0 * s);
    let mut pixels = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            // Distance to the capsule's spine, minus its radius: negative inside.
            let along = px.clamp(-half_length, half_length);
            let distance = ((px - along).powi(2) + py.powi(2)).sqrt() - radius;
            let coverage = (0.5 - distance).clamp(0.0, 1.0);
            let t = ((py + radius) / (2.0 * radius)).clamp(0.0, 1.0);
            let mix = |top: f32, bottom: f32| (top + (bottom - top) * t) as u8;
            pixels.extend_from_slice(&[
                mix(255.0, 224.0),
                mix(155.0, 102.0),
                mix(142.0, 90.0),
                (coverage * 255.0).round() as u8,
            ]);
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(pixels: &[u8], size: usize, x: usize, y: usize) -> u8 {
        pixels[(y * size + x) * 4 + 3]
    }

    #[test]
    fn is_a_centred_pill_wider_than_tall_at_every_size() {
        for size in [16, 32, 256] {
            let p = pill(size);
            let at = |fx: f32, fy: f32| {
                alpha(
                    &p,
                    size,
                    (fx * size as f32) as usize,
                    (fy * size as f32) as usize,
                )
            };
            assert_eq!(p.len(), size * size * 4);
            assert_eq!(at(0.5, 0.5), 255, "{size}: solid in the middle");
            assert_eq!(at(0.0, 0.0), 0, "{size}: clear in the corner");
            assert_eq!(at(0.5, 0.1), 0, "{size}: clear above");
            assert!(at(0.86, 0.5) > 200, "{size}: reaches sideways");
        }
    }

    #[test]
    fn is_mirror_symmetric() {
        let size = 32;
        let p = pill(size);
        for y in 0..size {
            for x in 0..size / 2 {
                assert_eq!(
                    alpha(&p, size, x, y),
                    alpha(&p, size, size - 1 - x, y),
                    "row {y}, column {x}"
                );
            }
        }
    }
}
