use std::fmt;
use std::time::Duration;

/// The shortest clip Trim or Make GIF will cut.
pub const MIN_CLIP: Duration = Duration::from_millis(100);
/// A GIF starts as the first few seconds of the clip.
pub const GIF_DEFAULT: Duration = Duration::from_secs(5);
/// GIFs past this length get huge for no good reason; Make GIF refuses them.
pub const GIF_MAX: Duration = Duration::from_secs(30);

/// A start and end time within a clip, start before end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrimRange {
    start: Duration,
    end: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrimError {
    Empty,
    TooShort,
    PastEnd,
    TooLongForGif,
}

impl fmt::Display for TrimError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            TrimError::Empty => "The end has to come after the start",
            TrimError::TooShort => "That clip is too short to keep",
            TrimError::PastEnd => "The start is past the end of the clip",
            TrimError::TooLongForGif => "A GIF can be at most 30 seconds long",
        })
    }
}

impl TrimRange {
    /// A range as the user picked it on a clip `duration` long. An end past the clip is pulled
    /// back to its end, since a slider that overshoots by a frame shouldn't fail.
    pub fn new(start: Duration, end: Duration, duration: Duration) -> Result<TrimRange, TrimError> {
        if start >= duration {
            return Err(TrimError::PastEnd);
        }
        let end = end.min(duration);
        if end <= start {
            return Err(TrimError::Empty);
        }
        if end - start < MIN_CLIP {
            return Err(TrimError::TooShort);
        }
        Ok(TrimRange { start, end })
    }

    pub fn start(&self) -> Duration {
        self.start
    }

    pub fn end(&self) -> Duration {
        self.end
    }

    pub fn len(&self) -> Duration {
        self.end - self.start
    }

    pub fn for_gif(self) -> Result<TrimRange, TrimError> {
        if self.len() > GIF_MAX {
            Err(TrimError::TooLongForGif)
        } else {
            Ok(self)
        }
    }

    /// The opening seconds of a clip `duration` long, the range Make GIF starts from.
    pub fn gif_default(duration: Duration) -> TrimRange {
        TrimRange {
            start: Duration::ZERO,
            end: duration.min(GIF_DEFAULT).max(MIN_CLIP),
        }
    }
}

/// `d` as FFmpeg reads a time: seconds with millisecond precision.
pub fn seconds(d: Duration) -> String {
    format!("{}.{:03}", d.as_secs(), d.subsec_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(secs: f64) -> Duration {
        Duration::from_secs_f64(secs)
    }

    #[test]
    fn rejects_backwards_empty_and_out_of_range() {
        let r = TrimRange::new;
        assert_eq!(r(s(3.0), s(2.0), s(10.0)), Err(TrimError::Empty));
        assert_eq!(r(s(3.0), s(3.0), s(10.0)), Err(TrimError::Empty));
        assert_eq!(r(s(3.0), s(3.05), s(10.0)), Err(TrimError::TooShort));
        assert_eq!(r(s(10.0), s(12.0), s(10.0)), Err(TrimError::PastEnd));
    }

    #[test]
    fn an_end_past_the_clip_is_clamped() {
        let r = TrimRange::new(s(8.0), s(12.0), s(10.0)).unwrap();
        assert_eq!(r.end(), s(10.0));
        assert_eq!(r.len(), s(2.0));
    }

    #[test]
    fn gif_range_defaults_to_the_opening_seconds_and_is_capped() {
        assert_eq!(TrimRange::gif_default(s(60.0)).len(), GIF_DEFAULT);
        assert_eq!(TrimRange::gif_default(s(2.0)).len(), s(2.0));
        let long = TrimRange::new(s(0.0), s(45.0), s(60.0)).unwrap();
        assert_eq!(long.for_gif(), Err(TrimError::TooLongForGif));
    }

    #[test]
    fn times_are_written_as_seconds() {
        assert_eq!(seconds(Duration::from_millis(83_250)), "83.250");
        assert_eq!(seconds(Duration::ZERO), "0.000");
    }
}
