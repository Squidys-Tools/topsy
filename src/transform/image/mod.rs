//! The image Transforms: Compress, Convert, Resize, Rotate and Remove metadata.

pub(super) mod encode;
pub(super) mod pixels;
pub(super) mod source;
#[cfg(test)]
pub(crate) mod tests;

use std::io::{self, Cursor};
use std::path::Path;
use std::sync::atomic::AtomicBool;

use image::{ImageDecoder, ImageReader, RgbaImage};
use windows::Win32::System::Com::COINIT_MULTITHREADED;

pub use self::encode::Meta;
use self::encode::encode;
use self::source::{Source, kind_of, load, orient};
use super::Steps;
use crate::model::image_geometry::{Resize, Rotation};
use crate::model::image_ops::{CompressBy, CompressOptions, ConvertOptions, Format, Kind};
use crate::model::orientation::Orientation;
use crate::model::pdf::PageSize;
use crate::model::size_search::fit_to_size;
use crate::model::{exif, jpeg, png, webp};
use crate::win32::{wic, with_com};

/// What [`probe`] learns without the full work of an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    pub kind: Kind,
    /// As shown, with the EXIF orientation applied. Zero for a PDF.
    pub width: u32,
    pub height: u32,
    /// Whether the format's pixels carry alpha, not whether any is actually see-through.
    pub has_alpha: bool,
    pub bytes: u64,
    /// For a PDF.
    pub pages: Option<u32>,
}

/// What an operation wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Written {
    pub format: Format,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    /// False when Compress by file size couldn't get under the target; this is the smallest try.
    pub fits: bool,
}

pub fn probe(path: &Path) -> io::Result<ImageInfo> {
    let bytes = std::fs::metadata(path)?.len();
    let mut head = Vec::with_capacity(64);
    io::Read::read_to_end(
        &mut io::Read::take(std::fs::File::open(path)?, 64),
        &mut head,
    )?;
    if Kind::sniff(&head) == Some(Kind::Pdf) {
        let pages = super::pdf::page_count(path)?;
        return Ok(ImageInfo {
            kind: Kind::Pdf,
            width: 0,
            height: 0,
            has_alpha: false,
            bytes,
            pages: Some(pages),
        });
    }
    let kind = kind_of(&head)?;
    let (w, h, has_alpha, o) = match source::image_format(kind) {
        Some(format) => {
            let file = io::BufReader::new(std::fs::File::open(path)?);
            let mut d = ImageReader::with_format(file, format)
                .into_decoder()
                .map_err(|_| source::unsupported())?;
            let o = d
                .orientation()
                .map(|o| Orientation::from_exif(u16::from(o.to_exif())))
                .unwrap_or(Orientation::Normal);
            let (w, h) = d.dimensions();
            (w, h, d.color_type().has_alpha(), o)
        }
        None => {
            let d = with_com(COINIT_MULTITHREADED, || wic::decode(path))?;
            let has_alpha = d.image.pixels().any(|p| p.0[3] < 255);
            (
                d.image.width(),
                d.image.height(),
                has_alpha,
                Orientation::from_exif(d.orientation),
            )
        }
    };
    let (width, height) = if o.swaps_axes() { (h, w) } else { (w, h) };
    Ok(ImageInfo {
        kind,
        width,
        height,
        has_alpha,
        bytes,
        pages: None,
    })
}

pub fn compress(
    src: &Path,
    options: &CompressOptions,
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<Written> {
    let steps = Steps::new(progress, cancel);
    steps.at(0.0)?;
    let s = load(src)?;
    steps.at(0.1)?;
    let format = options.format.resolve(s.kind, s.has_alpha());
    can_write(format)?;
    let (w, h) = options.largest_side.fit(s.image.width(), s.image.height());
    let base = pixels::scale(&s.image, w, h);
    steps.at(0.2)?;
    let unchanged = Format::matching(s.kind) == Some(format) && (w, h) == s.image.dimensions();
    let original = s.bytes.len() as u64;
    let (bytes, fits) = match options.by {
        CompressBy::Quality => {
            let bytes = encode_for(&s, &base, format, options.quality)?;
            (bytes, true)
        }
        CompressBy::FileSize if unchanged && original <= options.target_bytes => {
            (s.bytes.clone(), true)
        }
        CompressBy::FileSize => {
            let mut tries = 0.0f32;
            let mut sized: Option<(f64, RgbaImage)> = None;
            let (bytes, _, fits) =
                fit_to_size(options.target_bytes, |a| -> io::Result<(Vec<u8>, u64)> {
                    steps.check()?;
                    let picture = if a.scale >= 1.0 {
                        &base
                    } else {
                        if sized.as_ref().is_none_or(|(sc, _)| *sc != a.scale) {
                            let (sw, sh) = (scaled(w, a.scale), scaled(h, a.scale));
                            sized = Some((a.scale, pixels::scale(&base, sw, sh)));
                        }
                        &sized.as_ref().expect("just made").1
                    };
                    let bytes = encode_for(&s, picture, format, a.quality)?;
                    tries += 1.0;
                    steps.at(0.2 + 0.75 * (1.0 - 1.0 / (1.0 + tries / 3.0)))?;
                    let size = bytes.len() as u64;
                    Ok((bytes, size))
                })?;
            (bytes, fits)
        }
    };
    // Never hand back a bigger file in the same format and size.
    let bytes = if unchanged && bytes.len() as u64 >= original {
        s.bytes.clone()
    } else {
        bytes
    };
    let size = written_size(&bytes, format, (w, h));
    finish(out, &bytes, format, size, fits, &steps)
}

pub fn convert(
    src: &Path,
    options: &ConvertOptions,
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<Written> {
    if options.format == Format::Pdf {
        let steps = Steps::new(progress, cancel);
        let s = load(src)?;
        steps.at(0.3)?;
        let pdf = super::pdf::combine_sources(std::slice::from_ref(&s), PageSize::Image)?;
        return finish(out, &pdf, Format::Pdf, s.image.dimensions(), true, &steps);
    }
    let steps = Steps::new(progress, cancel);
    steps.at(0.0)?;
    can_write(options.format)?;
    let s = load(src)?;
    steps.at(0.3)?;
    let quality = if options.format.is_lossy() {
        options.quality
    } else {
        100
    };
    let bytes = encode_for(&s, &s.image, options.format, quality)?;
    finish(
        out,
        &bytes,
        options.format,
        s.image.dimensions(),
        true,
        &steps,
    )
}

pub fn resize(
    src: &Path,
    to: Resize,
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<Written> {
    let steps = Steps::new(progress, cancel);
    steps.at(0.0)?;
    let s = load(src)?;
    let format = same_format(&s)?;
    steps.at(0.2)?;
    let (w, h) = s.image.dimensions();
    let ((sw, sh), crop) = to.plan(w, h);
    if u64::from(sw) * u64::from(sh) > 400_000_000 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "That size is too large",
        ));
    }
    let mut picture = pixels::scale(&s.image, sw, sh);
    if let Some(rect) = crop {
        picture = pixels::crop(&picture, rect);
    }
    steps.at(0.6)?;
    let bytes = encode_for(&s, &picture, format, reencode_quality(&s))?;
    finish(out, &bytes, format, picture.dimensions(), true, &steps)
}

/// Turns or mirrors the picture as shown. A JPEG whose edges are whole blocks is turned without
/// re-encoding, so nothing is lost.
pub fn rotate(
    src: &Path,
    how: Rotation,
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<Written> {
    let steps = Steps::new(progress, cancel);
    steps.at(0.0)?;
    let bytes = std::fs::read(src)?;
    if kind_of(&bytes)? == Kind::Jpeg {
        let o = jpeg::orientation(&bytes).then(how.orientation());
        if let Some(turned) = jpeg::rotate_lossless(&bytes, o)? {
            let size = written_size(&turned, Format::Jpeg, (0, 0));
            return finish(out, &turned, Format::Jpeg, size, true, &steps);
        }
    }
    let s = load(src)?;
    let format = same_format(&s)?;
    steps.at(0.3)?;
    let picture = orient(s.image.clone(), how.orientation());
    let bytes = encode_for(&s, &picture, format, reencode_quality(&s))?;
    finish(out, &bytes, format, picture.dimensions(), true, &steps)
}

/// Drops EXIF (location, camera, dates), XMP, IPTC and comments, keeping the colour profile.
/// JPEG, PNG and WebP lose nothing else; other formats are rewritten. A photo that relied on its
/// EXIF orientation is turned upright first, so it doesn't fall over.
pub fn strip_metadata(
    src: &Path,
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<Written> {
    let steps = Steps::new(progress, cancel);
    steps.at(0.0)?;
    let bytes = std::fs::read(src)?;
    let kind = kind_of(&bytes)?;
    let stripped = match kind {
        Kind::Jpeg => {
            let o = jpeg::orientation(&bytes);
            if o == Orientation::Normal {
                Some(jpeg::markers::strip_metadata(&bytes)?)
            } else {
                // A turn that would split a block keeps a bare orientation tag instead.
                let stripped = match jpeg::rotate_lossless(&bytes, o)? {
                    Some(turned) => jpeg::markers::strip_metadata(&turned)?,
                    None => jpeg::markers::with_exif(
                        &jpeg::markers::strip_metadata(&bytes)?,
                        &exif::orientation_only(o as u16),
                    )?,
                };
                Some(stripped)
            }
        }
        Kind::Png | Kind::Webp if upright(&bytes, kind) => Some(match kind {
            Kind::Png => png::strip_metadata(&bytes)?,
            _ => webp::strip_metadata(&bytes)?,
        }),
        _ => None,
    };
    if let Some(stripped) = stripped {
        let format = Format::matching(kind).expect("a picture");
        let size = written_size(&stripped, format, (0, 0));
        return finish(out, &stripped, format, size, true, &steps);
    }
    let s = load(src)?;
    let format = same_format(&s)?;
    steps.at(0.3)?;
    let bytes = encode(
        &s.image,
        format,
        reencode_quality(&s),
        Meta {
            icc: s.icc.as_deref(),
            exif: None,
        },
    )?;
    finish(out, &bytes, format, s.image.dimensions(), true, &steps)
}

fn upright(bytes: &[u8], kind: Kind) -> bool {
    let exif = if kind == Kind::Png {
        png::exif(bytes)
    } else {
        webp::exif(bytes)
    };
    exif.and_then(|e| exif::orientation(&e))
        .is_none_or(|o| o == 1)
}

/// `picture` written as `format`, with the source's colour profile and its EXIF set upright and
/// sized to match.
fn encode_for(s: &Source, picture: &RgbaImage, format: Format, quality: u8) -> io::Result<Vec<u8>> {
    let exif = s.exif.clone().map(|mut e| {
        exif::normalise(&mut e, Some(picture.dimensions()));
        e
    });
    encode(
        picture,
        format,
        quality,
        Meta {
            icc: s.icc.as_deref(),
            exif: exif.as_deref(),
        },
    )
}

fn scaled(v: u32, s: f64) -> u32 {
    ((f64::from(v) * s).round() as u32).max(1)
}

fn same_format(s: &Source) -> io::Result<Format> {
    let format = Format::matching(s.kind).ok_or_else(source::unsupported)?;
    can_write(format)?;
    Ok(format)
}

fn can_write(format: Format) -> io::Result<()> {
    if format == Format::Heic && !wic::capabilities().heic_write {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "To save HEIC, install HEIF Image Extensions and HEVC Video Extensions from the Microsoft Store",
        ));
    }
    Ok(())
}

/// The quality a source is re-encoded at: what a JPEG was saved at, 90 for the other lossy
/// formats, and lossless for the rest.
fn reencode_quality(s: &Source) -> u8 {
    match s.kind {
        Kind::Jpeg => jpeg::quality(&s.bytes).unwrap_or(90).max(50),
        Kind::Webp | Kind::Heic | Kind::Avif => 90,
        _ => 100,
    }
}

/// The pixel size of a file just written, read back for the lossless paths.
fn written_size(bytes: &[u8], format: Format, fallback: (u32, u32)) -> (u32, u32) {
    let Some(f) = source::image_format(match format {
        Format::Jpeg => Kind::Jpeg,
        Format::Png => Kind::Png,
        Format::Webp => Kind::Webp,
        _ => return fallback,
    }) else {
        return fallback;
    };
    ImageReader::with_format(Cursor::new(bytes), f)
        .into_dimensions()
        .unwrap_or(fallback)
}

fn finish(
    out: &Path,
    bytes: &[u8],
    format: Format,
    (width, height): (u32, u32),
    fits: bool,
    steps: &Steps,
) -> io::Result<Written> {
    steps.check()?;
    std::fs::write(out, bytes)?;
    steps.at(1.0)?;
    Ok(Written {
        format,
        width,
        height,
        bytes: bytes.len() as u64,
        fits,
    })
}
