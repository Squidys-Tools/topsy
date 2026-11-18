use std::fmt;
use std::time::Duration;

use super::options::MaxResolution;

/// Short sides a file-size compress steps down through when the bits run thin.
const LADDER: [u32; 7] = [2160, 1440, 1080, 720, 540, 480, 360];
/// Below this many bits per pixel per frame H.264 turns to mush; a size target that would need
/// less gets a smaller picture first.
const COMFORTABLE_BPP: f64 = 0.04;
/// The floor at the smallest picture: past this a size target is refused, not met with soup.
const MIN_BPP: f64 = 0.012;
const MIN_VIDEO_BPS: f64 = 64_000.0;
/// What the MP4 boxes, indexes and encoder overshoot take beyond the streams themselves.
const CONTAINER_SHARE: f64 = 0.03;
const CONTAINER_FIXED: u64 = 48 * 1024;
const AUDIO_BPS: u32 = 128_000;
const AUDIO_BPS_LEAN: u32 = 96_000;
const AUDIO_MIN_KBPS: u32 = 32;
const AUDIO_MAX_KBPS: u32 = 320;

/// A picture size, both sides even as the encoders need.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

impl Resolution {
    /// `width` x `height` shrunk, never grown, until its short side is at most `max_short`.
    pub fn fit(width: u32, height: u32, max_short: Option<u32>) -> Resolution {
        let short = width.min(height).max(1);
        let scale = match max_short {
            Some(max) if short > max => max as f64 / short as f64,
            _ => 1.0,
        };
        let even = |v: u32| ((((v as f64) * scale).round() as u32) & !1).max(2);
        Resolution {
            width: even(width),
            height: even(height),
        }
    }

    pub fn short_side(self) -> u32 {
        self.width.min(self.height)
    }

    fn pixels(self) -> f64 {
        self.width as f64 * self.height as f64
    }
}

fn sane_fps(fps: f64) -> f64 {
    if fps.is_finite() && (1.0..=240.0).contains(&fps) {
        fps
    } else {
        30.0
    }
}

fn bpp(bps: f64, res: Resolution, fps: f64) -> f64 {
    bps / (res.pixels() * sane_fps(fps))
}

/// The most a quality compress may spend on the video: 80% of what the source spent on the
/// same pixels, so Compress always shrinks a file even when the slider asks for more.
pub fn quality_ceiling(source_bps: u32, source: Resolution, out: Resolution) -> u32 {
    let shrink = (out.pixels() / source.pixels()).min(1.0).powf(0.75);
    (source_bps as f64 * shrink * 0.8).max(MIN_VIDEO_BPS) as u32
}

/// What the size math needs to know about the source.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clip {
    /// As displayed, after rotation.
    pub resolution: Resolution,
    pub fps: f64,
    pub has_audio: bool,
    pub video_bps: Option<u32>,
}

/// The plan for "Compress by File size".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    pub video_bps: u32,
    /// Zero when there is no sound to keep.
    pub audio_bps: u32,
    pub resolution: Resolution,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeError {
    /// The target can't hold this clip; `smallest` is about the least it can be.
    TooSmall { smallest: u64 },
}

impl fmt::Display for SizeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SizeError::TooSmall { smallest } => write!(
                f,
                "That's too small for this clip; try {} or more",
                megabytes(*smallest)
            ),
        }
    }
}

/// `bytes` as the panel writes sizes, "4.2 MB", rounded up so a hint is always enough.
pub fn megabytes(bytes: u64) -> String {
    let mb = bytes as f64 / 1_000_000.0;
    if mb >= 10.0 {
        format!("{:.0} MB", mb.ceil())
    } else {
        format!("{:.1} MB", (mb * 10.0).ceil() / 10.0)
    }
}

fn stream_bytes(bps: f64, duration: Duration) -> f64 {
    bps * duration.as_secs_f64() / 8.0
}

/// Split `target` bytes between picture and sound for a clip `duration` long, stepping the
/// picture down when the bits per pixel get too thin to look like anything.
pub fn budget_for_size(
    target: u64,
    duration: Duration,
    clip: &Clip,
    max: MaxResolution,
) -> Result<Budget, SizeError> {
    let (source, fps) = (clip.resolution, clip.fps);
    let secs = duration.as_secs_f64().max(0.1);
    let usable = (target as f64 * (1.0 - CONTAINER_SHARE) - CONTAINER_FIXED as f64).max(0.0);
    let per_sec = usable * 8.0 / secs;
    let audio_bps = match clip.has_audio {
        false => 0,
        true if per_sec >= 1_000_000.0 => AUDIO_BPS,
        true => AUDIO_BPS_LEAN,
    };
    let mut video = per_sec - audio_bps as f64;
    if let Some(src) = clip.video_bps {
        video = video.min(src as f64);
    }

    let mut res = Resolution::fit(source.width, source.height, max.short_side());
    for step in LADDER {
        if bpp(video, res, fps) >= COMFORTABLE_BPP {
            break;
        }
        if step < res.short_side() {
            res = Resolution::fit(source.width, source.height, Some(step));
        }
    }

    let floor = (MIN_BPP * res.pixels() * sane_fps(fps)).max(MIN_VIDEO_BPS);
    if video < floor {
        let smallest = (stream_bytes(floor + audio_bps as f64, duration) + CONTAINER_FIXED as f64)
            / (1.0 - CONTAINER_SHARE);
        return Err(SizeError::TooSmall {
            smallest: smallest.ceil() as u64,
        });
    }
    Ok(Budget {
        video_bps: video as u32,
        audio_bps,
        resolution: res,
    })
}

/// The bitrate for another pass after a pass at `bps` came out `actual` bytes against a
/// `target`: scaled by the overshoot, with a margin so the next one lands under.
pub fn retry_bitrate(bps: u32, target: u64, actual: u64) -> u32 {
    let ratio = target as f64 / actual.max(1) as f64;
    ((bps as f64 * ratio * 0.93) as u32).max(MIN_VIDEO_BPS as u32)
}

/// The audio bitrate, kbps, that keeps sound `duration` long under `target` bytes.
pub fn audio_kbps_for_size(target: u64, duration: Duration) -> Result<u32, SizeError> {
    let secs = duration.as_secs_f64().max(0.1);
    let usable = target as f64 * (1.0 - CONTAINER_SHARE) - 8192.0;
    let kbps = ((usable * 8.0 / secs / 1000.0).floor() as u32).min(AUDIO_MAX_KBPS);
    if usable <= 0.0 || kbps < AUDIO_MIN_KBPS {
        let least = stream_bytes(AUDIO_MIN_KBPS as f64 * 1000.0, duration) + 8192.0;
        return Err(SizeError::TooSmall {
            smallest: (least / (1.0 - CONTAINER_SHARE)).ceil() as u64,
        });
    }
    Ok(kbps)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HD: Resolution = Resolution {
        width: 1920,
        height: 1080,
    };

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn clip(has_audio: bool, video_bps: Option<u32>) -> Clip {
        Clip {
            resolution: HD,
            fps: 30.0,
            has_audio,
            video_bps,
        }
    }

    fn res(width: u32, height: u32) -> Resolution {
        Resolution { width, height }
    }

    #[test]
    fn fit_shrinks_by_the_short_side_and_keeps_sides_even() {
        assert_eq!(Resolution::fit(1920, 1080, Some(720)), res(1280, 720));
        assert_eq!(Resolution::fit(1080, 1920, Some(720)), res(720, 1280));
        assert_eq!(Resolution::fit(1280, 720, Some(1080)), res(1280, 720));
        assert_eq!(Resolution::fit(853, 481, None), res(852, 480));
    }

    #[test]
    fn quality_never_spends_more_than_the_source() {
        assert_eq!(quality_ceiling(4_000_000, HD, HD), 3_200_000);
        assert!(quality_ceiling(4_000_000, HD, res(1280, 720)) < 3_200_000);
        assert_eq!(quality_ceiling(4_000_000, HD, res(3840, 2160)), 3_200_000);
    }

    #[test]
    fn a_size_budget_fits_inside_the_target() {
        let target = 25_000_000;
        let b = budget_for_size(target, secs(60), &clip(true, None), MaxResolution::Original);
        let b = b.unwrap();
        let bytes = (b.video_bps + b.audio_bps) as f64 * 60.0 / 8.0;
        assert!(bytes < target as f64 * 0.97, "{bytes}");
        assert_eq!(b.audio_bps, 128_000);
        assert_eq!(b.resolution, HD);
    }

    #[test]
    fn a_thin_budget_steps_the_picture_down_then_refuses() {
        let b = budget_for_size(
            8_000_000,
            secs(120),
            &clip(true, None),
            MaxResolution::Original,
        );
        let b = b.unwrap();
        assert!(b.resolution.short_side() < 1080, "{b:?}");
        assert_eq!(b.audio_bps, 96_000);
        let tiny = budget_for_size(1_000_000, secs(600), &clip(true, None), MaxResolution::P480);
        let Err(SizeError::TooSmall { smallest }) = tiny else {
            panic!("{tiny:?}")
        };
        assert!(smallest > 1_000_000);
        let enough = budget_for_size(smallest, secs(600), &clip(true, None), MaxResolution::P480);
        assert!(enough.is_ok());
    }

    #[test]
    fn a_roomy_target_spends_no_more_than_the_source() {
        let b = budget_for_size(
            500_000_000,
            secs(10),
            &clip(false, Some(6_000_000)),
            MaxResolution::P720,
        )
        .unwrap();
        assert_eq!(b.video_bps, 6_000_000);
        assert_eq!(b.audio_bps, 0);
        assert_eq!(b.resolution.short_side(), 720);
    }

    #[test]
    fn retry_lands_under_the_overshoot() {
        let next = retry_bitrate(2_000_000, 10_000_000, 11_000_000);
        assert!(next < (2_000_000.0 * 10.0 / 11.0) as u32);
    }

    #[test]
    fn audio_size_picks_a_rate_that_fits() {
        let three_min = secs(180);
        assert_eq!(audio_kbps_for_size(50_000_000, three_min), Ok(320));
        let kbps = audio_kbps_for_size(3_000_000, three_min).unwrap();
        assert!((120..=130).contains(&kbps), "{kbps}");
        assert!(kbps as f64 * 1000.0 * 180.0 / 8.0 < 3_000_000.0);
        let Err(SizeError::TooSmall { smallest }) = audio_kbps_for_size(500_000, three_min) else {
            panic!()
        };
        assert_eq!(audio_kbps_for_size(smallest, three_min), Ok(32));
    }

    #[test]
    fn megabytes_round_up_so_the_hint_is_enough() {
        assert_eq!(megabytes(4_210_000), "4.3 MB");
        assert_eq!(megabytes(25_100_000), "26 MB");
    }
}
