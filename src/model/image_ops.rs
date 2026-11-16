//! What the image Transform offers (formats, quality, sizes, turns) and the rules behind it:
//! picking an output format, fitting dimensions, mapping one quality slider onto each encoder,
//! and searching for the quality and size that land a file under a target.

use super::image_geometry::LargestSide;

/// A file's real format, from its first bytes rather than its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Jpeg,
    Png,
    Webp,
    Gif,
    Bmp,
    Tiff,
    Ico,
    Heic,
    Avif,
    Pdf,
}

impl Kind {
    pub fn sniff(head: &[u8]) -> Option<Kind> {
        let at = |o: usize, m: &[u8]| head.get(o..o + m.len()) == Some(m);
        Some(match () {
            _ if at(0, b"\xFF\xD8\xFF") => Kind::Jpeg,
            _ if at(0, b"\x89PNG\r\n\x1a\n") => Kind::Png,
            _ if at(0, b"RIFF") && at(8, b"WEBP") => Kind::Webp,
            _ if at(0, b"GIF87a") || at(0, b"GIF89a") => Kind::Gif,
            _ if at(0, b"BM") => Kind::Bmp,
            _ if at(0, b"II*\0") || at(0, b"MM\0*") => Kind::Tiff,
            _ if at(0, b"\0\0\x01\0") => Kind::Ico,
            _ if at(0, b"%PDF-") => Kind::Pdf,
            _ if at(4, b"ftyp") => {
                let brands = head.get(8..head.len().min(64))?;
                let has = |b: &[u8]| brands.chunks(4).any(|c| c == b);
                if has(b"avif") || has(b"avis") {
                    Kind::Avif
                } else if [
                    &b"heic"[..],
                    b"heix",
                    b"heim",
                    b"heis",
                    b"hevc",
                    b"hevx",
                    b"mif1",
                    b"msf1",
                ]
                .iter()
                .any(|b| has(b))
                {
                    Kind::Heic
                } else {
                    return None;
                }
            }
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Jpeg => "JPEG",
            Kind::Png => "PNG",
            Kind::Webp => "WebP",
            Kind::Gif => "GIF",
            Kind::Bmp => "BMP",
            Kind::Tiff => "TIFF",
            Kind::Ico => "ICO",
            Kind::Heic => "HEIC",
            Kind::Avif => "AVIF",
            Kind::Pdf => "PDF",
        }
    }
}

/// What Convert writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Jpeg,
    Png,
    Webp,
    Heic,
    Avif,
    Tiff,
    Bmp,
    Gif,
    Ico,
    Pdf,
}

impl Format {
    pub const ALL: [Format; 10] = [
        Format::Jpeg,
        Format::Png,
        Format::Heic,
        Format::Webp,
        Format::Avif,
        Format::Tiff,
        Format::Gif,
        Format::Bmp,
        Format::Ico,
        Format::Pdf,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Format::Jpeg => "JPEG",
            Format::Png => "PNG",
            Format::Webp => "WebP",
            Format::Heic => "HEIC",
            Format::Avif => "AVIF",
            Format::Tiff => "TIFF",
            Format::Bmp => "BMP",
            Format::Gif => "GIF",
            Format::Ico => "ICO",
            Format::Pdf => "PDF",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Jpeg => "jpg",
            Format::Png => "png",
            Format::Webp => "webp",
            Format::Heic => "heic",
            Format::Avif => "avif",
            Format::Tiff => "tiff",
            Format::Bmp => "bmp",
            Format::Gif => "gif",
            Format::Ico => "ico",
            Format::Pdf => "pdf",
        }
    }

    pub fn keeps_alpha(self) -> bool {
        !matches!(self, Format::Jpeg | Format::Bmp | Format::Pdf)
    }

    /// Whether a quality setting changes the output.
    pub fn is_lossy(self) -> bool {
        matches!(
            self,
            Format::Jpeg | Format::Webp | Format::Heic | Format::Avif
        )
    }

    /// The format a file of `kind` is rewritten in when an operation keeps its format.
    pub fn matching(kind: Kind) -> Option<Format> {
        Some(match kind {
            Kind::Gif => Format::Gif,
            Kind::Bmp => Format::Bmp,
            Kind::Tiff => Format::Tiff,
            Kind::Ico => Format::Ico,
            Kind::Pdf => return None,
            other => return Format::of(other),
        })
    }

    fn of(kind: Kind) -> Option<Format> {
        Some(match kind {
            Kind::Jpeg => Format::Jpeg,
            Kind::Png => Format::Png,
            Kind::Webp => Format::Webp,
            Kind::Heic => Format::Heic,
            Kind::Avif => Format::Avif,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressBy {
    Quality,
    FileSize,
}

impl CompressBy {
    pub fn label(self) -> &'static str {
        match self {
            CompressBy::Quality => "Quality",
            CompressBy::FileSize => "File size",
        }
    }
}

/// Compress's "Image format".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressFormat {
    /// JPEG, or PNG when the picture has transparency to keep.
    MostCompatible,
    SameAsOriginal,
    Jpeg,
    Png,
    Webp,
    Heic,
    Avif,
}

impl CompressFormat {
    pub const ALL: [CompressFormat; 7] = [
        CompressFormat::MostCompatible,
        CompressFormat::SameAsOriginal,
        CompressFormat::Jpeg,
        CompressFormat::Png,
        CompressFormat::Heic,
        CompressFormat::Webp,
        CompressFormat::Avif,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CompressFormat::MostCompatible => "Most compatible",
            CompressFormat::SameAsOriginal => "Same as original",
            CompressFormat::Jpeg => "JPEG",
            CompressFormat::Png => "PNG",
            CompressFormat::Webp => "WebP",
            CompressFormat::Heic => "HEIC",
            CompressFormat::Avif => "AVIF",
        }
    }

    /// The format to write for a source of `kind`, `has_alpha` when it has real transparency.
    pub fn resolve(self, kind: Kind, has_alpha: bool) -> Format {
        let compatible = if has_alpha { Format::Png } else { Format::Jpeg };
        match self {
            CompressFormat::MostCompatible => compatible,
            CompressFormat::SameAsOriginal => Format::of(kind).unwrap_or(compatible),
            CompressFormat::Jpeg => Format::Jpeg,
            CompressFormat::Png => Format::Png,
            CompressFormat::Webp => Format::Webp,
            CompressFormat::Heic => Format::Heic,
            CompressFormat::Avif => Format::Avif,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompressOptions {
    pub by: CompressBy,
    /// 1..=100, used when `by` is Quality.
    pub quality: u8,
    /// Used when `by` is FileSize.
    pub target_bytes: u64,
    pub format: CompressFormat,
    pub largest_side: LargestSide,
}

impl Default for CompressOptions {
    /// Droppy's: quality 75 %, most compatible, 1920 px.
    fn default() -> Self {
        Self {
            by: CompressBy::Quality,
            quality: 75,
            target_bytes: 1_000_000,
            format: CompressFormat::MostCompatible,
            largest_side: LargestSide::Px(1920),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConvertOptions {
    pub format: Format,
    /// For the lossy formats, 1..=100.
    pub quality: u8,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            format: Format::Jpeg,
            quality: 90,
        }
    }
}

/// The lossy WebP encoder's own quality for a slider value, so 75 % looks like libwebp's 75.
/// Measured on 12 MP photos: its 85 lands at the size and PSNR libwebp's 75 does.
pub fn webp_quality(q: u8) -> u8 {
    let q = u32::from(q.clamp(1, 100));
    (if q <= 75 {
        q * 85 / 75
    } else {
        85 + (q - 75) * 15 / 25
    }) as u8
}

/// The palette size a lossy PNG gets for a slider value; `None` keeps it lossless.
pub fn png_colors(q: u8) -> Option<usize> {
    match q {
        100.. => None,
        60.. => Some(256),
        _ => Some(1 << (4 + u32::from(q) * 4 / 60)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffing_reads_magic_not_names() {
        assert_eq!(Kind::sniff(b"\xFF\xD8\xFF\xE1"), Some(Kind::Jpeg));
        assert_eq!(Kind::sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some(Kind::Webp));
        assert_eq!(
            Kind::sniff(b"\0\0\0\x18ftypheic\0\0\0\0mif1heic"),
            Some(Kind::Heic)
        );
        assert_eq!(
            Kind::sniff(b"\0\0\0\x1cftypmif1\0\0\0\0mif1avifmiaf"),
            Some(Kind::Avif)
        );
        assert_eq!(Kind::sniff(b"\0\0\0\x18ftypmp42\0\0\0\0isommp42"), None);
        assert_eq!(Kind::sniff(b"%PDF-1.7"), Some(Kind::Pdf));
        assert_eq!(Kind::sniff(b"hello"), None);
    }

    #[test]
    fn most_compatible_is_jpeg_unless_there_is_transparency() {
        let mc = CompressFormat::MostCompatible;
        assert_eq!(mc.resolve(Kind::Heic, false), Format::Jpeg);
        assert_eq!(mc.resolve(Kind::Png, true), Format::Png);
        let same = CompressFormat::SameAsOriginal;
        assert_eq!(same.resolve(Kind::Webp, true), Format::Webp);
        assert_eq!(same.resolve(Kind::Bmp, false), Format::Jpeg);
        assert_eq!(same.resolve(Kind::Gif, true), Format::Png);
    }

    #[test]
    fn quality_maps_are_monotonic_and_in_range() {
        let w: Vec<u8> = (1..=100).map(webp_quality).collect();
        assert!(w.windows(2).all(|p| p[0] <= p[1]));
        assert_eq!((webp_quality(75), webp_quality(100)), (85, 100));
        assert_eq!(png_colors(100), None);
        assert_eq!(png_colors(75), Some(256));
        assert_eq!(png_colors(30), Some(64));
        assert_eq!(png_colors(1), Some(16));
    }
}
