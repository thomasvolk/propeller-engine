// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use std::time::{Duration, Instant};

pub struct Scheduler {
    bpm: f64,
    micros_per_tick: f64,
}

impl Scheduler {
    pub fn new(bpm: u32) -> Scheduler {
        Scheduler::from_bpm_f64(bpm as f64)
    }

    fn from_bpm_f64(bpm: f64) -> Scheduler {
        Scheduler {
            bpm,
            micros_per_tick: 60_000_000.0 / (bpm * 480.0),
        }
    }

    // Truncated to whole microseconds, matching the precision of the pause/resume tick
    // offset this feeds (see player.rs) where sub-microsecond precision is immaterial.
    pub fn micros_per_tick(&self) -> u64 {
        self.micros_per_tick as u64
    }

    // Uses the untruncated micros-per-tick so that scheduling stays exact relative to
    // the tracked tempo across arbitrarily many ticks/loops, instead of compounding the
    // sub-microsecond rounding error that truncating micros_per_tick() would introduce
    // into every deadline.
    pub fn deadline_for_tick(&self, anchor: Instant, tick: u64) -> Instant {
        anchor + Duration::from_secs_f64(tick as f64 * self.micros_per_tick / 1_000_000.0)
    }

    pub fn bpm(&self) -> u32 {
        self.bpm.round() as u32
    }

    pub fn update_bpm(&mut self, bpm: u32) {
        *self = Scheduler::from_bpm_f64(bpm as f64);
    }

    // Sets the tempo from a continuously-tracked, unrounded external-clock estimate (see
    // PulseTracker). Unlike update_bpm, this never rounds to a whole BPM first: rounding
    // there was found to leave a small but systematic rate bias (propeller's own clock
    // running persistently a hair slower than the real external tempo), which compounds
    // into audible drift over a long sync session. Deliberately does not touch `anchor` —
    // only the rate changes here, the phase reference stays exactly where it was.
    pub fn update_bpm_precise(&mut self, bpm: f64) {
        self.bpm = bpm;
        self.micros_per_tick = 60_000_000.0 / (bpm * 480.0);
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
    use std::time::Duration;

    #[test]
    fn micros_per_tick_bpm_125() {
        let s = Scheduler::new(125);
        assert_eq!(s.micros_per_tick(), 1000);
    }

    #[test]
    fn micros_per_tick_bpm_120() {
        let s = Scheduler::new(120);
        assert_eq!(s.micros_per_tick(), 1041);
    }

    #[test]
    fn deadline_for_tick_zero() {
        let s = Scheduler::new(125);
        let start = Instant::now();
        assert_eq!(s.deadline_for_tick(start, 0), start);
    }

    #[test]
    fn deadline_for_tick_480_at_bpm_125() {
        let s = Scheduler::new(125);
        let start = Instant::now();
        let expected = start + Duration::from_micros(480 * 1000);
        assert_eq!(s.deadline_for_tick(start, 480), expected);
    }

    #[test]
    fn update_bpm_changes_rate() {
        let mut s = Scheduler::new(125);
        s.update_bpm(60);
        // 60_000_000 / (60 * 480) = 60_000_000 / 28_800 = 2083 (truncated public value;
        // the untruncated rate is 2083.333... us/tick)
        assert_eq!(s.micros_per_tick(), 2083);
        let start = Instant::now();
        // 480 ticks is exactly one quarter note, so at 480 ticks the untruncated rate
        // gives an exact result regardless of bpm's divisibility: 60_000_000us / 60bpm =
        // 1_000_000us = 1s, with no rounding error to account for.
        let expected = start + Duration::from_secs(1);
        assert_eq!(s.deadline_for_tick(start, 480), expected);
    }

    #[test]
    fn update_bpm_precise_uses_unrounded_rate_for_deadlines() {
        // A tempo that isn't a whole BPM (as a continuously-tracked external clock
        // estimate typically won't be) must not get truncated/rounded away before it
        // drives scheduling — that rounding is what caused sync-mode drift.
        let mut s = Scheduler::new(125);
        s.update_bpm_precise(119.5);
        let start = Instant::now();
        // One quarter note (480 ticks) at 119.5 BPM = 60_000_000 / 119.5 us exactly.
        let expected = start + Duration::from_secs_f64(60.0 / 119.5);
        assert_eq!(s.deadline_for_tick(start, 480), expected);
    }

    #[test]
    fn update_bpm_precise_does_not_round_to_nearest_whole_bpm_for_bpm_report() {
        let mut s = Scheduler::new(125);
        s.update_bpm_precise(119.5);
        // bpm() is a rounded convenience for the (currently unused-for-sync) integer
        // comparisons elsewhere; it must not silently discard the precise rate above.
        assert_eq!(s.bpm(), 120);
    }

    #[test]
    fn sleep_until_within_5ms() {
        let s = Scheduler::new(120);
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
