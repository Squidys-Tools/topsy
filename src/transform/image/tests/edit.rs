use std::io;
use std::sync::atomic::AtomicBool;

use super::*;
use crate::model::exif::{self, has_gps, sample};
use crate::model::image_geometry::{Resize, Rotation};
use crate::model::image_ops::{ConvertOptions, Format, Kind};
use crate::model::orientation::Orientation;
use crate::model::{jpeg, png, webp};
use crate::transform::image::encode::{self, Meta};
use crate::transform::image::{convert, probe, resize, rotate, strip_metadata};

#[test]
fn resize_fits_scales_and_crops() {
    let dir = Scratch::new("resize");
    let src = dir.file("in.png", &png_of(&photo(400, 300, true)));
    let out = dir.path("out.png");
    for (how, want) in [
        (Resize::Percent(50), (200, 150)),
        (Resize::LargestSide(100), (100, 75)),
        (
            Resize::Exact {
                width: 120,
                height: 120,
                fill: false,
            },
            (120, 90),
        ),
        (
            Resize::Exact {
                width: 120,
                height: 120,
                fill: true,
            },
            (120, 120),
        ),
    ] {
        let w = resize(&src, how, &out, &quiet, &never()).unwrap();
        assert_eq!((w.width, w.height), want, "{how:?}");
        assert_eq!(decoded(&out).dimensions(), want);
    }
}

#[test]
fn jpeg_rotation_is_lossless_and_resets_the_tag() {
    let dir = Scratch::new("rotate");
    let img = photo(64, 48, false);
    let exif = sample::exif(1);
    let original = jpeg_of(&img, Some(&exif), Some(b"a profile"));
    let src = dir.file("in.jpg", &original);
    let out = dir.path("out.jpg");
    let w = rotate(&src, Rotation::Right, &out, &quiet, &never()).unwrap();
    assert_eq!((w.width, w.height), (48, 64));
    let turned = decoded(&out);
    let expected = image::imageops::rotate90(&decoded(&src));
    assert!(
        mean_error(&turned, &expected) < 0.5,
        "{}",
        mean_error(&turned, &expected)
    );
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(
        jpeg::quality(&bytes),
        jpeg::quality(&original),
        "same tables: not re-encoded"
    );
    assert_eq!(jpeg::orientation(&bytes), Orientation::Normal);
    let seg = jpeg::markers::segments(&bytes).unwrap();
    assert!(
        has_gps(&jpeg::markers::exif(&seg).unwrap()),
        "rotation keeps metadata"
    );
    assert_eq!(jpeg::markers::icc(&seg).as_deref(), Some(&b"a profile"[..]));

    // Turning back undoes it exactly.
    let back = dir.path("back.jpg");
    rotate(&out, Rotation::Left, &back, &quiet, &never()).unwrap();
    assert!(mean_error(&decoded(&back), &decoded(&src)) < 0.5);
}

#[test]
fn rotation_composes_with_the_photo_s_own_orientation() {
    let dir = Scratch::new("compose");
    // Stored sideways and tagged "turn right to view": turning it left shows the stored pixels.
    let stored = photo(64, 48, false);
    let src = dir.file("in.jpg", &jpeg_of(&stored, Some(&sample::exif(6)), None));
    let out = dir.path("out.jpg");
    let w = rotate(&src, Rotation::Left, &out, &quiet, &never()).unwrap();
    assert_eq!((w.width, w.height), (64, 48));
    assert!(mean_error(&decoded(&out), &decoded(&src)) < 0.5);
}

#[test]
fn rotation_re_encodes_when_the_edge_splits_a_block() {
    let dir = Scratch::new("unaligned");
    let img = photo(70, 50, false);
    let src = dir.file("in.jpg", &jpeg_of(&img, None, None));
    let out = dir.path("out.jpg");
    let w = rotate(&src, Rotation::FlipHorizontal, &out, &quiet, &never()).unwrap();
    assert_eq!((w.width, w.height), (70, 50));
    let expected = image::imageops::flip_horizontal(&decoded(&src));
    assert!(mean_error(&decoded(&out), &expected) < 3.0);
    // Other formats turn their pixels.
    let png = dir.file("in.png", &png_of(&photo(30, 20, true)));
    rotate(&png, Rotation::Left, &out, &quiet, &never()).unwrap();
    assert_eq!(
        decoded(&out),
        image::imageops::rotate270(&photo(30, 20, true))
    );
}

#[test]
fn removing_metadata_drops_the_location_but_not_the_colour_or_the_way_up() {
    let dir = Scratch::new("strip");
    let img = photo(64, 48, false);
    let src = dir.file(
        "in.jpg",
        &jpeg_of(&img, Some(&sample::exif(1)), Some(b"profile")),
    );
    let out = dir.path("out.jpg");
    strip_metadata(&src, &out, &quiet, &never()).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    let seg = jpeg::markers::segments(&bytes).unwrap();
    assert!(jpeg::markers::exif(&seg).is_none());
    assert_eq!(jpeg::markers::icc(&seg).as_deref(), Some(&b"profile"[..]));
    assert_eq!(decoded(&out), decoded(&src), "nothing re-encoded");

    // A photo shown turned by its tag stays turned once the tag is gone.
    let tagged = dir.file("tagged.jpg", &jpeg_of(&img, Some(&sample::exif(6)), None));
    let w = strip_metadata(&tagged, &out, &quiet, &never()).unwrap();
    assert_eq!((w.width, w.height), (48, 64));
    assert!(mean_error(&decoded(&out), &image::imageops::rotate90(&decoded(&src))) < 0.5);

    // When turning would split a block, a bare orientation tag stays and nothing is re-encoded.
    let odd = jpeg_of(&photo(70, 50, false), Some(&sample::exif(3)), None);
    let tagged = dir.file("odd.jpg", &odd);
    strip_metadata(&tagged, &out, &quiet, &never()).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    let exif = jpeg::markers::exif(&jpeg::markers::segments(&bytes).unwrap()).unwrap();
    assert_eq!((exif::orientation(&exif), has_gps(&exif)), (Some(3), false));
    assert_eq!(decoded(&out), decoded(&tagged));

    let png = png::with_metadata(&png_of(&img), Some(b"profile"), Some(&sample::exif(1))).unwrap();
    let src = dir.file("in.png", &png);
    strip_metadata(&src, &out, &quiet, &never()).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    assert!(png::exif(&bytes).is_none() && png::icc(&bytes).is_some());
    assert_eq!(decoded(&out), img);

    let webp = encode::encode(
        &img,
        Format::Webp,
        80,
        Meta {
            icc: Some(b"profile"),
            exif: Some(&sample::exif(1)),
        },
    )
    .unwrap();
    assert!(webp::exif(&webp).is_some_and(|e| has_gps(&e)));
    let src = dir.file("in.webp", &webp);
    strip_metadata(&src, &out, &quiet, &never()).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    assert!(webp::exif(&bytes).is_none() && webp::icc(&bytes).is_some());
}

#[test]
fn outputs_carry_metadata_upright_and_resized() {
    let dir = Scratch::new("carry");
    let src = dir.file(
        "in.jpg",
        &jpeg_of(
            &photo(64, 48, false),
            Some(&sample::exif(6)),
            Some(b"profile"),
        ),
    );
    let out = dir.path("out.jpg");
    let w = resize(&src, Resize::Percent(50), &out, &quiet, &never()).unwrap();
    assert_eq!(
        (w.width, w.height),
        (24, 32),
        "turned by its tag, then halved"
    );
    let bytes = std::fs::read(&out).unwrap();
    let exif = jpeg::markers::exif(&jpeg::markers::segments(&bytes).unwrap()).unwrap();
    assert_eq!(exif::orientation(&exif), Some(1));
    assert!(has_gps(&exif), "resizing isn't removing metadata");
}

#[test]
fn probe_reads_the_shown_size_and_alpha() {
    let dir = Scratch::new("probe");
    let src = dir.file(
        "in.jpg",
        &jpeg_of(&photo(64, 48, false), Some(&sample::exif(6)), None),
    );
    let info = probe(&src).unwrap();
    assert_eq!(
        (info.kind, info.width, info.height, info.has_alpha),
        (Kind::Jpeg, 48, 64, false)
    );
    let src = dir.file("in.png", &png_of(&photo(10, 10, true)));
    assert!(probe(&src).unwrap().has_alpha);
    let junk = dir.file("junk.png", b"not a picture");
    assert_eq!(probe(&junk).unwrap_err().kind(), io::ErrorKind::Unsupported);
}

#[test]
fn cancelling_stops_with_interrupted_and_progress_ends_at_one() {
    let dir = Scratch::new("cancel");
    let src = dir.file("in.png", &png_of(&photo(64, 48, false)));
    let err = convert(
        &src,
        &ConvertOptions::default(),
        &dir.path("o.jpg"),
        &quiet,
        &AtomicBool::new(true),
    )
    .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Interrupted);
    let seen = std::cell::RefCell::new(Vec::new());
    convert(
        &src,
        &ConvertOptions::default(),
        &dir.path("o.jpg"),
        &|p| seen.borrow_mut().push(p),
        &never(),
    )
    .unwrap();
    let seen = seen.into_inner();
    assert_eq!(seen.last(), Some(&1.0));
    assert!(seen.windows(2).all(|p| p[0] <= p[1]));
}
