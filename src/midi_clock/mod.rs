// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

pub mod tracker;

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use crate::loop_engine::LoopEngine;
use tracker::PulseTracker;

#[derive(Debug)]
pub enum ClockMessage {
    Pulse,
    Start,
    Continue,
    Stop,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SyncClockState {
    Waiting,
    Tracking,
    Lost,
}

pub struct MidiClockReceiver {
    state: Arc<Mutex<SyncClockState>>,
    _midi_conn: Option<Box<dyn std::any::Any + Send>>,
}

impl MidiClockReceiver {
    /// Production constructor: opens a real MIDI input port via midir.
    pub fn new(port_name: &str, engine: Arc<LoopEngine>) -> Result<Self, String> {
        use midir::MidiInput;
        let midi_in = MidiInput::new("propeller-sync").map_err(|e| e.to_string())?;
        let ports = midi_in.ports();
        let port = ports.iter()
            .find(|p| midi_in.port_name(p).as_deref() == Ok(port_name))
            .ok_or_else(|| format!("MIDI input port {:?} not found", port_name))?
            .clone();
        let (tx, rx) = mpsc::channel::<ClockMessage>();
        let conn = midi_in.connect(&port, "propeller-sync-input", move |_stamp, msg, _| {
            let m = match msg {
                [0xF8] => Some(ClockMessage::Pulse),
                [0xFA] => Some(ClockMessage::Start),
                [0xFB] => Some(ClockMessage::Continue),
                [0xFC] => Some(ClockMessage::Stop),
                _ => None,
            };
            if let Some(m) = m {
                let _ = tx.send(m);
            }
        }, ()).map_err(|e| e.to_string())?;

        let state = Arc::new(Mutex::new(SyncClockState::Waiting));
        let state_clone = Arc::clone(&state);
        std::thread::spawn(move || {
            run_receiver(rx, engine, state_clone);
        });
        Ok(MidiClockReceiver { state, _midi_conn: Some(Box::new(conn)) })
    }

    /// Test constructor: caller provides the channel receiver directly.
    #[cfg(test)]
    pub fn new_for_test(rx: mpsc::Receiver<ClockMessage>, engine: Arc<LoopEngine>) -> Self {
        let state = Arc::new(Mutex::new(SyncClockState::Waiting));
        let state_clone = Arc::clone(&state);
        std::thread::spawn(move || {
            run_receiver(rx, engine, state_clone);
        });
        MidiClockReceiver { state, _midi_conn: None }
    }

    #[cfg(test)]
    pub fn sync_clock_state(&self) -> SyncClockState {
        self.state.lock().unwrap().clone()
    }

    pub fn state_arc(&self) -> Arc<Mutex<SyncClockState>> {
        Arc::clone(&self.state)
    }
}

fn run_receiver(
    rx: mpsc::Receiver<ClockMessage>,
    engine: Arc<LoopEngine>,
    state: Arc<Mutex<SyncClockState>>,
) {
    let mut pulse_tracker = PulseTracker::new();
    let mut last_bpm: Option<u32> = None;

    loop {
        let timeout = pulse_tracker.timeout_duration()
            .unwrap_or_else(|| Duration::from_secs(1));

        match rx.recv_timeout(timeout) {
            Ok(ClockMessage::Pulse) => {
                let now = Instant::now();
                pulse_tracker.update(now);

                let current = state.lock().unwrap().clone();
                if current == SyncClockState::Lost || current == SyncClockState::Waiting {
                    *state.lock().unwrap() = SyncClockState::Tracking;
                }

                if let Some(bpm) = pulse_tracker.bpm() {
                    if last_bpm != Some(bpm) {
                        last_bpm = Some(bpm);
                        engine.sync_bpm_update(bpm);
                    }
                }
            }
            Ok(ClockMessage::Start) => {
                pulse_tracker.reset();
                last_bpm = None;
                *state.lock().unwrap() = SyncClockState::Tracking;
                engine.sync_start();
            }
            Ok(ClockMessage::Continue) => {
                let now = Instant::now();
                if pulse_tracker.is_clock_active(now) {
                    engine.sync_continue();
                }
            }
            Ok(ClockMessage::Stop) => {
                *state.lock().unwrap() = SyncClockState::Waiting;
                engine.sync_stop();
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let current = state.lock().unwrap().clone();
                if current != SyncClockState::Lost {
                    *state.lock().unwrap() = SyncClockState::Lost;
                    engine.sync_stop();
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Bar, Header, Note, NoteEvent, Project, ProjectStore, TimeSignature, Track};
    use crate::loop_engine::{EngineState, LoopEngine};
    use crate::loop_engine::midi::MockMidiOutput;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;

    fn make_engine_with_project() -> Arc<LoopEngine> {
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        {
            let project = Project {
                header: Header {
                    bpm: 300,
                    time_signature: TimeSignature { numerator: 1, denominator: 4 },
                },
                tracks: vec![Track {
                    name: "t".to_string(),
                    channel: 1,
                    instrument: 0,
                    bars: vec![Bar {
                        notes: vec![Note {
                            event: NoteEvent::Note { pitch: 60, velocity: 80 },
                            duration_ticks: 480,
                        }],
                    }],
                }],
            };
            store.write().unwrap().set_pending(project).unwrap();
            store.write().unwrap().commit_pending();
        }
        Arc::new(LoopEngine::new(store, Box::new(MockMidiOutput::new())))
    }

    fn make_engine_no_project() -> Arc<LoopEngine> {
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        Arc::new(LoopEngine::new(store, Box::new(MockMidiOutput::new())))
    }

    fn wait_for_sync_state(receiver: &MidiClockReceiver, target: SyncClockState, timeout_ms: u64) {
        let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
        while std::time::Instant::now() < deadline {
            if receiver.sync_clock_state() == target {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_for_engine_state(engine: &LoopEngine, target: EngineState, timeout_ms: u64) {
        let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
        while std::time::Instant::now() < deadline {
            if engine.state() == target {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    // T-25: feed 25 Pulse messages → state = Tracking (BPM derived, sync_bpm_update called)
    #[test]
    fn pulse_messages_transition_to_tracking() {
        let engine = make_engine_no_project();
        let (tx, rx) = mpsc::channel::<ClockMessage>();
        let receiver = MidiClockReceiver::new_for_test(rx, Arc::clone(&engine));

        assert_eq!(receiver.sync_clock_state(), SyncClockState::Waiting);

        // Feed 25 pulses at 120 BPM intervals (20_833 μs each)
        for _ in 0..25 {
            tx.send(ClockMessage::Pulse).unwrap();
            std::thread::sleep(Duration::from_micros(20_833));
        }

        wait_for_sync_state(&receiver, SyncClockState::Tracking, 2000);
        assert_eq!(receiver.sync_clock_state(), SyncClockState::Tracking);
    }

    // T-27: receiver processes Start → engine.sync_start() called; state = Tracking
    #[test]
    fn start_message_calls_sync_start_and_sets_tracking() {
        let engine = make_engine_with_project();
        let (tx, rx) = mpsc::channel::<ClockMessage>();
        let receiver = MidiClockReceiver::new_for_test(rx, Arc::clone(&engine));

        tx.send(ClockMessage::Start).unwrap();

        wait_for_sync_state(&receiver, SyncClockState::Tracking, 500);
        assert_eq!(receiver.sync_clock_state(), SyncClockState::Tracking);

        // engine.sync_start() → Running (project present)
        wait_for_engine_state(&engine, EngineState::Running, 500);
        assert_eq!(engine.state(), EngineState::Running);
        engine.sync_stop();
    }

    // T-29: Continue when clock active → sync_continue() called; Continue when clock inactive → ignored
    #[test]
    fn continue_when_clock_active_calls_sync_continue() {
        let engine = make_engine_with_project();
        let (tx, rx) = mpsc::channel::<ClockMessage>();
        let receiver = MidiClockReceiver::new_for_test(rx, Arc::clone(&engine));

        // Prime the pulse tracker with high-frequency pulses so is_clock_active = true
        for _ in 0..3 {
            tx.send(ClockMessage::Pulse).unwrap();
            std::thread::sleep(Duration::from_micros(20_833));
        }
        wait_for_sync_state(&receiver, SyncClockState::Tracking, 500);

        // Now Stop then Continue while clock is still active
        tx.send(ClockMessage::Stop).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        tx.send(ClockMessage::Continue).unwrap();

        wait_for_engine_state(&engine, EngineState::Running, 500);
        assert_eq!(engine.state(), EngineState::Running);
        engine.sync_stop();
    }

    #[test]
    fn continue_when_clock_not_active_is_ignored() {
        let engine = make_engine_with_project();
        let (tx, rx) = mpsc::channel::<ClockMessage>();
        let receiver = MidiClockReceiver::new_for_test(rx, Arc::clone(&engine));

        // No pulses — clock not active; send Continue
        tx.send(ClockMessage::Continue).unwrap();
        std::thread::sleep(Duration::from_millis(50));

        // Engine should stay Stopped (sync_continue not called)
        assert_eq!(engine.state(), EngineState::Stopped);
        drop(receiver);
    }

    // T-31: receiver processes Stop → sync_stop() called; state = Waiting
    #[test]
    fn stop_message_calls_sync_stop_and_sets_waiting() {
        let engine = make_engine_with_project();
        let (tx, rx) = mpsc::channel::<ClockMessage>();
        let receiver = MidiClockReceiver::new_for_test(rx, Arc::clone(&engine));

        // Start first
        tx.send(ClockMessage::Start).unwrap();
        wait_for_engine_state(&engine, EngineState::Running, 500);

        // Then stop
        tx.send(ClockMessage::Stop).unwrap();
        wait_for_engine_state(&engine, EngineState::Stopped, 500);
        wait_for_sync_state(&receiver, SyncClockState::Waiting, 500);

        assert_eq!(engine.state(), EngineState::Stopped);
        assert_eq!(receiver.sync_clock_state(), SyncClockState::Waiting);
    }

    // T-33: timeout → sync_stop() called; state = Lost; subsequent Pulse → state = Tracking, no sync_start
    #[test]
    fn timeout_declares_clock_lost_and_pulse_resumes_tracking_without_start() {
        // Prime with very high-frequency pulses (≈30_000 BPM → 2 ms interval → 7 ms timeout)
        let engine = make_engine_with_project();
        let (tx, rx) = mpsc::channel::<ClockMessage>();
        let receiver = MidiClockReceiver::new_for_test(rx, Arc::clone(&engine));

        // Start so engine is Running
        tx.send(ClockMessage::Start).unwrap();
        wait_for_engine_state(&engine, EngineState::Running, 500);

        // Feed 25 pulses at 2 ms spacing to set timeout_duration to ~7 ms
        for _ in 0..25 {
            tx.send(ClockMessage::Pulse).unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }

        // Now stop sending pulses — clock loss should trigger after ~7 ms
        // Wait 50 ms to be sure
        std::thread::sleep(Duration::from_millis(50));

        wait_for_sync_state(&receiver, SyncClockState::Lost, 500);
        assert_eq!(receiver.sync_clock_state(), SyncClockState::Lost);
        // Engine should be Stopped (sync_stop was called)
        wait_for_engine_state(&engine, EngineState::Stopped, 500);
        assert_eq!(engine.state(), EngineState::Stopped);

        // Now send pulses again — state should go to Tracking but engine stays Stopped
        for _ in 0..5 {
            tx.send(ClockMessage::Pulse).unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        wait_for_sync_state(&receiver, SyncClockState::Tracking, 500);
        assert_eq!(receiver.sync_clock_state(), SyncClockState::Tracking);
        // Engine must NOT have restarted (no sync_start called)
        assert_eq!(engine.state(), EngineState::Stopped, "engine must not restart after clock recovery without Start/Continue");
    }

    // T-35: after clock loss + resume, engine restarts only on new Start or Continue (AC-11)
    #[test]
    fn clock_resume_then_start_restarts_engine() {
        let engine = make_engine_with_project();
        let (tx, rx) = mpsc::channel::<ClockMessage>();
        let receiver = MidiClockReceiver::new_for_test(rx, Arc::clone(&engine));

        // Start engine
        tx.send(ClockMessage::Start).unwrap();
        wait_for_engine_state(&engine, EngineState::Running, 500);

        // Prime fast pulses, then let clock go lost
        for _ in 0..25 {
            tx.send(ClockMessage::Pulse).unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        std::thread::sleep(Duration::from_millis(50));
        wait_for_sync_state(&receiver, SyncClockState::Lost, 500);

        // Resume pulses (tracking restored, engine still stopped)
        for _ in 0..5 {
            tx.send(ClockMessage::Pulse).unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        wait_for_sync_state(&receiver, SyncClockState::Tracking, 500);
        assert_eq!(engine.state(), EngineState::Stopped);

        // Now send Start — engine should restart
        tx.send(ClockMessage::Start).unwrap();
        wait_for_engine_state(&engine, EngineState::Running, 500);
        assert_eq!(engine.state(), EngineState::Running);
        engine.sync_stop();
    }
}
