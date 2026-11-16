//! Writing HEIC: ImageMagick draws the picture as a PNG, FFmpeg's x265 encodes it (and its alpha,
//! when a pixel is see-through), and `model::heif` packs the frames with the colour profile and
//! EXIF.

use std::ffi::OsStr;
use std::io;
use std::path::Path;

use super::Scratch;
use super::tools::{Kit, path};
use crate::model::heif::{self, Heic, Plane, Still};
use crate::model::magick::{self, Job, Metadata};
use crate::transform::Steps;
use crate::transform::tools;
use crate::transform::video::FFMPEG;

pub(super) fn write(kit: &Kit, steps: &Steps, src: &Path, job: &Job, out: &Path) -> io::Result<()> {
    let ffmpeg = steps.tool(&FFMPEG, &|p| !p.ends_with("ffplay.exe"))?;
    let png = Scratch::new("png");
    let drawn = Job {
        quality: None,
        ..*job
    };
    kit.magick(&magick::args(&drawn, &path(src), &path(&png.0)), steps)?;
    let picture = image::open(&png.0).map_err(|_| failed())?.into_rgba8();
    let see_through = picture.pixels().any(|p| p[3] < 255);
    let (width, height) = picture.dimensions();
    drop(picture);
    let profile = |kind: &str| {
        let file = Scratch::new(kind);
        let args = [path(&png.0), format!("{kind}:{}", path(&file.0))];
        kit.magick(&args, steps).ok()?;
        std::fs::read(&file.0).ok().filter(|b| !b.is_empty())
    };
    let icc = profile("ICC");
    let exif = match job.metadata {
        Metadata::Keep => profile("EXIF"),
        Metadata::Remove => None,
    };
    let crf = heif::crf(job.quality.unwrap_or(90));
    let encode = |plane| -> io::Result<Still> {
        let mp4 = Scratch::new("mp4");
        let args = heif::encode_args(&path(&png.0), &path(&mp4.0), crf, plane);
        let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        tools::run(&ffmpeg, &args, steps.cancel(), &mut |_| {}).map_err(|e| match e.kind() {
            io::ErrorKind::Interrupted => e,
            _ => {
                crate::log(&format!("transform: heic: {e}"));
                failed()
            }
        })?;
        heif::from_mp4(&std::fs::read(&mp4.0)?).ok_or_else(failed)
    };
    let colour = encode(Plane::Colour)?;
    let alpha = see_through.then(|| encode(Plane::Alpha)).transpose()?;
    steps.check()?;
    std::fs::write(
        out,
        heif::write(&Heic {
            width,
            height,
            colour: &colour,
            alpha: alpha.as_ref(),
            icc: icc.as_deref(),
            exif: exif.as_deref(),
        }),
    )
}

fn failed() -> io::Error {
    io::Error::other("This picture couldn't be converted")
}
