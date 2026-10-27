//! rustc's bundled MinGW (used for the `x86_64-pc-windows-gnu` target) ships only a subset of
//! the Windows import libraries, and GPUI's dependencies ask for one it lacks (`ktmw32`).
//!
//! Rather than switch to the system gcc's whole runtime (mixing two MinGW versions is asking
//! for trouble), copy just the missing import libraries from a system MinGW-w64 into the build
//! output directory and add that to the link search path.

use std::path::PathBuf;
use std::process::Command;

/// Import libraries GPUI's dependency tree needs that rustc's bundled MinGW doesn't have.
const MISSING_IMPORT_LIBS: &[&str] = &["libktmw32.a"];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

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
