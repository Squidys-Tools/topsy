use image::Rgba;

use super::*;
use crate::model::image_ops::{
    CompressBy, CompressFormat, CompressOptions, ConvertOptions, Format,
};
use crate::transform::image::{compress, convert, pixels};
use crate::win32::{wic, with_com};
use windows::Win32::System::Com::COINIT_MULTITHREADED;

#[test]
fn compress_by_quality_caps_the_size_and_keeps_the_look() {
    let dir = Scratch::new("cq");
    let src = dir.file("in.png", &png_of(&photo(2400, 1600, false)));
    let out = dir.path("out.jpg");
    let w = compress(&src, &CompressOptions::default(), &out, &quiet, &never()).unwrap();
    assert_eq!((w.format, w.width, w.height), (Format::Jpeg, 1920, 1280));
    let back = decoded(&out);
    let reference = pixels::scale(&photo(2400, 1600, false), 1920, 1280);
    assert!(mean_error(&back, &reference) < 3.0);
    assert!(
        w.bytes < std::fs::metadata(&src).unwrap().len() / 4,
        "{} bytes",
        w.bytes
    );
}

#[test]
fn compress_by_file_size_lands_under_the_target_in_every_lossy_format() {
    let dir = Scratch::new("cs");
    let src = dir.file("in.png", &png_of(&photo(800, 600, false)));
    for (format, target) in [
        (CompressFormat::Jpeg, 40_000),
        (CompressFormat::Webp, 30_000),
        (CompressFormat::Png, 150_000),
    ] {
        let options = CompressOptions {
            by: CompressBy::FileSize,
            target_bytes: target,
            format,
            ..Default::default()
        };
        let out = dir.path("out");
        let w = compress(&src, &options, &out, &quiet, &never()).unwrap();
        let size = std::fs::metadata(&out).unwrap().len();
        assert!(
            w.fits && size <= target && size == w.bytes,
            "{format:?}: {size} for {target}"
        );
        assert!(
            size > target / 2,
            "{format:?}: {size} is far under {target}"
        );
        assert_eq!(decoded(&out).dimensions(), (w.width, w.height));
    }
    // Too small for any quality at full size: it shrinks the picture instead.
    let options = CompressOptions {
        by: CompressBy::FileSize,
        target_bytes: 2_500,
        format: CompressFormat::Jpeg,
        ..Default::default()
    };
    let w = compress(&src, &options, &dir.path("tiny.jpg"), &quiet, &never()).unwrap();
    assert!(w.fits && w.bytes <= 2_500 && w.width < 800, "{w:?}");
}

#[test]
fn most_compatible_keeps_transparency_as_png() {
    let dir = Scratch::new("alpha");
    let src = dir.file("in.png", &png_of(&photo(300, 200, true)));
    let out = dir.path("out.png");
    let options = CompressOptions {
        quality: 100,
        ..Default::default()
    };
    let w = compress(&src, &options, &out, &quiet, &never()).unwrap();
    assert_eq!(w.format, Format::Png);
    assert_eq!(
        decoded(&out),
        photo(300, 200, true),
        "quality 100 PNG is lossless"
    );
}

#[test]
fn compressing_never_hands_back_a_bigger_file_in_the_same_format() {
    let dir = Scratch::new("bigger");
    let small = jpeg_of(&photo(320, 240, false), None, None);
    let src = dir.file("in.jpg", &small);
    let options = CompressOptions {
        quality: 100,
        format: CompressFormat::SameAsOriginal,
        ..Default::default()
    };
    let out = dir.path("out.jpg");
    compress(&src, &options, &out, &quiet, &never()).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), small);
}

#[test]
fn convert_writes_every_format_and_reads_back() {
    let dir = Scratch::new("convert");
    let img = photo(256, 192, true);
    let src = dir.file("in.png", &png_of(&img));
    let caps = wic::capabilities();
    for format in Format::ALL {
        if format == Format::Heic && !caps.heic_write {
            continue;
        }
        let out = dir.path(&format!("out.{}", format.extension()));
        let w = convert(
            &src,
            &ConvertOptions {
                format,
                quality: 90,
            },
            &out,
            &quiet,
            &never(),
        )
        .unwrap();
        let bytes = std::fs::read(&out).unwrap();
        assert_eq!(w.bytes as usize, bytes.len());
        let back = match format {
            Format::Pdf => {
                assert_eq!(
                    lopdf::Document::load_mem(&bytes).unwrap().get_pages().len(),
                    1
                );
                continue;
            }
            Format::Heic | Format::Avif if !caps.heic_read || !caps.avif_read => continue,
            Format::Heic | Format::Avif => {
                with_com(COINIT_MULTITHREADED, || wic::decode(&out))
                    .unwrap()
                    .image
            }
            Format::Ico => {
                let icon =
                    image::load_from_memory_with_format(&bytes, image::ImageFormat::Ico).unwrap();
                assert_eq!(icon.width(), 256, "the largest size is first choice");
                continue;
            }
            _ => decoded(&out),
        };
        assert_eq!(back.dimensions(), (256, 192), "{format:?}");
        let visible = |p: &Rgba<u8>| p.0[3] == 255;
        let err: f64 = back
            .pixels()
            .zip(img.pixels())
            .filter(|(_, o)| visible(o))
            .map(|(a, b)| {
                (0..3)
                    .map(|c| f64::from(a.0[c].abs_diff(b.0[c])))
                    .sum::<f64>()
                    / 3.0
            })
            .sum::<f64>()
            / img.pixels().filter(|p| visible(p)).count() as f64;
        let limit = if format == Format::Gif { 12.0 } else { 6.0 };
        assert!(err < limit, "{format:?}: mean error {err}");
        if format == Format::Avif {
            // Windows' AV1 decoder shows only the colour; the alpha is an auxiliary picture.
            assert!(
                bytes.windows(9).any(|w| w == b"auxiliary"),
                "AVIF carries an alpha plane"
            );
        } else if format.keeps_alpha() && format != Format::Heic {
            assert_eq!(back.get_pixel(128, 96).0[3], 0, "{format:?} keeps the hole");
        }
    }
}

#[test]
fn icons_carry_every_size_up_to_the_picture() {
    let dir = Scratch::new("ico");
    let src = dir.file("in.png", &png_of(&photo(40, 20, false)));
    let out = dir.path("out.ico");
    convert(
        &src,
        &ConvertOptions {
            format: Format::Ico,
            quality: 90,
        },
        &out,
        &quiet,
        &never(),
    )
    .unwrap();
    let bytes = std::fs::read(&out).unwrap();
    let count = u16::from_le_bytes([bytes[4], bytes[5]]);
    let sizes: Vec<u8> = (0..count as usize).map(|i| bytes[6 + i * 16]).collect();
    assert_eq!(sizes, vec![16, 24, 32]);
}
