//! Turning a model into the turntable topsy plays: Blender renders it in the shared studio with
//! `devices/render.py`, and the frames are packed into a `spin.png`.

use std::path::{Path, PathBuf};
use std::process::Command;

use image::imageops::{self, FilterType};
use image::{ImageFormat, RgbaImage};
use model::headset::Form;
use model::headset::spin::{FRAME, FRAMES, cell, grid};

fn devices() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../devices")
}

/// `BLENDER`, else `blender` on `PATH`, else Blender's own installer's folder.
fn blender() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("BLENDER") {
        return Ok(path.into());
    }
    let on_path = std::env::var_os("PATH").into_iter().flat_map(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("blender.exe"))
            .collect::<Vec<_>>()
    });
    let installed = std::fs::read_dir(r"C:\Program Files\Blender Foundation")
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path().join("blender.exe"));
    on_path
        .chain(installed)
        .find(|path| path.is_file())
        .ok_or_else(|| "no Blender: install it, or set BLENDER to blender.exe".to_owned())
}

fn run_blender(script: &Path, args: &[&Path]) -> Result<(), String> {
    let mut command = Command::new(blender()?);
    command
        .args(["--background", "--factory-startup", "--python"])
        .arg(script);
    if !args.is_empty() {
        command.arg("--").args(args);
    }
    let output = command.output().map_err(|err| err.to_string())?;
    let log = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() || log.contains("Traceback") {
        return Err(format!("{} failed:\n{log}", script.display()));
    }
    Ok(())
}

/// `folder/model.blend` (or `model.glb`) into `folder/spin.png`.
pub fn render(folder: &Path) -> Result<(), String> {
    let model = ["model.blend", "model.glb"]
        .into_iter()
        .map(|name| folder.join(name))
        .find(|path| path.is_file())
        .ok_or_else(|| format!("no model.blend or model.glb in {}", folder.display()))?;
    turntable(&model, folder)
}

/// `source`, a model file or a generic shape's module, into `out/spin.png`.
fn turntable(source: &Path, out: &Path) -> Result<(), String> {
    let frames = std::env::temp_dir().join(format!("topsy-render-{}", std::process::id()));
    std::fs::create_dir_all(&frames).map_err(|err| err.to_string())?;
    run_blender(&devices().join("render.py"), &[source, &frames])?;
    let (width, height) = grid();
    let mut sheet = RgbaImage::new(width, height);
    for index in 0..FRAMES {
        let path = frames.join(format!("{index:02}.png"));
        let frame = image::open(&path)
            .map_err(|err| format!("{}: {err}", path.display()))?
            .into_rgba8();
        let frame = imageops::resize(&frame, FRAME, FRAME, FilterType::Lanczos3);
        let (x, y) = cell(index);
        imageops::replace(&mut sheet, &frame, x.into(), y.into());
    }
    std::fs::remove_dir_all(&frames).map_err(|err| err.to_string())?;
    let out = out.join("spin.png");
    sheet
        .save_with_format(&out, ImageFormat::Png)
        .map_err(|err| format!("{}: {err}", out.display()))?;
    println!("{}", out.display());
    Ok(())
}

/// Render each generic shape, or only `only`, from its module (`devices/generic/over_ear.py`
/// for `over-ear`) into `devices/generic/<shape>/spin.png`.
pub fn generic(only: Option<&str>) -> Result<(), String> {
    let generic = devices().join("generic");
    let forms: Vec<Form> = match only {
        Some(slug) => vec![
            Form::ALL
                .into_iter()
                .find(|form| form.slug() == slug)
                .ok_or_else(|| {
                    format!("no shape {slug:?}: stem-buds, buds, over-ear or speaker")
                })?,
        ],
        None => Form::ALL.to_vec(),
    };
    forms.iter().try_for_each(|form| {
        let module = generic.join(format!("{}.py", form.slug().replace('-', "_")));
        let out = generic.join(form.slug());
        std::fs::create_dir_all(&out).map_err(|err| err.to_string())?;
        turntable(&module, &out)
    })
}
