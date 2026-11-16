use std::path::{Path, PathBuf};

use super::*;
use crate::model::exif::has_gps;
use crate::model::image_geometry::{LargestSide, Resize, Rotation};
use crate::model::image_ops::{CompressBy, CompressOptions, ConvertOptions, Format};
use crate::model::jpeg;
use crate::transform::image::{compress, convert, probe, resize, rotate, strip_metadata};

/// Every operation on real photos, timed. `TOPSY_PHOTOS` names a folder of them; the outputs go
/// to its `out` folder for a look.
#[test]
#[ignore]
fn manual_run_on_real_photos() {
    use std::time::Instant;
    let dir = PathBuf::from(std::env::var("TOPSY_PHOTOS").expect("TOPSY_PHOTOS"));
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let report = |what: &str, t: Instant, path: &Path| {
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        eprintln!(
            "  {what:<28} {:>9} bytes {:>7.2} s",
            size,
            t.elapsed().as_secs_f64()
        );
    };
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        let src = entry.path();
        if !src.is_file() || probe(&src).is_err() {
            continue;
        }
        let stem = src.file_stem().unwrap().to_string_lossy().to_string();
        let info = probe(&src).unwrap();
        eprintln!("{stem}: {info:?}");
        let o = |name: &str| out.join(format!("{stem}-{name}"));
        let t = Instant::now();
        compress(
            &src,
            &CompressOptions::default(),
            &o("compress-default.jpg"),
            &quiet,
            &never(),
        )
        .unwrap();
        report("compress 75% 1920", t, &o("compress-default.jpg"));
        let t = Instant::now();
        let full = CompressOptions {
            largest_side: LargestSide::Original,
            ..Default::default()
        };
        compress(&src, &full, &o("compress-full.jpg"), &quiet, &never()).unwrap();
        report("compress 75% original size", t, &o("compress-full.jpg"));
        let t = Instant::now();
        let sized = CompressOptions {
            by: CompressBy::FileSize,
            target_bytes: 500_000,
            largest_side: LargestSide::Original,
            ..Default::default()
        };
        let w = compress(&src, &sized, &o("compress-500k.jpg"), &quiet, &never()).unwrap();
        report(
            &format!("compress to 500 kB ({}x{})", w.width, w.height),
            t,
            &o("compress-500k.jpg"),
        );
        for format in Format::ALL {
            let name = format!("convert.{}", format.extension());
            let t = Instant::now();
            match convert(
                &src,
                &ConvertOptions {
                    format,
                    quality: 75,
                },
                &o(&name),
                &quiet,
                &never(),
            ) {
                Ok(_) => report(&format!("convert {}", format.label()), t, &o(&name)),
                Err(e) => eprintln!("  convert {}: {e}", format.label()),
            }
        }
        let t = Instant::now();
        resize(
            &src,
            Resize::Percent(50),
            &o("resize-50.jpg"),
            &quiet,
            &never(),
        )
        .unwrap();
        report("resize 50%", t, &o("resize-50.jpg"));
        let t = Instant::now();
        rotate(
            &src,
            Rotation::Right,
            &o("rotate-right.jpg"),
            &quiet,
            &never(),
        )
        .unwrap();
        report("rotate right", t, &o("rotate-right.jpg"));
        let t = Instant::now();
        strip_metadata(&src, &o("stripped.jpg"), &quiet, &never()).unwrap();
        report("remove metadata", t, &o("stripped.jpg"));
        let stripped = std::fs::read(o("stripped.jpg")).unwrap();
        let gps = jpeg::markers::segments(&stripped)
            .ok()
            .and_then(|s| jpeg::markers::exif(&s))
            .is_some_and(|e| has_gps(&e));
        eprintln!("  GPS after removing metadata: {gps}");
        let t = Instant::now();
        let pages = crate::transform::pdf::combine(
            &[src.clone(), src.clone()],
            crate::model::pdf::PageSize::A4,
            &o("combined.pdf"),
            &quiet,
            &never(),
        )
        .unwrap();
        report(
            &format!("combine x2 ({pages} pages)"),
            t,
            &o("combined.pdf"),
        );
        let t = Instant::now();
        let how = crate::model::pdf::PdfCompress::Quality(crate::model::pdf::PdfQuality::Balanced);
        let w = crate::transform::pdf::compress(
            &o("combined.pdf"),
            how,
            &o("combined-small.pdf"),
            &quiet,
            &never(),
        )
        .unwrap();
        report(
            &format!("compress PDF (kept {})", w.kept_original),
            t,
            &o("combined-small.pdf"),
        );
        let t = Instant::now();
        let named = |i: usize| Ok(o(&format!("page-{}.png", i + 1)));
        let n = crate::transform::pdf::to_images(
            &o("combined-small.pdf"),
            crate::model::pdf::PageFormat::Png,
            150,
            &named,
            &quiet,
            &never(),
        )
        .unwrap();
        report(&format!("PDF to {n} PNGs at 150 dpi"), t, &o("page-1.png"));
    }
}
