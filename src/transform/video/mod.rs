//! Video, audio and GIF transforms, done by FFmpeg: downloaded on first use (see `tools`), run
//! as a process without a window. The command lines and the rules behind them are in
//! `crate::model::video`; this is the glue that runs them.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use image::RgbaImage;

use super::fetch::{Pinned, Status};
use super::tools::{self, Tool};
use crate::log::{LogErr, MissingOk};
use crate::model::video::{
    self as rules, CompressBy, CompressVideo, ConvertTarget, GifOptions, Probe, Progress, Rate,
    Rotate, TrimMode, TrimRange, megabytes, retry_bitrate,
};

/// Gyan Doshi's "essentials" build of the FFmpeg 9.0.2 release, GPL v3, statically linked:
/// libx264, libx265, libvpx, libopus and libmp3lame are all inside ffmpeg.exe.
pub const FFMPEG: Tool = Tool {
    archive: Pinned {
        label: "FFmpeg",
        host: "github.com",
        path: "/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip",
        size: 114_768_076,
        sha256: "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba",
        file: r"downloads\ffmpeg-9.0.2-essentials_build.zip",
    },
    dir: "ffmpeg-9.0.2",
    program: r"ffmpeg-9.0.2-essentials_build\bin\ffmpeg.exe",
};

pub fn status() -> Status {
    FFMPEG.status()
}

/// Download FFmpeg if it isn't here yet. Every operation does this itself too.
pub fn prepare(progress: &dyn Fn(f32), cancel: &AtomicBool) -> io::Result<()> {
    ffmpeg(progress, cancel).map(drop)
}

fn ffmpeg(progress: &dyn Fn(f32), cancel: &AtomicBool) -> io::Result<PathBuf> {
    let program = FFMPEG.get(progress, cancel)?;
    // The build ships a player too, a 100 MB file nothing here runs.
    fs::remove_file(program.with_file_name("ffplay.exe"))
        .missing_ok()
        .log_err();
    Ok(program)
}

fn ffprobe(ffmpeg: &Path) -> PathBuf {
    ffmpeg.with_file_name("ffprobe.exe")
}

fn not_ready() -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, "FFmpeg isn't downloaded yet")
}

/// What the file at `path` holds. FFmpeg has to be here already (see `prepare`).
pub fn probe(path: &Path) -> io::Result<Probe> {
    let program = FFMPEG.program()?;
    if !program.is_file() {
        return Err(not_ready());
    }
    probe_with(&program, path, &AtomicBool::new(false))
}

fn probe_with(ffmpeg: &Path, path: &Path, cancel: &AtomicBool) -> io::Result<Probe> {
    let mut args: Vec<&OsStr> = rules::PROBE_ARGS.iter().map(OsStr::new).collect();
    args.push(path.as_os_str());
    let unreadable = || io::Error::new(io::ErrorKind::InvalidData, "FFmpeg can't read this file");
    let json = tools::read(&ffprobe(ffmpeg), &args, cancel).map_err(|e| match e.kind() {
        io::ErrorKind::Interrupted => e,
        _ => unreadable(),
    })?;
    rules::parse_probe(&String::from_utf8_lossy(&json)).ok_or_else(unreadable)
}

/// FFmpeg, and what it says about `src`, which must have a picture when `video`.
fn ready(
    src: &Path,
    video: bool,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<(PathBuf, Probe)> {
    let program = ffmpeg(progress, cancel)?;
    let probe = probe_with(&program, src, cancel)?;
    if video && !probe.has_video() {
        return Err(invalid("This file has no picture"));
    }
    if !video && !probe.has_audio() {
        return Err(invalid("This file has no sound"));
    }
    Ok((program, probe))
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

/// Run FFmpeg with `args`, reporting progress through an output `total` long. A failure
/// becomes `what` went wrong; FFmpeg's own words go to the log.
fn run(
    ffmpeg: &Path,
    args: &[OsString],
    total: Duration,
    what: &str,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<()> {
    let tracker = Progress::new(total);
    let args: Vec<&OsStr> = args.iter().map(OsString::as_os_str).collect();
    progress(0.0);
    tools::run(ffmpeg, &args, cancel, &mut |line| {
        if let Some(f) = tracker.line(line) {
            progress(f);
        }
    })
    .map_err(|e| match e.kind() {
        io::ErrorKind::Interrupted => e,
        _ => {
            crate::log(&format!("{what}: {e}"));
            io::Error::other(format!("Couldn't {what}"))
        }
    })
}

fn size_of(path: &Path) -> io::Result<u64> {
    Ok(fs::metadata(path)?.len())
}

fn over_target(target: u64) -> io::Error {
    io::Error::other(format!(
        "Couldn't get it under {}; try a larger size",
        megabytes(target)
    ))
}

/// H.264 MP4 with AAC. A size target that comes out over gets one more pass at a bitrate
/// scaled by the overshoot.
pub fn compress_video(
    src: &Path,
    opts: CompressVideo,
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<()> {
    let (ff, probe) = ready(src, true, progress, cancel)?;
    let mut plan = rules::compress_plan(opts, &probe).map_err(|e| invalid(e.to_string()))?;
    let what = "compress this video";
    let args = rules::compress_video(src, out, &plan);
    run(&ff, &args, probe.duration, what, progress, cancel)?;
    let (CompressBy::FileSize(target), Rate::Bitrate(bps)) = (opts.by, plan.rate) else {
        return Ok(());
    };
    let actual = size_of(out)?;
    if actual <= target {
        return Ok(());
    }
    plan.rate = Rate::Bitrate(retry_bitrate(bps, target, actual));
    let args = rules::compress_video(src, out, &plan);
    run(&ff, &args, probe.duration, what, progress, cancel)?;
    if size_of(out)? > target {
        return Err(over_target(target));
    }
    Ok(())
}

/// Into `AudioFormat::for_compress(src)`, whose extension `out` must have.
pub fn compress_audio(
    src: &Path,
    by: CompressBy,
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<()> {
    let (ff, probe) = ready(src, false, progress, cancel)?;
    let kbps = rules::compress_audio_kbps(by, &probe).map_err(|e| invalid(e.to_string()))?;
    let what = "compress this audio";
    run(
        &ff,
        &rules::compress_audio(src, out, kbps),
        probe.duration,
        what,
        progress,
        cancel,
    )?;
    if let CompressBy::FileSize(target) = by
        && size_of(out)? > target
    {
        return Err(over_target(target));
    }
    Ok(())
}

/// Into `target`, whose extension `out` must have. Streams that already fit are copied.
pub fn convert(
    src: &Path,
    target: ConvertTarget,
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<()> {
    let video = matches!(target, ConvertTarget::Video(_));
    let (ff, probe) = ready(src, video, progress, cancel)?;
    let args = rules::convert(src, out, target, &probe);
    run(
        &ff,
        &args,
        probe.duration,
        "convert this file",
        progress,
        cancel,
    )
}

/// `out` has `src`'s extension for `TrimMode::Fast`, else `edited_extension`.
pub fn trim(
    src: &Path,
    range: TrimRange,
    mode: TrimMode,
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<()> {
    let program = ffmpeg(progress, cancel)?;
    let probe = probe_with(&program, src, cancel)?;
    let args = rules::trim(src, out, range, mode, &probe);
    run(
        &program,
        &args,
        range.len(),
        "trim this clip",
        progress,
        cancel,
    )
}

pub fn make_gif(
    src: &Path,
    opts: &GifOptions,
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<()> {
    let range = opts.range.for_gif().map_err(|e| invalid(e.to_string()))?;
    let (ff, probe) = ready(src, true, progress, cancel)?;
    let args = rules::gif(src, out, opts, &probe);
    run(&ff, &args, range.len(), "make the GIF", progress, cancel)
}

/// `out` has `edited_extension(src, true)`.
pub fn rotate(
    src: &Path,
    turn: Rotate,
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<()> {
    let (ff, probe) = ready(src, true, progress, cancel)?;
    let args = rules::rotate(src, out, turn, &probe);
    run(
        &ff,
        &args,
        probe.duration,
        "rotate this video",
        progress,
        cancel,
    )
}

/// `out` has `src`'s extension.
pub fn remove_metadata(
    src: &Path,
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<()> {
    let program = ffmpeg(progress, cancel)?;
    let probe = probe_with(&program, src, cancel)?;
    let args = rules::remove_metadata(src, out, &probe);
    run(
        &program,
        &args,
        probe.duration,
        "remove the metadata",
        progress,
        cancel,
    )
}

/// The frame shown at `at`, upright and no bigger than `max_side` either way, for the panel's
/// range preview. FFmpeg has to be here already (see `prepare`).
pub fn frame_at(path: &Path, at: Duration, max_side: u32) -> io::Result<RgbaImage> {
    let program = FFMPEG.program()?;
    if !program.is_file() {
        return Err(not_ready());
    }
    let cancel = AtomicBool::new(false);
    let probe = probe_with(&program, path, &cancel)?;
    let shown = probe
        .resolution()
        .ok_or_else(|| invalid("This file has no picture"))?;
    let (w, h) = rules::frame_size(shown.width, shown.height, max_side);
    // A seek to the very end finds no frame to show.
    let last = probe.duration.saturating_sub(Duration::from_millis(100));
    let args = rules::frame(path, at.min(last), (w, h), probe.hdr());
    let args: Vec<&OsStr> = args.iter().map(OsString::as_os_str).collect();
    let pixels = tools::read(&program, &args, &cancel)?;
    RgbaImage::from_raw(w, h, pixels)
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "No frame there"))
}

#[cfg(test)]
mod tests;
