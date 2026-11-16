//! JPEG without a decode: metadata stripping and lossless rotation. No tool fits this job:
//! libjpeg-turbo ships jpegtran for Windows only inside installers.

pub mod coeffs;
pub mod huffman;
pub mod lossless;
pub mod markers;
pub mod write;

use std::io;

use super::exif;
use super::orientation::Orientation;

/// `bytes` turned by `o` without re-encoding, its EXIF orientation reset and pixel size updated.
/// `None` when the turn would need to split an MCU at the edge (the caller re-encodes instead)
/// or the file is a kind this reader can't take.
pub fn rotate_lossless(bytes: &[u8], o: Orientation) -> io::Result<Option<Vec<u8>>> {
    let jpeg = match coeffs::read(bytes) {
        Ok(j) => j,
        Err(e) if e.kind() == io::ErrorKind::Unsupported => return Ok(None),
        Err(e) => return Err(e),
    };
    if !lossless::is_exact(&jpeg, o) {
        return Ok(None);
    }
    let mut out = lossless::transform(&jpeg, o);
    let size = (out.width as u32, out.height as u32);
    for (marker, payload) in &mut out.extras {
        if *marker == markers::APP1 && payload.starts_with(markers::EXIF_HEADER) {
            exif::normalise(&mut payload[markers::EXIF_HEADER.len()..], Some(size));
        }
    }
    write::write(&out).map(Some)
}

/// The orientation recorded in the file's EXIF.
pub fn orientation(bytes: &[u8]) -> Orientation {
    markers::segments(bytes)
        .ok()
        .and_then(|s| markers::exif(&s))
        .and_then(|e| exif::orientation(&e))
        .map(Orientation::from_exif)
        .unwrap_or(Orientation::Normal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::exif::sample;

    /// A smooth gradient with some edges, `w` x `h` RGB.
    fn picture(w: usize, h: usize) -> Vec<u8> {
        let mut v = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                let edge: i32 = if (x / 12 + y / 9) % 2 == 0 { -40 } else { 40 };
                let r = ((x * 170 / w) as i32 + 43 + edge).clamp(0, 255);
                let g = ((y * 170 / h) as i32 + 43 + edge).clamp(0, 255);
                v.extend_from_slice(&[r as u8, g as u8, (128 + edge) as u8]);
            }
        }
        v
    }

    /// A JPEG from the `image` crate's encoder, with a tiny ICC profile and `exif`.
    fn jpeg(w: usize, h: usize, quality: u8, exif: Option<&[u8]>) -> Vec<u8> {
        let mut plain = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut plain, quality)
            .encode(
                &picture(w, h),
                w as u32,
                h as u32,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
        let mut out = vec![0xFF, markers::SOI];
        for p in markers::icc_segments(&[1, 2, 3]) {
            markers::write_segment(&mut out, markers::APP2, &p);
        }
        out.extend_from_slice(&plain[2..]);
        match exif {
            Some(e) => markers::with_exif(&out, e).unwrap(),
            None => out,
        }
    }

    /// Basis function `u` of the 8x8 DCT-II at sample `x`, scaled as T.81 A.3.3 defines it.
    fn basis(u: usize, x: usize) -> f32 {
        let c = if u == 0 {
            std::f32::consts::FRAC_1_SQRT_2
        } else {
            1.0
        };
        0.5 * c * (((2 * x + 1) as f32 * u as f32 * std::f32::consts::PI) / 16.0).cos()
    }

    fn decode(bytes: &[u8]) -> image::RgbImage {
        image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg)
            .unwrap()
            .to_rgb8()
    }

    #[test]
    fn coefficients_survive_a_read_write_round_trip() {
        let bytes = jpeg(48, 40, 80, None);
        let first = coeffs::read(&bytes).unwrap();
        let again = coeffs::read(&write::write(&first).unwrap()).unwrap();
        assert_eq!(first, again);
    }

    /// Component `ci` of `jpeg` as samples, through a float IDCT (symmetric under transposes,
    /// unlike a decoder's integer one).
    fn plane(jpeg: &coeffs::Jpeg, ci: usize) -> (usize, usize, Vec<f32>) {
        let c = &jpeg.components[ci];
        let (bw, bh) = jpeg.used_blocks(c);
        let q = jpeg.qtables[c.tq as usize];
        let mut out = vec![0f32; bw * 8 * bh * 8];
        for by in 0..bh {
            for bx in 0..bw {
                let b = &c.blocks[by * c.bw + bx];
                for y in 0..8 {
                    for x in 0..8 {
                        let mut s = 0f32;
                        for v in 0..8 {
                            for u in 0..8 {
                                s += basis(u, x)
                                    * basis(v, y)
                                    * f32::from(b[v * 8 + u])
                                    * f32::from(q[v * 8 + u]);
                            }
                        }
                        out[(by * 8 + y) * bw * 8 + bx * 8 + x] = s;
                    }
                }
            }
        }
        (bw * 8, bh * 8, out)
    }

    #[test]
    fn lossless_rotation_matches_rotating_the_pixels() {
        for quality in [75, 95] {
            // MCU-aligned 64x48: 4:2:0 at q75, 4:4:4 at q95.
            let bytes = jpeg(64, 48, quality, Some(&sample::exif(1)));
            let src = coeffs::read(&bytes).unwrap();
            let src_px = decode(&bytes);
            for o in [
                Orientation::Rotate90,
                Orientation::Rotate270,
                Orientation::Rotate180,
                Orientation::FlipHorizontal,
                Orientation::FlipVertical,
                Orientation::Transpose,
                Orientation::Transverse,
            ] {
                let turned = rotate_lossless(&bytes, o)
                    .unwrap()
                    .expect("aligned, so exact");
                let dst = coeffs::read(&turned).unwrap();
                for ci in 0..3 {
                    let (w, h, a) = plane(&src, ci);
                    let (dw, _, b) = plane(&dst, ci);
                    for y in 0..h {
                        for x in 0..w {
                            let (nx, ny) = o.map(x as u32, y as u32, w as u32, h as u32);
                            let d = (a[y * w + x] - b[ny as usize * dw + nx as usize]).abs();
                            assert!(d < 1e-2, "q{quality} {o:?} component {ci} at {x},{y}: {d}");
                        }
                    }
                }
                // Through a real decoder, only its integer IDCT and upsampling rounding differ.
                let img = decode(&turned);
                let (w, h) = src_px.dimensions();
                let mut sum = 0f64;
                for y in 0..h {
                    for x in 0..w {
                        let (nx, ny) = o.map(x, y, w, h);
                        let (a, b) = (src_px.get_pixel(x, y).0, img.get_pixel(nx, ny).0);
                        sum += (0..3).map(|c| f64::from(a[c].abs_diff(b[c]))).sum::<f64>();
                    }
                }
                assert!(sum / f64::from(w * h * 3) < 0.5, "{o:?}");
                assert_eq!(orientation(&turned), Orientation::Normal);
            }
        }
    }

    #[test]
    fn unaligned_edges_are_refused_only_where_they_move() {
        let bytes = jpeg(70, 48, 75, None);
        assert!(
            rotate_lossless(&bytes, Orientation::FlipHorizontal)
                .unwrap()
                .is_none()
        );
        assert!(
            rotate_lossless(&bytes, Orientation::FlipVertical)
                .unwrap()
                .is_some()
        );
        assert!(
            rotate_lossless(&bytes, Orientation::Rotate90)
                .unwrap()
                .is_some()
        );
        assert!(
            rotate_lossless(&bytes, Orientation::Rotate270)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn rotation_keeps_icc_and_gps_and_resets_orientation() {
        let bytes = jpeg(64, 48, 75, Some(&sample::exif(6)));
        assert_eq!(orientation(&bytes), Orientation::Rotate90);
        let turned = rotate_lossless(&bytes, Orientation::Rotate90)
            .unwrap()
            .unwrap();
        let segs = markers::segments(&turned).unwrap();
        assert_eq!(markers::icc(&segs).unwrap(), vec![1, 2, 3]);
        let e = markers::exif(&segs).unwrap();
        assert_eq!(exif::orientation(&e), Some(1));
        assert!(exif::has_gps(&e));
    }

    #[test]
    fn progressive_and_restart_files_rewrite_to_the_same_pixels() {
        // Made by libjpeg (through Pillow): progressive 4:2:0 with optimised tables, a
        // progressive greyscale, and a 4:2:2 baseline with a restart every 3 MCUs.
        for bytes in [
            &include_bytes!("testdata/progressive.jpg")[..],
            &include_bytes!("testdata/grey-progressive.jpg")[..],
            &include_bytes!("testdata/restart.jpg")[..],
        ] {
            let jpeg = coeffs::read(bytes).unwrap();
            let rewritten = write::write(&jpeg).unwrap();
            let a = image::load_from_memory(bytes).unwrap().to_rgb8();
            let b = image::load_from_memory(&rewritten).unwrap().to_rgb8();
            assert_eq!(a, b);
            let turned = rotate_lossless(bytes, Orientation::Rotate90).unwrap();
            assert!(turned.is_none() || image::load_from_memory(&turned.unwrap()).is_ok());
        }
    }
}
