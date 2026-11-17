//! The rules of the video and audio Transform, done by FFmpeg: the options the panel offers,
//! the size math behind them, trim ranges, reading ffprobe and FFmpeg's progress, and the
//! command line each operation runs. The glue in `src/transform/video` runs them.

mod args;
mod budget;
mod codecs;
mod options;
mod probe;
mod progress;
mod trim;

pub use args::{
    Rate, compress_audio, compress_audio_kbps, compress_plan, compress_video, convert,
    edited_extension, frame, frame_size, gif, remove_metadata, rotate, trim,
};
pub use budget::{megabytes, retry_bitrate};
pub use options::{
    AudioFormat, CompressBy, CompressVideo, ConvertTarget, GifFps, GifOptions, GifWidth,
    MaxResolution, Quality, Rotate, TrimMode, VideoFormat,
};
pub use probe::{PROBE_ARGS, Probe, parse as parse_probe};
pub use progress::Progress;
pub use trim::{GIF_MAX, TrimRange};
