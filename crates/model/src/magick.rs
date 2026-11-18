//! The command lines the image Transforms hand to ImageMagick and oxipng, built from their
//! options, and what is read back from `magick identify`.

use super::image_geometry::{Crop, Rotation};
use super::image_ops::{Format, png_colors};
use super::orientation::Orientation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metadata {
    Keep,
    /// Everything but the colour profile: EXIF (location, camera, dates), XMP, IPTC, comments.
    Remove,
}

/// One ImageMagick run: read the first frame, turn it upright, reshape it, write it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Job {
    pub turn: Option<Rotation>,
    /// Exactly this size, after turning upright.
    pub size: Option<(u32, u32)>,
    /// Taken from the scaled picture.
    pub crop: Option<Crop>,
    pub format: Format,
    /// `None` keeps a JPEG's own quality (ImageMagick estimates it) and the encoder's default
    /// otherwise. For PNG it picks a palette (see `png_colors`).
    pub quality: Option<u8>,
    pub metadata: Metadata,
    /// Lossless WebP, for a source that was lossless WebP.
    pub lossless: bool,
}

impl Job {
    pub fn new(format: Format) -> Self {
        Self {
            turn: None,
            size: None,
            crop: None,
            format,
            quality: None,
            metadata: Metadata::Keep,
            lossless: false,
        }
    }
}

/// The sizes Windows asks an icon for; ICO holds at most 256 px.
const ICON_SIZES: [u32; 7] = [256, 128, 64, 48, 32, 24, 16];

/// The sizes an icon drawn `side` px square carries: every standard one up to its own, and 16
/// at least.
pub fn icon_sizes(side: u32) -> Vec<u32> {
    let top = side.clamp(16, 256);
    ICON_SIZES.into_iter().filter(|&s| s <= top).collect()
}

/// `magick` arguments for `job`, reading `src` and writing `out`. A frame suffix and a format
/// prefix pin what is read and written, whatever the names say; ImageMagick only parses the
/// last `[...]` of a name, so brackets in a file name are safe.
pub fn args(job: &Job, src: &str, out: &str) -> Vec<String> {
    let mut a: Vec<String> = vec![format!("{src}[0]"), "-auto-orient".into()];
    let mut push = |items: &[&str]| a.extend(items.iter().map(|s| s.to_string()));
    match job.turn {
        Some(Rotation::Left) => push(&["-rotate", "-90"]),
        Some(Rotation::Right) => push(&["-rotate", "90"]),
        Some(Rotation::Half) => push(&["-rotate", "180"]),
        Some(Rotation::FlipHorizontal) => push(&["-flop"]),
        Some(Rotation::FlipVertical) => push(&["-flip"]),
        None => {}
    }
    if let Some((w, h)) = job.size {
        push(&["-resize", &format!("{w}x{h}!")]);
    }
    if let Some((x, y, w, h)) = job.crop {
        push(&["-crop", &format!("{w}x{h}+{x}+{y}"), "+repage"]);
    }
    if job.metadata == Metadata::Remove {
        push(&["+profile", "!icc,*", "+set", "comment"]);
    }
    if !job.format.keeps_alpha() {
        push(&["-background", "white", "-alpha", "remove", "-alpha", "off"]);
    }
    let quality = job.quality.map(|q| q.clamp(1, 100));
    match job.format {
        Format::Png => {
            if let Some(n) = quality.and_then(png_colors) {
                push(&["-dither", "FloydSteinberg", "-colors", &n.to_string()]);
            }
        }
        Format::Webp if job.lossless => push(&["-define", "webp:lossless=true"]),
        Format::Tiff => push(&["-compress", "LZW"]),
        Format::Ico => push(&[
            "-resize",
            "256x256>",
            "-background",
            "none",
            "-gravity",
            "center",
            "-extent",
            "%[fx:max(w,h)]x%[fx:max(w,h)]",
        ]),
        _ => {}
    }
    if job.format.is_lossy()
        && let Some(q) = quality
    {
        push(&["-quality", &q.to_string()]);
    }
    a.push(format!("{}:{out}", magick_format(job.format)));
    a
}

fn magick_format(f: Format) -> &'static str {
    match f {
        Format::Jpeg => "JPEG",
        Format::Png => "PNG",
        Format::Webp => "WEBP",
        Format::Avif => "AVIF",
        Format::Tiff => "TIFF",
        Format::Bmp => "BMP",
        Format::Gif => "GIF",
        // Its ICO writer stores every size as an uncompressed bitmap, so ImageMagick only draws
        // the square and topsy packs the sizes as PNGs.
        Format::Ico => "PNG32",
        // HEVC is encoded by FFmpeg from this, and packed by `model::heif`.
        Format::Heic => "PNG32",
        Format::Pdf => "PDF",
    }
}

/// oxipng arguments that shrink the PNG at `path` in place without changing a pixel. ImageMagick
/// also writes every EXIF field as a text chunk; those go either way, the EXIF block itself
/// (eXIf) only when metadata is removed. The colour profile always stays.
pub fn oxipng_args(path: &str, metadata: Metadata) -> Vec<String> {
    let strip = match metadata {
        Metadata::Keep => "tEXt,zTXt,iTXt,orNT,bKGD",
        Metadata::Remove => "safe",
    };
    ["-q", "--fast", "-o", "2", "--strip", strip, path]
        .map(String::from)
        .to_vec()
}

/// Enough to plan a transform without decoding the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Info {
    /// As stored, before the orientation is applied.
    pub width: u32,
    pub height: u32,
    pub orientation: Orientation,
}

impl Info {
    /// The size as shown.
    pub fn upright(&self) -> (u32, u32) {
        if self.orientation.swaps_axes() {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }
}

/// What `INFO_FORMAT` prints.
pub const INFO_FORMAT: &str = "%w %h %[orientation]\\n";

pub fn info_args(src: &str) -> Vec<String> {
    ["identify", "-ping", "-format", INFO_FORMAT]
        .map(String::from)
        .into_iter()
        .chain([format!("{src}[0]")])
        .collect()
}

pub fn parse_info(line: &str) -> Option<Info> {
    let mut parts = line.split_whitespace();
    let width = parts.next()?.parse().ok()?;
    let height = parts.next()?.parse().ok()?;
    let orientation = match parts.next().unwrap_or("Undefined") {
        "TopRight" => Orientation::FlipHorizontal,
        "BottomRight" => Orientation::Rotate180,
        "BottomLeft" => Orientation::FlipVertical,
        "LeftTop" => Orientation::Transpose,
        "RightTop" => Orientation::Rotate90,
        "RightBottom" => Orientation::Transverse,
        "LeftBottom" => Orientation::Rotate270,
        _ => Orientation::Normal,
    };
    Some(Info {
        width,
        height,
        orientation,
    })
}

/// Arguments that print `True` when the picture has no see-through pixel. Decodes it all.
pub fn opaque_args(src: &str) -> Vec<String> {
    ["identify", "-format", "%[opaque]"]
        .map(String::from)
        .into_iter()
        .chain([format!("{src}[0]")])
        .collect()
}

/// Re-encodes an image taken out of a PDF as a JPEG of `n` components (1 grey, 3 RGB, 4 CMYK)
/// at exactly `size`, with nothing but the pixels.
pub fn pdf_image_args(src: &str, n: u8, size: (u32, u32), quality: u8, out: &str) -> Vec<String> {
    let kind = match n {
        1 => "Grayscale",
        4 => "ColorSeparation",
        _ => "TrueColor",
    };
    [
        src,
        "-resize",
        &format!("{}x{}!", size.0, size.1),
        "-strip",
        "-type",
        kind,
        "-quality",
        &quality.to_string(),
        &format!("JPEG:{out}"),
    ]
    .map(String::from)
    .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(a: &[String]) -> String {
        a.join(" ")
    }

    #[test]
    fn a_compress_scales_and_sets_quality_and_keeps_metadata() {
        let job = Job {
            size: Some((1920, 1440)),
            quality: Some(75),
            ..Job::new(Format::Jpeg)
        };
        assert_eq!(
            line(&args(&job, r"C:\a [1].heic", r"C:\o.jpg")),
            r"C:\a [1].heic[0] -auto-orient -resize 1920x1440! -background white -alpha remove -alpha off -quality 75 JPEG:C:\o.jpg"
        );
    }

    #[test]
    fn same_format_rewrites_leave_quality_to_imagemagick() {
        let job = Job {
            turn: Some(Rotation::Left),
            ..Job::new(Format::Jpeg)
        };
        let a = line(&args(&job, "a.jpg", "b.jpg"));
        assert!(a.contains("-rotate -90") && !a.contains("-quality"), "{a}");
        let flip = Job {
            turn: Some(Rotation::FlipHorizontal),
            ..Job::new(Format::Png)
        };
        assert!(line(&args(&flip, "a", "b")).contains("-flop"));
    }

    #[test]
    fn removing_metadata_keeps_the_colour_profile() {
        let job = Job {
            metadata: Metadata::Remove,
            lossless: true,
            ..Job::new(Format::Webp)
        };
        let a = line(&args(&job, "a.webp", "b.webp"));
        assert!(a.contains("+profile !icc,* +set comment"), "{a}");
        assert!(
            a.contains("webp:lossless=true") && a.ends_with("WEBP:b.webp"),
            "{a}"
        );
        assert_eq!(oxipng_args("p.png", Metadata::Remove)[5], "safe");
        assert!(!oxipng_args("p.png", Metadata::Keep)[5].contains("eXIf"));
    }

    #[test]
    fn crops_follow_the_resize_and_formats_get_their_options() {
        let job = Job {
            size: Some((1000, 500)),
            crop: Some((250, 0, 500, 500)),
            ..Job::new(Format::Tiff)
        };
        let a = line(&args(&job, "a", "b"));
        assert!(
            a.contains("-resize 1000x500! -crop 500x500+250+0 +repage"),
            "{a}"
        );
        assert!(
            a.contains("-compress LZW") && !a.contains("-alpha remove"),
            "{a}"
        );
        let png = Job {
            quality: Some(30),
            ..Job::new(Format::Png)
        };
        let a = line(&args(&png, "a", "b"));
        assert!(a.contains("-colors 64") && !a.contains("-quality"), "{a}");
        let avif = Job {
            quality: Some(90),
            ..Job::new(Format::Avif)
        };
        assert!(line(&args(&avif, "a", "b")).contains("-quality 90 AVIF:b"));
        let lossless = Job {
            quality: Some(100),
            ..Job::new(Format::Png)
        };
        assert!(!line(&args(&lossless, "a", "b")).contains("-colors"));
        let ico = line(&args(&Job::new(Format::Ico), "a", "b"));
        assert!(
            ico.contains("-resize 256x256>") && ico.ends_with("PNG32:b"),
            "{ico}"
        );
        assert_eq!(icon_sizes(300), [256, 128, 64, 48, 32, 24, 16]);
        assert_eq!(icon_sizes(50), [48, 32, 24, 16]);
        assert_eq!(icon_sizes(3), [16]);
    }

    #[test]
    fn identify_output_reads_back() {
        let i = parse_info("4624 3468 RightTop").unwrap();
        assert_eq!(i.orientation, Orientation::Rotate90);
        assert_eq!(i.upright(), (3468, 4624));
        let i = parse_info("10 20 Undefined").unwrap();
        assert_eq!(
            (i.upright(), i.orientation),
            ((10, 20), Orientation::Normal)
        );
        assert_eq!(parse_info("10"), None);
        assert_eq!(info_args("x.jpg").last().unwrap(), "x.jpg[0]");
    }

    #[test]
    fn pdf_images_keep_their_component_count() {
        let a = line(&pdf_image_args("i.ppm", 1, (100, 50), 72, "o.jpg"));
        assert_eq!(
            a,
            "i.ppm -resize 100x50! -strip -type Grayscale -quality 72 JPEG:o.jpg"
        );
    }
}
