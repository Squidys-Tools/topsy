//! Runs the real model: `cargo test --release background -- --ignored --nocapture`. Downloads
//! it first if it isn't there (177 MB), so these need the network once.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use image::{Rgb, RgbImage};

use super::*;

fn ensure_model() {
    if model_status() != ModelStatus::Ready {
        let started = Instant::now();
        download(&|_| {}, &AtomicBool::new(false)).expect("download the model");
        eprintln!("downloaded in {:.1?}", started.elapsed());
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("topsy-background-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// An orange disc on a busy blue-grey checker-and-stripe background.
fn scene(w: u32, h: u32) -> (RgbImage, impl Fn(u32, u32) -> f32) {
    let (cx, cy, r) = (w as f32 * 0.5, h as f32 * 0.55, h as f32 * 0.3);
    let inside = move |x: u32, y: u32| (x as f32 - cx).hypot(y as f32 - cy) - r;
    let img = RgbImage::from_fn(w, h, |x, y| {
        if inside(x, y) < 0.0 {
            Rgb([235, 120, 30])
        } else {
            let check = ((x / 40 + y / 40) % 2) as u8 * 50;
            let stripe = ((x + 2 * y) % 23 < 4) as u8 * 40;
            Rgb([60 + check, 80 + stripe, 110 + check])
        }
    });
    (img, inside)
}

#[test]
#[ignore = "downloads and runs the 177 MB model"]
fn cuts_a_shape_off_a_textured_background() {
    ensure_model();
    let (w, h) = (1600, 1200);
    let (img, inside) = scene(w, h);
    let src = scratch("scene.png");
    img.save(&src).unwrap();
    let out = scratch("scene-cut.png");
    let started = Instant::now();
    remove(&src, &out, &|_| {}, &AtomicBool::new(false)).unwrap();
    eprintln!("first remove (with model load): {:.1?}", started.elapsed());
    let cut = image::open(&out).unwrap().to_rgba8();
    assert_eq!(cut.dimensions(), (w, h));
    let (mut subject, mut n_subject, mut background, mut n_background) = (0.0, 0.0, 0.0, 0.0);
    for (x, y, px) in cut.enumerate_pixels() {
        let d = inside(x, y);
        if d < -8.0 {
            subject += px.0[3] as f64;
            n_subject += 1.0;
        } else if d > 8.0 {
            background += px.0[3] as f64;
            n_background += 1.0;
        }
    }
    let (subject, background) = (
        subject / n_subject / 255.0,
        background / n_background / 255.0,
    );
    eprintln!("mean alpha: subject {subject:.3}, background {background:.3}");
    assert!(subject > 0.95, "subject {subject}");
    assert!(background < 0.05, "background {background}");
    // Kept colour inside the subject.
    let centre = cut.get_pixel(w / 2, h * 55 / 100).0;
    assert!(centre[0] > 200 && centre[2] < 80, "{centre:?}");
}

#[test]
#[ignore = "cancels a real run"]
fn a_set_cancel_flag_stops_the_run() {
    ensure_model();
    let (img, _) = scene(640, 480);
    let src = scratch("cancel.png");
    img.save(&src).unwrap();
    let err = remove(
        &src,
        &scratch("cancel-cut.png"),
        &|_| {},
        &AtomicBool::new(true),
    )
    .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Interrupted);
}

/// Cuts every image in `TOPSY_BG_PHOTOS` (a folder) to `<name>.cut.png` beside it.
#[test]
#[ignore = "runs on photos you point it at"]
fn cut_photos() {
    let Some(dir) = std::env::var_os("TOPSY_BG_PHOTOS") else {
        return;
    };
    ensure_model();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name.contains(".cut.") || path.is_dir() {
            continue;
        }
        let out = path.with_file_name(format!("{name}.cut.png"));
        let started = Instant::now();
        remove(&path, &out, &|_| {}, &AtomicBool::new(false)).unwrap();
        eprintln!("{name}: {:.2?}", started.elapsed());
    }
}
