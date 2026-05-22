use std::time::{Duration, Instant};

pub struct Scheduler {
    bpm: u32,
    micros_per_tick: u64,
}

impl Scheduler {
    pub fn new(bpm: u32) -> Scheduler {
        let micros_per_tick = 60_000_000u64 / (bpm as u64 * 480);
        Scheduler { bpm, micros_per_tick }
    }

    pub fn micros_per_tick(&self) -> u64 {
        self.micros_per_tick
    }

    pub fn deadline_for_tick(&self, anchor: Instant, tick: u64) -> Instant {
        anchor + Duration::from_micros(tick * self.micros_per_tick)
    }

    pub fn bpm(&self) -> u32 {
        self.bpm
    }

    pub fn update_bpm(&mut self, bpm: u32) {
        self.bpm = bpm;
        self.micros_per_tick = 60_000_000u64 / (bpm as u64 * 480);
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

    // T-1: micros_per_tick returns correct values
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

    // T-3: deadline_for_tick returns correct Instants
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

    // T-5: update_bpm changes micros_per_tick and deadline_for_tick uses new rate
    #[test]
    fn update_bpm_changes_rate() {
        let mut s = Scheduler::new(125);
        s.update_bpm(60);
        // 60_000_000 / (60 * 480) = 60_000_000 / 28_800 = 2083
        assert_eq!(s.micros_per_tick(), 2083);
        let start = Instant::now();
        let expected = start + Duration::from_micros(480 * 2083);
        assert_eq!(s.deadline_for_tick(start, 480), expected);
    }

    // T-7: sleep_until wakes no more than 5 ms after the deadline
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
