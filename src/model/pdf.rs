//! What the PDF Transforms offer and the geometry behind them: page sizes for Combine, the
//! presets Compress steps through, and how far an embedded picture may shrink.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSize {
    /// Each page the size of its picture at 72 dpi, as Preview makes them.
    Image,
    A4,
    Letter,
}

impl PageSize {
    pub const ALL: [PageSize; 3] = [PageSize::Image, PageSize::A4, PageSize::Letter];

    pub fn label(self) -> &'static str {
        match self {
            PageSize::Image => "Fit to image",
            PageSize::A4 => "A4",
            PageSize::Letter => "US Letter",
        }
    }
}

/// What PDF to images writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageFormat {
    Png,
    Jpeg,
}

impl PageFormat {
    pub const ALL: [PageFormat; 2] = [PageFormat::Png, PageFormat::Jpeg];

    pub fn label(self) -> &'static str {
        match self {
            PageFormat::Png => "PNG",
            PageFormat::Jpeg => "JPEG",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            PageFormat::Png => "png",
            PageFormat::Jpeg => "jpg",
        }
    }
}

/// The resolutions PDF to images offers: screen, a sharp screen, print.
pub const PAGE_DPI: [u32; 3] = [72, 150, 300];

/// A page in points and where the picture sits on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub page: (f64, f64),
    /// x, y (from the bottom left), width, height.
    pub image: (f64, f64, f64, f64),
}

/// The page for a `w` x `h` pixel picture. A4 and Letter turn to landscape for a wide picture,
/// which is then scaled to fit and centred.
pub fn place(w: u32, h: u32, size: PageSize) -> Placement {
    let (w, h) = (f64::from(w.max(1)), f64::from(h.max(1)));
    let paper = match size {
        PageSize::Image => {
            return Placement {
                page: (w, h),
                image: (0.0, 0.0, w, h),
            };
        }
        PageSize::A4 => (595.28, 841.89),
        PageSize::Letter => (612.0, 792.0),
    };
    let (pw, ph) = if w > h { (paper.1, paper.0) } else { paper };
    let s = (pw / w).min(ph / h);
    let (iw, ih) = (w * s, h * s);
    Placement {
        page: (pw, ph),
        image: ((pw - iw) / 2.0, (ph - ih) / 2.0, iw, ih),
    }
}

/// The content stream that draws image `/Im0` at `p`.
pub fn draw(p: &Placement) -> Vec<u8> {
    let (x, y, w, h) = p.image;
    format!("q {w:.4} 0 0 {h:.4} {x:.4} {y:.4} cm /Im0 Do Q").into_bytes()
}

/// The pixels a page of `page` points takes when drawn to fit a square of `side` pixels.
pub fn fit_square(page: (f32, f32), side: i32) -> (i32, i32) {
    let side = side.max(1);
    let valid = |v: f32| v.is_finite() && v > 0.0;
    if !valid(page.0) || !valid(page.1) {
        return (side, side);
    }
    let long = page.0.max(page.1);
    let px = |v: f32| ((v / long * side as f32).round() as i32).clamp(1, side);
    (px(page.0), px(page.1))
}

/// How hard Compress PDF works on the pictures inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfQuality {
    High,
    Balanced,
    Small,
}

impl PdfQuality {
    pub const ALL: [PdfQuality; 3] = [PdfQuality::High, PdfQuality::Balanced, PdfQuality::Small];

    pub fn label(self) -> &'static str {
        match self {
            PdfQuality::High => "High quality",
            PdfQuality::Balanced => "Balanced",
            PdfQuality::Small => "Smallest",
        }
    }

    pub fn preset(self) -> Preset {
        match self {
            PdfQuality::High => LADDER[1],
            PdfQuality::Balanced => LADDER[2],
            PdfQuality::Small => LADDER[4],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PdfCompress {
    Quality(PdfQuality),
    FileSize(u64),
}

/// The most a picture keeps (dots per inch of the page it is on) and the JPEG quality it is
/// re-encoded at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    pub dpi: u32,
    pub quality: u8,
}

/// From gentlest to harshest; a size target walks it until the file fits.
pub const LADDER: [Preset; 6] = [
    Preset {
        dpi: 300,
        quality: 90,
    },
    Preset {
        dpi: 200,
        quality: 82,
    },
    Preset {
        dpi: 150,
        quality: 72,
    },
    Preset {
        dpi: 120,
        quality: 62,
    },
    Preset {
        dpi: 96,
        quality: 50,
    },
    Preset {
        dpi: 72,
        quality: 40,
    },
];

/// The size a `w` x `h` picture on a page of `page` points should shrink to, or `None` to leave
/// it. The picture is assumed to fill the page, which can only overstate how big it is shown,
/// so this never shrinks one below `dpi` where it really appears. Within a quarter of the
/// limit it is left alone; resampling that little costs more than it saves.
pub fn downsample(w: u32, h: u32, page: (f64, f64), dpi: u32) -> Option<(u32, u32)> {
    let (iw, ih) = (page.0 / 72.0, page.1 / 72.0);
    if iw <= 0.0 || ih <= 0.0 {
        return None;
    }
    let density = (f64::from(w) / iw).min(f64::from(h) / ih);
    if density <= f64::from(dpi) * 1.25 {
        return None;
    }
    let s = f64::from(dpi) / density;
    Some((
        ((f64::from(w) * s).round() as u32).max(1),
        ((f64::from(h) * s).round() as u32).max(1),
    ))
}

/// Whether a re-encoded picture should replace the one in the PDF. One that wasn't scaled down
/// has to save a tenth: a JPEG already near the preset's quality loses a generation for little.
pub fn worth_replacing(old: usize, new: usize, scaled: bool) -> bool {
    if scaled {
        new < old
    } else {
        new * 10 < old * 9
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_pages_are_the_picture_at_72_dpi() {
        let p = place(4032, 3024, PageSize::Image);
        assert_eq!(p.page, (4032.0, 3024.0));
        assert_eq!(p.image, (0.0, 0.0, 4032.0, 3024.0));
        assert_eq!(
            draw(&p),
            b"q 4032.0000 0 0 3024.0000 0.0000 0.0000 cm /Im0 Do Q".to_vec()
        );
    }

    #[test]
    fn paper_turns_for_wide_pictures_and_centres_them() {
        let p = place(4000, 3000, PageSize::A4);
        assert_eq!(p.page, (841.89, 595.28));
        let (x, y, w, h) = p.image;
        assert!((h - 595.28).abs() < 1e-9 && (w - 793.7066).abs() < 1e-3);
        assert!((x - (841.89 - w) / 2.0).abs() < 1e-9 && y == 0.0);
        let tall = place(1000, 3000, PageSize::Letter);
        assert_eq!(tall.page, (612.0, 792.0));
        assert!(tall.image.2 <= 612.0 && tall.image.3 <= 792.0);
    }

    #[test]
    fn downsampling_only_past_the_limit_and_never_below_it() {
        // A 300 dpi scan of a Letter page.
        let page = (612.0, 792.0);
        assert_eq!(downsample(2550, 3300, page, 150), Some((1275, 1650)));
        assert_eq!(downsample(2550, 3300, page, 300), None);
        assert_eq!(
            downsample(1300, 1700, page, 150),
            None,
            "within a quarter: left alone"
        );
        let (w, h) = downsample(4000, 1000, page, 72).unwrap();
        assert!(
            f64::from(w) / 8.5 >= 72.0 - 1.0 && f64::from(h) / 11.0 >= 72.0 - 1.0,
            "{w}x{h}"
        );
    }

    #[test]
    fn a_page_preview_fits_its_square_whatever_the_page() {
        assert_eq!(fit_square((612.0, 792.0), 200), (155, 200));
        assert_eq!(fit_square((1440.0, 810.0), 200), (200, 113));
        assert_eq!(fit_square((10.0, 10_000.0), 200), (1, 200));
        assert_eq!(fit_square((0.0, 0.0), 64), (64, 64));
        assert_eq!(fit_square((f32::NAN, 5.0), 64), (64, 64));
    }

    #[test]
    fn a_re_encode_has_to_earn_its_place() {
        assert!(worth_replacing(1000, 999, true));
        assert!(!worth_replacing(1000, 950, false));
        assert!(worth_replacing(1000, 800, false));
        assert!(!worth_replacing(1000, 1200, true));
    }

    #[test]
    fn the_ladder_gets_harsher() {
        assert!(
            LADDER
                .windows(2)
                .all(|p| p[0].dpi >= p[1].dpi && p[0].quality > p[1].quality)
        );
    }
}
