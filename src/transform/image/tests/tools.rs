//! Every operation through the real tools. Ignored: the first run downloads ImageMagick, oxipng
//! and pdfium into `%LOCALAPPDATA%\topsy\tools`. Run with `cargo test -- --ignored tools::`.

use std::io;
use std::sync::atomic::AtomicBool;

use super::*;
use crate::model::exif::{has_gps, sample};
use crate::model::image_geometry::{LargestSide, Resize, Rotation};
use crate::model::image_ops::{
    CompressBy, CompressFormat, CompressOptions, ConvertOptions, Format, Kind,
};
use crate::model::jpeg;
use crate::model::orientation::Orientation;
use crate::model::pdf::{PageFormat, PageSize, PdfCompress, PdfQuality};
use crate::transform::image::{compress, convert, probe, resize, rotate, strip_metadata};
use crate::transform::pdf;

fn adobe_rgb() -> Vec<u8> {
    std::fs::read(r"C:\Windows\System32\spool\drivers\color\AdobeRGB1998.icc").unwrap()
}

/// The chunk types of a PNG, in order.
fn chunks(png: &[u8]) -> Vec<String> {
    let mut at = 8;
    let mut out = Vec::new();
    while at + 8 <= png.len() {
        let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
        out.push(String::from_utf8_lossy(&png[at + 4..at + 8]).into_owned());
        at += 12 + len;
    }
    out
}

fn jpeg_exif(path: &std::path::Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).unwrap();
    jpeg::markers::exif(&jpeg::markers::segments(&bytes).ok()?)
}

#[test]
#[ignore = "downloads ImageMagick, oxipng and pdfium"]
fn compress_turns_upright_keeps_metadata_and_never_grows() {
    let dir = Scratch::new("t-compress");
    let src = dir.file(
        "turned.jpg",
        &jpeg_of(
            &photo(640, 480, false),
            Some(&sample::exif(6)),
            Some(&adobe_rgb()),
        ),
    );
    let out = dir.path("out.jpg");
    let w = compress(&src, &CompressOptions::default(), &out, &quiet, &never()).unwrap();
    assert_eq!((w.format, w.width, w.height), (Format::Jpeg, 480, 640));
    assert_eq!(decoded(&out).dimensions(), (480, 640));
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(jpeg::orientation(&bytes), Orientation::Normal);
    assert!(has_gps(&jpeg_exif(&out).unwrap()), "Compress keeps EXIF");
    assert!(jpeg::markers::icc(&jpeg::markers::segments(&bytes).unwrap()).is_some());

    let sized = CompressOptions {
        by: CompressBy::FileSize,
        target_bytes: 12_000,
        largest_side: LargestSide::Original,
        ..Default::default()
    };
    let w = compress(&src, &sized, &out, &quiet, &never()).unwrap();
    assert!(w.fits && w.bytes <= 12_000, "{w:?}");

    let roomy = CompressOptions {
        target_bytes: 10_000_000,
        ..sized
    };
    let upright = dir.file("upright.jpg", &jpeg_of(&photo(640, 480, false), None, None));
    compress(&upright, &roomy, &out, &quiet, &never()).unwrap();
    assert_eq!(
        std::fs::read(&out).unwrap(),
        std::fs::read(&upright).unwrap()
    );
}

#[test]
#[ignore = "downloads ImageMagick, oxipng and pdfium"]
fn transparency_stays_and_pngs_lose_their_text_chunks() {
    let dir = Scratch::new("t-alpha");
    let src = dir.file("a.png", &png_of(&photo(300, 200, true)));
    let out = dir.path("out.png");
    let w = compress(&src, &CompressOptions::default(), &out, &quiet, &never()).unwrap();
    assert_eq!(w.format, Format::Png);
    assert!(decoded(&out).pixels().any(|p| p.0[3] == 0));
    let same = CompressOptions {
        format: CompressFormat::Webp,
        ..Default::default()
    };
    let webp = dir.path("out.webp");
    compress(&src, &same, &webp, &quiet, &never()).unwrap();
    assert_eq!(probe(&webp).unwrap().kind, Kind::Webp);
    assert!(decoded(&webp).pixels().any(|p| p.0[3] < 10));

    let exif = dir.file(
        "e.jpg",
        &jpeg_of(
            &photo(64, 48, false),
            Some(&sample::exif(1)),
            Some(&adobe_rgb()),
        ),
    );
    let png = dir.path("e.png");
    let to_png = ConvertOptions {
        format: Format::Png,
        quality: 100,
    };
    convert(&exif, &to_png, &png, &quiet, &never()).unwrap();
    let kept = chunks(&std::fs::read(&png).unwrap());
    let colour = |c: &[String]| c.iter().any(|c| c == "iCCP");
    assert!(kept.contains(&"eXIf".into()) && colour(&kept), "{kept:?}");
    assert!(!kept.iter().any(|c| c.ends_with("Xt")), "{kept:?}");

    let bare = dir.path("bare.png");
    strip_metadata(&png, &bare, &quiet, &never()).unwrap();
    let left = chunks(&std::fs::read(&bare).unwrap());
    assert!(!left.contains(&"eXIf".into()), "{left:?}");
    assert!(colour(&left), "the colour profile stays: {left:?}");
}

#[test]
#[ignore = "downloads ImageMagick, oxipng and pdfium"]
fn every_format_converts_and_reads_back() {
    let dir = Scratch::new("t-convert");
    let src = dir.file("p.jpg", &jpeg_of(&photo(120, 80, false), None, None));
    for format in Format::ALL {
        let out = dir.path(&format!("out.{}", format.extension()));
        let opts = ConvertOptions {
            format,
            quality: 75,
        };
        let w = convert(&src, &opts, &out, &quiet, &never()).unwrap();
        let info = probe(&out).unwrap();
        assert_eq!(info.kind.label(), format.label(), "{format:?}");
        match format {
            Format::Pdf => assert_eq!(info.pages, Some(1)),
            Format::Ico => {
                assert_eq!((info.width, info.height), (64, 64));
                let ico = std::fs::read(&out).unwrap();
                let pngs = ico.windows(4).filter(|w| *w == b"\x89PNG").count();
                assert_eq!(pngs, 5, "64, 48, 32, 24 and 16, each a PNG");
            }
            _ => assert_eq!((info.width, info.height), (120, 80), "{format:?}"),
        }
        assert_eq!(w.bytes, std::fs::metadata(&out).unwrap().len());
    }
    let avif = dir.path("out.avif");
    let back = dir.path("back.png");
    let to_png = ConvertOptions {
        format: Format::Png,
        quality: 100,
    };
    convert(&avif, &to_png, &back, &quiet, &never()).unwrap();
    assert!(mean_error(&decoded(&back), &decoded(&src)) < 6.0);
}

#[test]
#[ignore = "downloads ImageMagick, oxipng and pdfium"]
fn heic_keeps_odd_sizes_transparency_and_exif() {
    let dir = Scratch::new("t-heic");
    let clear = photo(121, 81, true);
    let src = dir.file("p.png", &png_of(&clear));
    let heic = dir.path("p.heic");
    let to = |format| ConvertOptions {
        format,
        quality: 90,
    };
    let w = convert(&src, &to(Format::Heic), &heic, &quiet, &never()).unwrap();
    assert_eq!((w.width, w.height), (121, 81));
    let back = dir.path("back.png");
    convert(&heic, &to(Format::Png), &back, &quiet, &never()).unwrap();
    let back = decoded(&back);
    assert!(
        mean_error(&back, &clear) < 8.0,
        "{}",
        mean_error(&back, &clear)
    );
    let see_through = |i: &image::RgbaImage| i.pixels().filter(|p| p[3] < 128).count();
    assert!(see_through(&back).abs_diff(see_through(&clear)) < 200);

    let exif = dir.file(
        "e.jpg",
        &jpeg_of(&photo(64, 48, false), Some(&sample::exif(1)), None),
    );
    let (heic, jpg) = (dir.path("e.heic"), dir.path("e2.jpg"));
    convert(&exif, &to(Format::Heic), &heic, &quiet, &never()).unwrap();
    convert(&heic, &to(Format::Jpeg), &jpg, &quiet, &never()).unwrap();
    assert!(jpeg_exif(&jpg).is_some_and(|e| has_gps(&e)));
}

#[test]
#[ignore = "downloads ImageMagick, oxipng and pdfium"]
fn turns_sizes_and_strips() {
    let dir = Scratch::new("t-edit");
    let gps = dir.file(
        "g.jpg",
        &jpeg_of(&photo(64, 48, false), Some(&sample::exif(1)), None),
    );
    let out = dir.path("s.jpg");
    strip_metadata(&gps, &out, &quiet, &never()).unwrap();
    assert!(jpeg_exif(&out).is_none_or(|e| !has_gps(&e)));

    let png = dir.file("p.png", &png_of(&photo(64, 48, true)));
    let turned = dir.path("t.png");
    let w = rotate(&png, Rotation::Right, &turned, &quiet, &never()).unwrap();
    assert_eq!((w.width, w.height), (48, 64));
    assert_eq!(decoded(&turned).dimensions(), (48, 64));
    let half = dir.path("h.png");
    let w = resize(&png, Resize::Percent(50), &half, &quiet, &never()).unwrap();
    assert_eq!(decoded(&half).dimensions(), (32, 24));
    assert_eq!(w.format, Format::Png);

    let webp = dir.path("w.webp");
    let opts = ConvertOptions {
        format: Format::Webp,
        quality: 80,
    };
    convert(&gps, &opts, &webp, &quiet, &never()).unwrap();
    let flipped = dir.path("f.webp");
    rotate(&webp, Rotation::FlipHorizontal, &flipped, &quiet, &never()).unwrap();
    assert_eq!(probe(&flipped).unwrap().kind, Kind::Webp);
}

#[test]
#[ignore = "downloads ImageMagick, oxipng and pdfium"]
fn a_cancelled_run_is_interrupted() {
    let dir = Scratch::new("t-cancel");
    let src = dir.file("p.png", &png_of(&photo(64, 48, false)));
    let err = compress(
        &src,
        &CompressOptions::default(),
        &dir.path("o.jpg"),
        &quiet,
        &AtomicBool::new(true),
    )
    .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Interrupted);
}

#[test]
#[ignore = "downloads ImageMagick, oxipng and pdfium"]
fn pdfs_combine_compress_and_render() {
    let dir = Scratch::new("t-pdf");
    let alpha = dir.file("a.png", &png_of(&photo(40, 30, true)));
    let webp = dir.path("b.webp");
    let opts = ConvertOptions {
        format: Format::Webp,
        quality: 80,
    };
    convert(&alpha, &opts, &webp, &quiet, &never()).unwrap();
    let two = dir.path("two.pdf");
    let pages = pdf::combine(&[alpha, webp], PageSize::Image, &two, &quiet, &never());
    assert_eq!(pages.unwrap(), 2);
    let doc = lopdf::Document::load(&two).unwrap();
    assert!(
        crate::transform::pdf::tests::images(&doc)
            .iter()
            .any(|s| s.dict.has(b"SMask")),
        "transparency kept as a soft mask"
    );

    let big = dir.file("big.png", &png_of(&photo(1600, 2000, false)));
    let src = dir.path("in.pdf");
    pdf::combine(&[big], PageSize::Letter, &src, &quiet, &never()).unwrap();
    let before = std::fs::metadata(&src).unwrap().len();
    let out = dir.path("out.pdf");
    let how = PdfCompress::Quality(PdfQuality::Small);
    let w = pdf::compress(&src, how, &out, &quiet, &never()).unwrap();
    assert!(
        !w.kept_original && w.bytes < before / 4,
        "{before} -> {w:?}"
    );
    let again = dir.path("again.pdf");
    let w = pdf::compress(
        &out,
        PdfCompress::Quality(PdfQuality::High),
        &again,
        &quiet,
        &never(),
    );
    assert!(w.unwrap().kept_original);
    let sized = pdf::compress(&src, PdfCompress::FileSize(60_000), &out, &quiet, &never());
    assert!(sized.unwrap().fits);

    let named = |i: usize| Ok(dir.path(&format!("page-{}.png", i + 1)));
    let n = pdf::to_images(&two, PageFormat::Png, 144, &named, &quiet, &never()).unwrap();
    assert_eq!(n, 2);
    // 40 points at 144 dpi is 80 pixels.
    assert_eq!(decoded(&dir.path("page-1.png")).dimensions(), (80, 60));
    let named = |i: usize| Ok(dir.path(&format!("page-{}.jpg", i + 1)));
    pdf::to_images(&out, PageFormat::Jpeg, 72, &named, &quiet, &never()).unwrap();
    assert_eq!(decoded(&dir.path("page-1.jpg")).dimensions(), (612, 792));

    let bad = dir.file("bad.pdf", b"%PDF-1.7\nnot a pdf");
    let err = pdf::to_images(&bad, PageFormat::Png, 72, &named, &quiet, &never()).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}
