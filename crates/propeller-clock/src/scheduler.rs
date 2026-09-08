// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use std::time::{Duration, Instant};

// Standard MIDI clock resolution: 24 pulses per quarter note.
const PULSES_PER_QUARTER: f64 = 24.0;

pub struct ClockScheduler {
    bpm: f64,
    micros_per_pulse: f64,
}

impl ClockScheduler {
    pub fn new(bpm: f64) -> ClockScheduler {
        ClockScheduler {
            bpm,
            micros_per_pulse: 60_000_000.0 / (bpm * PULSES_PER_QUARTER),
        }
    }

    pub fn update_bpm(&mut self, bpm: f64) {
        self.bpm = bpm;
        self.micros_per_pulse = 60_000_000.0 / (bpm * PULSES_PER_QUARTER);
    }

    pub fn micros_per_pulse(&self) -> u64 {
        self.micros_per_pulse as u64
    }

    pub fn deadline_for_pulse(&self, anchor: Instant, pulse: u64) -> Instant {
        anchor + Duration::from_secs_f64(pulse as f64 * self.micros_per_pulse / 1_000_000.0)
    }

    pub fn sleep_until(&self, deadline: Instant) {
        let now = Instant::now();
        if deadline <= now {
            return;
        }
        let remaining = deadline - now;
        if remaining > Duration::from_micros(500) {
            std::thread::sleep(remaining - Duration::from_micros(500));
        }
        while Instant::now() < deadline {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn micros_per_pulse_at_120_bpm() {
        // 60_000_000 / (120 * 24) = 20_833.33...
        let s = ClockScheduler::new(120.0);
        assert_eq!(s.micros_per_pulse(), 20_833);
    }

    #[test]
    fn deadline_for_pulse_zero_is_anchor() {
        let s = ClockScheduler::new(120.0);
        let start = Instant::now();
        assert_eq!(s.deadline_for_pulse(start, 0), start);
    }

    #[test]
    fn deadline_for_pulse_24_is_one_quarter_note() {
        // At 120 BPM, one quarter note = 500ms = 24 pulses.
        let s = ClockScheduler::new(120.0);
        let start = Instant::now();
        let expected = start + Duration::from_millis(500);
        assert_eq!(s.deadline_for_pulse(start, 24), expected);
    }

    #[test]
    fn update_bpm_changes_rate() {
        let mut s = ClockScheduler::new(120.0);
        s.update_bpm(60.0);
        let start = Instant::now();
        // At 60 BPM, one quarter note = 1s = 24 pulses.
        let expected = start + Duration::from_secs(1);
        assert_eq!(s.deadline_for_pulse(start, 24), expected);
    }

    #[test]
    fn sleep_until_within_5ms() {
        let s = ClockScheduler::new(120.0);
        let deadline = Instant::now() + Duration::from_millis(10);
        s.sleep_until(deadline);
        let overshoot = Instant::now().saturating_duration_since(deadline);
        assert!(
            overshoot < Duration::from_millis(5),
            "overshot deadline by {:?}",
            overshoot
        );
    }
}
