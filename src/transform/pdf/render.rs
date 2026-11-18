//! PDF to images: pdfium draws each page, the `image` crate writes it.

use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};
use pdfium_render::prelude::{PdfRenderConfig, Pdfium, PdfiumError, PdfiumInternalError};

use crate::log::LogErr;
use crate::model::pdf::{PageFormat, fit_square};
use crate::transform::Steps;
use crate::transform::fetch::Pinned;
use crate::transform::tools::Tool;

/// pdfium as Chromium builds it, from bblanchon/pdfium-binaries (BSD-3-Clause and Apache-2.0),
/// matching the `pdfium_8125` API that pdfium-render is built against.
pub const PDFIUM: Tool = Tool {
    archive: Pinned {
        label: "pdfium",
        host: "github.com",
        path: "/bblanchon/pdfium-binaries/releases/download/chromium/8125/pdfium-win-x64.tgz",
        size: 3_904_987,
        sha256: "9a964261760cff52570aabf038ab346d3a5034dd8b30d0d5fe8861235d356b8b",
        file: r"tools\pdfium-win-x64-8125.tgz",
    },
    dir: "pdfium-8125",
    program: r"bin\pdfium.dll",
};

/// The largest side a page is drawn at, whatever its size and the dpi asked for.
const MAX_SIDE: i32 = 16_384;

/// pdfium-render binds the library once per process; the DLL then stays loaded.
static BOUND: Mutex<Option<&'static Pdfium>> = Mutex::new(None);

fn pdfium(dll: &Path) -> io::Result<&'static Pdfium> {
    let mut bound = BOUND.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = *bound {
        return Ok(p);
    }
    let bindings = Pdfium::bind_to_library(dll).map_err(|e| {
        crate::log(&format!("transform: pdfium: {e:?}"));
        io::Error::other("pdfium couldn't be loaded")
    })?;
    let p: &'static Pdfium = Box::leak(Box::new(Pdfium::new(bindings)));
    *bound = Some(p);
    Ok(p)
}

fn pdf_error(e: PdfiumError) -> io::Error {
    crate::log(&format!("transform: pdfium: {e:?}"));
    match e {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "This PDF is password-protected",
            )
        }
        _ => super::unreadable(),
    }
}

fn keep(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "dll") || p.starts_with("LICENSE")
}

/// Fetches pdfium if it isn't here yet.
pub fn fetch(progress: &dyn Fn(f32), cancel: &AtomicBool) -> io::Result<()> {
    PDFIUM.get_with(&keep, progress, cancel).map(drop)
}

/// Gets pdfium for page pictures unless it is here. Blocking, so a worker thread asks; a failure
/// only leaves the PDF with its icon.
pub fn ensure_renderer() {
    if PDFIUM.status() == crate::transform::fetch::Status::Missing {
        fetch(&|_| {}, &AtomicBool::new(false)).log_err();
    }
}

/// The first page drawn to fit `side` pixels square, as the BGRA pixels GPUI wants. `None` when
/// pdfium isn't here (this never downloads it) or the PDF won't open or draw.
pub fn first_page(path: &Path, side: i32) -> Option<image::RgbaImage> {
    let dll = PDFIUM.program().ok().filter(|p| p.is_file())?;
    let doc = pdfium(&dll).ok()?.load_pdf_from_file(path, None).ok()?;
    let page = doc.pages().get(0).ok()?;
    let (w, h) = fit_square((page.width().value, page.height().value), side);
    let config = PdfRenderConfig::new()
        .set_target_size(w, h)
        .render_form_data(true);
    let bitmap = page.render_with_config(&config).ok()?;
    image::RgbaImage::from_raw(
        bitmap.width().try_into().ok()?,
        bitmap.height().try_into().ok()?,
        bitmap.as_raw_bytes(),
    )
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
    let dll = steps.tool(&PDFIUM, &keep)?;
    steps.at(0.0)?;
    let doc = pdfium(&dll)?
        .load_pdf_from_file(src, None)
        .map_err(pdf_error)?;
    let pages = doc.pages();
    let count = pages.len().max(0) as usize;
    let config = PdfRenderConfig::new()
        .scale_page_by_factor(dpi.clamp(36, 1200) as f32 / 72.0)
        .set_maximum_width(MAX_SIDE)
        .set_maximum_height(MAX_SIDE)
        .render_form_data(true);
    for (i, page) in pages.iter().enumerate() {
        steps.at(i as f32 / count.max(1) as f32)?;
        let picture = page
            .render_with_config(&config)
            .and_then(|b| b.as_image())
            .map_err(pdf_error)?
            .into_rgb8();
        steps.check()?;
        let file = BufWriter::new(std::fs::File::create(out(i)?)?);
        let (w, h) = picture.dimensions();
        let written = match format {
            PageFormat::Png => {
                PngEncoder::new_with_quality(file, CompressionType::Best, FilterType::Adaptive)
                    .write_image(picture.as_raw(), w, h, ExtendedColorType::Rgb8)
            }
            PageFormat::Jpeg => JpegEncoder::new_with_quality(file, 90).write_image(
                picture.as_raw(),
                w,
                h,
                ExtendedColorType::Rgb8,
            ),
        };
        written.map_err(io::Error::other)?;
    }
    steps.at(1.0)?;
    Ok(count)
}
