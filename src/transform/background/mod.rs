//! Remove background: BiRefNet-lite run on the CPU in pure Rust, on-device and offline once its
//! model has been fetched. `model_status` and `download` drive the panel's "Get"; `remove` cuts
//! the subject out to a transparent PNG and `mask` gives the matte alone for a preview.

mod birefnet;
mod deform;
mod ops;
mod store;
mod swin;
mod weights;

use std::fs;
use std::io::{self, BufWriter};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::imageops::{self, FilterType as Resample};
use image::{DynamicImage, ImageDecoder, ImageReader, RgbaImage};

use birefnet::{BiRefNet, SIZE};
pub use model::download::ModelStatus;
use model::matte;
use ops::{Map, check};

/// Kept from first use for the app's life: loading parses 177 MB.
static NET: Mutex<Option<BiRefNet>> = Mutex::new(None);

pub fn model_status() -> ModelStatus {
    store::MODEL.status()
}

/// Fetches the model once over https (resuming a broken download), checks its SHA-256 and puts
/// it under `%LOCALAPPDATA%\topsy\models`. Nothing else here touches the network.
pub fn download(progress: &dyn Fn(f32), cancel: &AtomicBool) -> io::Result<()> {
    store::MODEL.download(progress, cancel).map(drop)
}

/// Cuts the subject of `src` out onto transparency, at its full size, into the PNG `out`.
pub fn remove(
    src: &Path,
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<()> {
    let image = decode(src)?;
    let (w, h) = (image.width(), image.height());
    let alpha = matte(&image, &|f| progress(f * 0.85), cancel)?;
    let rgba = image.to_rgba8();
    drop(image);
    let colours = foreground(&rgba, &alpha, cancel)?;
    progress(0.95);
    let mut pixels = RgbaImage::new(w, h);
    for (i, px) in pixels.pixels_mut().enumerate() {
        let a = alpha[i] * rgba.as_raw()[i * 4 + 3] as f32 / 255.0;
        px.0 = [
            colours[0][i],
            colours[1][i],
            colours[2][i],
            (a * 255.0).round() as u8,
        ];
    }
    drop((rgba, colours));
    check(cancel)?;
    let file = BufWriter::new(fs::File::create(out)?);
    let encoder = PngEncoder::new_with_quality(file, CompressionType::Fast, FilterType::Adaptive);
    DynamicImage::ImageRgba8(pixels)
        .write_with_encoder(encoder)
        .map_err(io::Error::other)?;
    progress(1.0);
    Ok(())
}

fn unsupported(err: image::ImageError) -> io::Error {
    crate::log(&format!("background: decode: {err}"));
    match err {
        image::ImageError::IoError(e) => e,
        image::ImageError::Unsupported(_) => io::Error::new(
            io::ErrorKind::Unsupported,
            "This image's format isn't supported",
        ),
        _ => io::Error::new(io::ErrorKind::InvalidData, "This image couldn't be read"),
    }
}

/// Decodes `src` upright, applying its EXIF orientation.
fn decode(src: &Path) -> io::Result<DynamicImage> {
    let reader = ImageReader::open(src)?.with_guessed_format()?;
    let mut decoder = reader.into_decoder().map_err(unsupported)?;
    let orientation = decoder.orientation().map_err(unsupported)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(unsupported)?;
    image.apply_orientation(orientation);
    Ok(image)
}

fn load() -> io::Result<BiRefNet> {
    let path = store::MODEL.path()?;
    if store::MODEL.status() != ModelStatus::Ready {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Get the background model first",
        ));
    }
    let bytes = fs::read(path)?;
    let mut weights = weights::Weights::parse(&bytes)?;
    drop(bytes);
    BiRefNet::load(&mut weights)
}

/// The refined alpha of `image`, 0..1 per pixel at full size.
fn matte(
    image: &DynamicImage,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<Vec<f32>> {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let rgb = image.to_rgb8();
    let small = imageops::resize(&rgb, SIZE as u32, SIZE as u32, Resample::Triangle);
    let input = Map {
        c: 3,
        h: SIZE,
        w: SIZE,
        data: matte::normalize(small.as_raw()),
    };
    let logits = {
        let mut net = NET.lock().unwrap_or_else(|e| e.into_inner());
        let loaded = match net.take() {
            Some(loaded) => loaded,
            None => load()?,
        };
        let loaded = net.insert(loaded);
        check(cancel)?;
        progress(0.05);
        loaded.forward(&input, cancel, &|f| progress(0.05 + f * 0.85))?
    };
    let soft = matte::upscale(&matte::sigmoid(&logits), SIZE, SIZE, w, h);
    check(cancel)?;
    let grey: Vec<f32> = rgb
        .pixels()
        .map(|p| (0.299 * p.0[0] as f32 + 0.587 * p.0[1] as f32 + 0.114 * p.0[2] as f32) / 255.0)
        .collect();
    drop(rgb);
    // The upscaled mask is soft over about one network pixel; the filter's window spans a few.
    let radius = (2.0 * w.max(h) as f32 / SIZE as f32).ceil().max(2.0) as usize;
    let mut alpha = matte::guided_filter(&grey, &soft, w, h, radius, 1e-3);
    matte::tidy(&mut alpha);
    progress(0.95);
    Ok(alpha)
}

/// The subject's own colour per plane, so edges don't carry a rim of the old background.
fn foreground(rgba: &RgbaImage, alpha: &[f32], cancel: &AtomicBool) -> io::Result<[Vec<u8>; 3]> {
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    let scale = (w.max(h) as f32 / SIZE as f32).max(1.0);
    let mut planes: [Vec<u8>; 3] = Default::default();
    for (c, plane) in planes.iter_mut().enumerate() {
        let src: Vec<f32> = rgba.pixels().map(|p| p.0[c] as f32 / 255.0).collect();
        *plane = matte::foreground(&src, alpha, w, h, scale)
            .iter()
            .map(|v| (v * 255.0).round() as u8)
            .collect();
        check(cancel)?;
    }
    Ok(planes)
}

#[cfg(test)]
mod tests;
