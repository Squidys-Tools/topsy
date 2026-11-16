//! What the image Transform offers (formats, quality, sizes, turns) and the rules behind it:
//! picking an output format and mapping the quality slider onto a PNG palette.

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
    Heic,
    Webp,
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
            Format::Heic => "HEIC",
            Format::Webp => "WebP",
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
            Format::Heic => "heic",
            Format::Webp => "webp",
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
            Format::Jpeg | Format::Heic | Format::Webp | Format::Avif
        )
    }

    /// The format a file of `kind` is rewritten in when an operation keeps its format, when it
    /// can be written.
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
            Kind::Heic => Format::Heic,
            Kind::Webp => Format::Webp,
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
    Heic,
    Webp,
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
            CompressFormat::Heic => "HEIC",
            CompressFormat::Webp => "WebP",
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
            CompressFormat::Heic => Format::Heic,
            CompressFormat::Webp => Format::Webp,
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

/// The quality a picture is re-encoded at when an operation keeps its format: `None` leaves it
/// to ImageMagick, which reads a JPEG's own; 90 for the other lossy formats.
pub fn reencode_quality(kind: Kind) -> Option<u8> {
    match kind {
        Kind::Webp | Kind::Avif => Some(90),
        _ => None,
    }
}

/// Whether a WebP file's first image chunk is lossless (VP8L) rather than lossy (VP8).
pub fn is_lossless_webp(head: &[u8]) -> bool {
    head.windows(4).find(|w| *w == b"VP8 " || *w == b"VP8L") == Some(b"VP8L")
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
    fn heic_stays_heic() {
        assert_eq!(Format::matching(Kind::Heic), Some(Format::Heic));
        assert_eq!(
            CompressFormat::SameAsOriginal.resolve(Kind::Heic, false),
            Format::Heic
        );
        assert_eq!(Format::matching(Kind::Gif), Some(Format::Gif));
        assert_eq!(Format::matching(Kind::Pdf), None);
    }

    #[test]
    fn lossless_webp_is_told_from_lossy() {
        assert!(is_lossless_webp(b"RIFF....WEBPVP8L"));
        assert!(is_lossless_webp(b"RIFF....WEBPVP8X....ALPHVP8L"));
        assert!(!is_lossless_webp(b"RIFF....WEBPVP8 "));
        assert_eq!(reencode_quality(Kind::Jpeg), None);
        assert_eq!(reencode_quality(Kind::Avif), Some(90));
    }

    #[test]
    fn png_palettes_shrink_with_quality() {
        assert_eq!(png_colors(100), None);
        assert_eq!(png_colors(75), Some(256));
        assert_eq!(png_colors(30), Some(64));
        assert_eq!(png_colors(1), Some(16));
    }
}
