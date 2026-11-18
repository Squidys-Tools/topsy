//! The system stats widget's rules: CPU usage from two samples of the system's times, memory
//! in use, the rolling history each graph draws, and how they read on the card.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How often the card samples while it is on screen.
pub const INTERVAL: Duration = Duration::from_secs(1);
/// Points each graph keeps.
pub const CAPACITY: usize = 40;
/// A previous sample older than this is from before the card was last put away: the next one
/// is a fresh baseline, not a long average.
const STALE: Duration = Duration::from_secs(3);
const GIB: f64 = (1u64 << 30) as f64;

/// The system's times since boot, in 100 ns units, as Windows reports them: `kernel`
/// includes `idle`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuTimes {
    pub idle: u64,
    pub kernel: u64,
    pub user: u64,
}

/// How busy the CPU was between two samples, 0 to 1; `None` if no time passed between them.
pub fn cpu_usage(before: CpuTimes, after: CpuTimes) -> Option<f32> {
    let idle = after.idle.wrapping_sub(before.idle);
    let total = after
        .kernel
        .wrapping_sub(before.kernel)
        .wrapping_add(after.user.wrapping_sub(before.user));
    if total == 0 {
        return None;
    }
    let busy = total.saturating_sub(idle);
    Some((busy as f64 / total as f64).clamp(0.0, 1.0) as f32)
}

/// Physical memory, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Memory {
    pub total: u64,
    pub available: u64,
}

impl Memory {
    fn used(self) -> u64 {
        self.total.saturating_sub(self.available)
    }

    /// The fraction in use, 0 to 1.
    pub fn load(self) -> f32 {
        if self.total == 0 {
            return 0.0;
        }
        (self.used() as f64 / self.total as f64).clamp(0.0, 1.0) as f32
    }

    /// Used and total as Droppy reads them: "14.4 / 24 GB".
    pub fn gauge(self) -> String {
        let used = self.used() as f64 / GIB;
        let total = (self.total as f64 / GIB).round();
        format!("{used:.1} / {total} GB")
    }
}

/// A fraction as a whole percent, without the sign: "31".
pub fn percent(fraction: f32) -> String {
    let percent = (fraction.clamp(0.0, 1.0) * 100.0).round() as u32;
    percent.to_string()
}

/// The last `CAPACITY` values, oldest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct History(VecDeque<f32>);

impl History {
    pub fn push(&mut self, value: f32) {
        if self.0.len() == CAPACITY {
            self.0.pop_front();
        }
        self.0.push_back(value);
    }

    pub fn values(&self) -> impl Iterator<Item = f32> + '_ {
        self.0.iter().copied()
    }

    /// The value the graph's top stands for: a little over the highest point, and never under a
    /// quarter, so a quiet CPU still moves and an idle one stays low.
    pub fn ceiling(&self) -> f32 {
        let highest = self.values().fold(0.0f32, f32::max);
        (highest * 1.25).clamp(0.25, 1.0)
    }
}

/// What the card shows, kept between openings.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    last: Option<(CpuTimes, Instant)>,
    sampled: Option<Instant>,
    cpu: Option<f32>,
    memory: Option<Memory>,
    cpu_history: History,
    memory_history: History,
}

impl Stats {
    /// When the next sample is due: at once if the last one is stale (a baseline for the CPU,
    /// and fresh memory), else a sampling interval on.
    pub fn next_sample(&self, now: Instant) -> Duration {
        match self.sampled {
            Some(at) if now.saturating_duration_since(at) < STALE => INTERVAL,
            Some(_) | None => Duration::ZERO,
        }
    }

    /// Take one sample; either half may have failed to read.
    pub fn sample(&mut self, times: Option<CpuTimes>, memory: Option<Memory>, now: Instant) {
        self.sampled = Some(now);
        if let Some(times) = times {
            let fresh = self
                .last
                .filter(|(_, at)| now.saturating_duration_since(*at) < STALE);
            if let Some((before, _)) = fresh
                && let Some(usage) = cpu_usage(before, times)
            {
                self.cpu = Some(usage);
                self.cpu_history.push(usage);
            }
            self.last = Some((times, now));
        }
        if let Some(memory) = memory {
            self.memory = Some(memory);
            self.memory_history.push(memory.load());
        }
    }

    /// The CPU's last usage, if two samples have been taken.
    pub fn cpu(&self) -> Option<f32> {
        self.cpu
    }

    pub fn memory(&self) -> Option<Memory> {
        self.memory
    }

    pub fn cpu_history(&self) -> &History {
        &self.cpu_history
    }

    pub fn memory_history(&self) -> &History {
        &self.memory_history
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn times(idle: u64, kernel: u64, user: u64) -> CpuTimes {
        CpuTimes { idle, kernel, user }
    }

    #[test]
    fn usage_counts_kernel_time_net_of_idle() {
        let before = times(1_000, 2_000, 500);
        // 100 idle, 200 kernel of which 100 idle, 100 user: 200 busy of 300.
        let after = times(1_100, 2_200, 600);
        let usage = cpu_usage(before, after).unwrap();
        assert!((usage - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn usage_without_time_passing_is_unknown() {
        let same = times(10, 20, 30);
        assert_eq!(cpu_usage(same, same), None);
    }

    #[test]
    fn usage_stays_in_range() {
        // Idle reported ahead of kernel time, as a racy read can.
        assert_eq!(cpu_usage(times(0, 0, 0), times(500, 100, 0)), Some(0.0));
        assert_eq!(cpu_usage(times(0, 0, 0), times(0, 100, 100)), Some(1.0));
    }

    #[test]
    fn history_keeps_the_newest_in_order() {
        let mut history = History::default();
        for i in 0..CAPACITY + 5 {
            history.push(i as f32);
        }
        let values: Vec<f32> = history.values().collect();
        assert_eq!(values.len(), CAPACITY);
        assert_eq!(values[0], 5.0);
        assert_eq!(values[CAPACITY - 1], (CAPACITY + 4) as f32);
        assert!(values.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn ceiling_sits_above_the_peak_within_bounds() {
        let mut history = History::default();
        assert_eq!(history.ceiling(), 0.25);
        history.push(0.05);
        assert_eq!(history.ceiling(), 0.25);
        history.push(0.6);
        assert!((history.ceiling() - 0.75).abs() < 1e-6);
        history.push(0.95);
        assert_eq!(history.ceiling(), 1.0);
    }

    #[test]
    fn memory_reads_like_droppy() {
        let gib = 1u64 << 30;
        let memory = Memory {
            total: 24 * gib,
            available: 24 * gib - (144 * gib) / 10,
        };
        assert_eq!(memory.gauge(), "14.4 / 24 GB");
        assert_eq!(percent(memory.load()), "60");
        // Windows reports a little under the installed amount.
        let reserved = Memory {
            total: 32 * gib - gib / 8,
            available: 16 * gib,
        };
        assert_eq!(reserved.gauge(), "15.9 / 32 GB");
    }

    #[test]
    fn empty_memory_is_unloaded() {
        let memory = Memory {
            total: 0,
            available: 0,
        };
        assert_eq!(memory.load(), 0.0);
    }

    #[test]
    fn percent_rounds_and_clamps() {
        assert_eq!(percent(0.314), "31");
        assert_eq!(percent(0.996), "100");
        assert_eq!(percent(1.5), "100");
        assert_eq!(percent(-0.2), "0");
    }

    #[test]
    fn first_sample_is_a_baseline() {
        let start = Instant::now();
        let mut stats = Stats::default();
        assert_eq!(stats.next_sample(start), Duration::ZERO);
        let memory = Memory {
            total: 100,
            available: 40,
        };
        stats.sample(Some(times(0, 0, 0)), Some(memory), start);
        assert_eq!(stats.cpu(), None);
        assert_eq!(stats.memory(), Some(memory));
        assert_eq!(stats.next_sample(start), INTERVAL);

        let second = start + INTERVAL;
        stats.sample(Some(times(50, 100, 100)), Some(memory), second);
        assert_eq!(stats.cpu(), Some(0.75));
        assert_eq!(stats.cpu_history().values().count(), 1);
        assert_eq!(stats.memory_history().values().count(), 2);
    }

    #[test]
    fn reopening_keeps_the_last_values_and_rebases() {
        let start = Instant::now();
        let mut stats = Stats::default();
        stats.sample(Some(times(0, 0, 0)), None, start);
        stats.sample(Some(times(50, 100, 100)), None, start + INTERVAL);
        let reopened = start + Duration::from_secs(60);
        assert_eq!(stats.next_sample(reopened), Duration::ZERO);
        // A minute of idling while put away must not read as the new usage.
        stats.sample(Some(times(10_000, 10_100, 100)), None, reopened);
        assert_eq!(stats.cpu(), Some(0.75));
        assert_eq!(stats.cpu_history().values().count(), 1);
        stats.sample(Some(times(10_100, 10_200, 100)), None, reopened + INTERVAL);
        assert_eq!(stats.cpu(), Some(0.0));
        assert_eq!(stats.cpu_history().values().count(), 2);
    }

    #[test]
    fn failed_read_keeps_the_previous_baseline() {
        let start = Instant::now();
        let mut stats = Stats::default();
        stats.sample(Some(times(0, 0, 0)), None, start);
        stats.sample(None, None, start + INTERVAL);
        stats.sample(Some(times(100, 200, 0)), None, start + INTERVAL * 2);
        assert_eq!(stats.cpu(), Some(0.5));
    }

    #[test]
    fn failing_reads_still_wait_an_interval() {
        let start = Instant::now();
        let mut stats = Stats::default();
        stats.sample(Some(times(0, 0, 0)), None, start);
        let later = start + Duration::from_secs(10);
        stats.sample(None, None, later);
        assert_eq!(stats.next_sample(later), INTERVAL);
    }
}
