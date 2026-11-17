use std::path::Path;

use super::trim::TrimRange;

/// How a clip is compressed, Droppy's "Compress by: Quality | File size".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressBy {
    Quality(Quality),
    /// A ceiling in bytes the result must land at or under.
    FileSize(u64),
}

impl CompressBy {}

impl Default for CompressBy {
    fn default() -> Self {
        CompressBy::Quality(Quality::default())
    }
}

/// "Quality 75%": the panel's slider, 0..=100.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Quality(u8);

/// Audio bitrates a quality picks from, kbps, low to high.
const AUDIO_LADDER: [u32; 7] = [64, 96, 128, 160, 192, 256, 320];

impl Quality {
    pub const DEFAULT: Quality = Quality(75);

    pub fn new(percent: u8) -> Self {
        Quality(percent.min(100))
    }

    #[cfg(test)]
    pub fn percent(self) -> u8 {
        self.0
    }

    /// x264's constant rate factor: 38 (small, soft) at 0% to 18 (visually lossless) at 100%,
    /// with x264's own default, 23, at the default 75%.
    pub fn crf(self) -> u8 {
        38 - self.0 / 5
    }

    /// The bitrate a lossy audio encode gets, kbps: 64 at 0% to 320 at 100%.
    pub fn audio_kbps(self) -> u32 {
        let step = (self.0 as f32 / 100.0 * (AUDIO_LADDER.len() - 1) as f32).round() as usize;
        AUDIO_LADDER[step]
    }
}

impl Default for Quality {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A cap on the picture, by its short side so a portrait clip and a landscape one of the same
/// camera land on the same "1080p".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaxResolution {
    #[default]
    Original,
    P2160,
    P1440,
    P1080,
    P720,
    P480,
}

impl MaxResolution {
    pub const ALL: [MaxResolution; 6] = [
        MaxResolution::Original,
        MaxResolution::P2160,
        MaxResolution::P1440,
        MaxResolution::P1080,
        MaxResolution::P720,
        MaxResolution::P480,
    ];

    pub fn short_side(self) -> Option<u32> {
        match self {
            MaxResolution::Original => None,
            MaxResolution::P2160 => Some(2160),
            MaxResolution::P1440 => Some(1440),
            MaxResolution::P1080 => Some(1080),
            MaxResolution::P720 => Some(720),
            MaxResolution::P480 => Some(480),
        }
    }
}

/// Compress video always writes H.264 MP4 with AAC, the format every device plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompressVideo {
    pub by: CompressBy,
    pub max_resolution: MaxResolution,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VideoFormat {
    #[default]
    Mp4H264,
    /// Tagged `hvc1`, which Apple's players need to play HEVC in MP4.
    Mp4Hevc,
    Mov,
    /// VP9 and Opus.
    WebM,
    Mkv,
}

impl VideoFormat {
    pub const ALL: [VideoFormat; 5] = [
        VideoFormat::Mp4H264,
        VideoFormat::Mp4Hevc,
        VideoFormat::Mov,
        VideoFormat::WebM,
        VideoFormat::Mkv,
    ];

    #[cfg(test)]
    pub fn label(self) -> &'static str {
        match self {
            VideoFormat::Mp4H264 => "MP4 (H.264)",
            VideoFormat::Mp4Hevc => "MP4 (HEVC)",
            VideoFormat::Mov => "MOV",
            VideoFormat::WebM => "WebM",
            VideoFormat::Mkv => "MKV",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            VideoFormat::Mp4H264 | VideoFormat::Mp4Hevc => "mp4",
            VideoFormat::Mov => "mov",
            VideoFormat::WebM => "webm",
            VideoFormat::Mkv => "mkv",
        }
    }

    pub(super) fn muxer(self) -> &'static str {
        match self {
            VideoFormat::Mp4H264 | VideoFormat::Mp4Hevc => "mp4",
            VideoFormat::Mov => "mov",
            VideoFormat::WebM => "webm",
            VideoFormat::Mkv => "matroska",
        }
    }

    /// The format an edit that re-encodes (exact trim, rotate) writes for the video at `src`:
    /// its own container where FFmpeg writes one, else MP4. HEVC stays HEVC.
    pub fn for_edit(src: &Path, video_codec: &str) -> VideoFormat {
        match extension(src).as_str() {
            "mov" => VideoFormat::Mov,
            "mkv" => VideoFormat::Mkv,
            "webm" => VideoFormat::WebM,
            _ if video_codec == "hevc" => VideoFormat::Mp4Hevc,
            _ => VideoFormat::Mp4H264,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AudioFormat {
    #[default]
    M4a,
    Mp3,
    Wav,
    Flac,
    /// Opus in Ogg.
    Ogg,
    Aiff,
}

impl AudioFormat {
    pub const ALL: [AudioFormat; 6] = [
        AudioFormat::M4a,
        AudioFormat::Mp3,
        AudioFormat::Wav,
        AudioFormat::Flac,
        AudioFormat::Ogg,
        AudioFormat::Aiff,
    ];
    /// What Convert offers for a video's sound alone.
    pub const FROM_VIDEO: [AudioFormat; 4] = [
        AudioFormat::M4a,
        AudioFormat::Mp3,
        AudioFormat::Wav,
        AudioFormat::Flac,
    ];

    #[cfg(test)]
    pub fn label(self) -> &'static str {
        match self {
            AudioFormat::M4a => "M4A (AAC)",
            AudioFormat::Mp3 => "MP3",
            AudioFormat::Wav => "WAV",
            AudioFormat::Flac => "FLAC",
            AudioFormat::Ogg => "OGG (Opus)",
            AudioFormat::Aiff => "AIFF",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            AudioFormat::M4a => "m4a",
            AudioFormat::Mp3 => "mp3",
            AudioFormat::Wav => "wav",
            AudioFormat::Flac => "flac",
            AudioFormat::Ogg => "ogg",
            AudioFormat::Aiff => "aiff",
        }
    }

    pub(super) fn muxer(self) -> &'static str {
        match self {
            AudioFormat::M4a => "ipod",
            AudioFormat::Mp3 => "mp3",
            AudioFormat::Wav => "wav",
            AudioFormat::Flac => "flac",
            AudioFormat::Ogg => "ogg",
            AudioFormat::Aiff => "aiff",
        }
    }

    pub fn lossy(self) -> bool {
        matches!(self, AudioFormat::M4a | AudioFormat::Mp3 | AudioFormat::Ogg)
    }

    /// The format a file at `path` is in, if it is one FFmpeg here writes.
    pub fn of(path: &Path) -> Option<AudioFormat> {
        Some(match extension(path).as_str() {
            "m4a" | "aac" => AudioFormat::M4a,
            "mp3" => AudioFormat::Mp3,
            "wav" => AudioFormat::Wav,
            "flac" => AudioFormat::Flac,
            "ogg" | "opus" => AudioFormat::Ogg,
            "aif" | "aiff" => AudioFormat::Aiff,
            _ => return None,
        })
    }

    /// What Compress and exact Trim write for the sound at `src`. Compress keeps a lossy
    /// format and turns lossless (or unwritable) sound into AAC, since only a lossy encode
    /// can shrink it; Trim keeps any format it can write.
    pub fn for_compress(src: &Path) -> AudioFormat {
        AudioFormat::of(src)
            .filter(|f| f.lossy())
            .unwrap_or(AudioFormat::M4a)
    }

    pub fn for_trim(src: &Path) -> AudioFormat {
        AudioFormat::of(src).unwrap_or(AudioFormat::M4a)
    }
}

pub(super) fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// What Convert can turn a file into: another video, or just its sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConvertTarget {
    Video(VideoFormat),
    Audio(AudioFormat),
}

impl ConvertTarget {
    pub fn extension(self) -> &'static str {
        match self {
            ConvertTarget::Video(f) => f.extension(),
            ConvertTarget::Audio(f) => f.extension(),
        }
    }
}

/// Exact re-encodes from the frame asked for; Fast copies the streams untouched, so the cut
/// moves back to the key frame before the start, and keeps the original's quality and format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrimMode {
    #[default]
    Exact,
    Fast,
}

impl TrimMode {
    #[cfg(test)]
    pub fn label(self) -> &'static str {
        match self {
            TrimMode::Exact => "Exact",
            TrimMode::Fast => "Fast (key frames)",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GifFps {
    Ten,
    #[default]
    Fifteen,
    TwentyFour,
}

impl GifFps {
    pub const ALL: [GifFps; 3] = [GifFps::Ten, GifFps::Fifteen, GifFps::TwentyFour];

    pub fn fps(self) -> u32 {
        match self {
            GifFps::Ten => 10,
            GifFps::Fifteen => 15,
            GifFps::TwentyFour => 24,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GifWidth {
    Original,
    W640,
    #[default]
    W480,
    W320,
}

impl GifWidth {
    pub fn pixels(self) -> Option<u32> {
        match self {
            GifWidth::Original => None,
            GifWidth::W640 => Some(640),
            GifWidth::W480 => Some(480),
            GifWidth::W320 => Some(320),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GifOptions {
    pub range: TrimRange,
    pub fps: GifFps,
    pub width: GifWidth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotate {
    Left,
    Right,
    Half,
    FlipHorizontal,
    FlipVertical,
}

impl Rotate {
    pub const ALL: [Rotate; 5] = [
        Rotate::Left,
        Rotate::Right,
        Rotate::Half,
        Rotate::FlipHorizontal,
        Rotate::FlipVertical,
    ];

    #[cfg(test)]
    pub fn label(self) -> &'static str {
        match self {
            Rotate::Left => "Rotate left",
            Rotate::Right => "Rotate right",
            Rotate::Half => "Rotate 180°",
            Rotate::FlipHorizontal => "Flip horizontal",
            Rotate::FlipVertical => "Flip vertical",
        }
    }

    pub(super) fn filter(self) -> &'static str {
        match self {
            Rotate::Left => "transpose=cclock",
            Rotate::Right => "transpose=clock",
            Rotate::Half => "hflip,vflip",
            Rotate::FlipHorizontal => "hflip",
            Rotate::FlipVertical => "vflip",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slider_maps_to_crf_and_audio_bitrates() {
        assert_eq!(Quality::new(250).percent(), 100);
        assert_eq!(Quality::new(0).crf(), 38);
        assert_eq!(Quality::DEFAULT.crf(), 23);
        assert_eq!(Quality::new(100).crf(), 18);
        assert_eq!(Quality::new(0).audio_kbps(), 64);
        assert_eq!(Quality::new(50).audio_kbps(), 160);
        assert_eq!(Quality::new(100).audio_kbps(), 320);
    }

    #[test]
    fn audio_keeps_its_format_where_it_can() {
        let p = Path::new;
        assert_eq!(AudioFormat::for_compress(p("a.MP3")), AudioFormat::Mp3);
        assert_eq!(AudioFormat::for_compress(p("a.opus")), AudioFormat::Ogg);
        assert_eq!(AudioFormat::for_compress(p("a.flac")), AudioFormat::M4a);
        assert_eq!(AudioFormat::for_compress(p("a.wma")), AudioFormat::M4a);
        assert_eq!(AudioFormat::for_trim(p("a.flac")), AudioFormat::Flac);
        assert_eq!(AudioFormat::for_trim(p("a.wma")), AudioFormat::M4a);
    }

    #[test]
    fn edits_keep_the_container_and_hevc() {
        let p = Path::new;
        assert_eq!(VideoFormat::for_edit(p("a.MOV"), "hevc"), VideoFormat::Mov);
        assert_eq!(
            VideoFormat::for_edit(p("a.mp4"), "hevc"),
            VideoFormat::Mp4Hevc
        );
        assert_eq!(
            VideoFormat::for_edit(p("a.avi"), "mpeg4"),
            VideoFormat::Mp4H264
        );
        assert_eq!(VideoFormat::for_edit(p("a.webm"), "vp9"), VideoFormat::WebM);
    }
}
