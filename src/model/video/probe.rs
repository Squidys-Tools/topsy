use std::time::Duration;

use serde_json::Value;

use super::budget::Resolution;

/// What ffprobe says about a file, as much as Transform needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Probe {
    pub duration: Duration,
    pub video: Option<VideoStream>,
    pub audio: Option<AudioStream>,
    /// The whole file's bitrate, when the container says.
    pub bit_rate: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VideoStream {
    pub codec: String,
    /// As shown, after `rotation`.
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// Clockwise degrees the picture is turned on playback: 0, 90, 180 or 270.
    pub rotation: u32,
    pub bit_rate: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioStream {
    pub codec: String,
    pub bit_rate: Option<u32>,
    /// More than 16 bits a sample, which a lossless copy should keep.
    pub deep: bool,
}

impl Probe {
    pub fn has_video(&self) -> bool {
        self.video.is_some()
    }

    pub fn has_audio(&self) -> bool {
        self.audio.is_some()
    }

    pub fn resolution(&self) -> Option<Resolution> {
        let v = self.video.as_ref()?;
        Some(Resolution {
            width: v.width,
            height: v.height,
        })
    }

    /// The video stream's bitrate, or what the file spends beyond its sound.
    pub fn video_bit_rate(&self) -> Option<u32> {
        let v = self.video.as_ref()?;
        v.bit_rate.or_else(|| {
            let audio = self.audio.as_ref().and_then(|a| a.bit_rate).unwrap_or(0);
            self.bit_rate
                .map(|b| b.saturating_sub(audio))
                .filter(|&b| b > 0)
        })
    }

    pub fn video_codec(&self) -> &str {
        self.video.as_ref().map_or("", |v| &v.codec)
    }

    pub fn audio_codec(&self) -> &str {
        self.audio.as_ref().map_or("", |a| &a.codec)
    }
}

/// The arguments for ffprobe that `parse` reads the answer of.
pub const PROBE_ARGS: [&str; 6] = [
    "-v",
    "error",
    "-print_format",
    "json",
    "-show_format",
    "-show_streams",
];

/// Read `ffprobe -print_format json -show_format -show_streams`. `None` when the file has
/// neither picture nor sound.
pub fn parse(json: &str) -> Option<Probe> {
    let root: Value = serde_json::from_str(json).ok()?;
    let streams = root.get("streams")?.as_array()?;
    let format = root.get("format");
    let kind = |s: &&Value, k: &str| s.get("codec_type").and_then(Value::as_str) == Some(k);
    let cover = |s: &&Value| {
        s.pointer("/disposition/attached_pic")
            .and_then(Value::as_i64)
            == Some(1)
    };
    let video = streams
        .iter()
        .find(|s| kind(s, "video") && !cover(s))
        .and_then(video_stream);
    let audio = streams
        .iter()
        .find(|s| kind(s, "audio"))
        .map(|s| AudioStream {
            codec: text(s, "codec_name"),
            bit_rate: number(s.get("bit_rate")),
            deep: deep(s),
        });
    if video.is_none() && audio.is_none() {
        return None;
    }
    let seconds = |v: Option<&Value>| {
        v.and_then(|v| v.as_str()?.parse::<f64>().ok())
            .filter(|s| s.is_finite() && *s > 0.0)
    };
    let duration = seconds(format.and_then(|f| f.get("duration")))
        .or_else(|| streams.iter().find_map(|s| seconds(s.get("duration"))))
        .unwrap_or(0.0);
    Some(Probe {
        duration: Duration::from_secs_f64(duration),
        video,
        audio,
        bit_rate: number(format.and_then(|f| f.get("bit_rate"))),
    })
}

fn video_stream(s: &Value) -> Option<VideoStream> {
    let width = s.get("width")?.as_u64()? as u32;
    let height = s.get("height")?.as_u64()? as u32;
    let rotation = rotation(s);
    let (width, height) = if rotation % 180 == 90 {
        (height, width)
    } else {
        (width, height)
    };
    let fps = ["avg_frame_rate", "r_frame_rate"]
        .iter()
        .filter_map(|k| ratio(s.get(*k)?.as_str()?))
        .find(|f| f.is_finite() && *f > 0.0)
        .unwrap_or(30.0);
    Some(VideoStream {
        codec: text(s, "codec_name"),
        width,
        height,
        fps,
        rotation,
        bit_rate: number(s.get("bit_rate")),
    })
}

/// ffprobe gives the display matrix's turn counter-clockwise (-90 for a phone held upright);
/// older files carry a clockwise `rotate` tag instead.
fn rotation(s: &Value) -> u32 {
    let matrix = s
        .get("side_data_list")
        .and_then(Value::as_array)
        .and_then(|list| list.iter().find_map(|d| d.get("rotation")?.as_f64()))
        .map(|ccw| -ccw);
    let tag = || s.pointer("/tags/rotate")?.as_str()?.parse::<f64>().ok();
    let degrees = matrix.or_else(tag).unwrap_or(0.0);
    ((degrees / 90.0).round() as i64 * 90).rem_euclid(360) as u32
}

/// Lossless sound with more than 16 bits a sample. Lossy decoders all hand out floats, which
/// says nothing about depth, so only lossless codecs count.
fn deep(s: &Value) -> bool {
    let codec = text(s, "codec_name");
    if !(codec.starts_with("pcm_") || codec == "flac" || codec == "alac") {
        return false;
    }
    match number(s.get("bits_per_raw_sample")).filter(|&b| b > 0) {
        Some(bits) => bits > 16,
        None => !matches!(
            s.get("sample_fmt").and_then(Value::as_str),
            Some("s16" | "s16p" | "u8" | "u8p")
        ),
    }
}

fn text(s: &Value, key: &str) -> String {
    s.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn number(v: Option<&Value>) -> Option<u32> {
    let v = v?;
    v.as_u64()
        .or_else(|| v.as_str()?.parse().ok())
        .map(|n| n.min(u32::MAX as u64) as u32)
}

fn ratio(text: &str) -> Option<f64> {
    let (n, d) = text.split_once('/')?;
    let (n, d): (f64, f64) = (n.parse().ok()?, d.parse().ok()?);
    (d != 0.0).then(|| n / d)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHONE: &str = r#"{
      "streams": [
        {"index": 0, "codec_name": "hevc", "codec_type": "video", "width": 1920, "height": 1080,
         "r_frame_rate": "30/1", "avg_frame_rate": "30000/1001", "bit_rate": "15000000",
         "side_data_list": [{"side_data_type": "Display Matrix", "rotation": -90}]},
        {"index": 1, "codec_name": "aac", "codec_type": "audio", "sample_fmt": "fltp",
         "bit_rate": "192000"},
        {"index": 2, "codec_name": "none", "codec_type": "data"}
      ],
      "format": {"duration": "12.345000", "bit_rate": "15300000"}
    }"#;

    #[test]
    fn a_phone_clip_reads_upright() {
        let p = parse(PHONE).unwrap();
        let v = p.video.as_ref().unwrap();
        assert_eq!((v.width, v.height, v.rotation), (1080, 1920, 90));
        assert!((v.fps - 29.97).abs() < 0.01);
        assert_eq!(p.video_codec(), "hevc");
        assert_eq!(p.video_bit_rate(), Some(15_000_000));
        assert_eq!(p.audio.as_ref().unwrap().bit_rate, Some(192_000));
        assert_eq!(p.duration, Duration::from_secs_f64(12.345));
    }

    #[test]
    fn music_with_cover_art_is_sound_only() {
        let json = r#"{"streams": [
            {"codec_name": "mp3", "codec_type": "audio", "sample_fmt": "fltp"},
            {"codec_name": "mjpeg", "codec_type": "video", "width": 600, "height": 600,
             "disposition": {"attached_pic": 1}}
          ], "format": {"duration": "200.0", "bit_rate": "320000"}}"#;
        let p = parse(json).unwrap();
        assert!(!p.has_video() && p.has_audio());
        assert!(!p.audio.unwrap().deep);
    }

    #[test]
    fn deep_samples_and_missing_rates() {
        let json = r#"{"streams": [
            {"codec_name": "flac", "codec_type": "audio", "sample_fmt": "s32",
             "bits_per_raw_sample": "24"},
            {"codec_name": "h264", "codec_type": "video", "width": 640, "height": 360,
             "avg_frame_rate": "0/0", "r_frame_rate": "25/1", "tags": {"rotate": "180"}}
          ], "format": {"duration": "N/A", "bit_rate": "1000000"}}"#;
        let p = parse(json).unwrap();
        assert!(p.audio.as_ref().unwrap().deep);
        let v = p.video.as_ref().unwrap();
        assert_eq!(
            (v.width, v.height, v.rotation, v.fps),
            (640, 360, 180, 25.0)
        );
        assert_eq!(p.video_bit_rate(), Some(1_000_000));
        assert_eq!(p.duration, Duration::ZERO);
    }

    #[test]
    fn nothing_playable_is_none() {
        assert_eq!(parse(r#"{"streams": [], "format": {}}"#), None);
        assert_eq!(parse("not json"), None);
    }
}
