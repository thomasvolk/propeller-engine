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
        let n = self.history.len();
        let total_micros: u128 = self.history.iter()
            .zip(self.history.iter().skip(1))
            .map(|(a, b)| b.duration_since(*a).as_micros())
            .sum();
        let count = (n - 1) as u128;
        let avg_micros = total_micros / count;
        if avg_micros == 0 {
            return None;
        }
        // MIDI clock: 24 pulses per quarter note
        // BPM = 60_000_000 / (avg_interval_micros * 24)
        let bpm = 60_000_000_u128 / (avg_micros * 24);
        Some(bpm as u32)
    }

    pub fn timeout_duration(&self) -> Option<Duration> {
        if self.history.len() < 2 {
            return None;
        }
        let last = self.history.back().unwrap();
        let prev = self.history.iter().rev().nth(1).unwrap();
        let last_interval = last.duration_since(*prev);
        Some(last_interval.mul_f64(3.5))
    }

    pub fn is_clock_active(&self, now: Instant) -> bool {
        match (self.timeout_duration(), self.history.back()) {
            (Some(timeout), Some(last)) => now.duration_since(*last) <= timeout,
            _ => false,
        }
    }

    pub fn reset(&mut self) {
        self.history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // T-1: bpm() returns None with fewer than 2 pulses; returns ~120 after 25 evenly-spaced pulses
    #[test]
    fn bpm_none_with_no_pulses() {
        let tracker = PulseTracker::new();
        assert_eq!(tracker.bpm(), None);
    }

    #[test]
    fn bpm_none_with_one_pulse() {
        let mut tracker = PulseTracker::new();
        tracker.update(Instant::now());
        assert_eq!(tracker.bpm(), None);
    }

    #[test]
    fn bpm_near_120_after_25_pulses() {
        let mut tracker = PulseTracker::new();
        let base = Instant::now();
        // 120 BPM, 24 PPQN → interval = 60_000_000 / (120 * 24) = 20_833 μs
        let interval = Duration::from_micros(20_833);
        for i in 0..25u32 {
            tracker.update(base + interval * i);
        }
        let bpm = tracker.bpm().expect("should have BPM after 25 pulses");
        assert!((bpm as i32 - 120).abs() <= 1, "expected ~120 BPM, got {bpm}");
    }

    // T-3: timeout_duration() returns None before 2 pulses; ~72.9 ms at 120 BPM
    #[test]
    fn timeout_duration_none_with_no_pulses() {
        assert_eq!(PulseTracker::new().timeout_duration(), None);
    }

    #[test]
    fn timeout_duration_none_with_one_pulse() {
        let mut tracker = PulseTracker::new();
        tracker.update(Instant::now());
        assert_eq!(tracker.timeout_duration(), None);
    }

    #[test]
    fn timeout_duration_at_120_bpm() {
        let mut tracker = PulseTracker::new();
        let base = Instant::now();
        let interval = Duration::from_micros(20_833);
        tracker.update(base);
        tracker.update(base + interval);
        let timeout = tracker.timeout_duration().expect("should have timeout after 2 pulses");
        // 3.5 × 20_833 μs = 72_916 μs ≈ 72.9 ms
        let expected = Duration::from_micros(72_916);
        let diff = if timeout > expected { timeout - expected } else { expected - timeout };
        assert!(diff < Duration::from_millis(2), "expected ~72.9 ms, got {timeout:?}");
    }

    // T-5: is_clock_active() true just after a pulse; false after 4 intervals of silence
    #[test]
    fn is_clock_active_true_immediately_after_pulse() {
        let mut tracker = PulseTracker::new();
        let base = Instant::now();
        let interval = Duration::from_micros(20_833);
        tracker.update(base);
        tracker.update(base + interval);
        // Right at the last pulse timestamp — well within timeout
        assert!(tracker.is_clock_active(base + interval));
        // 1 ms after — still well within the ~72.9 ms timeout
        assert!(tracker.is_clock_active(base + interval + Duration::from_millis(1)));
    }

    #[test]
    fn is_clock_active_false_after_4_intervals_silence() {
        let mut tracker = PulseTracker::new();
        let base = Instant::now();
        let interval = Duration::from_micros(20_833);
        tracker.update(base);
        tracker.update(base + interval);
        // 4 intervals of silence: 4 × 20_833 μs = 83_332 μs > timeout (~72_916 μs)
        let after_silence = base + interval + interval * 4;
        assert!(!tracker.is_clock_active(after_silence));
    }

    // T-7: reset() clears all state — bpm(), timeout_duration(), is_clock_active() all reset
    #[test]
    fn reset_clears_all_state() {
        let mut tracker = PulseTracker::new();
        let base = Instant::now();
        let interval = Duration::from_micros(20_833);
        for i in 0..5u32 {
            tracker.update(base + interval * i);
        }
        assert!(tracker.bpm().is_some());
        assert!(tracker.timeout_duration().is_some());
        assert!(tracker.is_clock_active(base + interval * 4 + Duration::from_millis(1)));

        tracker.reset();

        assert_eq!(tracker.bpm(), None);
        assert_eq!(tracker.timeout_duration(), None);
        assert!(!tracker.is_clock_active(base + interval * 5));
    }
}
