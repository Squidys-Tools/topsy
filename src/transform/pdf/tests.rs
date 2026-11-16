use lopdf::{Document, dictionary};

use super::*;
use crate::model::exif::sample;
use crate::model::jpeg;
use crate::transform::image::tests::{Scratch, jpeg_of, never, photo, quiet};

pub fn images(doc: &Document) -> Vec<&lopdf::Stream> {
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

fn media_box(doc: &Document, page: lopdf::ObjectId) -> Vec<f32> {
    doc.get_dictionary(page)
        .unwrap()
        .get(b"MediaBox")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_float().unwrap())
        .collect()
}

#[test]
fn combine_keeps_order_embeds_jpegs_as_they_are_and_appends_pdfs() {
    let dir = Scratch::new("combine");
    let jpeg = jpeg_of(&photo(64, 48, false), Some(&sample::exif(1)), None);
    let tall = jpeg_of(&photo(30, 40, false), None, None);
    let a = dir.file("a.jpg", &jpeg);
    let b = dir.file("b.jpg", &tall);
    let two = dir.path("two.pdf");
    let pages = combine(
        &[a.clone(), b.clone()],
        PageSize::Image,
        &two,
        &quiet,
        &never(),
    );
    assert_eq!(pages.unwrap(), 2);
    assert_eq!(page_count(&two).unwrap(), 2);

    let all = dir.path("all.pdf");
    let pages = combine(&[b, two, a], PageSize::A4, &all, &quiet, &never()).unwrap();
    assert_eq!(pages, 4);
    let doc = Document::load(&all).unwrap();
    let ids: Vec<_> = doc.page_iter().collect();
    assert_eq!(ids.len(), 4);
    let first = media_box(&doc, ids[0]);
    assert!(
        (first[2] - 595.28).abs() < 0.01,
        "tall picture, portrait A4: {first:?}"
    );
    let last = media_box(&doc, ids[3]);
    assert!(
        (last[2] - 841.89).abs() < 0.01,
        "wide picture, landscape A4: {last:?}"
    );
    assert_eq!(
        media_box(&doc, ids[1]),
        [0.0, 0.0, 64.0, 48.0],
        "the appended PDF keeps its own page size"
    );
    let pics = images(&doc);
    assert_eq!(pics.len(), 4);
    assert!(pics.iter().all(|s| s.filters().unwrap() == [b"DCTDecode"]));
    assert!(
        pics.iter()
            .any(|s| s.content == jpeg::markers::strip_metadata(&jpeg).unwrap()),
        "embedded without re-encoding"
    );
}

#[test]
fn pages_turn_and_metadata_goes() {
    let dir = Scratch::new("pdfr");
    let a = dir.file("a.jpg", &jpeg_of(&photo(20, 10, false), None, None));
    let src = dir.path("in.pdf");
    combine(&[a.clone(), a], PageSize::Image, &src, &quiet, &never()).unwrap();
    let mut doc = Document::load(&src).unwrap();
    let info = doc.add_object(dictionary! { "Author" => Object::string_literal("Someone") });
    doc.trailer.set("Info", info);
    doc.save(&src).unwrap();

    let out = dir.path("out.pdf");
    assert_eq!(
        rotate_pages(&src, Rotation::Left, &out, &quiet, &never()).unwrap(),
        2
    );
    let turned = Document::load(&out).unwrap();
    for page in turned.page_iter() {
        let rotate = turned.get_dictionary(page).unwrap().get(b"Rotate").unwrap();
        assert_eq!(rotate.as_i64().unwrap(), 270);
    }
    let err = rotate_pages(&src, Rotation::FlipHorizontal, &out, &quiet, &never()).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);

    strip_metadata(&src, &out, &quiet, &never()).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    assert!(!bytes.windows(7).any(|w| w == b"Someone"));
    assert_eq!(Document::load_mem(&bytes).unwrap().get_pages().len(), 2);
}

#[test]
fn pdfs_without_pictures_compress_without_fetching_anything() {
    let dir = Scratch::new("pdfplain");
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let mut content = lopdf::Stream::new(dictionary! {}, b"0 0 m 100 100 l S\n".repeat(400));
    content.allows_compression = false;
    let content = doc.add_object(content);
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages, "Contents" => content,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
    });
    doc.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let src = dir.path("plain.pdf");
    doc.save(&src).unwrap();

    let out = dir.path("out.pdf");
    let how = PdfCompress::Quality(crate::model::pdf::PdfQuality::High);
    let w = compress(&src, how, &out, &quiet, &never()).unwrap();
    assert!(!w.kept_original && w.bytes < std::fs::metadata(&src).unwrap().len() / 4);
    assert_eq!(page_count(&out).unwrap(), 1);
}

#[test]
fn damaged_and_protected_pdfs_read_as_errors() {
    let dir = Scratch::new("pdfbad");
    let bad = dir.file("bad.pdf", b"%PDF-1.7\nthis is not a pdf");
    let out = dir.path("o.pdf");
    let how = PdfCompress::Quality(crate::model::pdf::PdfQuality::High);
    let err = compress(&bad, how, &out, &quiet, &never()).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    let locked = dir.file("locked.pdf", b"%PDF-1.7\ntrailer << /Encrypt 5 0 R >>");
    let err = strip_metadata(&locked, &out, &quiet, &never()).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert!(page_count(&bad).is_err());
}
