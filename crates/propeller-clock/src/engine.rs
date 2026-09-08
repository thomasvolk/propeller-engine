// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use crate::midi::ClockOutput;
use crate::scheduler::ClockScheduler;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClockState {
    Stopped,
    Running,
    Paused,
}

pub enum ClockCommand {
    Start,
    Pause,
    Resume,
    Stop,
    SetBpm(f64),
    Seek(u16),
}

// Microseconds to wait before the first pulse on start/restart so that a MIDI Start
// byte reaches the device before the first Timing Clock pulse.
const START_LATENCY_MICROS: u64 = 20_000;

pub struct ClockEngine {
    sender: mpsc::Sender<ClockCommand>,
    state: Arc<Mutex<ClockState>>,
    pulse_count: Arc<AtomicU64>,
    bpm: Arc<Mutex<f64>>,
    position: Arc<AtomicU16>,
}

impl ClockEngine {
    pub fn new(output: Box<dyn ClockOutput>, initial_bpm: f64, spp_enabled: bool) -> ClockEngine {
        let (sender, receiver) = mpsc::channel();
        let state = Arc::new(Mutex::new(ClockState::Stopped));
        let pulse_count = Arc::new(AtomicU64::new(0));
        let bpm = Arc::new(Mutex::new(initial_bpm));
        let position = Arc::new(AtomicU16::new(0));

        let state_clone = Arc::clone(&state);
        let pulse_count_clone = Arc::clone(&pulse_count);
        let position_clone = Arc::clone(&position);

        std::thread::spawn(move || {
            run_clock_loop(
                receiver,
                output,
                state_clone,
                pulse_count_clone,
                position_clone,
                initial_bpm,
                spp_enabled,
            );
        });

        ClockEngine {
            sender,
            state,
            pulse_count,
            bpm,
            position,
        }
    }

    pub fn start(&self) {
        let _ = self.sender.send(ClockCommand::Start);
    }

    pub fn pause(&self) {
        let _ = self.sender.send(ClockCommand::Pause);
    }

    pub fn resume(&self) {
        let _ = self.sender.send(ClockCommand::Resume);
    }

    pub fn stop(&self) {
        let _ = self.sender.send(ClockCommand::Stop);
    }

    pub fn set_bpm(&self, bpm: f64) {
        *self.bpm.lock().unwrap() = bpm;
        let _ = self.sender.send(ClockCommand::SetBpm(bpm));
    }

    // Caller (ipc dispatch) is responsible for only calling this while stopped.
    pub fn seek(&self, position: u16) {
        self.position.store(position, Ordering::Relaxed);
        let _ = self.sender.send(ClockCommand::Seek(position));
    }

    pub fn position(&self) -> u16 {
        self.position.load(Ordering::Relaxed)
    }

    pub fn state(&self) -> ClockState {
        *self.state.lock().unwrap()
    }

    pub fn bpm(&self) -> f64 {
        *self.bpm.lock().unwrap()
    }

    pub fn pulse_count(&self) -> u64 {
        self.pulse_count.load(Ordering::Relaxed)
    }

    // Ensures a trailing MIDI Stop reaches the port before the daemon process exits.
    pub fn stop_and_wait(&self) {
        let s = self.state();
        if s == ClockState::Running || s == ClockState::Paused {
            self.stop();
            let deadline = Instant::now() + Duration::from_millis(100);
            while Instant::now() < deadline {
                if self.state() == ClockState::Stopped {
                    return;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
}

enum PollResult {
    Elapsed,
    Command(ClockCommand),
    Disconnected,
}

// Sleep until deadline, checking for commands every ~1ms.
fn sleep_until_with_poll(
    deadline: Instant,
    receiver: &mpsc::Receiver<ClockCommand>,
    scheduler: &ClockScheduler,
) -> PollResult {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return PollResult::Elapsed;
        }
        let remaining = deadline - now;
        if remaining > Duration::from_millis(2) {
            std::thread::sleep(Duration::from_millis(1));
            match receiver.try_recv() {
                Ok(cmd) => return PollResult::Command(cmd),
                Err(mpsc::TryRecvError::Disconnected) => return PollResult::Disconnected,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        } else {
            scheduler.sleep_until(deadline);
            return PollResult::Elapsed;
        }
    }
}

fn run_clock_loop(
    receiver: mpsc::Receiver<ClockCommand>,
    mut output: Box<dyn ClockOutput>,
    state: Arc<Mutex<ClockState>>,
    pulse_count: Arc<AtomicU64>,
    position: Arc<AtomicU16>,
    initial_bpm: f64,
    spp_enabled: bool,
) {
    let mut scheduler = ClockScheduler::new(initial_bpm);
    let mut anchor = Instant::now();
    // Pulses emitted since the anchor was last rebased (a bpm change rebases it);
    // used only for scheduling deadlines, never reported externally.
    let mut phase_pulse: u64 = 0;
    let mut current_state = ClockState::Stopped;

    fn do_start(
        output: &mut dyn ClockOutput,
        anchor: &mut Instant,
        phase_pulse: &mut u64,
        pulse_count: &AtomicU64,
        position: &AtomicU16,
    ) {
        *phase_pulse = 0;
        pulse_count.store(0, Ordering::Relaxed);
        position.store(0, Ordering::Relaxed);
        *anchor = Instant::now() + Duration::from_micros(START_LATENCY_MICROS);
        if let Err(e) = output.clock_start() {
            eprintln!("propeller-clock: MIDI clock_start failed: {e}");
        }
    }

    fn do_stop(
        output: &mut dyn ClockOutput,
        phase_pulse: &mut u64,
        pulse_count: &AtomicU64,
        position: &AtomicU16,
    ) {
        *phase_pulse = 0;
        pulse_count.store(0, Ordering::Relaxed);
        position.store(0, Ordering::Relaxed);
        if let Err(e) = output.clock_stop() {
            eprintln!("propeller-clock: MIDI clock_stop failed: {e}");
        }
    }

    // Only meaningful while stopped (see ClockEngine::seek); sends Song Position Pointer
    // (0xF2) so downstream devices learn where a subsequent Start/Continue should resume.
    fn do_seek(output: &mut dyn ClockOutput, spp_enabled: bool, position: u16) {
        if !spp_enabled {
            return;
        }
        if let Err(e) = output.song_position(position) {
            eprintln!("propeller-clock: MIDI song_position failed: {e}");
        }
    }

    fn set_state(state: &Arc<Mutex<ClockState>>, current: &mut ClockState, s: ClockState) {
        *current = s;
        *state.lock().unwrap() = s;
    }

    loop {
        match current_state {
            ClockState::Stopped => match receiver.recv() {
                Ok(ClockCommand::Start) => {
                    do_start(
                        output.as_mut(),
                        &mut anchor,
                        &mut phase_pulse,
                        &pulse_count,
                        &position,
                    );
                    set_state(&state, &mut current_state, ClockState::Running);
                }
                Ok(ClockCommand::SetBpm(bpm)) => scheduler.update_bpm(bpm),
                Ok(ClockCommand::Seek(pos)) => do_seek(output.as_mut(), spp_enabled, pos),
                Ok(ClockCommand::Pause | ClockCommand::Resume | ClockCommand::Stop) => {}
                Err(_) => return,
            },
            ClockState::Running => {
                let deadline = scheduler.deadline_for_pulse(anchor, phase_pulse);
                let poll_result = match sleep_until_with_poll(deadline, &receiver, &scheduler) {
                    PollResult::Elapsed => {
                        if let Err(e) = output.clock_tick() {
                            eprintln!("propeller-clock: MIDI clock_tick failed: {e}");
                        }
                        phase_pulse += 1;
                        pulse_count.fetch_add(1, Ordering::Relaxed);
                        // The sleep above only polls every ~1ms, which can be longer than a
                        // whole pulse interval at fast tempos, so a command queued during
                        // that interval would otherwise sit unnoticed for many pulses. Poll
                        // once more right after each tick to keep response time bounded by
                        // one pulse interval regardless of tempo.
                        match receiver.try_recv() {
                            Ok(cmd) => Some(cmd),
                            Err(mpsc::TryRecvError::Disconnected) => return,
                            Err(mpsc::TryRecvError::Empty) => None,
                        }
                    }
                    PollResult::Command(cmd) => Some(cmd),
                    PollResult::Disconnected => return,
                };

                match poll_result {
                    Some(ClockCommand::Pause) => {
                        if let Err(e) = output.clock_stop() {
                            eprintln!("propeller-clock: MIDI clock_stop failed: {e}");
                        }
                        set_state(&state, &mut current_state, ClockState::Paused);
                    }
                    Some(ClockCommand::Stop) => {
                        do_stop(output.as_mut(), &mut phase_pulse, &pulse_count, &position);
                        set_state(&state, &mut current_state, ClockState::Stopped);
                    }
                    Some(ClockCommand::Start) => {
                        do_start(
                            output.as_mut(),
                            &mut anchor,
                            &mut phase_pulse,
                            &pulse_count,
                            &position,
                        );
                    }
                    Some(ClockCommand::SetBpm(bpm)) => {
                        // Rebase anchor to the current phase so only the rate changes,
                        // not the phase already elapsed (no timing jump).
                        anchor = scheduler.deadline_for_pulse(anchor, phase_pulse);
                        phase_pulse = 0;
                        scheduler.update_bpm(bpm);
                    }
                    // Seek is only meaningful while stopped; ignore if it arrives here.
                    Some(ClockCommand::Seek(_)) | Some(ClockCommand::Resume) | None => {}
                }
            }
            ClockState::Paused => match receiver.recv() {
                Ok(ClockCommand::Resume) => {
                    anchor = Instant::now()
                        - Duration::from_micros(phase_pulse * scheduler.micros_per_pulse());
                    if let Err(e) = output.clock_continue() {
                        eprintln!("propeller-clock: MIDI clock_continue failed: {e}");
                    }
                    set_state(&state, &mut current_state, ClockState::Running);
                }
                Ok(ClockCommand::Start) => {
                    do_start(
                        output.as_mut(),
                        &mut anchor,
                        &mut phase_pulse,
                        &pulse_count,
                        &position,
                    );
                    set_state(&state, &mut current_state, ClockState::Running);
                }
                Ok(ClockCommand::Stop) => {
                    do_stop(output.as_mut(), &mut phase_pulse, &pulse_count, &position);
                    set_state(&state, &mut current_state, ClockState::Stopped);
                }
                Ok(ClockCommand::SetBpm(bpm)) => scheduler.update_bpm(bpm),
                // Seek is only meaningful while stopped; ignore if it arrives here.
                Ok(ClockCommand::Seek(_)) | Ok(ClockCommand::Pause) => {}
                Err(_) => return,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi::{CapturingClockOutput, ClockEvent};
    use std::time::Duration;

    fn wait_for_state(engine: &ClockEngine, target: ClockState, timeout_ms: u64) {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        while Instant::now() < deadline {
            if engine.state() == target {
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn wait_for_event(
        events: &std::sync::Arc<std::sync::Mutex<Vec<ClockEvent>>>,
        target: &ClockEvent,
        timeout_ms: u64,
    ) -> bool {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        while Instant::now() < deadline {
            if events.lock().unwrap().contains(target) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        false
    }

    fn wait_for_pulse_count_at_least(engine: &ClockEngine, min: u64, timeout_ms: u64) -> bool {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        while Instant::now() < deadline {
            if engine.pulse_count() >= min {
                return true;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        false
    }

    #[test]
    fn new_engine_is_stopped() {
        let (output, _) = CapturingClockOutput::new();
        let engine = ClockEngine::new(Box::new(output), 120.0, true);
        assert_eq!(engine.state(), ClockState::Stopped);
        assert_eq!(engine.pulse_count(), 0);
    }

    #[test]
    fn start_transitions_to_running_and_sends_midi_start() {
        let (output, events) = CapturingClockOutput::new();
        let engine = ClockEngine::new(Box::new(output), 6000.0, true);
        engine.start();
        wait_for_state(&engine, ClockState::Running, 500);
        assert_eq!(engine.state(), ClockState::Running);
        assert!(events.lock().unwrap().contains(&ClockEvent::Start));
        engine.stop();
    }

    #[test]
    fn running_engine_emits_pulses() {
        // 6000 BPM -> ~416us per pulse, fast enough to observe several pulses quickly.
        let (output, events) = CapturingClockOutput::new();
        let engine = ClockEngine::new(Box::new(output), 6000.0, true);
        engine.start();
        assert!(
            wait_for_pulse_count_at_least(&engine, 5, 500),
            "expected at least 5 pulses"
        );
        let tick_count = events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| **e == ClockEvent::Tick)
            .count();
        assert!(tick_count >= 5);
        engine.stop();
    }

    #[test]
    fn pause_sends_stop_and_freezes_pulse_count() {
        let (output, events) = CapturingClockOutput::new();
        let engine = ClockEngine::new(Box::new(output), 6000.0, true);
        engine.start();
        assert!(wait_for_pulse_count_at_least(&engine, 3, 500));
        engine.pause();
        wait_for_state(&engine, ClockState::Paused, 500);

        let frozen = engine.pulse_count();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            engine.pulse_count(),
            frozen,
            "pulse count must stay frozen while paused"
        );
        assert!(events.lock().unwrap().contains(&ClockEvent::Stop));
        engine.stop();
    }

    #[test]
    fn resume_sends_continue_and_keeps_emitting() {
        let (output, events) = CapturingClockOutput::new();
        let engine = ClockEngine::new(Box::new(output), 6000.0, true);
        engine.start();
        assert!(wait_for_pulse_count_at_least(&engine, 3, 500));
        engine.pause();
        wait_for_state(&engine, ClockState::Paused, 500);
        let frozen = engine.pulse_count();

        engine.resume();
        wait_for_state(&engine, ClockState::Running, 500);
        assert!(events.lock().unwrap().contains(&ClockEvent::Continue));

        assert!(
            wait_for_pulse_count_at_least(&engine, frozen + 3, 500),
            "expected pulse count to keep advancing past the frozen value after resume"
        );
        engine.stop();
    }

    #[test]
    fn stop_resets_pulse_count_and_sends_stop() {
        let (output, events) = CapturingClockOutput::new();
        let engine = ClockEngine::new(Box::new(output), 6000.0, true);
        engine.start();
        assert!(wait_for_pulse_count_at_least(&engine, 3, 500));

        engine.stop();
        wait_for_state(&engine, ClockState::Stopped, 500);
        assert_eq!(engine.pulse_count(), 0);
        assert!(events.lock().unwrap().contains(&ClockEvent::Stop));
    }

    #[test]
    fn stop_while_paused_resets_and_stops() {
        let (output, _) = CapturingClockOutput::new();
        let engine = ClockEngine::new(Box::new(output), 6000.0, true);
        engine.start();
        assert!(wait_for_pulse_count_at_least(&engine, 3, 500));
        engine.pause();
        wait_for_state(&engine, ClockState::Paused, 500);

        engine.stop();
        wait_for_state(&engine, ClockState::Stopped, 500);
        assert_eq!(engine.pulse_count(), 0);
    }

    #[test]
    fn start_while_paused_restarts_from_zero() {
        let (output, events) = CapturingClockOutput::new();
        let engine = ClockEngine::new(Box::new(output), 6000.0, true);
        engine.start();
        assert!(wait_for_pulse_count_at_least(&engine, 3, 500));
        engine.pause();
        wait_for_state(&engine, ClockState::Paused, 500);

        engine.start();
        wait_for_state(&engine, ClockState::Running, 500);
        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| **e == ClockEvent::Start)
                .count(),
            2,
            "expected a second MIDI Start after restarting from paused"
        );
        engine.stop();
    }

    #[test]
    fn bpm_change_does_not_stop_engine() {
        let (output, _) = CapturingClockOutput::new();
        let engine = ClockEngine::new(Box::new(output), 120.0, true);
        engine.start();
        wait_for_state(&engine, ClockState::Running, 500);

        engine.set_bpm(200.0);
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(engine.state(), ClockState::Running);
        assert_eq!(engine.bpm(), 200.0);
        engine.stop();
    }

    #[test]
    fn set_bpm_while_stopped_is_recorded() {
        let (output, _) = CapturingClockOutput::new();
        let engine = ClockEngine::new(Box::new(output), 120.0, true);
        engine.set_bpm(140.0);
        assert_eq!(engine.bpm(), 140.0);
        assert_eq!(engine.state(), ClockState::Stopped);
    }

    #[test]
    fn seek_while_stopped_sends_song_position() {
        let (output, events) = CapturingClockOutput::new();
        let engine = ClockEngine::new(Box::new(output), 120.0, true);
        engine.seek(240);
        assert!(wait_for_event(&events, &ClockEvent::SongPosition(240), 500));
        assert_eq!(engine.position(), 240);
    }

    #[test]
    fn seek_disabled_via_flag_does_not_send_song_position() {
        let (output, events) = CapturingClockOutput::new();
        let engine = ClockEngine::new(Box::new(output), 120.0, false);
        engine.seek(240);
        std::thread::sleep(Duration::from_millis(50));
        assert!(events.lock().unwrap().is_empty());
        assert_eq!(
            engine.position(),
            240,
            "position is still tracked even when SPP output is disabled"
        );
    }

    #[test]
    fn start_resets_tracked_position_to_zero() {
        let (output, _) = CapturingClockOutput::new();
        let engine = ClockEngine::new(Box::new(output), 6000.0, true);
        engine.seek(500);
        assert_eq!(engine.position(), 500);

        engine.start();
        wait_for_state(&engine, ClockState::Running, 500);
        assert_eq!(engine.position(), 0);
        engine.stop();
    }
}
