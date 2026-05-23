use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub struct PulseTracker {
    history: VecDeque<Instant>,
}

impl PulseTracker {
    pub fn new() -> PulseTracker {
        PulseTracker { history: VecDeque::new() }
    }

    pub fn update(&mut self, now: Instant) {
        self.history.push_back(now);
        while self.history.len() > 25 {
            self.history.pop_front();
        }
    }

    pub fn bpm(&self) -> Option<u32> {
        if self.history.len() < 2 {
            return None;
        }
        let entries: Vec<Instant> = self.history.iter().copied().collect();
        let intervals: Vec<u128> = entries.windows(2)
            .map(|w| w[1].duration_since(w[0]).as_nanos())
            .collect();
        let total: u128 = intervals.iter().sum();
        let avg = total / intervals.len() as u128;
        if avg == 0 {
            return None;
        }
        // BPM = 60_000_000_000 ns / (avg_ns * 24 pulses_per_beat)
        Some((60_000_000_000u128 / (avg * 24)) as u32)
    }

    pub fn timeout_duration(&self) -> Option<Duration> {
        if self.history.len() < 2 {
            return None;
        }
        let len = self.history.len();
        let last_interval = self.history[len - 1].duration_since(self.history[len - 2]);
        let nanos = (last_interval.as_nanos() as f64 * 3.5) as u64;
        Some(Duration::from_nanos(nanos))
    }

    pub fn is_clock_active(&self, now: Instant) -> bool {
        let last = match self.history.back() {
            Some(t) => *t,
            None => return false,
        };
        let timeout = match self.timeout_duration() {
            Some(t) => t,
            None => return false,
        };
        now.saturating_duration_since(last) < timeout
    }

    pub fn reset(&mut self) {
        self.history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // T-1: bpm() returns None with <2 pulses; ~120 after 25 evenly-spaced pulses
    #[test]
    fn bpm_none_with_fewer_than_two_pulses() {
        let mut t = PulseTracker::new();
        assert_eq!(t.bpm(), None);
        t.update(Instant::now());
        assert_eq!(t.bpm(), None);
    }

    #[test]
    fn bpm_returns_120_after_25_pulses() {
        let mut t = PulseTracker::new();
        // 120 BPM, 24-PPQN: interval = 60_000_000 / (120 * 24) = 20_833 μs
        let interval = Duration::from_micros(20_833);
        let base = Instant::now();
        for i in 0..25u64 {
            t.update(base + interval * i as u32);
        }
        let bpm = t.bpm().expect("expected Some BPM");
        assert!((bpm as i64 - 120).abs() <= 1, "expected ~120 BPM, got {bpm}");
    }

    // T-3: timeout_duration() at 120 BPM ≈ 72.9 ms; None when no interval
    #[test]
    fn timeout_duration_none_with_no_interval() {
        let t = PulseTracker::new();
        assert_eq!(t.timeout_duration(), None);
    }

    #[test]
    fn timeout_duration_at_120_bpm() {
        let mut t = PulseTracker::new();
        let interval = Duration::from_micros(20_833);
        let base = Instant::now();
        t.update(base);
        t.update(base + interval);
        // 3.5 × 20_833 μs = 72_915.5 μs ≈ 72.9 ms
        let td = t.timeout_duration().expect("expected Some timeout");
        let expected = Duration::from_micros(72_915);
        let diff = if td > expected { td - expected } else { expected - td };
        assert!(diff < Duration::from_micros(500), "timeout {td:?} not ~72.9ms");
    }

    // T-5: is_clock_active just after pulse = true; after 4 intervals = false
    #[test]
    fn is_clock_active_true_just_after_pulse() {
        let mut t = PulseTracker::new();
        let interval = Duration::from_millis(20);
        let base = Instant::now();
        for i in 0..25u64 {
            t.update(base + interval * i as u32);
        }
        let last = base + interval * 24;
        assert!(t.is_clock_active(last + Duration::from_millis(1)));
    }

    #[test]
    fn is_clock_active_false_after_four_intervals() {
        let mut t = PulseTracker::new();
        let interval = Duration::from_millis(20);
        let base = Instant::now();
        for i in 0..25u64 {
            t.update(base + interval * i as u32);
        }
        let last = base + interval * 24;
        // timeout = 3.5 × 20ms = 70ms; 4 intervals = 80ms > 70ms
        assert!(!t.is_clock_active(last + Duration::from_millis(80)));
    }

    // T-7: reset() clears all state
    #[test]
    fn reset_clears_all_state() {
        let mut t = PulseTracker::new();
        let base = Instant::now();
        for i in 0..25u64 {
            t.update(base + Duration::from_millis(i * 20));
        }
        t.reset();
        assert_eq!(t.bpm(), None);
        assert_eq!(t.timeout_duration(), None);
        assert!(!t.is_clock_active(Instant::now()));
    }
}
