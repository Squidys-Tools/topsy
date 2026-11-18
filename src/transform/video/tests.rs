//! Every operation on clips FFmpeg makes itself. They download FFmpeg (115 MB) on first run
//! and take a while: `cargo test video -- --ignored --test-threads=1`.

use std::time::Instant;

use super::*;
use crate::log::{LogErr, MissingOk};
use crate::model::video::{
    AudioFormat, GifFps, GifOptions, GifWidth, MaxResolution, Quality, VideoFormat,
};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("topsy-video-{}-{name}", std::process::id()));
    fs::remove_dir_all(&dir).missing_ok().log_err();
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn no_progress(_: f32) {}

fn ff() -> PathBuf {
    ffmpeg(&no_progress, &AtomicBool::new(false)).unwrap()
}

/// Make a clip with FFmpeg: `args` between the inputs and the output.
fn make(dir: &Path, name: &str, args: &[&str]) -> PathBuf {
    let out = dir.join(name);
    let mut all: Vec<&OsStr> = ["-hide_banner", "-loglevel", "error", "-y"]
        .into_iter()
        .chain(args.iter().copied())
        .map(OsStr::new)
        .collect();
    all.push(out.as_os_str());
    tools::run(&ff(), &all, &AtomicBool::new(false), &mut |_| {}).unwrap();
    out
}

const PICTURE: [&str; 4] = [
    "-f",
    "lavfi",
    "-i",
    "testsrc2=size=1280x720:rate=30:duration=5",
];
const SOUND: [&str; 4] = ["-f", "lavfi", "-i", "sine=frequency=440:duration=5"];

fn clip(dir: &Path) -> PathBuf {
    let mut args = PICTURE.to_vec();
    args.extend(SOUND);
    args.extend(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"]);
    args.extend([
        "-metadata",
        "title=Secret",
        "-metadata",
        "location=+51.5+000.1/",
    ]);
    make(dir, "clip.mp4", &args)
}

fn timed<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let t = f();
    eprintln!("{what}: {:.2?}", start.elapsed());
    t
}

fn report(path: &Path) -> Probe {
    let p = probe(path).unwrap();
    eprintln!(
        "  {} {} bytes, {:.2?}, video {:?}, audio {:?}",
        path.file_name().unwrap().to_string_lossy(),
        fs::metadata(path).unwrap().len(),
        p.duration,
        p.video
            .as_ref()
            .map(|v| (&v.codec, v.width, v.height, v.rotation)),
        p.audio.as_ref().map(|a| &a.codec),
    );
    p
}

fn near(d: Duration, secs: f64) -> bool {
    (d.as_secs_f64() - secs).abs() < 0.25
}

#[test]
#[ignore = "downloads FFmpeg and encodes"]
fn video_operations_on_generated_clips() {
    let dir = scratch("ops");
    let cancel = AtomicBool::new(false);
    let src = clip(&dir);
    let probe_src = report(&src);
    assert!(near(probe_src.duration, 5.0));

    let out = dir.join("compressed.mp4");
    let opts = CompressVideo {
        by: CompressBy::Quality(Quality::new(40)),
        max_resolution: MaxResolution::P480,
    };
    timed("compress by quality", || {
        compress_video(&src, opts, &out, &no_progress, &cancel)
    })
    .unwrap();
    let p = report(&out);
    assert_eq!(
        (p.video_codec(), p.resolution().unwrap().height),
        ("h264", 480)
    );

    let out = dir.join("sized.mp4");
    let target = 400_000;
    let opts = CompressVideo {
        by: CompressBy::FileSize(target),
        max_resolution: MaxResolution::Original,
    };
    timed("compress to 400 KB", || {
        compress_video(&src, opts, &out, &no_progress, &cancel)
    })
    .unwrap();
    assert!(fs::metadata(&out).unwrap().len() <= target);
    report(&out);

    for format in VideoFormat::ALL {
        let out = dir.join(format!("converted-{format:?}.{}", format.extension()));
        let target = ConvertTarget::Video(format);
        timed(format.label(), || {
            convert(&src, target, &out, &no_progress, &cancel)
        })
        .unwrap();
        let p = report(&out);
        let want = match format {
            VideoFormat::Mp4Hevc => "hevc",
            VideoFormat::WebM => "vp9",
            _ => "h264",
        };
        assert_eq!(p.video_codec(), want);
        assert!(near(p.duration, 5.0));
    }
    for format in AudioFormat::FROM_VIDEO {
        let out = dir.join(format!("sound.{}", format.extension()));
        let target = ConvertTarget::Audio(format);
        timed(format.label(), || {
            convert(&src, target, &out, &no_progress, &cancel)
        })
        .unwrap();
        assert!(!report(&out).has_video());
    }

    let range = TrimRange::new(
        Duration::from_secs(1),
        Duration::from_secs(3),
        probe_src.duration,
    )
    .unwrap();
    for mode in [TrimMode::Exact, TrimMode::Fast] {
        let out = dir.join(format!("trim-{mode:?}.mp4"));
        timed(mode.label(), || {
            trim(&src, range, mode, &out, &no_progress, &cancel)
        })
        .unwrap();
        let p = report(&out);
        assert!(
            p.duration <= Duration::from_secs_f64(3.1),
            "{mode:?} {:?}",
            p.duration
        );
        if mode == TrimMode::Exact {
            assert!(near(p.duration, 2.0));
        }
    }

    let out = dir.join("clip.gif");
    let gif = GifOptions {
        range,
        fps: GifFps::Fifteen,
        width: GifWidth::W320,
    };
    timed("gif", || make_gif(&src, &gif, &out, &no_progress, &cancel)).unwrap();
    let decoded = image::open(&out).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (320, 180));

    let out = dir.join("plain.mp4");
    timed("remove metadata", || {
        remove_metadata(&src, &out, &no_progress, &cancel)
    })
    .unwrap();
    let bytes = fs::read(&out).unwrap();
    let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
    assert!(!has(b"Secret") && !has(b"Lavf"));
    assert!(fs::read(&src).unwrap().windows(6).any(|w| w == b"Secret"));

    let frame = timed("frame", || frame_at(&src, Duration::from_secs(2), 256)).unwrap();
    assert_eq!(frame.dimensions(), (256, 144));
    assert!(frame.pixels().any(|p| p.0[0] > 200));
}

#[test]
#[ignore = "downloads FFmpeg and encodes"]
fn rotation_is_applied_once() {
    let dir = scratch("rotate");
    let cancel = AtomicBool::new(false);
    let plain = clip(&dir);
    // A phone held upright: the pixels are landscape and the file says to turn them.
    let src = make(
        &dir,
        "upright.mp4",
        &[
            "-display_rotation:v:0",
            "90",
            "-i",
            plain.to_str().unwrap(),
            "-c",
            "copy",
        ],
    );
    let p = report(&src);
    assert_eq!(p.video.as_ref().unwrap().rotation, 270);
    assert_eq!(p.resolution().unwrap().height, 1280);

    let out = dir.join("plain.mp4");
    remove_metadata(&src, &out, &no_progress, &cancel).unwrap();
    assert_eq!(
        report(&out).video,
        p.video,
        "metadata removal keeps the turn"
    );

    for (turn, size) in [
        (Rotate::Right, (1280, 720)),
        (Rotate::Half, (720, 1280)),
        (Rotate::FlipHorizontal, (720, 1280)),
    ] {
        let out = dir.join(format!("{turn:?}.mp4"));
        timed(turn.label(), || {
            rotate(&src, turn, &out, &no_progress, &cancel)
        })
        .unwrap();
        let v = report(&out).video.unwrap();
        assert_eq!(
            (v.width, v.height, v.rotation),
            (size.0, size.1, 0),
            "{turn:?}"
        );
    }

    let frame = frame_at(&src, Duration::from_secs(1), 200).unwrap();
    assert_eq!(frame.dimensions(), (113, 200));

    let hevc = make(
        &dir,
        "hevc.mkv",
        &[
            PICTURE.as_slice(),
            &["-c:v", "libx265", "-x265-params", "log-level=error"],
        ]
        .concat(),
    );
    let out = dir.join("hevc-trim.mkv");
    let range = TrimRange::new(
        Duration::ZERO,
        Duration::from_secs(2),
        Duration::from_secs(5),
    )
    .unwrap();
    trim(&hevc, range, TrimMode::Exact, &out, &no_progress, &cancel).unwrap();
    assert_eq!(report(&out).video_codec(), "hevc");
}

#[test]
#[ignore = "downloads FFmpeg and encodes"]
fn audio_operations_on_a_generated_tone() {
    let dir = scratch("audio");
    let cancel = AtomicBool::new(false);
    let tone = make(
        &dir,
        "tone.flac",
        &[
            SOUND.as_slice(),
            &["-sample_fmt", "s32", "-metadata", "artist=Someone"],
        ]
        .concat(),
    );
    assert!(report(&tone).audio.unwrap().deep);
    let mp3 = make(
        &dir,
        "tone.mp3",
        &[SOUND.as_slice(), &["-b:a", "192k"]].concat(),
    );

    let out = dir.join("small.mp3");
    let by = CompressBy::Quality(Quality::new(50));
    compress_audio(&mp3, by, &out, &no_progress, &cancel).unwrap();
    assert!(fs::metadata(&out).unwrap().len() < fs::metadata(&mp3).unwrap().len());
    let out = dir.join(format!(
        "small.{}",
        AudioFormat::for_compress(&tone).extension()
    ));
    compress_audio(
        &tone,
        CompressBy::FileSize(60_000),
        &out,
        &no_progress,
        &cancel,
    )
    .unwrap();
    assert!(fs::metadata(&out).unwrap().len() <= 60_000);
    assert_eq!(report(&out).audio_codec(), "aac");

    for format in AudioFormat::ALL {
        let out = dir.join(format!("converted.{}", format.extension()));
        convert(
            &tone,
            ConvertTarget::Audio(format),
            &out,
            &no_progress,
            &cancel,
        )
        .unwrap();
        let p = report(&out);
        assert!(near(p.duration, 5.0), "{format:?}");
    }
    let wav = report(&dir.join("converted.wav"));
    assert_eq!(wav.audio_codec(), "pcm_s24le");

    let range = TrimRange::new(
        Duration::from_secs(1),
        Duration::from_secs(2),
        Duration::from_secs(5),
    )
    .unwrap();
    for mode in [TrimMode::Exact, TrimMode::Fast] {
        let out = dir.join(format!("trim-{mode:?}.flac"));
        trim(&tone, range, mode, &out, &no_progress, &cancel).unwrap();
        assert!(near(report(&out).duration, 1.0), "{mode:?}");
    }

    let out = dir.join("plain.flac");
    remove_metadata(&tone, &out, &no_progress, &cancel).unwrap();
    let bytes = fs::read(&out).unwrap();
    assert!(!bytes.windows(7).any(|w| w == b"Someone"));
}

#[test]
#[ignore = "downloads FFmpeg"]
fn cancel_stops_an_encode() {
    let dir = scratch("cancel");
    let src = clip(&dir);
    let cancel = AtomicBool::new(false);
    let out = dir.join("never.webm");
    let target = ConvertTarget::Video(VideoFormat::WebM);
    let err = convert(
        &src,
        target,
        &out,
        &|f| {
            if f > 0.0 {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed)
            }
        },
        &cancel,
    )
    .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Interrupted);
}

/// A real clip, by hand: `REAL_CLIP=<path> cargo test real_clip -- --ignored --nocapture`.
#[test]
#[ignore = "needs a real clip"]
fn real_clip() {
    let Some(src) = std::env::var_os("REAL_CLIP").map(PathBuf::from) else {
        return;
    };
    let dir = scratch("real");
    let cancel = AtomicBool::new(false);
    let p = report(&src);
    let out = dir.join("compressed.mp4");
    let opts = CompressVideo::default();
    timed("compress 75%", || {
        compress_video(&src, opts, &out, &no_progress, &cancel)
    })
    .unwrap();
    report(&out);
    let out = dir.join("720p.mp4");
    let opts = CompressVideo {
        by: CompressBy::Quality(Quality::DEFAULT),
        max_resolution: MaxResolution::P720,
    };
    timed("compress 720p", || {
        compress_video(&src, opts, &out, &no_progress, &cancel)
    })
    .unwrap();
    report(&out);
    let out = dir.join("10mb.mp4");
    let opts = CompressVideo {
        by: CompressBy::FileSize(10_000_000),
        max_resolution: MaxResolution::Original,
    };
    timed("compress to 10 MB", || {
        compress_video(&src, opts, &out, &no_progress, &cancel)
    })
    .unwrap();
    report(&out);
    let end = p.duration.min(Duration::from_secs(4));
    let range = TrimRange::new(Duration::ZERO, end, p.duration).unwrap();
    let gif = GifOptions {
        range,
        fps: GifFps::Fifteen,
        width: GifWidth::W480,
    };
    let out = dir.join("clip.gif");
    timed("gif", || make_gif(&src, &gif, &out, &no_progress, &cancel)).unwrap();
    eprintln!("  gif {} bytes", fs::metadata(&out).unwrap().len());
    let out = dir.join(format!(
        "trim.{}",
        src.extension().unwrap().to_string_lossy()
    ));
    timed("fast trim", || {
        trim(&src, range, TrimMode::Fast, &out, &no_progress, &cancel)
    })
    .unwrap();
    report(&out);
    let out = dir.join("audio.m4a");
    let target = ConvertTarget::Audio(AudioFormat::M4a);
    timed("to m4a", || {
        convert(&src, target, &out, &no_progress, &cancel)
    })
    .unwrap();
    report(&out);
    timed("frame", || frame_at(&src, Duration::from_secs(1), 320)).unwrap();
    eprintln!("results in {}", dir.display());
}
