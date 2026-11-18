//! The Pomodoro timer's rules: a focus phase and a break phase, each a whole number of minutes,
//! counted down from start to finish with pauses in between. A phase that runs out leaves the
//! timer idle on the other one, so the next press starts it.

use std::time::{Duration, Instant};

pub const MINUTE_STEP: u32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Focus,
    Break,
}

impl Phase {
    pub fn other(self) -> Self {
        match self {
            Phase::Focus => Phase::Break,
            Phase::Break => Phase::Focus,
        }
    }

    /// The minutes it may be set to.
    fn range(self) -> (u32, u32) {
        match self {
            Phase::Focus => (5, 60),
            Phase::Break => (5, 30),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clock {
    Idle,
    Running { ends: Instant },
    Paused { left: Duration },
}

#[derive(Debug)]
pub struct Pomodoro {
    phase: Phase,
    focus: u32,
    rest: u32,
    clock: Clock,
}

impl Default for Pomodoro {
    fn default() -> Self {
        Self {
            phase: Phase::Focus,
            focus: 25,
            rest: 5,
            clock: Clock::Idle,
        }
    }
}

impl Pomodoro {
    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn clock(&self) -> Clock {
        self.clock
    }

    /// The minutes set for the current phase.
    pub fn minutes(&self) -> u32 {
        match self.phase {
            Phase::Focus => self.focus,
            Phase::Break => self.rest,
        }
    }

    fn full(&self) -> Duration {
        Duration::from_secs(u64::from(self.minutes()) * 60)
    }

    /// Start the phase from its full time, or resume it from where it was paused.
    pub fn start(&mut self, now: Instant) {
        let left = match self.clock {
            Clock::Idle => self.full(),
            Clock::Paused { left } => left,
            Clock::Running { .. } => return,
        };
        self.clock = Clock::Running { ends: now + left };
    }

    pub fn pause(&mut self, now: Instant) {
        if let Clock::Running { ends } = self.clock {
            self.clock = Clock::Paused {
                left: ends.saturating_duration_since(now),
            };
        }
    }

    /// Back to idle, with the phase's full time.
    pub fn reset(&mut self) {
        self.clock = Clock::Idle;
    }

    /// Switch to `phase`, idle and full.
    pub fn choose(&mut self, phase: Phase) {
        self.phase = phase;
        self.reset();
    }

    /// Change the current phase's minutes by `steps` of [`MINUTE_STEP`], within its range. Only
    /// an idle timer can be changed; true if the minutes changed.
    pub fn adjust(&mut self, steps: i32) -> bool {
        if self.clock != Clock::Idle {
            return false;
        }
        let (low, high) = self.phase.range();
        let step = steps.saturating_mul(MINUTE_STEP as i32);
        let wanted = self.minutes().saturating_add_signed(step).clamp(low, high);
        let minutes = match self.phase {
            Phase::Focus => &mut self.focus,
            Phase::Break => &mut self.rest,
        };
        let changed = *minutes != wanted;
        *minutes = wanted;
        changed
    }

    /// Whether `steps` would change the minutes now, so the button can show as dead.
    pub fn can_adjust(&self, steps: i32) -> bool {
        let (low, high) = self.phase.range();
        self.clock == Clock::Idle
            && match steps.signum() {
                1 => self.minutes() < high,
                -1 => self.minutes() > low,
                _ => false,
            }
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        match self.clock {
            Clock::Idle => self.full(),
            Clock::Running { ends } => ends.saturating_duration_since(now),
            Clock::Paused { left } => left,
        }
    }

    /// How long until the countdown next reads a new second, while it runs.
    pub fn until_next_second(&self, now: Instant) -> Option<Duration> {
        let Clock::Running { .. } = self.clock else {
            return None;
        };
        let left = self.remaining(now);
        if left.is_zero() {
            return None;
        }
        let into = Duration::from_nanos(u64::from(left.subsec_nanos()));
        Some(if into.is_zero() {
            Duration::from_secs(1)
        } else {
            into
        })
    }

    /// End the running phase if its time is up: the timer goes idle on the other phase, and the
    /// phase that ended is returned.
    pub fn finish(&mut self, now: Instant) -> Option<Phase> {
        let Clock::Running { ends } = self.clock else {
            return None;
        };
        if now < ends {
            return None;
        }
        let ended = self.phase;
        self.choose(ended.other());
        Some(ended)
    }
}

/// `mm:ss`, counting a started second as whole so the clock reads `00:00` only at the end.
pub fn clock(left: Duration) -> String {
    let seconds = left.as_secs() + u64::from(left.subsec_nanos() > 0);
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    #[test]
    fn runs_down_from_the_phase_time() {
        let t = Instant::now();
        let mut p = Pomodoro::default();
        assert_eq!(clock(p.remaining(t)), "25:00");
        p.start(t);
        assert_eq!(clock(p.remaining(t)), "25:00");
        assert_eq!(clock(p.remaining(t + Duration::from_millis(1))), "25:00");
        assert_eq!(clock(p.remaining(t + SECOND)), "24:59");
        assert_eq!(p.finish(t + SECOND), None);
        p.start(t + SECOND);
        assert_eq!(
            clock(p.remaining(t + SECOND)),
            "24:59",
            "a second start is ignored"
        );
    }

    #[test]
    fn pause_and_resume_keep_the_time_left() {
        let t = Instant::now();
        let mut p = Pomodoro::default();
        p.start(t);
        p.pause(t + Duration::from_secs(60));
        let later = t + Duration::from_secs(3600);
        assert_eq!(p.remaining(later), Duration::from_secs(24 * 60));
        assert_eq!(p.finish(later), None, "a paused phase never runs out");
        p.start(later);
        assert_eq!(
            p.clock(),
            Clock::Running {
                ends: later + Duration::from_secs(24 * 60)
            }
        );
        assert_eq!(clock(p.remaining(later + SECOND)), "23:59");
    }

    #[test]
    fn a_finish_flips_the_phase_and_waits() {
        let t = Instant::now();
        let mut p = Pomodoro::default();
        p.start(t);
        let ends = t + Duration::from_secs(25 * 60);
        assert_eq!(p.finish(ends - Duration::from_millis(1)), None);
        assert_eq!(p.finish(ends), Some(Phase::Focus));
        assert_eq!(p.phase(), Phase::Break);
        assert_eq!(p.clock(), Clock::Idle);
        assert_eq!(clock(p.remaining(ends)), "05:00");
        assert_eq!(p.finish(ends), None);

        p.start(ends);
        assert_eq!(
            p.finish(ends + Duration::from_secs(301)),
            Some(Phase::Break)
        );
        assert_eq!(p.phase(), Phase::Focus);
    }

    #[test]
    fn reset_and_choose_go_back_to_the_full_time() {
        let t = Instant::now();
        let mut p = Pomodoro::default();
        p.start(t);
        p.reset();
        assert_eq!(p.clock(), Clock::Idle);
        assert_eq!(clock(p.remaining(t + SECOND * 90)), "25:00");

        p.start(t);
        p.choose(Phase::Break);
        assert_eq!(p.clock(), Clock::Idle);
        assert_eq!(clock(p.remaining(t)), "05:00");
    }

    #[test]
    fn minutes_move_in_steps_within_range_only_while_idle() {
        let t = Instant::now();
        let mut p = Pomodoro::default();
        for _ in 0..10 {
            p.adjust(1);
        }
        assert_eq!(p.minutes(), 60);
        assert!(!p.can_adjust(1));
        assert!(!p.adjust(1));
        assert_eq!(clock(p.remaining(t)), "60:00");
        for _ in 0..20 {
            p.adjust(-1);
        }
        assert_eq!(p.minutes(), 5);
        assert!(!p.can_adjust(-1));

        p.choose(Phase::Break);
        assert!(p.adjust(5));
        assert_eq!(p.minutes(), 30);
        p.choose(Phase::Focus);
        assert_eq!(p.minutes(), 5, "each phase keeps its own minutes");

        p.start(t);
        assert!(!p.can_adjust(1));
        assert!(!p.adjust(1));
        p.pause(t);
        assert!(!p.adjust(1));
        assert_eq!(p.minutes(), 5);
    }

    #[test]
    fn the_redraw_lands_on_the_next_second() {
        let t = Instant::now();
        let mut p = Pomodoro::default();
        assert_eq!(p.until_next_second(t), None);
        p.start(t);
        assert_eq!(p.until_next_second(t), Some(SECOND));
        let at = t + Duration::from_millis(300);
        assert_eq!(p.until_next_second(at), Some(Duration::from_millis(700)));
        assert_eq!(p.until_next_second(t + Duration::from_secs(25 * 60)), None);
        p.pause(at);
        assert_eq!(p.until_next_second(at), None);
    }
}
