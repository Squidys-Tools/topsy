//! The bridge from the panel's options to the engines that do the work: a snapshot of the
//! options taken on the UI thread, and each file sent to the engine for its kind.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use gpui::{Context, Div};

use super::Panel;
use super::background::{self, ModelStatus};
use super::job::{Input, into, reserve};
use super::options::{BITRATES, Options, VIDEO_TARGETS};
use super::video;
use super::widgets::note;
use crate::model::transform::{Action, Kind, result_paths};
use crate::model::video::{
    AudioFormat, CompressBy, CompressVideo, ConvertTarget, GifFps, GifOptions, GifWidth,
    MaxResolution, Quality, Rotate, TrimMode, TrimRange, VideoFormat, edited_extension,
};

/// A snapshot of what to do, sent to the worker thread.
pub enum Work {
    Zip(String),
    Rename(Vec<String>),
    Background,
    Each(Each),
}

/// The options for a per-file action, read once from the panel.
pub struct Each {
    action: Action,
    media: Media,
}

/// What video and audio files get.
struct Media {
    compress: CompressVideo,
    audio: CompressBy,
    convert_video: ConvertTarget,
    convert_audio: ConvertTarget,
    range: Option<TrimRange>,
    trim: TrimMode,
    gif: Option<GifOptions>,
    rotate: Rotate,
}

/// The present participle the progress line uses.
pub fn verb(action: Action) -> &'static str {
    match action {
        Action::Compress => "Compressing",
        Action::Convert => "Converting",
        Action::Resize => "Resizing",
        Action::Trim => "Trimming",
        Action::MakeGif => "Making GIF",
        Action::RemoveBackground => "Removing background",
        Action::Rotate => "Rotating",
        Action::RemoveMetadata => "Removing metadata",
        Action::CombinePdf => "Combining",
        Action::Zip => "Zipping",
        Action::Rename => "Renaming",
    }
}

pub fn button_label(panel: &Panel, action: Action) -> (String, bool) {
    let ready = match action {
        Action::Trim => panel.trim_range().is_some(),
        Action::MakeGif => panel.trim_range().is_some_and(|r| r.for_gif().is_ok()),
        _ => true,
    };
    (action.label().into(), ready)
}

/// "8 MB" or "500 KB" in bytes, in the decimal units the panel shows sizes in.
fn bytes(label: &str) -> u64 {
    let (n, unit) = label.split_once(' ').unwrap_or((label, ""));
    let n: u64 = n.parse().unwrap_or(0);
    match unit {
        "KB" => n * 1_000,
        "MB" => n * 1_000_000,
        _ => n,
    }
}

fn media(o: &Options, range: Option<TrimRange>) -> Media {
    let pick = |list: &[VideoFormat], i: usize| list[i.min(list.len() - 1)];
    let quality = Quality::new((o.quality * 100.0).round() as u8);
    let by = if o.by_size {
        CompressBy::FileSize(bytes(VIDEO_TARGETS[o.target.min(VIDEO_TARGETS.len() - 1)]))
    } else {
        CompressBy::Quality(quality)
    };
    // Each bitrate's place on `Quality::audio_kbps`'s ladder: 320, 256, 192, 128, 96 kbps.
    const BITRATE_QUALITY: [u8; BITRATES.len()] = [100, 83, 67, 33, 17];
    let audio = match by {
        CompressBy::Quality(_) => CompressBy::Quality(Quality::new(
            BITRATE_QUALITY[o.bitrate.min(BITRATES.len() - 1)],
        )),
        size => size,
    };
    let videos = VideoFormat::ALL.len();
    let convert_video = if o.convert < videos {
        ConvertTarget::Video(pick(&VideoFormat::ALL, o.convert))
    } else {
        let sound = AudioFormat::FROM_VIDEO;
        ConvertTarget::Audio(sound[(o.convert - videos).min(sound.len() - 1)])
    };
    let sound = AudioFormat::ALL;
    let convert_audio = ConvertTarget::Audio(sound[o.convert.min(sound.len() - 1)]);
    // `GIF_WIDTHS` runs from narrow to wide, then the original size.
    let widths = [
        GifWidth::W320,
        GifWidth::W480,
        GifWidth::W640,
        GifWidth::Original,
    ];
    Media {
        compress: CompressVideo {
            by,
            max_resolution: MaxResolution::ALL[o.resolution.min(MaxResolution::ALL.len() - 1)],
        },
        audio,
        convert_video,
        convert_audio,
        range,
        trim: if o.trim_fast {
            TrimMode::Fast
        } else {
            TrimMode::Exact
        },
        gif: range.map(|range| GifOptions {
            range,
            fps: GifFps::ALL[o.fps.min(GifFps::ALL.len() - 1)],
            width: widths[o.gif_width.min(widths.len() - 1)],
        }),
        rotate: Rotate::ALL[o.rotation.min(Rotate::ALL.len() - 1)],
    }
}

pub fn work(panel: &Panel, action: Action) -> Result<Work, String> {
    Ok(match action {
        Action::RemoveBackground => Work::Background,
        _ => Work::Each(Each {
            action,
            media: media(&panel.options, panel.trim_range()),
        }),
    })
}

/// What has to happen once before the files: fetching the background model or FFmpeg on first
/// use, with the progress line saying so.
pub fn prepare(
    work: &Work,
    inputs: &[Input],
    progress: &dyn Fn(String, f32),
    cancel: &AtomicBool,
) -> io::Result<()> {
    match work {
        Work::Background if background::model_status() != ModelStatus::Ready => {
            background::download(&|p| progress("Getting the model…".into(), p), cancel)
        }
        Work::Each(_)
            if inputs
                .iter()
                .any(|i| matches!(i.kind, Kind::Video | Kind::Audio))
                && video::status() != super::fetch::Status::Ready =>
        {
            video::prepare(&|p| progress("Getting FFmpeg…".into(), p), cancel)
        }
        _ => Ok(()),
    }
}

/// A result beside `src`, `name (suffix).ext`, made by `make` and deleted again if it fails.
fn beside(
    src: &Path,
    action: Action,
    ext: &str,
    make: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<Vec<PathBuf>> {
    let candidates =
        result_paths(src, action, ext).ok_or_else(|| io::Error::other("No folder to save in"))?;
    Ok(vec![into(reserve(candidates)?, make)?])
}

fn own_extension(src: &Path) -> String {
    (src.extension())
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn run_one(
    work: &Work,
    input: &Input,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<Vec<PathBuf>> {
    let src = input.path.as_path();
    match (work, input.kind) {
        (Work::Background, _) => beside(src, Action::RemoveBackground, "png", |out| {
            background::remove(src, out, progress, cancel)
        }),
        (Work::Each(each), Kind::Video | Kind::Audio) => {
            media_one(each.action, &each.media, src, input.kind, progress, cancel)
        }
        (Work::Each(each), _) => Err(io::Error::other(format!(
            "{} isn't ready for this kind of file yet",
            each.action.label()
        ))),
        _ => Err(io::Error::other("Not a per-file action")),
    }
}

fn media_one(
    action: Action,
    m: &Media,
    src: &Path,
    kind: Kind,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> io::Result<Vec<PathBuf>> {
    let is_video = kind == Kind::Video;
    let range = || {
        m.range
            .ok_or_else(|| io::Error::other("The clip isn't loaded yet"))
    };
    match action {
        Action::Compress if is_video => beside(src, action, "mp4", |out| {
            video::compress_video(src, m.compress, out, progress, cancel)
        }),
        Action::Compress => beside(
            src,
            action,
            AudioFormat::for_compress(src).extension(),
            |out| video::compress_audio(src, m.audio, out, progress, cancel),
        ),
        Action::Convert => {
            let target = if is_video {
                m.convert_video
            } else {
                m.convert_audio
            };
            beside(src, action, target.extension(), |out| {
                video::convert(src, target, out, progress, cancel)
            })
        }
        Action::Trim => {
            let range = range()?;
            let ext = match m.trim {
                TrimMode::Fast => own_extension(src),
                TrimMode::Exact => edited_extension(src, is_video).to_owned(),
            };
            beside(src, action, &ext, |out| {
                video::trim(src, range, m.trim, out, progress, cancel)
            })
        }
        Action::MakeGif => {
            let gif = m
                .gif
                .ok_or_else(|| io::Error::other("The clip isn't loaded yet"))?;
            beside(src, action, "gif", |out| {
                video::make_gif(src, &gif, out, progress, cancel)
            })
        }
        Action::Rotate => beside(src, action, edited_extension(src, true), |out| {
            video::rotate(src, m.rotate, out, progress, cancel)
        }),
        Action::RemoveMetadata => beside(src, action, &own_extension(src), |out| {
            video::remove_metadata(src, out, progress, cancel)
        }),
        _ => Err(io::Error::other(format!(
            "{} doesn't apply to this file",
            action.label()
        ))),
    }
}

pub fn run_all(
    work: &Work,
    inputs: &[Input],
    progress: &dyn Fn(String, f32),
    cancel: &AtomicBool,
) -> io::Result<Vec<PathBuf>> {
    let _ = (work, inputs, progress, cancel);
    Err(io::Error::other("Combine into PDF isn't ready yet"))
}

impl Panel {
    pub fn background_row(&self, _cx: &mut Context<Self>) -> Div {
        note(match background::model_status() {
            ModelStatus::Ready => "Runs on this PC; the picture never leaves it.",
            _ => "The first run downloads the 178 MB model, once. After that it runs on this PC.",
        })
    }
}
