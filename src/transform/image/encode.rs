//! Writing pixels in each output format, carrying the colour profile and EXIF where the format
//! can hold them.

use std::io::{self, Cursor};

use image::codecs::bmp::BmpEncoder;
use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::codecs::tiff::TiffEncoder;
use image::{ExtendedColorType, ImageEncoder, RgbaImage};
use windows::Win32::System::Com::COINIT_MULTITHREADED;

use super::pixels::{flatten, scale};
use crate::model::image_ops::{Format, png_colors, webp_quality};
use crate::model::jpeg::encode as jpeg;
use crate::model::{png, png_opt, quantize};
use crate::win32::{wic, with_com};

/// Metadata to write. The EXIF is already upright (orientation 1) and sized for the output.
#[derive(Clone, Copy, Default)]
pub struct Meta<'a> {
    pub icc: Option<&'a [u8]>,
    pub exif: Option<&'a [u8]>,
}

/// The AV1 encoder's speed, 1 (best) to 10. 6 keeps a 12 MP photo to seconds.
const AVIF_SPEED: u8 = 6;

/// `image` as a `format` file. `quality` (1..=100) steers the lossy formats; for PNG it picks the
/// palette size, with 100 keeping it lossless.
pub fn encode(image: &RgbaImage, format: Format, quality: u8, meta: Meta) -> io::Result<Vec<u8>> {
    let (w, h) = image.dimensions();
    let opaque = image.pixels().all(|p| p.0[3] == 255);
    let quality = quality.clamp(1, 100);
    match format {
        Format::Jpeg => {
            let rgb = flatten(image);
            let grey = rgb
                .as_chunks::<3>()
                .0
                .iter()
                .all(|p| p[0] == p[1] && p[1] == p[2]);
            let data: Vec<u8> = if grey {
                rgb.iter().step_by(3).copied().collect()
            } else {
                rgb
            };
            let px = jpeg::Pixels {
                width: w as usize,
                height: h as usize,
                channels: if grey { 1 } else { 3 },
                data: &data,
            };
            jpeg::encode(
                &px,
                quality,
                &jpeg::Metadata {
                    exif: meta.exif,
                    icc: meta.icc,
                },
            )
        }
        Format::Png => {
            let rgba = png_opt::Rgba {
                width: w as usize,
                height: h as usize,
                data: image.as_raw(),
            };
            let file = png_colors(quality)
                .and_then(|n| quantize::quantize(image.as_raw(), w as usize, n))
                .map(|q| png_opt::encode_indexed(&q, w as usize, h as usize))
                .unwrap_or_else(|| png_opt::encode(&rgba));
            png::with_metadata(&file, meta.icc, meta.exif)
        }
        Format::Webp => {
            let mut out = Vec::new();
            let mut enc = image_webp_lossy::WebPEncoder::new(&mut out);
            let mut params = image_webp_lossy::EncoderParams::default();
            params.use_lossy = true;
            params.lossy_quality = webp_quality(quality);
            enc.set_params(params);
            if let Some(icc) = meta.icc {
                enc.set_icc_profile(icc.to_vec());
            }
            if let Some(exif) = meta.exif {
                enc.set_exif_metadata(exif.to_vec());
            }
            let result = if opaque {
                enc.encode(&flatten(image), w, h, image_webp_lossy::ColorType::Rgb8)
            } else {
                enc.encode(image.as_raw(), w, h, image_webp_lossy::ColorType::Rgba8)
            };
            result.map_err(|e| failed("WebP", e))?;
            Ok(out)
        }
        Format::Heic => with_com(COINIT_MULTITHREADED, || {
            wic::encode_heic(image, quality, meta.icc)
        }),
        Format::Avif => avif(image, quality, opaque, meta),
        Format::Tiff => {
            let mut out = Cursor::new(Vec::new());
            let mut enc = TiffEncoder::new(&mut out);
            if let Some(icc) = meta.icc {
                let _ = enc.set_icc_profile(icc.to_vec());
            }
            if opaque {
                enc.write_image(&flatten(image), w, h, ExtendedColorType::Rgb8)
            } else {
                enc.write_image(image.as_raw(), w, h, ExtendedColorType::Rgba8)
            }
            .map_err(|e| failed("TIFF", e))?;
            Ok(out.into_inner())
        }
        Format::Bmp => {
            let mut out = Vec::new();
            BmpEncoder::new(&mut out)
                .write_image(&flatten(image), w, h, ExtendedColorType::Rgb8)
                .map_err(|e| failed("BMP", e))?;
            Ok(out)
        }
        Format::Gif => gif(image),
        Format::Ico => ico(image),
        Format::Pdf => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "PDF is written by the PDF Transforms",
        )),
    }
}

fn failed(what: &str, e: impl std::fmt::Display) -> io::Error {
    crate::log(&format!("transform: {what} encode: {e}"));
    io::Error::other(format!("topsy couldn't write the {what} file"))
}

fn avif(image: &RgbaImage, quality: u8, opaque: bool, meta: Meta) -> io::Result<Vec<u8>> {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let mut enc = ravif::Encoder::new()
        .with_quality(f32::from(quality))
        .with_alpha_quality(f32::from(quality))
        .with_speed(AVIF_SPEED);
    if let Some(exif) = meta.exif {
        enc = enc.with_exif(exif);
    }
    let encoded = if opaque {
        let px: Vec<ravif::RGB8> = image
            .pixels()
            .map(|p| ravif::RGB8::new(p.0[0], p.0[1], p.0[2]))
            .collect();
        enc.encode_rgb(ravif::Img::new(&px, w, h))
    } else {
        let px: Vec<ravif::RGBA8> = image
            .pixels()
            .map(|p| ravif::RGBA8::new(p.0[0], p.0[1], p.0[2], p.0[3]))
            .collect();
        enc.encode_rgba(ravif::Img::new(&px, w, h))
    };
    encoded.map(|e| e.avif_file).map_err(|e| failed("AVIF", e))
}

/// GIF holds 256 colours and on/off transparency: alpha is cut at half, then the picture is
/// quantised with dithering.
fn gif(image: &RgbaImage) -> io::Result<Vec<u8>> {
    let (w, h) = image.dimensions();
    if w > 65535 || h > 65535 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "GIF can't be larger than 65535 pixels a side",
        ));
    }
    let cut: Vec<u8> = image
        .pixels()
        .flat_map(|p| {
            if p.0[3] < 128 {
                [0, 0, 0, 0]
            } else {
                [p.0[0], p.0[1], p.0[2], 255]
            }
        })
        .collect();
    let indexed = quantize::quantize(&cut, w as usize, 256).unwrap_or_else(|| exact_palette(&cut));
    let palette: Vec<u8> = indexed
        .palette
        .iter()
        .flat_map(|c| [c[0], c[1], c[2]])
        .collect();
    let transparent = indexed
        .palette
        .iter()
        .position(|c| c[3] == 0)
        .map(|i| i as u8);
    let mut out = Vec::new();
    {
        let mut enc =
            gif::Encoder::new(&mut out, w as u16, h as u16, &[]).map_err(|e| failed("GIF", e))?;
        let frame = gif::Frame {
            width: w as u16,
            height: h as u16,
            buffer: indexed.indices.into(),
            palette: Some(palette),
            transparent,
            ..gif::Frame::default()
        };
        enc.write_frame(&frame).map_err(|e| failed("GIF", e))?;
    }
    Ok(out)
}

/// The palette of a picture that already has 256 colours or fewer.
fn exact_palette(rgba: &[u8]) -> quantize::Indexed {
    let mut palette: Vec<[u8; 4]> = Vec::new();
    let mut lookup = std::collections::HashMap::new();
    let indices = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let c = [p[0], p[1], p[2], p[3]];
            *lookup.entry(c).or_insert_with(|| {
                palette.push(c);
                (palette.len() - 1) as u8
            })
        })
        .collect();
    quantize::Indexed { palette, indices }
}

/// The sizes Windows asks an icon for, up to 256.
const ICON_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

/// A Windows icon with every standard size up to the picture's own (16 at least), each a PNG.
/// A picture that isn't square is centred on a transparent square first.
fn ico(image: &RgbaImage) -> io::Result<Vec<u8>> {
    let (w, h) = image.dimensions();
    let side = w.max(h);
    let mut square = RgbaImage::new(side, side);
    image::imageops::overlay(
        &mut square,
        image,
        i64::from((side - w) / 2),
        i64::from((side - h) / 2),
    );
    let top = side.clamp(16, 256);
    let square = scale(&square, top, top);
    let pngs: Vec<(u32, Vec<u8>)> = ICON_SIZES
        .iter()
        .filter(|&&s| s <= top)
        .map(|&s| {
            let small = scale(&square, s, s);
            (
                s,
                png_opt::encode(&png_opt::Rgba {
                    width: s as usize,
                    height: s as usize,
                    data: small.as_raw(),
                }),
            )
        })
        .collect();
    let frames = pngs
        .iter()
        .map(|(s, png)| IcoFrame::with_encoded(png.as_slice(), *s, *s, ExtendedColorType::Rgba8))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| failed("ICO", e))?;
    let mut out = Vec::new();
    IcoEncoder::new(&mut out)
        .encode_images(&frames)
        .map_err(|e| failed("ICO", e))?;
    Ok(out)
}
