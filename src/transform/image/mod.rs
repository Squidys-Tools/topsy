//! The image Transforms: Compress, Convert, Resize, Rotate and Remove metadata. ImageMagick does
//! the pixels and oxipng squeezes the PNGs; both are fetched during the first operation that
//! needs them. A JPEG is turned or stripped without re-encoding where its edges allow it.

#[cfg(test)]
pub(crate) mod tests;
mod tools;

use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use image::{ImageDecoder, ImageFormat, ImageReader};

pub(super) use self::tools::Kit;
use self::tools::path;
pub use self::tools::{MAGICK, OXIPNG};
use super::Steps;
use crate::model::download::ModelStatus;
use crate::model::exif;
use crate::model::image_geometry::{Resize, Rotation, fit_within};
use crate::model::image_ops::{
    CompressBy, CompressOptions, ConvertOptions, Format, Kind, is_lossless_webp, reencode_quality,
};
use crate::model::jpeg;
use crate::model::magick::{self, Job, Metadata};
use crate::model::orientation::Orientation;
use crate::model::pdf::PageSize;
use crate::model::size_search::fit_to_size;

/// What [`probe`] learns without the full work of an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    pub kind: Kind,
    /// As shown, with the EXIF orientation applied. Zero for a PDF, and for HEIC or AVIF until
    /// ImageMagick has been fetched.
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
    let kind = Kind::sniff(&head(path)?).ok_or_else(unsupported)?;
    let mut info = ImageInfo {
        kind,
        width: 0,
        height: 0,
        has_alpha: false,
        bytes,
        pages: None,
    };
    if kind == Kind::Pdf {
        info.pages = Some(super::pdf::page_count(path)?);
        return Ok(info);
    }
    let (w, h, o) = match decoder_format(kind) {
        Some(format) => {
            let file = io::BufReader::new(std::fs::File::open(path)?);
            let mut d = ImageReader::with_format(file, format)
                .into_decoder()
                .map_err(|_| tools::unreadable())?;
            info.has_alpha = d.color_type().has_alpha();
            let o = d
                .orientation()
                .map(|o| Orientation::from_exif(u16::from(o.to_exif())))
                .unwrap_or(Orientation::Normal);
            let (w, h) = d.dimensions();
            (w, h, o)
        }
        None if MAGICK.status() == ModelStatus::Ready => {
            let never = AtomicBool::new(false);
            let steps = Steps::new(&|_| {}, &never);
            let kit = Kit {
                magick: MAGICK.program()?,
                oxipng: None,
            };
            let i = kit.info(path, &steps)?;
            (i.width, i.height, i.orientation)
        }
        None => (0, 0, Orientation::Normal),
    };
    (info.width, info.height) = if o.swaps_axes() { (h, w) } else { (w, h) };
    Ok(info)
}

pub fn compress(
    src: &Path,
    options: &CompressOptions,
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<Written> {
    let steps = Steps::new(progress, cancel);
    let kind = picture_kind(src)?;
    let may_be_png = [true, false]
        .iter()
        .any(|&a| options.format.resolve(kind, a) == Format::Png);
    let kit = Kit::get(&steps, may_be_png)?;
    steps.at(0.0)?;
    let (w0, h0) = kit.info(src, &steps)?.upright();
    let has_alpha = kind != Kind::Jpeg
        && options.format.resolve(kind, true) != options.format.resolve(kind, false)
        && !kit.opaque(src, &steps)?;
    let format = options.format.resolve(kind, has_alpha);
    let (w, h) = options.largest_side.fit(w0, h0);
    let size = ((w, h) != (w0, h0)).then_some((w, h));
    let unchanged = Format::matching(kind) == Some(format) && size.is_none();
    let original = std::fs::metadata(src)?.len();
    steps.at(0.1)?;
    let job = |quality: u8, size: Option<(u32, u32)>| Job {
        size,
        quality: Some(quality),
        ..Job::new(format)
    };
    let (dims, fits) = match options.by {
        CompressBy::Quality => {
            run(&kit, &steps, src, &job(options.quality, size), out)?;
            ((w, h), true)
        }
        CompressBy::FileSize if unchanged && original <= options.target_bytes => {
            std::fs::copy(src, out)?;
            ((w, h), true)
        }
        CompressBy::FileSize => {
            let scratch = Scratch::new(format.extension());
            let mut tries = 0.0f32;
            let ((bytes, dims), _, fits) = fit_to_size(options.target_bytes, |a| {
                let dims = if a.scale >= 1.0 {
                    (w, h)
                } else {
                    fit_within(w, h, scaled(w.max(h), a.scale))
                };
                let size = (dims != (w0, h0)).then_some(dims);
                run(&kit, &steps, src, &job(a.quality, size), &scratch.0)?;
                let bytes = std::fs::read(&scratch.0)?;
                tries += 1.0;
                steps.at(0.1 + 0.8 * (1.0 - 1.0 / (1.0 + tries / 3.0)))?;
                let len = bytes.len() as u64;
                io::Result::Ok(((bytes, dims), len))
            })?;
            std::fs::write(out, bytes)?;
            (dims, fits)
        }
    };
    if format == Format::Png {
        kit.optimise_png(out, Metadata::Keep, &steps)?;
    }
    // Never hand back a bigger file in the same format and size.
    if unchanged && std::fs::metadata(out)?.len() >= original {
        std::fs::copy(src, out)?;
    }
    finish(out, format, dims, fits, &steps)
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
        let info = probe(src)?;
        super::pdf::combine(&[src.to_path_buf()], PageSize::Image, out, progress, cancel)?;
        return finish(out, Format::Pdf, (info.width, info.height), true, &steps);
    }
    let steps = Steps::new(progress, cancel);
    picture_kind(src)?;
    let kit = Kit::get(&steps, options.format == Format::Png)?;
    steps.at(0.0)?;
    let (w, h) = kit.info(src, &steps)?.upright();
    steps.at(0.2)?;
    let job = Job {
        quality: options.format.is_lossy().then_some(options.quality),
        ..Job::new(options.format)
    };
    run(&kit, &steps, src, &job, out)?;
    if options.format == Format::Png {
        kit.optimise_png(out, Metadata::Keep, &steps)?;
    }
    let dims = if options.format == Format::Ico {
        let (iw, ih) = fit_within(w, h, 256);
        (iw.max(ih), iw.max(ih))
    } else {
        (w, h)
    };
    finish(out, options.format, dims, true, &steps)
}

pub fn resize(
    src: &Path,
    to: Resize,
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<Written> {
    let steps = Steps::new(progress, cancel);
    let (kind, format) = kept(src)?;
    let kit = Kit::get(&steps, format == Format::Png)?;
    steps.at(0.0)?;
    let (w, h) = kit.info(src, &steps)?.upright();
    let ((sw, sh), crop) = to.plan(w, h);
    if u64::from(sw) * u64::from(sh) > 400_000_000 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "That size is too large",
        ));
    }
    steps.at(0.2)?;
    let job = Job {
        size: Some((sw, sh)),
        crop,
        ..same_format_job(src, kind, format)?
    };
    run(&kit, &steps, src, &job, out)?;
    if format == Format::Png {
        kit.optimise_png(out, Metadata::Keep, &steps)?;
    }
    let dims = crop.map_or((sw, sh), |(_, _, cw, ch)| (cw, ch));
    finish(out, format, dims, true, &steps)
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
    let (kind, format) = kept(src)?;
    if kind == Kind::Jpeg {
        let bytes = std::fs::read(src)?;
        let o = jpeg::orientation(&bytes).then(how.orientation());
        if let Some(turned) = jpeg::rotate_lossless(&bytes, o)? {
            return finish_bytes(out, &turned, Format::Jpeg, &steps);
        }
    }
    let kit = Kit::get(&steps, format == Format::Png)?;
    steps.at(0.0)?;
    let (w, h) = kit.info(src, &steps)?.upright();
    steps.at(0.2)?;
    let job = Job {
        turn: Some(how),
        ..same_format_job(src, kind, format)?
    };
    run(&kit, &steps, src, &job, out)?;
    if format == Format::Png {
        kit.optimise_png(out, Metadata::Keep, &steps)?;
    }
    let dims = if how.orientation().swaps_axes() {
        (h, w)
    } else {
        (w, h)
    };
    finish(out, format, dims, true, &steps)
}

/// Drops EXIF (location, camera, dates), XMP, IPTC and comments, keeping the colour profile. A
/// photo that relied on its EXIF orientation is turned upright first, so it doesn't fall over.
/// A JPEG loses nothing else; other formats are rewritten (lossless ones losslessly).
pub fn strip_metadata(
    src: &Path,
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<Written> {
    let steps = Steps::new(progress, cancel);
    let (kind, format) = kept(src)?;
    if kind == Kind::Jpeg {
        let bytes = std::fs::read(src)?;
        let o = jpeg::orientation(&bytes);
        let stripped = if o == Orientation::Normal {
            jpeg::markers::strip_metadata(&bytes)?
        } else {
            // A turn that would split a block keeps a bare orientation tag instead.
            match jpeg::rotate_lossless(&bytes, o)? {
                Some(turned) => jpeg::markers::strip_metadata(&turned)?,
                None => jpeg::markers::with_exif(
                    &jpeg::markers::strip_metadata(&bytes)?,
                    &exif::orientation_only(o as u16),
                )?,
            }
        };
        return finish_bytes(out, &stripped, Format::Jpeg, &steps);
    }
    let kit = Kit::get(&steps, format == Format::Png)?;
    steps.at(0.0)?;
    let (w, h) = kit.info(src, &steps)?.upright();
    steps.at(0.2)?;
    let job = Job {
        metadata: Metadata::Remove,
        ..same_format_job(src, kind, format)?
    };
    run(&kit, &steps, src, &job, out)?;
    if format == Format::Png {
        kit.optimise_png(out, Metadata::Remove, &steps)?;
    }
    finish(out, format, (w, h), true, &steps)
}

/// Rewriting in the picture's own format, at the quality it was saved at where that is known.
fn same_format_job(src: &Path, kind: Kind, format: Format) -> io::Result<Job> {
    Ok(Job {
        quality: reencode_quality(kind),
        lossless: kind == Kind::Webp && is_lossless_webp(&head(src)?),
        ..Job::new(format)
    })
}

fn run(kit: &Kit, steps: &Steps, src: &Path, job: &Job, out: &Path) -> io::Result<()> {
    if job.format != Format::Ico {
        return kit
            .magick(&magick::args(job, &path(src), &path(out)), steps)
            .map(drop);
    }
    let square = Scratch::new("png");
    kit.magick(&magick::args(job, &path(src), &path(&square.0)), steps)?;
    std::fs::write(out, tools::pack_icon(&std::fs::read(&square.0)?)?)
}

/// The first bytes of a file, enough to tell its format.
fn head(path: &Path) -> io::Result<Vec<u8>> {
    let mut head = Vec::with_capacity(64);
    std::fs::File::open(path)?.take(64).read_to_end(&mut head)?;
    Ok(head)
}

fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "topsy can't open this kind of picture",
    )
}

fn picture_kind(src: &Path) -> io::Result<Kind> {
    match Kind::sniff(&head(src)?) {
        Some(Kind::Pdf) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "This is a PDF, not a picture",
        )),
        Some(kind) => Ok(kind),
        None => Err(unsupported()),
    }
}

/// The picture's kind and the format an operation that keeps it writes.
fn kept(src: &Path) -> io::Result<(Kind, Format)> {
    let kind = picture_kind(src)?;
    Ok((kind, Format::kept(kind).ok_or_else(unsupported)?))
}

fn decoder_format(kind: Kind) -> Option<ImageFormat> {
    Some(match kind {
        Kind::Jpeg => ImageFormat::Jpeg,
        Kind::Png => ImageFormat::Png,
        Kind::Webp => ImageFormat::WebP,
        Kind::Gif => ImageFormat::Gif,
        Kind::Bmp => ImageFormat::Bmp,
        Kind::Tiff => ImageFormat::Tiff,
        Kind::Ico => ImageFormat::Ico,
        Kind::Heic | Kind::Avif | Kind::Pdf => return None,
    })
}

fn scaled(v: u32, s: f64) -> u32 {
    ((f64::from(v) * s).round() as u32).max(1)
}

/// A file in the temp folder for tries, deleted when dropped.
pub(super) struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(extension: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let name = format!("topsy-{}-{n}.{extension}", std::process::id());
        Self(std::env::temp_dir().join(name))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn finish_bytes(out: &Path, bytes: &[u8], format: Format, steps: &Steps) -> io::Result<Written> {
    steps.check()?;
    std::fs::write(out, bytes)?;
    let dims = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Jpeg)
        .into_dimensions()
        .unwrap_or((0, 0));
    finish(out, format, dims, true, steps)
}

fn finish(
    out: &Path,
    format: Format,
    (width, height): (u32, u32),
    fits: bool,
    steps: &Steps,
) -> io::Result<Written> {
    steps.at(1.0)?;
    Ok(Written {
        format,
        width,
        height,
        bytes: std::fs::metadata(out)?.len(),
        fits,
    })
}
