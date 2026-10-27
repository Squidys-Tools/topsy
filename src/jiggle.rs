//! Detects the "jiggle a drag to summon the tray" gesture from raw pointer samples.
//!
//! Pure logic (no Win32), so it's unit-testable. The mouse hook feeds it moves
//! and button transitions; it reports when a held-button drag reverses
//! horizontal direction enough times in a short window.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub struct JiggleConfig {
    /// Minimum horizontal travel for a stroke to count as a direction change.
    pub min_stroke_px: i32,
    /// Direction reversals needed to trigger.
    pub reversals: usize,
    /// All reversals must fall inside this window.
    pub window: Duration,
    /// Ignore the first moments of a press so a plain click-drag start isn't a jiggle.
    pub min_hold: Duration,
}

impl Default for JiggleConfig {
    fn default() -> Self {
        Self {
            min_stroke_px: 40,
            reversals: 3,
            window: Duration::from_millis(700),
            min_hold: Duration::from_millis(120),
        }
    }
}

pub struct JiggleDetector {
    config: JiggleConfig,
    pressed_at: Option<Instant>,
    /// Extreme x of the stroke currently in progress.
    anchor_x: i32,
    /// +1 right, -1 left, 0 unknown.
    direction: i32,
    reversal_times: VecDeque<Instant>,
}

impl JiggleDetector {
    pub fn new(config: JiggleConfig) -> Self {
        Self {
            config,
            pressed_at: None,
            anchor_x: 0,
            direction: 0,
            reversal_times: VecDeque::new(),
        }
    }

    pub fn button_down(&mut self, now: Instant, x: i32) {
        self.pressed_at = Some(now);
        self.anchor_x = x;
        self.direction = 0;
        self.reversal_times.clear();
    }

    pub fn button_up(&mut self) {
        self.pressed_at = None;
        self.direction = 0;
        self.reversal_times.clear();
    }

    /// Feed a pointer move. Returns true when the gesture completes.
    pub fn moved(&mut self, now: Instant, x: i32) -> bool {
        let Some(pressed_at) = self.pressed_at else {
            return false;
        };
        let delta = x - self.anchor_x;
        let dir = delta.signum();

        if self.direction == 0 {
            if delta.abs() >= self.config.min_stroke_px {
                self.direction = dir;
                self.anchor_x = x;
            }
            return false;
        }

        if dir == self.direction || dir == 0 {
            // Still travelling the same way: extend the stroke.
            self.anchor_x = x;
            return false;
        }

        if delta.abs() >= self.config.min_stroke_px {
            self.direction = dir;
            self.anchor_x = x;
            self.reversal_times.push_back(now);
            while let Some(&first) = self.reversal_times.front() {
                if now.duration_since(first) > self.config.window {
                    self.reversal_times.pop_front();
                } else {
                    break;
                }
            }
            if self.reversal_times.len() >= self.config.reversals
                && now.duration_since(pressed_at) >= self.config.min_hold
            {
                self.reversal_times.clear();
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(d: &mut JiggleDetector, t0: Instant, samples: &[(u64, i32)]) -> bool {
        let mut hit = false;
        for &(ms, x) in samples {
            hit |= d.moved(t0 + Duration::from_millis(ms), x);
        }
        hit
    }

    #[test]
    fn side_to_side_drag_triggers() {
        let t0 = Instant::now();
        let mut d = JiggleDetector::new(JiggleConfig::default());
        d.button_down(t0, 500);
        let hit = feed(
            &mut d,
            t0,
            &[
                (50, 560),
                (100, 500),
                (150, 560),
                (200, 500),
                (250, 560),
                (300, 500),
            ],
        );
        assert!(hit);
    }

    #[test]
    fn straight_drag_does_not_trigger() {
        let t0 = Instant::now();
        let mut d = JiggleDetector::new(JiggleConfig::default());
        d.button_down(t0, 100);
        let samples: Vec<_> = (1..60).map(|i| (i as u64 * 16, 100 + i * 20)).collect();
        assert!(!feed(&mut d, t0, &samples));
    }

    #[test]
    fn tremor_below_stroke_threshold_does_not_trigger() {
        let t0 = Instant::now();
        let mut d = JiggleDetector::new(JiggleConfig::default());
        d.button_down(t0, 500);
        let samples: Vec<_> = (1..80)
            .map(|i| (i as u64 * 10, 500 + if i % 2 == 0 { 8 } else { -8 }))
            .collect();
        assert!(!feed(&mut d, t0, &samples));
    }

    #[test]
    fn slow_wandering_outside_window_does_not_trigger() {
        let t0 = Instant::now();
        let mut d = JiggleDetector::new(JiggleConfig::default());
        d.button_down(t0, 500);
        // One reversal every 600ms: never 3 inside 700ms.
        let samples = [
            (600, 560),
            (1200, 500),
            (1800, 560),
            (2400, 500),
            (3000, 560),
        ];
        assert!(!feed(&mut d, t0, &samples));
    }

    #[test]
    fn nothing_without_button_held() {
        let t0 = Instant::now();
        let mut d = JiggleDetector::new(JiggleConfig::default());
        let hit = feed(
            &mut d,
            t0,
            &[
                (50, 560),
                (100, 500),
                (150, 560),
                (200, 500),
                (250, 560),
                (300, 500),
            ],
        );
        assert!(!hit);
    }

    #[test]
    fn release_resets_progress() {
        let t0 = Instant::now();
        let mut d = JiggleDetector::new(JiggleConfig::default());
        d.button_down(t0, 500);
        feed(&mut d, t0, &[(50, 560), (100, 500), (150, 560)]);
        d.button_up();
        d.button_down(t0 + Duration::from_millis(200), 500);
        assert!(!feed(&mut d, t0, &[(250, 560), (300, 500)]));
    }
}
