use std::time::Duration;

/// Reads FFmpeg's `-progress pipe:1` report, `key=value` lines in blocks every half second,
/// into a fraction of an output `total` long.
pub struct Progress {
    total_us: u64,
}

impl Progress {
    pub fn new(total: Duration) -> Self {
        Progress {
            total_us: total.as_micros().max(1) as u64,
        }
    }

    /// The fraction done, for the lines that tell it. `out_time_us` is `N/A` until the first
    /// frame is out. (`out_time_ms` is microseconds too, an old FFmpeg slip, so it is ignored.)
    pub fn line(&self, line: &str) -> Option<f32> {
        let (key, value) = line.trim().split_once('=')?;
        match key {
            "out_time_us" => {
                let us = value.parse::<i64>().ok()?.max(0) as u64;
                Some((us as f64 / self.total_us as f64).min(1.0) as f32)
            }
            "progress" if value == "end" => Some(1.0),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_out_time_against_the_total() {
        let p = Progress::new(Duration::from_secs(10));
        assert_eq!(p.line("out_time_us=2500000"), Some(0.25));
        assert_eq!(p.line("out_time_ms=2500000"), None);
        assert_eq!(p.line("out_time_us=N/A"), None);
        assert_eq!(p.line("out_time_us=-23220"), Some(0.0));
        assert_eq!(p.line("out_time_us=99000000"), Some(1.0));
        assert_eq!(p.line("progress=continue"), None);
        assert_eq!(p.line("progress=end"), Some(1.0));
        assert_eq!(p.line("frame=12"), None);
    }
}
