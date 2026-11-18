use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use image::{Rgba, RgbaImage};

use crate::log::{LogErr, MissingOk};
use model::jpeg::markers;

mod manual;
mod tools;

/// Something like a photo: smooth light, texture, and a few hard edges. `alpha` fades the
/// left third out and cuts a hole in the middle.
pub fn photo(w: u32, h: u32, alpha: bool) -> RgbaImage {
    let mut seed = 0x2545_F491u32;
    RgbaImage::from_fn(w, h, |x, y| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let noise = (seed % 9) as f32 - 4.0;
        let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
        let sky = 200.0 - 120.0 * fy;
        let hill = if fy > 0.6 + 0.1 * (fx * 9.0).sin() {
            90.0
        } else {
            0.0
        };
        let r = (sky - hill + 40.0 * (fx * 5.0).sin() + noise).clamp(0.0, 255.0) as u8;
        let g = (sky * 0.9 + hill * 0.4 + noise).clamp(0.0, 255.0) as u8;
        let b = (255.0 - hill * 1.5 - 60.0 * fx + noise).clamp(0.0, 255.0) as u8;
        let a = if !alpha {
            255
        } else if (fx - 0.5).abs() < 0.1 && (fy - 0.5).abs() < 0.1 {
            0
        } else {
            (fx * 3.0 * 255.0).clamp(0.0, 255.0) as u8
        };
        Rgba([r, g, b, a])
    })
}

pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("topsy-transform-{}-{tag}", std::process::id()));
        std::fs::remove_dir_all(&dir).missing_ok().log_err();
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    pub fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.path(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).missing_ok().log_err();
    }
}

pub fn png_of(img: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

/// A quality 90 JPEG of `img`, with `exif` and `icc` blocks when given.
pub fn jpeg_of(img: &RgbaImage, exif: Option<&[u8]>, icc: Option<&[u8]>) -> Vec<u8> {
    let rgb = image::DynamicImage::ImageRgba8(img.clone()).into_rgb8();
    let mut plain = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut plain, 90)
        .encode_image(&rgb)
        .unwrap();
    let mut out = vec![0xFF, markers::SOI];
    for p in icc.map(markers::icc_segments).unwrap_or_default() {
        markers::write_segment(&mut out, markers::APP2, &p);
    }
    out.extend_from_slice(&plain[2..]);
    match exif {
        Some(e) => markers::with_exif(&out, e).unwrap(),
        None => out,
    }
}

pub fn never() -> AtomicBool {
    AtomicBool::new(false)
}

pub fn quiet(_: f32) {}

pub fn decoded(path: &Path) -> RgbaImage {
    image::load_from_memory(&std::fs::read(path).unwrap())
        .unwrap()
        .into_rgba8()
}

pub fn mean_error(a: &RgbaImage, b: &RgbaImage) -> f64 {
    assert_eq!(a.dimensions(), b.dimensions());
    let sum: u64 = a
        .as_raw()
        .iter()
        .zip(b.as_raw())
        .map(|(x, y)| u64::from(x.abs_diff(*y)))
        .sum();
    sum as f64 / a.as_raw().len() as f64
}
