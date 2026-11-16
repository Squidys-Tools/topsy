use super::*;
use crate::model::video::options::{GifFps, GifWidth, MaxResolution, Quality};
use crate::model::video::probe::{AudioStream, VideoStream};

const HEAD: &str = "-hide_banner -nostdin -loglevel error -y -progress pipe:1 -nostats";

fn line(args: Vec<OsString>) -> String {
    let all: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let all = all.join(" ");
    all.strip_prefix(HEAD)
        .map_or(all.clone(), |s| s.trim().to_owned())
}

fn phone(codec: &str, audio: &str) -> Probe {
    Probe {
        duration: Duration::from_secs(60),
        video: Some(VideoStream {
            codec: codec.into(),
            width: 1080,
            height: 1920,
            fps: 30.0,
            rotation: 90,
            bit_rate: Some(10_000_000),
        }),
        audio: Some(AudioStream {
            codec: audio.into(),
            bit_rate: Some(192_000),
            deep: false,
        }),
        bit_rate: None,
    }
}

fn song(codec: &str, bit_rate: Option<u32>) -> Probe {
    Probe {
        duration: Duration::from_secs(180),
        video: None,
        audio: Some(AudioStream {
            codec: codec.into(),
            bit_rate,
            deep: codec == "flac",
        }),
        bit_rate,
    }
}

fn p(s: &str) -> &Path {
    Path::new(s)
}

fn secs(a: u64, b: u64) -> TrimRange {
    TrimRange::new(
        Duration::from_secs(a),
        Duration::from_secs(b),
        Duration::from_secs(60),
    )
    .unwrap()
}

#[test]
fn compress_by_quality_caps_the_rate_and_scales_by_the_short_side() {
    let opts = CompressVideo {
        by: CompressBy::Quality(Quality::DEFAULT),
        max_resolution: MaxResolution::P720,
    };
    let plan = compress_plan(opts, &phone("hevc", "aac")).unwrap();
    assert_eq!(
        line(compress_video(p("in.mov"), p("out.mp4"), &plan)),
        "-i in.mov -map 0:V:0 -map 0:a:0? -vf scale=720:1280 \
         -c:v libx264 -preset medium -crf 23 -pix_fmt yuv420p \
         -maxrate 4354648 -bufsize 8709296 -c:a aac -b:a 128k \
         -movflags +faststart -f mp4 out.mp4"
    );
}

#[test]
fn compress_to_a_size_sets_a_bitrate() {
    let opts = CompressVideo {
        by: CompressBy::FileSize(25_000_000),
        max_resolution: MaxResolution::Original,
    };
    let plan = compress_plan(opts, &phone("h264", "aac")).unwrap();
    assert_eq!(plan.scale, None);
    let Rate::Bitrate(bps) = plan.rate else {
        panic!("{plan:?}")
    };
    assert!((bps + plan.audio_kbps * 1000) as f64 * 60.0 / 8.0 < 25_000_000.0);
    let args = line(compress_video(p("in.mp4"), p("out.mp4"), &plan));
    assert!(
        args.contains(&format!("-c:v libx264 -preset medium -b:v {bps} ")),
        "{args}"
    );
    let tiny = CompressVideo {
        by: CompressBy::FileSize(100_000),
        max_resolution: MaxResolution::Original,
    };
    assert!(compress_plan(tiny, &phone("h264", "aac")).is_err());
}

#[test]
fn compress_audio_keeps_lossy_formats_and_shrinks() {
    let q = CompressBy::Quality(Quality::DEFAULT);
    assert_eq!(compress_audio_kbps(q, &song("mp3", Some(128_000))), Ok(102));
    assert_eq!(
        compress_audio_kbps(q, &song("flac", Some(900_000))),
        Ok(256)
    );
    let by_size = compress_audio_kbps(CompressBy::FileSize(3_000_000), &song("mp3", None));
    assert!(by_size.is_ok_and(|k| k < 133));
    assert_eq!(
        line(compress_audio(p("a.mp3"), p("o.mp3"), 102)),
        "-i a.mp3 -map 0:a:0 -vn -c:a libmp3lame -b:a 102k -f mp3 o.mp3"
    );
    assert_eq!(
        line(compress_audio(p("a.flac"), p("o.m4a"), 256)),
        "-i a.flac -map 0:a:0 -vn -c:a aac -b:a 256k -movflags +faststart -f ipod o.m4a"
    );
}

#[test]
fn convert_copies_what_fits_and_encodes_the_rest() {
    let hevc = phone("hevc", "aac");
    let to = |f| {
        line(convert(
            p("in.mov"),
            p("out"),
            ConvertTarget::Video(f),
            &hevc,
        ))
    };
    assert_eq!(
        to(VideoFormat::Mp4Hevc),
        "-i in.mov -map 0:V:0 -map 0:a? -c:v copy -tag:v hvc1 -c:a copy \
         -movflags +faststart -f mp4 out"
    );
    assert_eq!(
        to(VideoFormat::Mp4H264),
        "-i in.mov -map 0:V:0 -map 0:a? -c:v libx264 -preset medium -crf 18 \
         -pix_fmt yuv420p -c:a copy -movflags +faststart -f mp4 out"
    );
    assert_eq!(
        to(VideoFormat::WebM),
        "-i in.mov -map 0:V:0 -map 0:a? -c:v libvpx-vp9 -crf 31 -b:v 0 -row-mt 1 \
         -deadline good -cpu-used 4 -pix_fmt yuv420p -c:a libopus -b:a 128k -f webm out"
    );
    assert_eq!(
        to(VideoFormat::Mkv),
        "-i in.mov -map 0:V:0 -map 0:a? -c:v copy -c:a copy -f matroska out"
    );
    let h264 = phone("h264", "pcm_s16le");
    let hevc_out = line(convert(
        p("in.avi"),
        p("out"),
        ConvertTarget::Video(VideoFormat::Mp4Hevc),
        &h264,
    ));
    assert!(hevc_out.contains("-c:v libx265 -preset medium -crf 22 -tag:v hvc1"));
    assert!(hevc_out.contains("-c:a aac -b:a 192k"));
}

#[test]
fn convert_to_sound_copies_or_encodes() {
    let to = |f, probe: &Probe| line(convert(p("in"), p("o"), ConvertTarget::Audio(f), probe));
    assert_eq!(
        to(AudioFormat::M4a, &phone("h264", "aac")),
        "-i in -map 0:a:0 -vn -c:a copy -movflags +faststart -f ipod o"
    );
    assert_eq!(
        to(AudioFormat::Mp3, &song("aac", Some(128_000))),
        "-i in -map 0:a:0 -vn -c:a libmp3lame -b:a 128k -f mp3 o"
    );
    assert_eq!(
        to(AudioFormat::Wav, &song("flac", None)),
        "-i in -map 0:a:0 -vn -c:a pcm_s24le -f wav o"
    );
    assert_eq!(
        to(AudioFormat::Aiff, &song("mp3", Some(320_000))),
        "-i in -map 0:a:0 -vn -c:a pcm_s16be -f aiff o"
    );
    assert_eq!(
        to(AudioFormat::Ogg, &song("mp3", Some(320_000))),
        "-i in -map 0:a:0 -vn -c:a libopus -b:a 160k -f ogg o"
    );
}

#[test]
fn trim_fast_copies_and_exact_encodes() {
    let probe = phone("hevc", "aac");
    let fast = trim(p("a.mp4"), p("o.mp4"), secs(5, 12), TrimMode::Fast, &probe);
    assert_eq!(
        line(fast),
        "-ss 5.000 -i a.mp4 -t 7.000 -map 0:V:0 -map 0:a? -c copy -avoid_negative_ts make_zero \
         -movflags +faststart o.mp4"
    );
    let exact = line(trim(
        p("a.mp4"),
        p("o.mp4"),
        secs(5, 12),
        TrimMode::Exact,
        &probe,
    ));
    assert!(exact.starts_with("-ss 5.000 -i a.mp4 -t 7.000 -map 0:V:0 -map 0:a? -c:v libx265"));
    assert!(exact.ends_with("-c:a copy -movflags +faststart -f mp4 o.mp4"));
    let wav = line(trim(
        p("a.wav"),
        p("o.wav"),
        secs(1, 2),
        TrimMode::Exact,
        &song("pcm_s16le", Some(1_411_200)),
    ));
    assert_eq!(
        wav,
        "-ss 1.000 -i a.wav -t 1.000 -map 0:a:0 -vn -c:a pcm_s16le -f wav o.wav"
    );
    let flac = song("flac", None);
    let fast_flac = line(trim(
        p("a.flac"),
        p("o.flac"),
        secs(1, 2),
        TrimMode::Fast,
        &flac,
    ));
    assert!(
        fast_flac.ends_with("-c:a flac -f flac o.flac"),
        "{fast_flac}"
    );
    let mp3 = line(trim(
        p("a.mp3"),
        p("o.mp3"),
        secs(1, 2),
        TrimMode::Fast,
        &song("mp3", None),
    ));
    assert_eq!(
        mp3,
        "-ss 1.000 -i a.mp3 -t 1.000 -map 0:a:0 -c copy -avoid_negative_ts make_zero o.mp3"
    );
    assert_eq!(edited_extension(p("a.MOV"), true), "mov");
    assert_eq!(edited_extension(p("a.avi"), true), "mp4");
    assert_eq!(edited_extension(p("a.wma"), false), "m4a");
}

#[test]
fn gif_builds_its_palette_in_one_graph() {
    let opts = GifOptions {
        range: secs(2, 6),
        fps: GifFps::Fifteen,
        width: GifWidth::W480,
    };
    assert_eq!(
        line(gif(p("a.mp4"), p("o.gif"), &opts, &phone("h264", "aac"))),
        "-ss 2.000 -i a.mp4 -t 4.000 -filter_complex \
         [0:V:0]fps=15,scale=480:853:flags=lanczos,split[a][b];\
         [a]palettegen=stats_mode=diff[p];\
         [b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle \
         -an -loop 0 -f gif o.gif"
    );
    let original = GifOptions {
        width: GifWidth::Original,
        ..opts
    };
    assert_eq!(gif_size(320, 240, &original), (320, 240));
    let wide = GifOptions {
        width: GifWidth::W640,
        ..opts
    };
    assert_eq!(gif_size(320, 240, &wide), (320, 240));
}

#[test]
fn rotate_filters_and_keeps_the_codec() {
    let left = line(rotate(
        p("a.MOV"),
        p("o.mov"),
        Rotate::Left,
        &phone("hevc", "aac"),
    ));
    assert_eq!(
        left,
        "-i a.MOV -map 0:V:0 -map 0:a? -vf transpose=cclock -c:v libx265 -preset medium \
         -crf 22 -tag:v hvc1 -x265-params log-level=error -pix_fmt yuv420p -c:a copy \
         -movflags +faststart -f mov o.mov"
    );
    let flip = line(rotate(
        p("a.webm"),
        p("o.webm"),
        Rotate::FlipHorizontal,
        &phone("vp9", "opus"),
    ));
    assert!(flip.contains("-vf hflip -c:v libvpx-vp9"), "{flip}");
    assert!(flip.ends_with("-c:a copy -f webm o.webm"), "{flip}");
}

#[test]
fn remove_metadata_copies_and_strips() {
    assert_eq!(
        line(remove_metadata(
            p("a.mp4"),
            p("o.mp4"),
            &phone("h264", "aac")
        )),
        "-i a.mp4 -map 0:V:0 -map 0:a? -c copy -map_metadata -1 -map_chapters -1 \
         -fflags +bitexact -flags:v +bitexact -flags:a +bitexact -movflags +faststart o.mp4"
    );
    let mp3 = line(remove_metadata(p("a.mp3"), p("o.mp3"), &song("mp3", None)));
    assert!(
        mp3.starts_with("-i a.mp3 -map 0:a -c copy -map_metadata -1"),
        "{mp3}"
    );
}

#[test]
fn a_frame_is_raw_rgba_fitted_in_the_box() {
    assert_eq!(frame_size(1080, 1920, 320), (180, 320));
    assert_eq!(frame_size(1920, 1080, 4000), (1920, 1080));
    assert_eq!(
        line(frame(p("a.mp4"), Duration::from_millis(1500), (180, 320))),
        "-hide_banner -nostdin -loglevel error -ss 1.500 -i a.mp4 -map 0:V:0 -frames:v 1 -an \
         -vf scale=180:320:flags=bilinear -f rawvideo -pix_fmt rgba pipe:1"
    );
}
