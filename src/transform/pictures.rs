//! Images and PDFs: the panel's options in the image and PDF engines' terms, and each file sent
//! to the right one.

use std::cell::RefCell;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use super::engines::bytes;
use super::job::{Input, into, reserve};
use super::options::{IMAGE_TARGETS, Options, PDF_TARGETS, PERCENTS, SIDES};
use super::{image, pdf};
use crate::log::LogErr;
use crate::model::download::ModelStatus;
use crate::model::image_geometry::{LargestSide, Resize, Rotation};
use crate::model::image_ops::{
    CompressBy, CompressFormat, CompressOptions, ConvertOptions, Format, Kind as Sniffed,
};
use crate::model::pdf::{PAGE_DPI, PageFormat, PageSize, PdfCompress, PdfQuality};
use crate::model::transform::{Action, Kind, combined_paths, page_paths, result_paths};

/// What images and PDFs get.
pub struct Pictures {
    compress: CompressOptions,
    convert: Format,
    resize: Option<Resize>,
    rotation: Rotation,
    pdf: PdfCompress,
    pages: PageFormat,
    dpi: u32,
    page_size: PageSize,
}

fn at<T: Copy>(list: &[T], i: usize) -> T {
    list[i.min(list.len() - 1)]
}

/// Resize's size, or `None` while the exact width and height aren't both filled in.
pub fn resize(o: &Options) -> Option<Resize> {
    match o.resize_by {
        0 => (at(&SIDES[1..], o.resize_side).strip_suffix(" px")?)
            .parse()
            .ok()
            .map(Resize::LargestSide),
        1 => (at(&PERCENTS, o.percent).trim_end_matches('%'))
            .parse()
            .ok()
            .map(Resize::Percent),
        _ => {
            let n = |s: &str| {
                s.trim()
                    .parse::<u32>()
                    .ok()
                    .filter(|n| (1..=30_000).contains(n))
            };
            Some(Resize::Exact {
                width: n(&o.width)?,
                height: n(&o.height)?,
                fill: o.fit == 1,
            })
        }
    }
}

pub fn pictures(o: &Options) -> Pictures {
    Pictures {
        compress: CompressOptions {
            by: if o.by_size {
                CompressBy::FileSize
            } else {
                CompressBy::Quality
            },
            quality: (o.quality * 100.0).round() as u8,
            target_bytes: bytes(at(&IMAGE_TARGETS, o.target)),
            format: at(&CompressFormat::ALL, o.image_format),
            largest_side: at(&LargestSide::PRESETS, o.side),
        },
        convert: at(&Format::ALL, o.convert),
        resize: resize(o),
        rotation: at(&Rotation::ALL, o.rotation),
        pdf: if o.by_size {
            PdfCompress::FileSize(bytes(at(&PDF_TARGETS, o.target)))
        } else {
            PdfCompress::Quality(at(&PdfQuality::ALL, o.pdf_quality))
        },
        pages: at(&PageFormat::ALL, o.convert),
        dpi: at(&PAGE_DPI, o.dpi),
        page_size: at(&PageSize::ALL, o.page),
    }
}

fn sniff(path: &Path) -> Option<Sniffed> {
    let mut head = [0u8; 64];
    let n = fs::File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .ok()?;
    Sniffed::sniff(&head[..n])
}

/// Fetches ImageMagick and pdfium before the files when they will be needed, so the progress
/// line can say so. A JPEG turned or stripped losslessly needs neither.
pub fn prepare(
    action: Action,
    p: &Pictures,
    inputs: &[Input],
    progress: &dyn Fn(String, f32),
    cancel: &AtomicBool,
) -> io::Result<()> {
    let magick = inputs.iter().any(|i| {
        i.kind == Kind::Image
            && match action {
                Action::Compress | Action::Resize => true,
                Action::Convert => p.convert != Format::Pdf,
                Action::Rotate | Action::RemoveMetadata | Action::CombinePdf => {
                    sniff(&i.path) != Some(Sniffed::Jpeg)
                }
                _ => false,
            }
    });
    let ready = |t: &super::tools::Tool| t.status() == ModelStatus::Ready;
    if magick && !(ready(&image::MAGICK) && ready(&image::OXIPNG)) {
        image::fetch(&|f| progress("Getting ImageMagick…".into(), f), cancel)?;
    }
    let pages = action == Action::Convert && inputs.iter().any(|i| i.kind == Kind::Pdf);
    if pages && !ready(&pdf::PDFIUM) {
        pdf::fetch(&|f| progress("Getting the PDF renderer…".into(), f), cancel)?;
    }
    Ok(())
}

fn over_target() -> io::Error {
    io::Error::other("Couldn't get it under that size; try a larger one")
}

fn no_folder() -> io::Error {
    io::Error::other("No folder to save in")
}

fn paths(src: &Path, action: Action, ext: &str) -> io::Result<impl Iterator<Item = PathBuf>> {
    result_paths(src, action, ext).ok_or_else(no_folder)
}

/// Like `engines::beside`, for an image whose format is only settled by the engine (Compress
/// turns an opaque PNG into a JPEG): a result in another format is renamed to match.
fn written(
    src: &Path,
    action: Action,
    guess: &'static str,
    make: impl FnOnce(&Path) -> io::Result<image::Written>,
) -> io::Result<Vec<PathBuf>> {
    let mut format = None;
    let out = into(reserve(paths(src, action, guess)?)?, |out| {
        let written = make(out)?;
        if !written.fits {
            return Err(over_target());
        }
        format = Some(written.format);
        Ok(())
    })?;
    let ext = format.map_or(guess, Format::extension);
    if ext == guess {
        return Ok(vec![out]);
    }
    let to = reserve(paths(src, action, ext)?).inspect_err(|_| drop(fs::remove_file(&out)))?;
    fs::rename(&out, &to).inspect_err(|_| {
        fs::remove_file(&out).log_err();
        fs::remove_file(&to).log_err();
    })?;
    Ok(vec![to])
}

fn doesnt_apply(action: Action) -> io::Result<Vec<PathBuf>> {
    Err(io::Error::other(format!(
        "{} doesn't apply to this file",
        action.label()
    )))
}

pub fn image_one(
    action: Action,
    p: &Pictures,
    src: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<Vec<PathBuf>> {
    let info = image::probe(src)?;
    let kept = Format::matching(info.kind)
        .unwrap_or(Format::Png)
        .extension();
    match action {
        Action::Compress => {
            let guess = p.compress.format.resolve(info.kind, info.has_alpha);
            written(src, action, guess.extension(), |out| {
                image::compress(src, &p.compress, out, progress, cancel)
            })
        }
        Action::Convert => {
            let options = ConvertOptions {
                format: p.convert,
                ..ConvertOptions::default()
            };
            written(src, action, p.convert.extension(), |out| {
                image::convert(src, &options, out, progress, cancel)
            })
        }
        Action::Resize => {
            let to = p
                .resize
                .ok_or_else(|| io::Error::other("Enter a width and a height"))?;
            written(src, action, kept, |out| {
                image::resize(src, to, out, progress, cancel)
            })
        }
        Action::Rotate => written(src, action, kept, |out| {
            image::rotate(src, p.rotation, out, progress, cancel)
        }),
        Action::RemoveMetadata => written(src, action, kept, |out| {
            image::strip_metadata(src, out, progress, cancel)
        }),
        _ => doesnt_apply(action),
    }
}

pub fn pdf_one(
    action: Action,
    p: &Pictures,
    src: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<Vec<PathBuf>> {
    let pdf = |make: &dyn Fn(&Path) -> io::Result<()>| {
        Ok(vec![into(reserve(paths(src, action, "pdf")?)?, make)?])
    };
    match action {
        Action::Compress => pdf(
            &|out| match pdf::compress(src, p.pdf, out, progress, cancel)?.fits {
                true => Ok(()),
                false => Err(over_target()),
            },
        ),
        Action::Convert => to_images(src, p, progress, cancel),
        Action::Rotate => {
            pdf(&|out| pdf::rotate_pages(src, p.rotation, out, progress, cancel).map(drop))
        }
        Action::RemoveMetadata => {
            pdf(&|out| pdf::strip_metadata(src, out, progress, cancel).map(drop))
        }
        _ => doesnt_apply(action),
    }
}

/// A picture per page, `report (page 3).png`; none are left behind if one fails.
fn to_images(
    src: &Path,
    p: &Pictures,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<Vec<PathBuf>> {
    let made = RefCell::new(Vec::new());
    let out = |page: usize| {
        let path = reserve(page_paths(src, page, p.pages.extension()).ok_or_else(no_folder)?)?;
        made.borrow_mut().push(path.clone());
        Ok(path)
    };
    let done = pdf::to_images(src, p.pages, p.dpi, &out, progress, cancel);
    let made = made.into_inner();
    match done {
        Ok(_) => Ok(made),
        Err(err) => {
            for path in made {
                fs::remove_file(path).log_err();
            }
            Err(err)
        }
    }
}

/// Combine into PDF: `Combined.pdf` beside the first file, in the order given.
pub fn combine(
    p: &Pictures,
    inputs: &[Input],
    progress: &dyn Fn(String, f32),
    cancel: &AtomicBool,
) -> io::Result<Vec<PathBuf>> {
    let sources: Vec<PathBuf> = inputs.iter().map(|i| i.path.clone()).collect();
    let out = reserve(combined_paths(&sources, "Combined", "pdf").ok_or_else(no_folder)?)?;
    let step = |f: f32| progress("Combining…".into(), f);
    Ok(vec![into(out, |out| {
        pdf::combine(&sources, p.page_size, out, &step, cancel).map(drop)
    })?])
}
