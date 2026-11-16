//! The PDF Transforms: Combine into PDF, Compress PDF, PDF to images, and turning pages or
//! dropping a PDF's metadata.

mod combine;
mod compress;
#[cfg(test)]
mod tests;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use lopdf::{Document, Object};
use windows::Win32::System::Com::COINIT_MULTITHREADED;

use super::Steps;
use super::image::encode::{Meta, encode};
use super::image::source::{Source, load as load_picture};
use crate::model::image_geometry::Rotation;
use crate::model::image_ops::{Format, Kind};
use crate::model::pdf::{LADDER, PageFormat, PageSize, PdfCompress};
use crate::win32::{pdf_render, with_com};

pub use combine::Builder;

/// What Compress PDF wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdfWritten {
    pub bytes: u64,
    /// The result would have been bigger, so the original's bytes were kept.
    pub kept_original: bool,
    /// False when a size target couldn't be reached; this is the smallest result.
    pub fits: bool,
}

pub(crate) fn unreadable() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "This PDF couldn't be read")
}

/// Parses a PDF for rewriting. Protected PDFs are refused, since rewriting one would drop its
/// protection.
pub(crate) fn load(bytes: &[u8]) -> io::Result<Document> {
    let protected = || {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "This PDF is password-protected",
        )
    };
    let doc = Document::load_mem(bytes).map_err(|e| {
        crate::log(&format!("transform: pdf load: {e}"));
        if bytes.windows(8).any(|w| w == b"/Encrypt") {
            protected()
        } else {
            unreadable()
        }
    })?;
    if doc.trailer.has(b"Encrypt") || doc.was_encrypted() {
        return Err(protected());
    }
    Ok(doc)
}

pub(crate) fn save(doc: &mut Document) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    doc.save_modern(&mut out).map_err(|e| {
        crate::log(&format!("transform: pdf save: {e}"));
        io::Error::other("topsy couldn't write the PDF")
    })?;
    Ok(out)
}

/// The page count, from Windows' PDF reader (quick even for a large file).
pub fn page_count(path: &Path) -> io::Result<u32> {
    with_com(COINIT_MULTITHREADED, || pdf_render::open(path)?.pages())
}

/// One PDF from pictures and PDFs, in order: a page for each picture, every page of each PDF.
/// Returns the page count.
pub fn combine(
    srcs: &[PathBuf],
    size: PageSize,
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<u32> {
    let steps = Steps::new(progress, cancel);
    let mut builder = Builder::new();
    for (i, src) in srcs.iter().enumerate() {
        steps.at(0.9 * i as f32 / srcs.len().max(1) as f32)?;
        let head = std::fs::read(src)?;
        if Kind::sniff(&head) == Some(Kind::Pdf) {
            builder.add_pdf(&head)?;
        } else {
            builder.add_picture(&load_picture(src)?, size)?;
        }
    }
    steps.check()?;
    let (bytes, pages) = builder.finish()?;
    std::fs::write(out, bytes)?;
    steps.at(1.0)?;
    Ok(pages)
}

/// A PDF of `sources`, a page each.
pub(crate) fn combine_sources(sources: &[Source], size: PageSize) -> io::Result<Vec<u8>> {
    let mut builder = Builder::new();
    for s in sources {
        builder.add_picture(s, size)?;
    }
    builder.finish().map(|(bytes, _)| bytes)
}

pub fn compress(
    src: &Path,
    how: PdfCompress,
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<PdfWritten> {
    let steps = Steps::new(progress, cancel);
    steps.at(0.0)?;
    let original = std::fs::read(src)?;
    let presets = match how {
        PdfCompress::Quality(q) => vec![q.preset()],
        PdfCompress::FileSize(_) => LADDER.to_vec(),
    };
    let target = match how {
        PdfCompress::Quality(_) => u64::MAX,
        PdfCompress::FileSize(t) => t,
    };
    let mut best: Option<Vec<u8>> = None;
    for (i, preset) in presets.iter().enumerate() {
        let span = 1.0 / presets.len() as f32;
        let mut doc = load(&original)?;
        compress::shrink(
            &mut doc,
            *preset,
            &steps,
            (i as f32 * span, (i as f32 + 0.9) * span),
        )?;
        let bytes = save(&mut doc)?;
        let done = bytes.len() as u64 <= target;
        if best.as_ref().is_none_or(|b| bytes.len() < b.len()) {
            best = Some(bytes);
        }
        if done {
            break;
        }
    }
    let best = best.unwrap_or_default();
    let kept_original = best.is_empty() || best.len() >= original.len();
    let bytes = if kept_original { &original } else { &best };
    steps.check()?;
    std::fs::write(out, bytes)?;
    steps.at(1.0)?;
    Ok(PdfWritten {
        bytes: bytes.len() as u64,
        kept_original,
        fits: bytes.len() as u64 <= target,
    })
}

/// Each page as a picture at `dpi`, written to `out(page index)`. Returns how many were written.
pub fn to_images(
    src: &Path,
    format: PageFormat,
    dpi: u32,
    out: &dyn Fn(usize) -> io::Result<PathBuf>,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<usize> {
    let steps = Steps::new(progress, cancel);
    steps.at(0.0)?;
    with_com(COINIT_MULTITHREADED, || {
        let doc = pdf_render::open(src)?;
        let pages = doc.pages()? as usize;
        for i in 0..pages {
            steps.at(i as f32 / pages.max(1) as f32)?;
            let page = doc.render(i as u32, dpi.clamp(36, 1200))?;
            let (format, quality) = match format {
                PageFormat::Png => (Format::Png, 100),
                PageFormat::Jpeg => (Format::Jpeg, 90),
            };
            let bytes = encode(&page, format, quality, Meta::default())?;
            steps.check()?;
            std::fs::write(out(i)?, bytes)?;
        }
        steps.at(1.0)?;
        Ok(pages)
    })
}

/// Turns every page. PDF pages can turn but not mirror.
pub fn rotate_pages(
    src: &Path,
    how: Rotation,
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<u32> {
    let degrees = match how {
        Rotation::Right => 90,
        Rotation::Half => 180,
        Rotation::Left => 270,
        Rotation::FlipHorizontal | Rotation::FlipVertical => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "PDF pages can be turned but not mirrored",
            ));
        }
    };
    let steps = Steps::new(progress, cancel);
    steps.at(0.0)?;
    let mut doc = load(&std::fs::read(src)?)?;
    let pages: Vec<_> = doc.page_iter().collect();
    for &page in &pages {
        let now = combine::inherited(&doc, page, b"Rotate")
            .and_then(|r| r.as_i64().ok())
            .unwrap_or(0);
        let dict = doc.get_dictionary_mut(page).map_err(|_| unreadable())?;
        dict.set("Rotate", (now + degrees).rem_euclid(360));
    }
    write(&mut doc, out, &steps)?;
    Ok(pages.len() as u32)
}

/// Drops the document information (title, author, the app that made it, dates) and its XMP.
pub fn strip_metadata(
    src: &Path,
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<u64> {
    let steps = Steps::new(progress, cancel);
    steps.at(0.0)?;
    let mut doc = load(&std::fs::read(src)?)?;
    doc.trailer.remove(b"Info");
    let catalog = doc.catalog_mut().map_err(|_| unreadable())?;
    catalog.remove(b"Metadata");
    catalog.remove(b"PieceInfo");
    for id in doc.page_iter().collect::<Vec<_>>() {
        if let Ok(Object::Dictionary(page)) = doc.get_object_mut(id) {
            page.remove(b"Metadata");
            page.remove(b"PieceInfo");
        }
    }
    doc.prune_objects();
    write(&mut doc, out, &steps)
}

fn write(doc: &mut Document, out: &Path, steps: &Steps) -> io::Result<u64> {
    let bytes = save(doc)?;
    steps.check()?;
    std::fs::write(out, &bytes)?;
    steps.at(1.0)?;
    Ok(bytes.len() as u64)
}
