//! rustc's bundled MinGW (used for the `x86_64-pc-windows-gnu` target) ships only a subset of
//! the Windows import libraries, and GPUI's dependencies ask for one it lacks (`ktmw32`).
//!
//! Rather than switch to the system gcc's whole runtime (mixing two MinGW versions is asking
//! for trouble), copy just the missing import libraries from a system MinGW-w64 into the build
//! output directory and add that to the link search path.
//!
//! It also gives the exe its icon: the pill from `src/model/pill.rs`, written as a binary `.res`
//! and turned into an object by the same MinGW's `windres`. No `.rc` script, so no preprocessor.

use std::path::PathBuf;
use std::process::Command;

#[path = "src/model/pill.rs"]
mod pill;

/// Import libraries GPUI's dependency tree needs that rustc's bundled MinGW doesn't have.
const MISSING_IMPORT_LIBS: &[&str] = &["libktmw32.a"];

/// Resource id 1: GPUI's windows load their icon from it, and so does the tray icon.
const ICON_ID: u16 = 1;
const ICON_SIZES: &[usize] = &[16, 20, 24, 32, 40, 48, 64, 256];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/model/pill.rs");

    let target_is_gnu_windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu");
    if !target_is_gnu_windows {
        return;
    }

    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let mut copied_any = false;
    for lib in MISSING_IMPORT_LIBS {
        match find_in_system_mingw(lib) {
            Some(path) => {
                std::fs::copy(&path, out_dir.join(lib)).expect("copy import library");
                copied_any = true;
            }
            None => println!(
                "cargo:warning={lib} not found: install MinGW-w64 gcc (e.g. `scoop install gcc` \
                 or MSYS2) and make sure `gcc` is on PATH, or linking will fail"
            ),
        }
    }
    if copied_any {
        println!("cargo:rustc-link-search=native={}", out_dir.display());
    }

    let res = out_dir.join("topsy.res");
    std::fs::write(&res, icon_res()).expect("write topsy.res");
    let obj = out_dir.join("topsy-res.o");
    let status = Command::new("windres")
        .args([
            "--input-format=res",
            "--output-format=coff",
            "--target=pe-x86-64",
        ])
        .arg("-i")
        .arg(&res)
        .arg("-o")
        .arg(&obj)
        .status();
    if !matches!(status, Ok(s) if s.success()) {
        panic!(
            "windres failed or is missing: it comes with MinGW-w64 gcc, which must be on PATH \
             (see README, Building)"
        );
    }
    // Every link, tests included, so a test can check the icon made it in.
    println!("cargo:rustc-link-arg={}", obj.display());
}

/// A `.res` file: an empty first entry, one RT_ICON per size, then the RT_GROUP_ICON listing them.
fn icon_res() -> Vec<u8> {
    let mut out = Vec::new();
    resource(&mut out, 0, 0, &[]);
    let mut group = vec![0, 0, 1, 0];
    group.extend_from_slice(&(ICON_SIZES.len() as u16).to_le_bytes());
    for (i, &size) in ICON_SIZES.iter().enumerate() {
        let image = dib(size);
        let id = ICON_ID + 1 + i as u16;
        let side = if size >= 256 { 0 } else { size as u8 };
        group.extend_from_slice(&[side, side, 0, 0, 1, 0, 32, 0]);
        group.extend_from_slice(&(image.len() as u32).to_le_bytes());
        group.extend_from_slice(&id.to_le_bytes());
        resource(&mut out, 3, id, &image);
    }
    resource(&mut out, 14, ICON_ID, &group);
    out
}

/// One `.res` entry with a numeric type and name, padded to a DWORD.
fn resource(out: &mut Vec<u8>, kind: u16, name: u16, data: &[u8]) {
    let header: [u32; 8] = [
        data.len() as u32,
        32,
        0xffff | (kind as u32) << 16,
        0xffff | (name as u32) << 16,
        0,
        // Moveable, pure, discardable; language neutral.
        0x1030,
        0,
        0,
    ];
    for word in header {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(data);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

/// An icon image as a DIB: a header with doubled height, the colour rows bottom-up, then an
/// all-clear AND mask (ignored when the colour has alpha, but its space is expected).
fn dib(size: usize) -> Vec<u8> {
    let pixels = pill::pill(size);
    let mask_row = size.div_ceil(32) * 4;
    let mut out = Vec::new();
    for word in [40u32, size as u32, 2 * size as u32] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(&[1, 0, 32, 0]);
    let image_size = (size * size * 4 + mask_row * size) as u32;
    for word in [0u32, image_size, 0, 0, 0, 0] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    for row in pixels.chunks(size * 4).rev() {
        out.extend_from_slice(row);
    }
    out.resize(out.len() + mask_row * size, 0);
    out
}

/// Ask the system gcc where it keeps `lib`. `gcc -print-file-name` echoes the bare name back
/// when it can't find the file, so an existing absolute path is the success signal.
fn find_in_system_mingw(lib: &str) -> Option<PathBuf> {
    let out = Command::new("gcc")
        .arg(format!("-print-file-name={lib}"))
        .output()
        .ok()?;
    let path = PathBuf::from(String::from_utf8(out.stdout).ok()?.trim());
    (path.is_absolute() && path.exists()).then_some(path)
}
