//! The tools behind the image Transforms: ImageMagick for decoding, reshaping and encoding,
//! oxipng for squeezing the PNGs it writes. Both are fetched on first use.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder, ImageFormat, imageops};

use crate::transform::Steps;
use crate::transform::fetch::Pinned;
use crate::transform::tools::{self, Tool};
use model::magick::{self, Info, Metadata};

/// The official portable build (ImageMagick License). It reads HEIC and AVIF itself, through
/// libheif, so no Windows extension is needed, but has no HEVC encoder, so it can't write HEIC.
/// Its exes link the Visual C++ runtime (vcruntime140.dll, vcomp140.dll).
pub const MAGICK: Tool = Tool {
    archive: Pinned {
        label: "ImageMagick",
        host: "github.com",
        path: "/ImageMagick/ImageMagick/releases/download/7.1.2-35/ImageMagick-7.1.2-35-portable-Q16-x64.7z",
        size: 12_106_737,
        sha256: "e7c629f3530ccb9d4918a15f0e928254feda28e84a7fa794b25873d152c075c1",
        file: r"tools\ImageMagick-7.1.2-35-portable-Q16-x64.7z",
    },
    dir: "imagemagick-7.1.2-35",
    program: "magick.exe",
};

/// The archive holds eight identical 32 MB exes; `magick.exe` does every job, so only it, its
/// configuration and its licence are unpacked.
pub fn magick_files(name: &Path) -> bool {
    match name.extension().and_then(OsStr::to_str) {
        Some("exe") => name == Path::new("magick.exe"),
        _ => true,
    }
}

/// MIT. Built with MSVC, so it also links vcruntime140.dll.
pub const OXIPNG: Tool = Tool {
    archive: Pinned {
        label: "oxipng",
        host: "github.com",
        path: "/oxipng/oxipng/releases/download/v10.2.1/oxipng-10.2.1-x86_64-pc-windows-msvc.zip",
        size: 497_302,
        sha256: "7e940f83ee46874b73f53031f96a15834cb70b220af27391fb06fe7b4dd798e1",
        file: r"tools\oxipng-10.2.1-x86_64-pc-windows-msvc.zip",
    },
    dir: "oxipng-10.2.1",
    program: r"oxipng-10.2.1-x86_64-pc-windows-msvc\oxipng.exe",
};

/// The tools an operation needs, fetched up front.
pub struct Kit {
    pub magick: PathBuf,
    pub oxipng: Option<PathBuf>,
}

impl Kit {
    pub fn get(steps: &Steps, png: bool) -> io::Result<Kit> {
        let magick = steps.tool(&MAGICK, &magick_files)?;
        let oxipng = if png {
            Some(steps.tool(&OXIPNG, &|_| true)?)
        } else {
            None
        };
        Ok(Kit { magick, oxipng })
    }

    /// Runs ImageMagick, turning a failure into a message fit for a toast (the detail is logged).
    pub fn magick(&self, args: &[String], steps: &Steps) -> io::Result<String> {
        let mut out = String::new();
        let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        tools::run(&self.magick, &args, steps.cancel(), &mut |l| {
            out.push_str(l);
            out.push('\n');
        })
        .map_err(friendly)?;
        Ok(out)
    }

    pub fn info(&self, src: &Path, steps: &Steps) -> io::Result<Info> {
        let out = self.magick(&magick::info_args(&path(src)), steps)?;
        magick::parse_info(&out).ok_or_else(unreadable)
    }

    /// Whether the picture has no see-through pixel. Decodes it all.
    pub fn opaque(&self, src: &Path, steps: &Steps) -> io::Result<bool> {
        Ok(self.magick(&magick::opaque_args(&path(src)), steps)?.trim() == "True")
    }

    /// Squeezes a PNG in place, losslessly. Without oxipng (not fetched) it is left as it is.
    pub fn optimise_png(&self, png: &Path, metadata: Metadata, steps: &Steps) -> io::Result<()> {
        let Some(oxipng) = &self.oxipng else {
            return Ok(());
        };
        let args = magick::oxipng_args(&path(png), metadata);
        let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        tools::run(oxipng, &args, steps.cancel(), &mut |_| {}).map_err(friendly)
    }
}

pub fn path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

pub fn unreadable() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "This picture is damaged or can't be read",
    )
}

fn friendly(e: io::Error) -> io::Error {
    match e.kind() {
        io::ErrorKind::Interrupted | io::ErrorKind::NotFound => e,
        _ => io::Error::other("This picture couldn't be converted"),
    }
}

/// A Windows icon of a square PNG, with each of its sizes stored as a PNG.
pub fn pack_icon(square: &[u8]) -> io::Result<Vec<u8>> {
    let picture = image::load_from_memory_with_format(square, ImageFormat::Png)
        .map_err(|_| unreadable())?
        .into_rgba8();
    let pngs = magick::icon_sizes(picture.width())
        .into_iter()
        .map(|s| {
            let small = imageops::resize(&picture, s, s, imageops::FilterType::Lanczos3);
            let mut png = Vec::new();
            PngEncoder::new_with_quality(&mut png, CompressionType::Best, FilterType::Adaptive)
                .write_image(small.as_raw(), s, s, ExtendedColorType::Rgba8)
                .map(|()| (s, png))
        })
        .collect::<Result<Vec<_>, _>>();
    let packed = pngs.and_then(|pngs| {
        let frames = pngs
            .iter()
            .map(|(s, png)| IcoFrame::with_encoded(png, *s, *s, ExtendedColorType::Rgba8))
            .collect::<Result<Vec<_>, _>>()?;
        let mut out = Vec::new();
        IcoEncoder::new(&mut out).encode_images(&frames)?;
        Ok(out)
    });
    packed.map_err(|e| {
        crate::log(&format!("transform: icon: {e}"));
        io::Error::other("This picture couldn't be converted")
    })
}
