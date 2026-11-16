use std::sync::atomic::AtomicBool;

use lopdf::Document;

use super::*;
use crate::model::exif::sample;
use crate::model::jpeg;
use crate::model::pdf::PdfQuality;
use crate::transform::image::tests::{Scratch, jpeg_of, photo, png_of};

fn never() -> AtomicBool {
    AtomicBool::new(false)
}

fn quiet(_: f32) {}

fn images(doc: &Document) -> Vec<&lopdf::Stream> {
    doc.objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .filter(|s| {
            s.dict
                .get(b"Subtype")
                .and_then(Object::as_name)
                .is_ok_and(|n| n == b"Image")
        })
        .collect()
}

#[test]
fn combine_keeps_order_embeds_jpegs_as_they_are_and_appends_pdfs() {
    let dir = Scratch::new("combine");
    let jpeg = jpeg_of(&photo(64, 48, false), Some(&sample::exif(1)), None);
    let a = dir.file("a.jpg", &jpeg);
    let b = dir.file("b.png", &png_of(&photo(40, 30, true)));
    let two = dir.path("two.pdf");
    assert_eq!(
        combine(
            &[a.clone(), b.clone()],
            PageSize::Image,
            &two,
            &quiet,
            &never()
        )
        .unwrap(),
        2
    );
    let all = dir.path("all.pdf");
    let pages = combine(&[b, two, a], PageSize::A4, &all, &quiet, &never()).unwrap();
    assert_eq!(pages, 4);

    let doc = Document::load(&all).unwrap();
    assert_eq!(doc.get_pages().len(), 4);
    let first = doc.get_dictionary(doc.page_iter().next().unwrap()).unwrap();
    let media: Vec<f32> = first
        .get(b"MediaBox")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_float().unwrap())
        .collect();
    assert!(
        (media[2] - 841.89).abs() < 0.01 && (media[3] - 595.28).abs() < 0.01,
        "wide picture, landscape A4: {media:?}"
    );
    let pics = images(&doc);
    let dct: Vec<_> = pics
        .iter()
        .filter(|s| s.filters().unwrap() == [b"DCTDecode"])
        .collect();
    assert_eq!(
        dct.len(),
        2,
        "the JPEG on its own page and inside the appended PDF"
    );
    assert_eq!(
        dct[0].content,
        jpeg::markers::strip_metadata(&jpeg).unwrap(),
        "embedded without re-encoding"
    );
    assert!(
        pics.iter().any(|s| s.dict.has(b"SMask")),
        "transparency kept as a soft mask"
    );
}

#[test]
fn compress_shrinks_big_pictures_and_never_grows_a_file() {
    let dir = Scratch::new("pdfc");
    // 1600 x 2000 on a Letter page is about 180 dpi.
    let big = dir.file("big.png", &png_of(&photo(1600, 2000, false)));
    let src = dir.path("in.pdf");
    combine(&[big], PageSize::Letter, &src, &quiet, &never()).unwrap();
    let before = std::fs::metadata(&src).unwrap().len();
    let out = dir.path("out.pdf");
    let w = compress(
        &src,
        PdfCompress::Quality(PdfQuality::Small),
        &out,
        &quiet,
        &never(),
    )
    .unwrap();
    assert!(
        !w.kept_original && w.bytes < before / 4,
        "{} -> {}",
        before,
        w.bytes
    );
    let doc = Document::load(&out).unwrap();
    assert_eq!(doc.get_pages().len(), 1);
    let pic = images(&doc)[0];
    let width = pic.dict.get(b"Width").unwrap().as_i64().unwrap();
    // 96 dpi for the 11 inch side, the one that fills the page.
    assert!((840..=850).contains(&width), "{width}");

    // Already small: the original comes back untouched.
    let again = dir.path("again.pdf");
    let w = compress(
        &out,
        PdfCompress::Quality(PdfQuality::High),
        &again,
        &quiet,
        &never(),
    )
    .unwrap();
    assert!(w.kept_original);
    assert_eq!(std::fs::read(&again).unwrap(), std::fs::read(&out).unwrap());
}

#[test]
fn compress_to_a_size_walks_down_until_it_fits() {
    let dir = Scratch::new("pdfs");
    let big = dir.file("big.png", &png_of(&photo(1600, 2000, false)));
    let src = dir.path("in.pdf");
    combine(&[big], PageSize::Letter, &src, &quiet, &never()).unwrap();
    let out = dir.path("out.pdf");
    let w = compress(&src, PdfCompress::FileSize(60_000), &out, &quiet, &never()).unwrap();
    assert!(w.fits && w.bytes <= 60_000, "{w:?}");
    assert_eq!(Document::load(&out).unwrap().get_pages().len(), 1);
}

#[test]
fn pages_turn_and_metadata_goes() {
    let dir = Scratch::new("pdfr");
    let a = dir.file("a.png", &png_of(&photo(20, 10, false)));
    let src = dir.path("in.pdf");
    combine(&[a.clone(), a], PageSize::Image, &src, &quiet, &never()).unwrap();
    let mut doc = Document::load(&src).unwrap();
    let info = doc.add_object(lopdf::dictionary! { "Author" => Object::string_literal("Someone") });
    doc.trailer.set("Info", info);
    doc.save(&src).unwrap();

    let out = dir.path("out.pdf");
    assert_eq!(
        rotate_pages(&src, Rotation::Left, &out, &quiet, &never()).unwrap(),
        2
    );
    let turned = Document::load(&out).unwrap();
    for page in turned.page_iter() {
        assert_eq!(
            turned
                .get_dictionary(page)
                .unwrap()
                .get(b"Rotate")
                .unwrap()
                .as_i64()
                .unwrap(),
            270
        );
    }
    let err = rotate_pages(&src, Rotation::FlipHorizontal, &out, &quiet, &never()).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);

    strip_metadata(&src, &out, &quiet, &never()).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    assert!(!bytes.windows(7).any(|w| w == b"Someone"));
    assert_eq!(Document::load_mem(&bytes).unwrap().get_pages().len(), 2);
}

#[test]
fn pages_render_to_pictures_at_the_asked_resolution() {
    let dir = Scratch::new("pdfi");
    let a = dir.file("a.png", &png_of(&photo(144, 72, false)));
    let src = dir.path("in.pdf");
    combine(&[a.clone(), a], PageSize::Image, &src, &quiet, &never()).unwrap();
    assert_eq!(page_count(&src).unwrap(), 2);
    let named = |i: usize| Ok(dir.path(&format!("page-{}.png", i + 1)));
    assert_eq!(
        to_images(&src, PageFormat::Png, 144, &named, &quiet, &never()).unwrap(),
        2
    );
    for i in 0..2 {
        // 144 points at 144 dpi is two inches: 288 pixels.
        let page = image::open(dir.path(&format!("page-{}.png", i + 1))).unwrap();
        assert_eq!((page.width(), page.height()), (288, 144));
    }
}

#[test]
fn damaged_and_protected_pdfs_read_as_errors() {
    let dir = Scratch::new("pdfbad");
    let bad = dir.file("bad.pdf", b"%PDF-1.7\nthis is not a pdf");
    let err = compress(
        &bad,
        PdfCompress::Quality(PdfQuality::High),
        &dir.path("o.pdf"),
        &quiet,
        &never(),
    )
    .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}
