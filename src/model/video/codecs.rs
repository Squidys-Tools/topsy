//! Which encoder writes what, and when a stream can be copied instead of encoded again.

use super::args::Args;
use super::options::{AudioFormat, VideoFormat};
use super::probe::Hdr;

/// HDR brought down to SDR (BT.709) with the Mobius curve: tones SDR can show stay as they are
/// and only the highlights roll off. zscale needs linear float light to map in.
pub const TO_SDR: &str = "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,\
                          tonemap=tonemap=mobius:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p";

/// A video encoder and the settings it gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoder {
    X264,
    /// Tagged `hvc1` in MP4 and MOV, which Apple's players need.
    X265,
    Vp9,
}

impl Encoder {
    /// The encoder for `format`. MOV and MKV hold either H.264 or HEVC; `hevc` keeps an HEVC
    /// source in HEVC there.
    pub fn for_format(format: VideoFormat, hevc: bool) -> Encoder {
        match format {
            VideoFormat::Mp4H264 => Encoder::X264,
            VideoFormat::Mp4Hevc => Encoder::X265,
            VideoFormat::WebM => Encoder::Vp9,
            VideoFormat::Mov | VideoFormat::Mkv if hevc => Encoder::X265,
            VideoFormat::Mov | VideoFormat::Mkv => Encoder::X264,
        }
    }

    /// The rate factor for an encode meant to keep the picture as it was (Convert, exact Trim,
    /// Rotate): visually lossless for x264, and the equivalent for the others.
    fn keep_crf(self) -> u8 {
        match self {
            Encoder::X264 => 18,
            Encoder::X265 => 22,
            Encoder::Vp9 => 31,
        }
    }

    /// Push the encoder at its keeping quality.
    pub(super) fn keep(self, a: &mut Args, hdr: Option<Hdr>) {
        self.crf(a, self.keep_crf(), hdr);
    }

    /// Whether this encoder writes HDR as HDR. Only HEVC does: H.264 and VP9 are written 8-bit
    /// for every player, so an HDR source is tone-mapped first (`TO_SDR`).
    pub fn keeps_hdr(self) -> bool {
        self == Encoder::X265
    }

    /// `hdr` is the source's curve, kept when `keeps_hdr`, else already mapped away.
    pub(super) fn crf(self, a: &mut Args, crf: u8, hdr: Option<Hdr>) {
        let crf = crf.to_string();
        match self {
            Encoder::X264 => a.push(&["-c:v", "libx264", "-preset", "medium", "-crf", &crf]),
            Encoder::X265 => a.push(&[
                "-c:v",
                "libx265",
                "-preset",
                "medium",
                "-crf",
                &crf,
                "-tag:v",
                "hvc1",
                "-x265-params",
                "log-level=error",
            ]),
            Encoder::Vp9 => a.push(&[
                "-c:v",
                "libvpx-vp9",
                "-crf",
                &crf,
                "-b:v",
                "0",
                "-row-mt",
                "1",
                "-deadline",
                "good",
                "-cpu-used",
                "4",
            ]),
        }
        match hdr.filter(|_| self.keeps_hdr()) {
            Some(hdr) => a.push(&[
                "-pix_fmt",
                "yuv420p10le",
                "-color_primaries",
                "bt2020",
                "-color_trc",
                hdr.transfer(),
                "-colorspace",
                "bt2020nc",
            ]),
            // 8-bit 4:2:0 plays everywhere; a 10-bit or 4:2:2 source would otherwise stay so.
            None => a.push(&["-pix_fmt", "yuv420p"]),
        }
    }
}

/// Whether a `codec` video stream can go into `format` as it is.
pub fn copies_video(format: VideoFormat, codec: &str) -> bool {
    match format {
        VideoFormat::Mp4H264 => codec == "h264",
        VideoFormat::Mp4Hevc => codec == "hevc",
        VideoFormat::Mov => matches!(codec, "h264" | "hevc" | "prores" | "mjpeg"),
        VideoFormat::WebM => matches!(codec, "vp8" | "vp9" | "av1"),
        VideoFormat::Mkv => !codec.is_empty(),
    }
}

/// Whether a `codec` sound stream can go into a video `format` as it is.
pub fn copies_audio_into(format: VideoFormat, codec: &str) -> bool {
    match format {
        VideoFormat::Mp4H264 | VideoFormat::Mp4Hevc | VideoFormat::Mov => {
            matches!(codec, "aac" | "mp3" | "alac")
        }
        VideoFormat::WebM => matches!(codec, "opus" | "vorbis"),
        VideoFormat::Mkv => !codec.is_empty(),
    }
}

/// Whether a `codec` sound stream already is `format`, so Convert can copy it.
pub fn copies_audio(format: AudioFormat, codec: &str) -> bool {
    match format {
        AudioFormat::M4a => matches!(codec, "aac" | "alac"),
        AudioFormat::Mp3 => codec == "mp3",
        AudioFormat::Ogg => matches!(codec, "opus" | "vorbis"),
        AudioFormat::Flac => codec == "flac",
        AudioFormat::Wav => matches!(codec, "pcm_s16le" | "pcm_s24le" | "pcm_s32le" | "pcm_f32le"),
        AudioFormat::Aiff => matches!(codec, "pcm_s16be" | "pcm_s24be" | "pcm_s32be"),
    }
}

/// The sound for a video `format`: copied when it fits, else AAC (Opus in WebM).
pub(super) fn audio_into(a: &mut Args, format: VideoFormat, codec: &str) {
    if copies_audio_into(format, codec) {
        a.push(&["-c:a", "copy"]);
    } else if format == VideoFormat::WebM {
        a.push(&["-c:a", "libopus", "-b:a", "128k"]);
    } else {
        a.push(&["-c:a", "aac", "-b:a", "192k"]);
    }
}

/// The bitrate Convert and exact Trim give a lossy `format`: high, but no more than the
/// source spent when it was lossy itself.
pub fn keep_kbps(format: AudioFormat, source_bps: Option<u32>) -> u32 {
    let high = match format {
        AudioFormat::Ogg => 160,
        _ => 256,
    };
    match source_bps {
        Some(bps) if bps < 500_000 => high.min((bps / 1000).max(96)),
        _ => high,
    }
}

/// Encode sound as `format`; `kbps` for the lossy ones, `deep` keeps 24 bits in PCM.
pub(super) fn audio_as(a: &mut Args, format: AudioFormat, kbps: u32, deep: bool) {
    let rate = format!("{kbps}k");
    match format {
        AudioFormat::M4a => a.push(&["-c:a", "aac", "-b:a", &rate]),
        AudioFormat::Mp3 => a.push(&["-c:a", "libmp3lame", "-b:a", &rate]),
        AudioFormat::Ogg => a.push(&["-c:a", "libopus", "-b:a", &rate]),
        AudioFormat::Flac => a.push(&["-c:a", "flac"]),
        AudioFormat::Wav if deep => a.push(&["-c:a", "pcm_s24le"]),
        AudioFormat::Wav => a.push(&["-c:a", "pcm_s16le"]),
        AudioFormat::Aiff if deep => a.push(&["-c:a", "pcm_s24be"]),
        AudioFormat::Aiff => a.push(&["-c:a", "pcm_s16be"]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hevc_stays_hevc_where_the_container_holds_it() {
        assert_eq!(Encoder::for_format(VideoFormat::Mov, true), Encoder::X265);
        assert_eq!(Encoder::for_format(VideoFormat::Mov, false), Encoder::X264);
        assert_eq!(
            Encoder::for_format(VideoFormat::Mp4H264, true),
            Encoder::X264
        );
        assert_eq!(Encoder::for_format(VideoFormat::WebM, true), Encoder::Vp9);
    }

    #[test]
    fn copies_only_what_the_target_holds() {
        assert!(copies_video(VideoFormat::Mov, "hevc"));
        assert!(!copies_video(VideoFormat::Mp4H264, "hevc"));
        assert!(!copies_video(VideoFormat::WebM, "h264"));
        assert!(copies_audio_into(VideoFormat::Mkv, "pcm_s16le"));
        assert!(!copies_audio_into(VideoFormat::Mp4H264, "pcm_s16le"));
        assert!(copies_audio(AudioFormat::Ogg, "opus"));
        assert!(!copies_audio(AudioFormat::Aiff, "pcm_s16le"));
    }

    #[test]
    fn convert_never_spends_more_than_a_lossy_source() {
        assert_eq!(keep_kbps(AudioFormat::M4a, None), 256);
        assert_eq!(keep_kbps(AudioFormat::M4a, Some(1_411_000)), 256);
        assert_eq!(keep_kbps(AudioFormat::Mp3, Some(128_000)), 128);
        assert_eq!(keep_kbps(AudioFormat::Ogg, Some(320_000)), 160);
        assert_eq!(keep_kbps(AudioFormat::M4a, Some(48_000)), 96);
    }
}
