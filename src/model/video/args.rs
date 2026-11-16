//! FFmpeg command lines for each operation, built from the options and what ffprobe said.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use super::budget::quality_ceiling;
use super::budget::{Clip, Resolution, SizeError, audio_kbps_for_size, budget_for_size};
use super::codecs::{self, Encoder, copies_audio, copies_video};
use super::options::{
    AudioFormat, CompressBy, CompressVideo, ConvertTarget, GifOptions, Rotate, TrimMode,
    VideoFormat, extension,
};
use super::probe::Probe;
use super::trim::{TrimRange, seconds};

/// An FFmpeg command line under construction.
pub struct Args(Vec<OsString>);

impl Args {
    /// Quiet but for errors, never asking, reporting progress on stdout for `Progress`.
    fn new() -> Args {
        let mut a = Args(Vec::new());
        a.push(&["-hide_banner", "-nostdin", "-loglevel", "error", "-y"]);
        a.push(&["-progress", "pipe:1", "-nostats"]);
        a
    }

    pub(super) fn push(&mut self, items: &[&str]) {
        self.0.extend(items.iter().map(OsString::from));
    }

    /// `src`, read from `from` on: seeking before `-i` jumps to the key frame before and,
    /// when encoding, decodes forward to the exact time.
    fn input(&mut self, src: &Path, from: Option<Duration>) {
        if let Some(from) = from {
            self.push(&["-ss", &seconds(from)]);
        }
        self.push(&["-i"]);
        self.0.push(src.into());
    }

    /// The picture and every sound track, leaving out cover art, subtitles and the data
    /// tracks phones add, which most containers refuse.
    fn video_and_sound(&mut self) {
        self.push(&["-map", "0:V:0", "-map", "0:a?"]);
    }

    /// Finish with `out`, in `muxer` (or the one its extension names).
    fn output(mut self, out: &Path, muxer: Option<&str>) -> Vec<OsString> {
        if matches!(muxer, Some("mp4" | "mov" | "ipod"))
            || muxer.is_none() && matches!(extension(out).as_str(), "mp4" | "m4v" | "mov" | "m4a")
        {
            self.push(&["-movflags", "+faststart"]);
        }
        if let Some(muxer) = muxer {
            self.push(&["-f", muxer]);
        }
        self.0.push(out.into());
        self.0
    }
}

/// How Compress spends the video's bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rate {
    /// Constant quality, held under `ceiling` bits a second when the source's rate is known.
    Crf {
        crf: u8,
        ceiling: Option<u32>,
    },
    Bitrate(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompressPlan {
    pub rate: Rate,
    pub audio_kbps: u32,
    /// The picture size, when it changes.
    pub scale: Option<Resolution>,
}

/// Plan "Compress video" for the clip `probe` describes.
pub fn compress_plan(opts: CompressVideo, probe: &Probe) -> Result<CompressPlan, SizeError> {
    let source = probe.resolution().unwrap_or(Resolution {
        width: 1920,
        height: 1080,
    });
    let changed = |r: Resolution| (r != source).then_some(r);
    match opts.by {
        CompressBy::Quality(q) => {
            let out = Resolution::fit(
                source.width,
                source.height,
                opts.max_resolution.short_side(),
            );
            let ceiling = probe
                .video_bit_rate()
                .map(|b| quality_ceiling(b, source, out));
            let audio = probe.audio.as_ref().and_then(|a| a.bit_rate);
            Ok(CompressPlan {
                rate: Rate::Crf {
                    crf: q.crf(),
                    ceiling,
                },
                audio_kbps: audio.map_or(128, |b| (b / 1000).clamp(64, 128)),
                scale: changed(out),
            })
        }
        CompressBy::FileSize(target) => {
            let clip = Clip {
                resolution: source,
                fps: probe.video.as_ref().map_or(30.0, |v| v.fps),
                has_audio: probe.has_audio(),
                video_bps: probe.video_bit_rate(),
            };
            let b = budget_for_size(target, probe.duration, &clip, opts.max_resolution)?;
            Ok(CompressPlan {
                rate: Rate::Bitrate(b.video_bps),
                audio_kbps: (b.audio_bps / 1000).max(64),
                scale: changed(b.resolution),
            })
        }
    }
}

pub fn compress_video(src: &Path, out: &Path, plan: &CompressPlan) -> Vec<OsString> {
    let mut a = Args::new();
    a.input(src, None);
    a.push(&["-map", "0:V:0", "-map", "0:a:0?"]);
    if let Some(r) = plan.scale {
        a.push(&["-vf", &format!("scale={}:{}", r.width, r.height)]);
    }
    match plan.rate {
        Rate::Crf { crf, ceiling } => {
            Encoder::X264.crf(&mut a, crf);
            if let Some(max) = ceiling {
                a.push(&[
                    "-maxrate",
                    &max.to_string(),
                    "-bufsize",
                    &(2 * max).to_string(),
                ]);
            }
        }
        Rate::Bitrate(bps) => a.push(&[
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-b:v",
            &bps.to_string(),
            "-pix_fmt",
            "yuv420p",
        ]),
    }
    a.push(&["-c:a", "aac", "-b:a", &format!("{}k", plan.audio_kbps)]);
    a.output(out, Some("mp4"))
}

/// The bitrate, kbps, "Compress audio" uses for the sound `probe` describes. A quality never
/// spends more than 80% of what a lossy source did, so the file shrinks.
pub fn compress_audio_kbps(by: CompressBy, probe: &Probe) -> Result<u32, SizeError> {
    match by {
        CompressBy::Quality(q) => {
            let source = probe.audio.as_ref().and_then(|a| a.bit_rate);
            Ok(match source {
                Some(bps) if bps < 500_000 => q.audio_kbps().min(bps * 8 / 10_000).max(32),
                _ => q.audio_kbps(),
            })
        }
        CompressBy::FileSize(target) => audio_kbps_for_size(target, probe.duration),
    }
}

/// "Compress audio" to `AudioFormat::for_compress(src)`.
pub fn compress_audio(src: &Path, out: &Path, kbps: u32) -> Vec<OsString> {
    let format = AudioFormat::for_compress(src);
    let mut a = Args::new();
    a.input(src, None);
    a.push(&["-map", "0:a:0", "-vn"]);
    codecs::audio_as(&mut a, format, kbps, false);
    a.output(out, Some(format.muxer()))
}

pub fn convert(src: &Path, out: &Path, target: ConvertTarget, probe: &Probe) -> Vec<OsString> {
    let mut a = Args::new();
    a.input(src, None);
    match target {
        ConvertTarget::Video(format) => {
            a.video_and_sound();
            if copies_video(format, probe.video_codec()) {
                a.push(&["-c:v", "copy"]);
                if probe.video_codec() == "hevc" && format != VideoFormat::Mkv {
                    a.push(&["-tag:v", "hvc1"]);
                }
            } else {
                Encoder::for_format(format, false).keep(&mut a);
            }
            codecs::audio_into(&mut a, format, probe.audio_codec());
            a.output(out, Some(format.muxer()))
        }
        ConvertTarget::Audio(format) => {
            a.push(&["-map", "0:a:0", "-vn"]);
            sound_as(&mut a, format, probe);
            a.output(out, Some(format.muxer()))
        }
    }
}

/// Sound written as `format`: copied when it already is, else encoded at a keeping quality.
fn sound_as(a: &mut Args, format: AudioFormat, probe: &Probe) {
    let audio = probe.audio.as_ref();
    if copies_audio(format, probe.audio_codec()) {
        a.push(&["-c:a", "copy"]);
    } else {
        let kbps = codecs::keep_kbps(format, audio.and_then(|a| a.bit_rate));
        codecs::audio_as(a, format, kbps, audio.is_some_and(|a| a.deep));
    }
}

/// The extension an exact Trim or a Rotate writes for `src`; Fast trim and Remove metadata
/// keep `src`'s own.
pub fn edited_extension(src: &Path, video: bool) -> &'static str {
    if video {
        VideoFormat::for_edit(src, "").extension()
    } else {
        AudioFormat::for_trim(src).extension()
    }
}

pub fn trim(
    src: &Path,
    out: &Path,
    range: TrimRange,
    mode: TrimMode,
    probe: &Probe,
) -> Vec<OsString> {
    let mut a = Args::new();
    a.input(src, Some(range.start()));
    a.push(&["-t", &seconds(range.len())]);
    // A copied FLAC keeps its header's old length, and lossless sound loses nothing to an
    // encode, so it is always cut exactly.
    let lossless = AudioFormat::of(src).is_some_and(|f| !f.lossy());
    let mode = match mode {
        TrimMode::Fast if lossless && !probe.has_video() => TrimMode::Exact,
        mode => mode,
    };
    match (mode, probe.has_video()) {
        (TrimMode::Fast, video) => {
            if video {
                a.video_and_sound();
            } else {
                a.push(&["-map", "0:a:0"]);
            }
            a.push(&["-c", "copy", "-avoid_negative_ts", "make_zero"]);
            a.output(out, None)
        }
        (TrimMode::Exact, true) => {
            let format = VideoFormat::for_edit(src, probe.video_codec());
            a.video_and_sound();
            Encoder::for_format(format, probe.video_codec() == "hevc").keep(&mut a);
            codecs::audio_into(&mut a, format, probe.audio_codec());
            a.output(out, Some(format.muxer()))
        }
        (TrimMode::Exact, false) => {
            let format = AudioFormat::for_trim(src);
            a.push(&["-map", "0:a:0", "-vn"]);
            let audio = probe.audio.as_ref();
            let kbps = codecs::keep_kbps(format, audio.and_then(|a| a.bit_rate));
            codecs::audio_as(&mut a, format, kbps, audio.is_some_and(|a| a.deep));
            a.output(out, Some(format.muxer()))
        }
    }
}

/// The GIF's size for a clip shown `width` x `height`: as wide as asked, never wider than the
/// clip, its shape kept.
pub fn gif_size(width: u32, height: u32, opts: &GifOptions) -> (u32, u32) {
    let (w, h) = (width.max(1), height.max(1));
    let target = opts.width.pixels().map_or(w, |p| p.min(w));
    let scaled = ((h as f64 * target as f64 / w as f64).round() as u32).max(1);
    (target, scaled)
}

/// One pass: the palette is built from the clip's own frames, then used to draw them, each
/// frame dithered only where it changed so the GIF stays small.
pub fn gif(src: &Path, out: &Path, opts: &GifOptions, probe: &Probe) -> Vec<OsString> {
    let source = probe.resolution().unwrap_or(Resolution {
        width: 480,
        height: 270,
    });
    let (w, h) = gif_size(source.width, source.height, opts);
    let graph = format!(
        "[0:V:0]fps={},scale={w}:{h}:flags=lanczos,split[a][b];\
         [a]palettegen=stats_mode=diff[p];\
         [b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle",
        opts.fps.fps()
    );
    let mut a = Args::new();
    a.input(src, Some(opts.range.start()));
    a.push(&["-t", &seconds(opts.range.len()), "-filter_complex", &graph]);
    a.push(&["-an", "-loop", "0"]);
    a.output(out, Some("gif"))
}

/// FFmpeg turns the frames upright as it decodes and writes no rotation, so the turn the
/// filter adds is the only one players apply.
pub fn rotate(src: &Path, out: &Path, turn: Rotate, probe: &Probe) -> Vec<OsString> {
    let format = VideoFormat::for_edit(src, probe.video_codec());
    let mut a = Args::new();
    a.input(src, None);
    a.video_and_sound();
    a.push(&["-vf", turn.filter()]);
    Encoder::for_format(format, probe.video_codec() == "hevc").keep(&mut a);
    codecs::audio_into(&mut a, format, probe.audio_codec());
    a.output(out, Some(format.muxer()))
}

/// Every tag, chapter and the encoder's own name gone, the streams copied untouched. The
/// rotation is the stream's display matrix, not a tag, so it stays and the picture stays
/// upright.
pub fn remove_metadata(src: &Path, out: &Path, probe: &Probe) -> Vec<OsString> {
    let mut a = Args::new();
    a.input(src, None);
    if probe.has_video() {
        a.video_and_sound();
    } else {
        a.push(&["-map", "0:a"]);
    }
    a.push(&["-c", "copy", "-map_metadata", "-1", "-map_chapters", "-1"]);
    a.push(&[
        "-fflags",
        "+bitexact",
        "-flags:v",
        "+bitexact",
        "-flags:a",
        "+bitexact",
    ]);
    a.output(out, None)
}

/// The size a frame of a clip shown `width` x `height` is decoded at to fit in `max_side`.
pub fn frame_size(width: u32, height: u32, max_side: u32) -> (u32, u32) {
    let (w, h) = (width.max(1) as f64, height.max(1) as f64);
    let scale = (max_side.max(1) as f64 / w.max(h)).min(1.0);
    (
        ((w * scale).round() as u32).max(1),
        ((h * scale).round() as u32).max(1),
    )
}

/// One frame at `at`, `size` big, as raw RGBA on stdout.
pub fn frame(src: &Path, at: Duration, size: (u32, u32)) -> Vec<OsString> {
    let mut a = Args(Vec::new());
    a.push(&["-hide_banner", "-nostdin", "-loglevel", "error"]);
    a.input(src, Some(at));
    a.push(&["-map", "0:V:0", "-frames:v", "1", "-an"]);
    a.push(&[
        "-vf",
        &format!("scale={}:{}:flags=bilinear", size.0, size.1),
    ]);
    a.push(&["-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"]);
    a.0
}

#[cfg(test)]
#[path = "args_tests.rs"]
mod tests;
