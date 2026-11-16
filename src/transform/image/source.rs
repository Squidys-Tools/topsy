//! Reading a picture for a Transform: its real format, its pixels turned upright, and the
//! colour profile and EXIF to carry into what is written.

use std::io::{self, Cursor};
use std::path::Path;

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, RgbaImage};
use windows::Win32::System::Com::COINIT_MULTITHREADED;

use crate::model::image_ops::Kind;
use crate::model::orientation::Orientation;
use crate::model::{heif, jpeg, png, webp};
use crate::win32::{wic, with_com};

pub struct Source {
    pub kind: Kind,
    pub bytes: Vec<u8>,
    /// Upright: the EXIF orientation is applied.
    pub image: RgbaImage,
    /// How the stored pixels were turned to make `image` upright.
    pub orientation: Orientation,
    pub icc: Option<Vec<u8>>,
    /// The TIFF structure inside an EXIF block, without the `Exif\0\0` prefix.
    pub exif: Option<Vec<u8>>,
}

impl Source {
    pub fn has_alpha(&self) -> bool {
        self.image.pixels().any(|p| p.0[3] < 255)
    }
}

pub fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "topsy can't open this kind of picture",
    )
}

pub fn kind_of(bytes: &[u8]) -> io::Result<Kind> {
    match Kind::sniff(bytes) {
        Some(Kind::Pdf) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "This is a PDF, not a picture",
        )),
        Some(kind) => Ok(kind),
        None => Err(unsupported()),
    }
}

pub fn image_format(kind: Kind) -> Option<ImageFormat> {
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

pub fn load(path: &Path) -> io::Result<Source> {
    let bytes = std::fs::read(path)?;
    let kind = kind_of(&bytes)?;
    let (image, orientation, icc) = match image_format(kind) {
        Some(format) => decode(&bytes, format)?,
        None => decode_wic(path, kind)?,
    };
    let exif = match kind {
        Kind::Jpeg => jpeg::markers::segments(&bytes)
            .ok()
            .and_then(|s| jpeg::markers::exif(&s)),
        Kind::Png => png::exif(&bytes),
        Kind::Webp => webp::exif(&bytes),
        Kind::Heic | Kind::Avif => heif::exif(&bytes),
        _ => None,
    };
    let icc = icc.or_else(|| match kind {
        Kind::Heic | Kind::Avif => heif::icc(&bytes),
        _ => None,
    });
    Ok(Source {
        kind,
        bytes,
        image: orient(image, orientation),
        orientation,
        icc,
        exif,
    })
}

fn decode(
    bytes: &[u8],
    format: ImageFormat,
) -> io::Result<(RgbaImage, Orientation, Option<Vec<u8>>)> {
    let bad = |e: image::ImageError| {
        crate::log(&format!("transform: decode: {e}"));
        match e {
            image::ImageError::Limits(_) => io::Error::other("This picture is too large to open"),
            image::ImageError::Unsupported(_) => unsupported(),
            _ => io::Error::new(
                io::ErrorKind::InvalidData,
                "This picture is damaged or incomplete",
            ),
        }
    };
    let mut decoder = ImageReader::with_format(Cursor::new(bytes), format)
        .into_decoder()
        .map_err(bad)?;
    let icc = decoder.icc_profile().ok().flatten();
    let orientation = decoder
        .orientation()
        .map(|o| Orientation::from_exif(u16::from(o.to_exif())))
        .unwrap_or(Orientation::Normal);
    let image = DynamicImage::from_decoder(decoder)
        .map_err(bad)?
        .into_rgba8();
    Ok((image, orientation, icc))
}

fn decode_wic(path: &Path, kind: Kind) -> io::Result<(RgbaImage, Orientation, Option<Vec<u8>>)> {
    let decoded = with_com(COINIT_MULTITHREADED, || wic::decode(path)).map_err(|e| {
        if e.kind() != io::ErrorKind::Unsupported {
            return e;
        }
        let message = match kind {
            Kind::Avif => {
                "To open AVIF pictures, install AV1 Video Extension from the Microsoft Store"
            }
            _ => "To open HEIC photos, install HEIF Image Extensions from the Microsoft Store",
        };
        io::Error::new(io::ErrorKind::Unsupported, message)
    })?;
    Ok((
        decoded.image,
        Orientation::from_exif(decoded.orientation),
        decoded.icc,
    ))
}

/// `image` turned by `o`.
pub fn orient(image: RgbaImage, o: Orientation) -> RgbaImage {
    use image::imageops;
    let (transpose, flip_h, flip_v) = o.parts();
    let mut image = if transpose { transposed(&image) } else { image };
    if flip_h {
        imageops::flip_horizontal_in_place(&mut image);
    }
    if flip_v {
        imageops::flip_vertical_in_place(&mut image);
    }
    image
}

fn transposed(image: &RgbaImage) -> RgbaImage {
    let (w, h) = image.dimensions();
    let src = image.as_raw();
    let mut out = vec![0u8; src.len()];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let (s, d) = ((y * w as usize + x) * 4, (x * h as usize + y) * 4);
            out[d..d + 4].copy_from_slice(&src[s..s + 4]);
        }
    }
    RgbaImage::from_raw(h, w, out).expect("same pixel count")
}
