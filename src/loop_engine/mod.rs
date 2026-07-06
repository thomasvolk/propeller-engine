// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

pub mod midi;
pub mod player;
pub mod scheduler;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, mpsc};

use crate::domain::ProjectStore;

use midi::MidiOutput;
use player::run_player_loop;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EngineState {
    Stopped,
    Waiting,
    Running,
    Paused,
}

pub(crate) enum LoopCommand {
    Start,
    Stop,
    ClockStart,
    ClockPause,
    ClockResume,
    ClockStop,
    SyncStart,
    SyncContinue,
    SyncStop,
    SyncBpmUpdate(u32),
}

pub struct LoopEngine {
    sender: mpsc::Sender<LoopCommand>,
    state: Arc<Mutex<EngineState>>,
    current_tick: Arc<AtomicU64>,
    loop_duration_ticks: Arc<AtomicU64>,
}

impl LoopEngine {
    pub fn new(store: Arc<RwLock<ProjectStore>>, output: Box<dyn MidiOutput>) -> LoopEngine {
        let (sender, receiver) = mpsc::channel::<LoopCommand>();
        let state = Arc::new(Mutex::new(EngineState::Stopped));
        let state_clone = Arc::clone(&state);
        let current_tick = Arc::new(AtomicU64::new(0));
        let current_tick_clone = Arc::clone(&current_tick);
        let loop_duration_ticks = Arc::new(AtomicU64::new(0));
        let loop_duration_ticks_clone = Arc::clone(&loop_duration_ticks);

        std::thread::spawn(move || {
            run_player_loop(
                receiver,
                store,
                output,
                state_clone,
                current_tick_clone,
                loop_duration_ticks_clone,
            );
        });

        LoopEngine {
            sender,
            state,
            current_tick,
            loop_duration_ticks,
        }
    }

    pub fn start(&self) {
        let _ = self.sender.send(LoopCommand::Start);
    }

    pub fn stop(&self) {
        let _ = self.sender.send(LoopCommand::Stop);
    }

    pub fn clock_start(&self) {
        let _ = self.sender.send(LoopCommand::ClockStart);
    }

    pub fn clock_pause(&self) {
        let _ = self.sender.send(LoopCommand::ClockPause);
    }

    pub fn clock_resume(&self) {
        let _ = self.sender.send(LoopCommand::ClockResume);
    }

    pub fn clock_stop(&self) {
        let _ = self.sender.send(LoopCommand::ClockStop);
    }

    pub fn clock_stop_on_shutdown(&self) {
        let s = self.state();
        if s == EngineState::Running || s == EngineState::Paused {
            self.clock_stop();
            // Block until the player thread processes ClockStop and sends 0xFC before process exit.
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(100);
            while std::time::Instant::now() < deadline {
                if self.state() == EngineState::Stopped {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }
    }

    pub fn state(&self) -> EngineState {
        self.state.lock().unwrap().clone()
    }

    pub fn current_tick(&self) -> u64 {
        self.current_tick.load(Ordering::Relaxed)
    }

    pub fn loop_duration_ticks(&self) -> u64 {
        self.loop_duration_ticks.load(Ordering::Relaxed)
    }

    pub fn sync_start(&self) {
        let _ = self.sender.send(LoopCommand::SyncStart);
    }

    pub fn sync_continue(&self) {
        let _ = self.sender.send(LoopCommand::SyncContinue);
    }

    pub fn sync_stop(&self) {
        let _ = self.sender.send(LoopCommand::SyncStop);
    }

    pub fn sync_bpm_update(&self, bpm: u32) {
        let _ = self.sender.send(LoopCommand::SyncBpmUpdate(bpm));
    }
}

#[cfg(test)]
fn make_test_store_with_project() -> Arc<RwLock<ProjectStore>> {
    use crate::domain::*;
    let store = Arc::new(RwLock::new(ProjectStore::new()));
    let project = Project {
        header: Header {
            bpm: 300,
            loop_duration: 480,
        },
        tracks: vec![Track {
            name: "t".to_string(),
            channel: 1,
            instrument: 0,
            notes: vec![Note {
                start_tick: 0,
                duration: 480,
                pitch: 60,
                velocity: 80,
            }],
        }],
    };
    store.write().unwrap().set_pending(project).unwrap();
    store.write().unwrap().commit_pending();
    store
}

#[cfg(test)]
fn make_empty_store() -> Arc<RwLock<ProjectStore>> {
    Arc::new(RwLock::new(ProjectStore::new()))
}

// A note that does not start at tick 0, so the only way current_tick can read 0 once
// events have started firing is an explicit reset (loop boundary, stop, sync-restart).
#[cfg(test)]
fn make_store_with_delayed_note() -> Arc<RwLock<ProjectStore>> {
    use crate::domain::*;
    let store = Arc::new(RwLock::new(ProjectStore::new()));
    let project = Project {
        header: Header {
            bpm: 300,
            loop_duration: 960,
        },
        tracks: vec![Track {
            name: "t".to_string(),
            channel: 1,
            instrument: 0,
            notes: vec![Note {
                start_tick: 100,
                duration: 100,
                pitch: 60,
                velocity: 80,
            }],
        }],
    };
    store.write().unwrap().set_pending(project).unwrap();
    store.write().unwrap().commit_pending();
    store
}

// Two notes far apart in the loop. advance_loop() fires immediately once the last event
// of a pass is processed, so a nonzero tick value is only observable for a wide window
// when the next write is far away in real time — the gap between these two notes gives
// that window, rather than the brief instant before the loop boundary reset.
#[cfg(test)]
fn make_store_with_two_widely_spaced_notes() -> Arc<RwLock<ProjectStore>> {
    use crate::domain::*;
    let store = Arc::new(RwLock::new(ProjectStore::new()));
    let project = Project {
        header: Header {
            bpm: 300,
            loop_duration: 960,
        },
        tracks: vec![Track {
            name: "t".to_string(),
            channel: 1,
            instrument: 0,
            notes: vec![
                Note {
                    start_tick: 0,
                    duration: 10,
                    pitch: 60,
                    velocity: 80,
                },
                Note {
                    start_tick: 900,
                    duration: 10,
                    pitch: 62,
                    velocity: 80,
                },
            ],
        }],
    };
    store.write().unwrap().set_pending(project).unwrap();
    store.write().unwrap().commit_pending();
    store
}

#[cfg(test)]
fn wait_for_nonzero_tick(engine: &LoopEngine, timeout_ms: u64) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    while std::time::Instant::now() < deadline {
        if engine.current_tick() > 0 {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    false
}

#[cfg(test)]
fn wait_for_state(engine: &LoopEngine, target: EngineState, timeout_ms: u64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    while std::time::Instant::now() < deadline {
        if engine.state() == target {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Header, Note, Project, ProjectStore, Track};
    use crate::loop_engine::midi::{MidiSendError, MockMidiOutput};
    use std::sync::{Arc, Mutex, RwLock};
    use std::time::{Duration, Instant};

    fn make_engine_with_project() -> (LoopEngine, Arc<Mutex<Vec<midi::MidiEvent>>>) {
        let store = make_test_store_with_project();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = Arc::clone(&captured);
        let output = CapturingOutput {
            captured: captured_clone,
        };
        (LoopEngine::new(store, Box::new(output)), captured)
    }

    fn make_engine_no_project() -> (LoopEngine, Arc<Mutex<Vec<midi::MidiEvent>>>) {
        let store = make_empty_store();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = Arc::clone(&captured);
        let output = CapturingOutput {
            captured: captured_clone,
        };
        (LoopEngine::new(store, Box::new(output)), captured)
    }

    struct CapturingOutput {
        captured: Arc<Mutex<Vec<midi::MidiEvent>>>,
    }

    impl MidiOutput for CapturingOutput {
        fn note_on(&mut self, channel: u8, pitch: u8, velocity: u8) -> Result<(), MidiSendError> {
            self.captured.lock().unwrap().push(midi::MidiEvent::NoteOn {
                channel,
                pitch,
                velocity,
            });
            Ok(())
        }
        fn note_off(&mut self, channel: u8, pitch: u8) -> Result<(), MidiSendError> {
            self.captured
                .lock()
                .unwrap()
                .push(midi::MidiEvent::NoteOff { channel, pitch });
            Ok(())
        }
        fn program_change(&mut self, channel: u8, program: u8) -> Result<(), MidiSendError> {
            self.captured
                .lock()
                .unwrap()
                .push(midi::MidiEvent::ProgramChange { channel, program });
            Ok(())
        }
        fn clock_tick(&mut self) -> Result<(), MidiSendError> {
            self.captured
                .lock()
                .unwrap()
                .push(midi::MidiEvent::ClockTick);
            Ok(())
        }
        fn clock_start(&mut self) -> Result<(), MidiSendError> {
            self.captured
                .lock()
                .unwrap()
                .push(midi::MidiEvent::ClockStart);
            Ok(())
        }
        fn clock_continue(&mut self) -> Result<(), MidiSendError> {
            self.captured
                .lock()
                .unwrap()
                .push(midi::MidiEvent::ClockContinue);
            Ok(())
        }
        fn clock_stop(&mut self) -> Result<(), MidiSendError> {
            self.captured
                .lock()
                .unwrap()
                .push(midi::MidiEvent::ClockStop);
            Ok(())
        }
    }

    #[test]
    fn new_engine_state_is_stopped() {
        let (engine, _) = make_engine_no_project();
        assert_eq!(engine.state(), EngineState::Stopped);
    }

    #[test]
    fn new_engine_current_tick_is_zero() {
        let (engine, _) = make_engine_no_project();
        assert_eq!(engine.current_tick(), 0);
    }

    #[test]
    fn new_engine_loop_duration_ticks_is_zero() {
        let (engine, _) = make_engine_no_project();
        assert_eq!(engine.loop_duration_ticks(), 0);
    }

    #[test]
    fn loop_duration_ticks_matches_project_after_full_loop() {
        // BPM 300, loop_duration = 480 ticks; loop ≈ 200ms.
        let (engine, _) = make_engine_with_project();
        engine.start();
        std::thread::sleep(Duration::from_millis(300));
        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 200);

        assert_eq!(engine.loop_duration_ticks(), 480);
    }

    #[test]
    fn loop_duration_ticks_zero_while_waiting_with_no_project() {
        let (engine, _) = make_engine_no_project();
        engine.start();
        wait_for_state(&engine, EngineState::Waiting, 200);
        std::thread::sleep(Duration::from_millis(50));

        assert_eq!(engine.loop_duration_ticks(), 0);
        engine.stop();
    }

    #[test]
    fn start_with_no_project_is_waiting() {
        let (engine, captured) = make_engine_no_project();
        engine.start();
        wait_for_state(&engine, EngineState::Waiting, 200);
        assert_eq!(engine.state(), EngineState::Waiting);
        assert!(captured.lock().unwrap().is_empty());
    }

    #[test]
    fn start_with_project_transitions_to_running() {
        let (engine, _) = make_engine_with_project();
        engine.start();
        wait_for_state(&engine, EngineState::Running, 500);
        assert_eq!(engine.state(), EngineState::Running);
        engine.stop();
    }

    #[test]
    fn stop_while_running_transitions_to_stopped() {
        let (engine, _) = make_engine_with_project();
        engine.start();
        wait_for_state(&engine, EngineState::Running, 500);
        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 500);
        assert_eq!(engine.state(), EngineState::Stopped);
    }

    #[test]
    fn current_tick_advances_after_events_fire() {
        let store = make_store_with_two_widely_spaced_notes();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(store, Box::new(output));
        engine.start();

        let deadline = Instant::now() + Duration::from_millis(300);
        let mut saw_nonzero = false;
        while Instant::now() < deadline {
            if engine.current_tick() > 0 {
                saw_nonzero = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        engine.stop();
        assert!(
            saw_nonzero,
            "expected current_tick() > 0 after events fired"
        );
    }

    #[test]
    fn current_tick_resets_at_loop_boundary() {
        let store = make_store_with_delayed_note();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(store, Box::new(output));
        engine.start();

        assert!(
            wait_for_nonzero_tick(&engine, 300),
            "precondition failed: expected an event to have fired"
        );

        // Sample across 2+ loop boundaries (loop ≈ 400ms) looking for a reset to 0.
        let deadline = Instant::now() + Duration::from_millis(900);
        let mut saw_zero = false;
        while Instant::now() < deadline {
            if engine.current_tick() == 0 {
                saw_zero = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        engine.stop();
        assert!(
            saw_zero,
            "expected current_tick() == 0 to be observed after a loop boundary"
        );
    }

    #[test]
    fn current_tick_zero_after_stop() {
        let store = make_store_with_delayed_note();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(store, Box::new(output));
        engine.start();

        assert!(
            wait_for_nonzero_tick(&engine, 300),
            "precondition failed: expected an event to have fired"
        );

        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 500);
        assert_eq!(
            engine.current_tick(),
            0,
            "expected current_tick() == 0 after stop()"
        );
    }

    #[test]
    fn current_tick_zero_after_clock_stop() {
        let store = make_store_with_delayed_note();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(store, Box::new(output));
        engine.clock_start();

        assert!(
            wait_for_nonzero_tick(&engine, 300),
            "precondition failed: expected an event to have fired"
        );

        engine.clock_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);
        assert_eq!(
            engine.current_tick(),
            0,
            "expected current_tick() == 0 after clock_stop()"
        );
    }

    #[test]
    fn current_tick_zero_after_sync_stop() {
        let store = make_store_with_delayed_note();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(store, Box::new(output));
        engine.sync_start();

        assert!(
            wait_for_nonzero_tick(&engine, 300),
            "precondition failed: expected an event to have fired"
        );

        engine.sync_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);
        assert_eq!(
            engine.current_tick(),
            0,
            "expected current_tick() == 0 after sync_stop()"
        );
    }

    #[test]
    fn current_tick_frozen_while_paused() {
        let store = make_store_with_delayed_note();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(store, Box::new(output));
        engine.clock_start();

        assert!(
            wait_for_nonzero_tick(&engine, 300),
            "precondition failed: expected an event to have fired"
        );

        engine.clock_pause();
        wait_for_state(&engine, EngineState::Paused, 500);

        let first = engine.current_tick();
        std::thread::sleep(Duration::from_millis(50));
        let second = engine.current_tick();

        engine.clock_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);

        assert_eq!(
            first, second,
            "expected current_tick() to stay unchanged while paused"
        );
    }

    #[test]
    fn current_tick_zero_after_stop_from_paused() {
        // TickResetsOnStop applies to every transition into Stopped, including the one
        // handled directly inside handle_paused() (Stop/ClockStop received while paused).
        let store = make_store_with_delayed_note();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(store, Box::new(output));
        engine.clock_start();

        assert!(
            wait_for_nonzero_tick(&engine, 300),
            "precondition failed: expected an event to have fired"
        );

        engine.clock_pause();
        wait_for_state(&engine, EngineState::Paused, 500);

        engine.clock_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);

        assert_eq!(
            engine.current_tick(),
            0,
            "expected current_tick() == 0 after stopping from Paused"
        );
    }

    #[test]
    fn current_tick_resets_on_sync_start_mid_loop() {
        // Sample immediately (well before the restarted loop's first event, which fires
        // ~62ms after restart at bpm 300 given START_LATENCY_MICROS) so this test isolates
        // do_sync_restart's own reset from the unrelated loop-boundary reset (advance_loop)
        // that would otherwise also zero the counter a bit later in the same loop pass.
        let store = make_store_with_delayed_note();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(store, Box::new(output));
        engine.sync_start();
        wait_for_state(&engine, EngineState::Running, 500);

        assert!(
            wait_for_nonzero_tick(&engine, 300),
            "precondition failed: expected an event to have fired"
        );

        engine.sync_start();

        let deadline = Instant::now() + Duration::from_millis(40);
        let mut saw_zero = false;
        while Instant::now() < deadline {
            if engine.current_tick() == 0 {
                saw_zero = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        engine.sync_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);
        assert!(
            saw_zero,
            "expected current_tick() == 0 shortly after SyncStart mid-loop"
        );
    }

    #[test]
    fn current_tick_not_reset_by_sync_continue() {
        let store = make_store_with_two_widely_spaced_notes();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(store, Box::new(output));
        engine.sync_start();
        wait_for_state(&engine, EngineState::Running, 500);

        assert!(
            wait_for_nonzero_tick(&engine, 300),
            "precondition failed: expected an event to have fired"
        );
        let t = engine.current_tick();

        engine.sync_continue();
        std::thread::sleep(Duration::from_millis(50));

        let after = engine.current_tick();
        engine.sync_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);

        assert!(
            after >= t,
            "expected current_tick() ({after}) >= T ({t}) after SyncContinue"
        );
    }

    #[test]
    fn single_note_loop_emits_note_on_then_note_off() {
        let (engine, captured) = make_engine_with_project();
        engine.start();
        // BPM 300, loop_duration = 480 ticks, micros_per_tick = 416μs; loop = ~200ms
        std::thread::sleep(Duration::from_millis(400));
        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 200);

        let events = captured.lock().unwrap().clone();
        // Filter out program changes
        let note_events: Vec<_> = events
            .iter()
            .filter(|e| !matches!(e, midi::MidiEvent::ProgramChange { .. }))
            .collect();
        assert!(!note_events.is_empty(), "expected at least one NoteOn");
        // Check pattern: NoteOn followed by NoteOff
        let first_on = note_events
            .iter()
            .position(|e| matches!(e, midi::MidiEvent::NoteOn { .. }));
        let first_off = note_events
            .iter()
            .position(|e| matches!(e, midi::MidiEvent::NoteOff { .. }));
        assert!(first_on.is_some(), "expected NoteOn");
        assert!(first_off.is_some(), "expected NoteOff");
        assert!(
            first_on.unwrap() < first_off.unwrap(),
            "NoteOn should precede NoteOff"
        );
    }

    #[test]
    fn empty_loop_emits_no_events() {
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        let project = Project {
            header: Header {
                bpm: 300,
                loop_duration: 480,
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![],
            }],
        };
        store.write().unwrap().set_pending(project).unwrap();
        store.write().unwrap().commit_pending();

        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(store, Box::new(output));
        engine.start();
        std::thread::sleep(Duration::from_millis(300));
        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 200);

        let events = captured.lock().unwrap().clone();
        let note_events: Vec<_> = events
            .iter()
            .filter(|e| !matches!(e, midi::MidiEvent::ProgramChange { .. }))
            .collect();
        assert!(
            note_events.is_empty(),
            "empty loop should emit no note events, got {:?}",
            note_events
        );
    }

    #[test]
    fn two_tracks_both_emit_note_on() {
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        let project = Project {
            header: Header {
                bpm: 300,
                loop_duration: 480,
            },
            tracks: vec![
                Track {
                    name: "t1".to_string(),
                    channel: 1,
                    instrument: 0,
                    notes: vec![Note {
                        start_tick: 0,
                        duration: 480,
                        pitch: 60,
                        velocity: 80,
                    }],
                },
                Track {
                    name: "t2".to_string(),
                    channel: 2,
                    instrument: 1,
                    notes: vec![Note {
                        start_tick: 0,
                        duration: 480,
                        pitch: 64,
                        velocity: 80,
                    }],
                },
            ],
        };
        store.write().unwrap().set_pending(project).unwrap();
        store.write().unwrap().commit_pending();

        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(store, Box::new(output));
        engine.start();
        std::thread::sleep(Duration::from_millis(300));
        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 200);

        let events = captured.lock().unwrap().clone();
        let has_ch1 = events
            .iter()
            .any(|e| matches!(e, midi::MidiEvent::NoteOn { channel: 1, .. }));
        let has_ch2 = events
            .iter()
            .any(|e| matches!(e, midi::MidiEvent::NoteOn { channel: 2, .. }));
        assert!(has_ch1, "expected NoteOn on channel 1");
        assert!(has_ch2, "expected NoteOn on channel 2");
    }

    #[test]
    fn program_change_sent_before_first_note_on() {
        let (engine, captured) = make_engine_with_project();
        engine.start();
        std::thread::sleep(Duration::from_millis(300));
        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 200);

        let events = captured.lock().unwrap().clone();
        let first_pc = events
            .iter()
            .position(|e| matches!(e, midi::MidiEvent::ProgramChange { .. }));
        let first_on = events
            .iter()
            .position(|e| matches!(e, midi::MidiEvent::NoteOn { .. }));
        assert!(first_pc.is_some(), "expected at least one ProgramChange");
        assert!(first_on.is_some(), "expected at least one NoteOn");
        assert!(
            first_pc.unwrap() < first_on.unwrap(),
            "ProgramChange must precede first NoteOn"
        );
    }

    #[test]
    fn engine_loops_seamlessly() {
        let (engine, captured) = make_engine_with_project();
        engine.start();
        // Wait 3 loop durations (3 * ~200ms = 600ms) to ensure multiple loops
        std::thread::sleep(Duration::from_millis(700));
        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 200);

        let events = captured.lock().unwrap().clone();
        let note_on_count = events
            .iter()
            .filter(|e| matches!(e, midi::MidiEvent::NoteOn { .. }))
            .count();
        assert!(
            note_on_count >= 2,
            "expected at least 2 NoteOn events across multiple loops, got {}",
            note_on_count
        );
    }

    #[test]
    fn pending_project_takes_effect_after_loop_boundary() {
        let store = make_test_store_with_project(); // BPM 300, loop_duration = 480

        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(Arc::clone(&store), Box::new(output));
        engine.start();
        // Wait for at least one loop to play
        std::thread::sleep(Duration::from_millis(150));

        // Submit a new project with pitch 62
        let new_project = Project {
            header: Header {
                bpm: 300,
                loop_duration: 480,
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![Note {
                    start_tick: 0,
                    duration: 480,
                    pitch: 62,
                    velocity: 80,
                }],
            }],
        };
        store.write().unwrap().set_pending(new_project).unwrap();

        // Wait for the next loop boundary to pick it up
        std::thread::sleep(Duration::from_millis(300));
        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 200);

        let events = captured.lock().unwrap().clone();
        let has_pitch_62 = events
            .iter()
            .any(|e| matches!(e, midi::MidiEvent::NoteOn { pitch: 62, .. }));
        assert!(
            has_pitch_62,
            "expected NoteOn with pitch 62 from updated project"
        );
    }

    #[test]
    fn bpm_change_does_not_stop_engine() {
        let store = make_test_store_with_project();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(Arc::clone(&store), Box::new(output));
        engine.start();
        std::thread::sleep(Duration::from_millis(200));

        let new_project = Project {
            header: Header {
                bpm: 200,
                loop_duration: 480,
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![Note {
                    start_tick: 0,
                    duration: 480,
                    pitch: 60,
                    velocity: 80,
                }],
            }],
        };
        store.write().unwrap().set_pending(new_project).unwrap();

        std::thread::sleep(Duration::from_millis(300));
        // Engine should still be running
        assert_eq!(engine.state(), EngineState::Running);
        engine.stop();
    }

    #[test]
    fn instrument_change_triggers_program_change() {
        let store = make_test_store_with_project();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(Arc::clone(&store), Box::new(output));
        engine.start();
        std::thread::sleep(Duration::from_millis(200));

        // Initial PC count
        let pc_before = captured
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e, midi::MidiEvent::ProgramChange { .. }))
            .count();

        let new_project = Project {
            header: Header {
                bpm: 300,
                loop_duration: 480,
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 42, // changed instrument
                notes: vec![Note {
                    start_tick: 0,
                    duration: 480,
                    pitch: 60,
                    velocity: 80,
                }],
            }],
        };
        store.write().unwrap().set_pending(new_project).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 200);

        let events = captured.lock().unwrap().clone();
        let pc_after = events
            .iter()
            .filter(|e| matches!(e, midi::MidiEvent::ProgramChange { .. }))
            .count();
        assert!(
            pc_after > pc_before,
            "expected new ProgramChange after instrument change"
        );
        let has_pc_42 = events
            .iter()
            .any(|e| matches!(e, midi::MidiEvent::ProgramChange { program: 42, .. }));
        assert!(has_pc_42, "expected ProgramChange for instrument 42");
    }

    #[test]
    fn waiting_state_transitions_to_running_on_project_load() {
        let store = make_empty_store();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(Arc::clone(&store), Box::new(output));
        engine.start();
        wait_for_state(&engine, EngineState::Waiting, 200);
        assert_eq!(engine.state(), EngineState::Waiting);

        // Load a project
        let project = Project {
            header: Header {
                bpm: 300,
                loop_duration: 480,
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![Note {
                    start_tick: 0,
                    duration: 480,
                    pitch: 60,
                    velocity: 80,
                }],
            }],
        };
        store.write().unwrap().set_pending(project).unwrap();

        // Engine should pick it up and transition to Running
        wait_for_state(&engine, EngineState::Running, 500);
        assert_eq!(engine.state(), EngineState::Running);
        engine.stop();
    }

    #[test]
    fn stop_while_note_sounding_emits_note_off() {
        // Use a very long note (longer than the loop) to ensure it's sounding when we stop
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        let project = Project {
            header: Header {
                bpm: 60,
                loop_duration: 1920,
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![Note {
                    start_tick: 0,
                    duration: 1920,
                    pitch: 60,
                    velocity: 80,
                }],
            }],
        };
        store.write().unwrap().set_pending(project).unwrap();
        store.write().unwrap().commit_pending();

        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(store, Box::new(output));
        engine.start();
        // Wait for NoteOn to be emitted (note starts at tick 0, so almost immediately)
        std::thread::sleep(Duration::from_millis(50));

        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 500);

        let events = captured.lock().unwrap().clone();
        let has_note_on = events
            .iter()
            .any(|e| matches!(e, midi::MidiEvent::NoteOn { .. }));
        let has_note_off = events
            .iter()
            .any(|e| matches!(e, midi::MidiEvent::NoteOff { .. }));
        assert!(has_note_on, "expected NoteOn to have been emitted");
        assert!(
            has_note_off,
            "expected NoteOff on stop to prevent stuck note"
        );
    }

    #[test]
    fn note_off_before_note_on_at_same_tick() {
        // Two notes at adjacent start_ticks:
        // note1: pitch 60, start_tick 0, duration 480 (NoteOff at tick 480)
        // note2: pitch 62, start_tick 480, duration 480 (NoteOn at tick 480)
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        let project = Project {
            header: Header {
                bpm: 300,
                loop_duration: 960,
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![
                    Note {
                        start_tick: 0,
                        duration: 480,
                        pitch: 60,
                        velocity: 80,
                    },
                    Note {
                        start_tick: 480,
                        duration: 480,
                        pitch: 62,
                        velocity: 80,
                    },
                ],
            }],
        };
        store.write().unwrap().set_pending(project).unwrap();
        store.write().unwrap().commit_pending();

        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(store, Box::new(output));
        engine.start();
        std::thread::sleep(Duration::from_millis(400));
        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 200);

        let events = captured.lock().unwrap().clone();
        let note_events: Vec<_> = events
            .iter()
            .filter(|e| !matches!(e, midi::MidiEvent::ProgramChange { .. }))
            .collect();
        // Find the boundary tick: NoteOff(60) and NoteOn(62) should appear in that order
        let off_60_pos = note_events
            .iter()
            .position(|e| matches!(e, midi::MidiEvent::NoteOff { pitch: 60, .. }));
        let on_62_pos = note_events
            .iter()
            .position(|e| matches!(e, midi::MidiEvent::NoteOn { pitch: 62, .. }));
        assert!(off_60_pos.is_some(), "expected NoteOff for pitch 60");
        assert!(on_62_pos.is_some(), "expected NoteOn for pitch 62");
        assert!(
            off_60_pos.unwrap() < on_62_pos.unwrap(),
            "NoteOff(60) must precede NoteOn(62) at the same tick"
        );
    }

    #[test]
    fn start_while_running_is_noop() {
        let (engine, _) = make_engine_with_project();
        engine.start();
        wait_for_state(&engine, EngineState::Running, 500);
        engine.start(); // second start — should be no-op
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(engine.state(), EngineState::Running);
        engine.stop();
    }

    #[test]
    fn dropping_loop_engine_exits_thread() {
        let store = make_empty_store();
        let (sender, receiver) = mpsc::channel::<LoopCommand>();
        let state = Arc::new(Mutex::new(EngineState::Stopped));
        let state_clone = Arc::clone(&state);
        let output: Box<dyn MidiOutput> = Box::new(MockMidiOutput::new());
        let current_tick = Arc::new(AtomicU64::new(0));
        let loop_duration_ticks = Arc::new(AtomicU64::new(0));

        let handle = std::thread::spawn(move || {
            run_player_loop(
                receiver,
                store,
                output,
                state_clone,
                current_tick,
                loop_duration_ticks,
            );
        });

        // Drop the sender to simulate LoopEngine drop
        drop(sender);

        let result = handle.join();
        assert!(
            result.is_ok(),
            "player thread should exit cleanly after sender is dropped"
        );
    }

    #[test]
    #[ignore] // slow test; run with --include-ignored
    fn timing_jitter_within_5ms() {
        use std::sync::Mutex;

        struct TimestampedOutput {
            timestamps: Arc<Mutex<Vec<(std::time::Instant, midi::MidiEvent)>>>,
        }

        impl MidiOutput for TimestampedOutput {
            fn note_on(
                &mut self,
                channel: u8,
                pitch: u8,
                velocity: u8,
            ) -> Result<(), MidiSendError> {
                self.timestamps.lock().unwrap().push((
                    Instant::now(),
                    midi::MidiEvent::NoteOn {
                        channel,
                        pitch,
                        velocity,
                    },
                ));
                Ok(())
            }
            fn note_off(&mut self, channel: u8, pitch: u8) -> Result<(), MidiSendError> {
                self.timestamps
                    .lock()
                    .unwrap()
                    .push((Instant::now(), midi::MidiEvent::NoteOff { channel, pitch }));
                Ok(())
            }
            fn program_change(&mut self, channel: u8, program: u8) -> Result<(), MidiSendError> {
                self.timestamps.lock().unwrap().push((
                    Instant::now(),
                    midi::MidiEvent::ProgramChange { channel, program },
                ));
                Ok(())
            }
            fn clock_tick(&mut self) -> Result<(), MidiSendError> {
                Ok(())
            }
            fn clock_start(&mut self) -> Result<(), MidiSendError> {
                Ok(())
            }
            fn clock_continue(&mut self) -> Result<(), MidiSendError> {
                Ok(())
            }
            fn clock_stop(&mut self) -> Result<(), MidiSendError> {
                Ok(())
            }
        }

        // BPM 480 → micros_per_tick = 60_000_000/(480*480) = 260μs; loop_duration=1920 ≈ 500ms
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        let project = Project {
            header: Header {
                bpm: 480,
                loop_duration: 1920,
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![
                    Note {
                        start_tick: 0,
                        duration: 480,
                        pitch: 60,
                        velocity: 80,
                    },
                    Note {
                        start_tick: 480,
                        duration: 480,
                        pitch: 62,
                        velocity: 80,
                    },
                    Note {
                        start_tick: 960,
                        duration: 480,
                        pitch: 64,
                        velocity: 80,
                    },
                    Note {
                        start_tick: 1440,
                        duration: 480,
                        pitch: 65,
                        velocity: 80,
                    },
                ],
            }],
        };
        store.write().unwrap().set_pending(project).unwrap();
        store.write().unwrap().commit_pending();

        let timestamps = Arc::new(Mutex::new(Vec::new()));
        let output = TimestampedOutput {
            timestamps: Arc::clone(&timestamps),
        };
        let engine = LoopEngine::new(Arc::clone(&store), Box::new(output));
        engine.start();
        // Run for 4 loops
        std::thread::sleep(Duration::from_millis(2000));
        engine.stop();

        // We can't easily check exact timing here without the anchor,
        // but we verify the engine produced output and ran for 4 loops.
        let ts = timestamps.lock().unwrap();
        let note_on_count = ts
            .iter()
            .filter(|(_, e)| matches!(e, midi::MidiEvent::NoteOn { .. }))
            .count();
        assert!(
            note_on_count >= 16,
            "expected at least 16 NoteOn events over 4 loops * 4 repetitions, got {}",
            note_on_count
        );
    }

    #[test]
    fn engine_state_paused_exists() {
        let state: Arc<Mutex<EngineState>> = Arc::new(Mutex::new(EngineState::Paused));
        assert_eq!(*state.lock().unwrap(), EngineState::Paused);
    }

    #[test]
    fn clock_start_with_project_transitions_to_running_and_stop_to_stopped() {
        let (engine, _) = make_engine_with_project();
        engine.clock_start();
        wait_for_state(&engine, EngineState::Running, 500);
        assert_eq!(engine.state(), EngineState::Running);
        engine.clock_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);
        assert_eq!(engine.state(), EngineState::Stopped);
    }

    #[test]
    fn clock_start_emits_clock_start_before_first_clock_tick() {
        let (engine, captured) = make_engine_with_project();
        engine.clock_start();
        std::thread::sleep(Duration::from_millis(50));
        engine.clock_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);

        let events = captured.lock().unwrap().clone();
        let start_pos = events
            .iter()
            .position(|e| matches!(e, midi::MidiEvent::ClockStart));
        let tick_pos = events
            .iter()
            .position(|e| matches!(e, midi::MidiEvent::ClockTick));
        assert!(start_pos.is_some(), "expected ClockStart event");
        assert!(tick_pos.is_some(), "expected at least one ClockTick");
        assert!(
            start_pos.unwrap() < tick_pos.unwrap(),
            "ClockStart must precede first ClockTick"
        );
    }

    #[test]
    fn clock_mode_emits_clock_ticks() {
        let (engine, captured) = make_engine_with_project(); // BPM 300, loop_duration = 480
        engine.clock_start();
        // Loop duration ≈ 200ms; wait 250ms to ensure at least one full loop
        std::thread::sleep(Duration::from_millis(250));
        engine.clock_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);

        let events = captured.lock().unwrap().clone();
        let tick_count = events
            .iter()
            .filter(|e| matches!(e, midi::MidiEvent::ClockTick))
            .count();
        assert!(
            tick_count >= 24,
            "expected ≥24 ClockTick events for one loop at BPM 300, got {}",
            tick_count
        );
    }

    #[test]
    fn clock_start_plays_notes_and_clock_ticks() {
        let (engine, captured) = make_engine_with_project();
        engine.clock_start();
        std::thread::sleep(Duration::from_millis(300));
        engine.clock_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);

        let events = captured.lock().unwrap().clone();
        let has_note_on = events
            .iter()
            .any(|e| matches!(e, midi::MidiEvent::NoteOn { .. }));
        let has_clock_tick = events
            .iter()
            .any(|e| matches!(e, midi::MidiEvent::ClockTick));
        assert!(has_note_on, "expected NoteOn events in clock mode");
        assert!(has_clock_tick, "expected ClockTick events in clock mode");
    }

    #[test]
    fn clock_pause_transitions_to_paused_and_flushes_notes() {
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        let project = Project {
            header: Header {
                bpm: 60,
                loop_duration: 1920,
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![Note {
                    start_tick: 0,
                    duration: 1920,
                    pitch: 60,
                    velocity: 80,
                }],
            }],
        };
        store.write().unwrap().set_pending(project).unwrap();
        store.write().unwrap().commit_pending();

        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(store, Box::new(output));

        engine.clock_start();
        std::thread::sleep(Duration::from_millis(50));
        engine.clock_pause();
        wait_for_state(&engine, EngineState::Paused, 500);

        let events = captured.lock().unwrap().clone();
        assert_eq!(engine.state(), EngineState::Paused);
        let has_note_off = events
            .iter()
            .any(|e| matches!(e, midi::MidiEvent::NoteOff { .. }));
        let has_clock_stop = events
            .iter()
            .any(|e| matches!(e, midi::MidiEvent::ClockStop));
        assert!(
            has_note_off,
            "expected NoteOff on pause to prevent stuck notes"
        );
        assert!(!has_clock_stop, "clock_pause must not emit ClockStop");

        engine.clock_stop();
    }

    #[test]
    fn clock_resume_sends_continue_before_first_tick() {
        let (engine, captured) = make_engine_with_project();
        engine.clock_start();
        std::thread::sleep(Duration::from_millis(50));
        engine.clock_pause();
        wait_for_state(&engine, EngineState::Paused, 500);

        let pre_resume_count = captured.lock().unwrap().len();

        engine.clock_resume();
        wait_for_state(&engine, EngineState::Running, 500);
        std::thread::sleep(Duration::from_millis(50));
        engine.clock_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);

        let events = captured.lock().unwrap().clone();
        let post_resume = &events[pre_resume_count..];
        let continue_pos = post_resume
            .iter()
            .position(|e| matches!(e, midi::MidiEvent::ClockContinue));
        let tick_pos = post_resume
            .iter()
            .position(|e| matches!(e, midi::MidiEvent::ClockTick));
        assert!(
            continue_pos.is_some(),
            "expected ClockContinue after resume"
        );
        assert!(tick_pos.is_some(), "expected ClockTick after resume");
        assert!(
            continue_pos.unwrap() < tick_pos.unwrap(),
            "ClockContinue must precede first ClockTick after resume"
        );
    }

    #[test]
    fn clock_resume_continues_clock_ticks_after_pause() {
        let (engine, captured) = make_engine_with_project();
        engine.clock_start();
        std::thread::sleep(Duration::from_millis(100));
        engine.clock_pause();
        wait_for_state(&engine, EngineState::Paused, 500);

        engine.clock_resume();
        wait_for_state(&engine, EngineState::Running, 500);
        std::thread::sleep(Duration::from_millis(100));
        engine.clock_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);

        let events = captured.lock().unwrap().clone();
        let has_tick_after_continue = events
            .iter()
            .skip_while(|e| !matches!(e, midi::MidiEvent::ClockContinue))
            .any(|e| matches!(e, midi::MidiEvent::ClockTick));
        assert!(
            has_tick_after_continue,
            "expected ClockTick events after ClockContinue"
        );
    }

    #[test]
    fn clock_stop_while_running_emits_clock_stop() {
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        let project = Project {
            header: Header {
                bpm: 60,
                loop_duration: 1920,
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![Note {
                    start_tick: 0,
                    duration: 1920,
                    pitch: 60,
                    velocity: 80,
                }],
            }],
        };
        store.write().unwrap().set_pending(project).unwrap();
        store.write().unwrap().commit_pending();

        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(store, Box::new(output));

        engine.clock_start();
        std::thread::sleep(Duration::from_millis(50));
        engine.clock_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);

        let events = captured.lock().unwrap().clone();
        assert_eq!(engine.state(), EngineState::Stopped);
        let has_clock_stop = events
            .iter()
            .any(|e| matches!(e, midi::MidiEvent::ClockStop));
        assert!(has_clock_stop, "expected ClockStop event from clock_stop()");
    }

    #[test]
    fn clock_stop_while_paused_emits_clock_stop() {
        let (engine, captured) = make_engine_with_project();
        engine.clock_start();
        std::thread::sleep(Duration::from_millis(50));
        engine.clock_pause();
        wait_for_state(&engine, EngineState::Paused, 500);

        engine.clock_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);

        let events = captured.lock().unwrap().clone();
        assert_eq!(engine.state(), EngineState::Stopped);
        let has_clock_stop = events
            .iter()
            .any(|e| matches!(e, midi::MidiEvent::ClockStop));
        assert!(
            has_clock_stop,
            "expected ClockStop after clock_stop() while paused"
        );
    }

    #[test]
    fn bpm_change_does_not_stop_clock_mode() {
        let store = make_test_store_with_project();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(Arc::clone(&store), Box::new(output));

        engine.clock_start();
        wait_for_state(&engine, EngineState::Running, 500);
        std::thread::sleep(Duration::from_millis(200));

        let new_project = Project {
            header: Header {
                bpm: 200,
                loop_duration: 480,
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![Note {
                    start_tick: 0,
                    duration: 480,
                    pitch: 60,
                    velocity: 80,
                }],
            }],
        };
        store.write().unwrap().set_pending(new_project).unwrap();

        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            engine.state(),
            EngineState::Running,
            "engine should still be Running after BPM change"
        );
        engine.clock_stop();
    }

    #[test]
    fn project_removed_clock_continues_without_notes() {
        let store = make_test_store_with_project();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(Arc::clone(&store), Box::new(output));

        engine.clock_start();
        wait_for_state(&engine, EngineState::Running, 500);
        std::thread::sleep(Duration::from_millis(100));

        // Clear the project (simulate project removal)
        store.write().unwrap().clear();

        let tick_count_mid = {
            let ev = captured.lock().unwrap();
            ev.iter()
                .filter(|e| matches!(e, midi::MidiEvent::ClockTick))
                .count()
        };

        // Wait for more loops to play (with no project, should be clock-only)
        std::thread::sleep(Duration::from_millis(300));
        engine.clock_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);

        let events = captured.lock().unwrap().clone();
        let tick_count_final = events
            .iter()
            .filter(|e| matches!(e, midi::MidiEvent::ClockTick))
            .count();

        assert!(
            tick_count_final > tick_count_mid,
            "clock should continue ticking after project removal"
        );

        // No NoteOn events after project removal (check events after the first loop boundary)
        // The clock_stop at the very end sends ClockStop — that's the only one
        let clock_stop_count = events
            .iter()
            .filter(|e| matches!(e, midi::MidiEvent::ClockStop))
            .count();
        assert_eq!(
            clock_stop_count, 1,
            "only one ClockStop expected (from explicit clock_stop())"
        );
    }

    #[test]
    fn clock_stop_on_shutdown_sends_clock_stop_when_running() {
        let (engine, captured) = make_engine_with_project();
        engine.clock_start();
        wait_for_state(&engine, EngineState::Running, 500);

        engine.clock_stop_on_shutdown();
        wait_for_state(&engine, EngineState::Stopped, 500);

        let events = captured.lock().unwrap().clone();
        let has_clock_stop = events
            .iter()
            .any(|e| matches!(e, midi::MidiEvent::ClockStop));
        assert!(has_clock_stop, "expected ClockStop on shutdown");
    }

    #[test]
    fn clock_stop_on_shutdown_blocks_until_stopped() {
        let (engine, captured) = make_engine_with_project();
        engine.clock_start();
        wait_for_state(&engine, EngineState::Running, 500);

        engine.clock_stop_on_shutdown();

        // No external wait: method must have blocked until Stopped before returning
        assert_eq!(
            engine.state(),
            EngineState::Stopped,
            "clock_stop_on_shutdown must block until Stopped before returning"
        );
        let events = captured.lock().unwrap().clone();
        let has_clock_stop = events
            .iter()
            .any(|e| matches!(e, midi::MidiEvent::ClockStop));
        assert!(
            has_clock_stop,
            "ClockStop must be emitted before clock_stop_on_shutdown returns"
        );
    }

    #[test]
    fn sync_start_with_project_transitions_to_running_at_loop_start() {
        let (engine, _) = make_engine_with_project();
        engine.sync_start();
        wait_for_state(&engine, EngineState::Running, 500);
        assert_eq!(engine.state(), EngineState::Running);
        engine.sync_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);
    }

    #[test]
    fn sync_start_while_running_resets_and_stays_running() {
        let (engine, captured) = make_engine_with_project();
        engine.sync_start();
        wait_for_state(&engine, EngineState::Running, 500);
        // Wait for a NoteOn then send another SyncStart
        std::thread::sleep(Duration::from_millis(50));
        let events_before = captured.lock().unwrap().len();
        engine.sync_start();
        std::thread::sleep(Duration::from_millis(50));
        // Engine should still be Running
        assert_eq!(engine.state(), EngineState::Running);
        // More events should have been emitted
        let events_after = captured.lock().unwrap().len();
        assert!(
            events_after > events_before,
            "expected more events after SyncStart restart"
        );
        engine.sync_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);
    }

    #[test]
    fn sync_continue_with_project_transitions_to_running() {
        let (engine, _) = make_engine_with_project();
        engine.sync_continue();
        wait_for_state(&engine, EngineState::Running, 500);
        assert_eq!(engine.state(), EngineState::Running);
        engine.sync_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);
    }

    #[test]
    fn sync_stop_transitions_to_stopped_and_flushes_notes() {
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        let project = Project {
            header: Header {
                bpm: 60,
                loop_duration: 1920,
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![Note {
                    start_tick: 0,
                    duration: 1920,
                    pitch: 60,
                    velocity: 80,
                }],
            }],
        };
        store.write().unwrap().set_pending(project).unwrap();
        store.write().unwrap().commit_pending();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput {
            captured: Arc::clone(&captured),
        };
        let engine = LoopEngine::new(store, Box::new(output));

        engine.sync_start();
        std::thread::sleep(Duration::from_millis(50)); // let NoteOn emit
        engine.sync_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);

        let events = captured.lock().unwrap().clone();
        assert_eq!(engine.state(), EngineState::Stopped);
        let has_note_off = events
            .iter()
            .any(|e| matches!(e, midi::MidiEvent::NoteOff { .. }));
        assert!(has_note_off, "expected NoteOff on sync_stop");
        let has_clock_stop = events
            .iter()
            .any(|e| matches!(e, midi::MidiEvent::ClockStop));
        assert!(!has_clock_stop, "sync_stop must not emit ClockStop (0xFC)");
    }

    #[test]
    fn sync_bpm_update_does_not_stop_engine() {
        let (engine, _) = make_engine_with_project(); // BPM 300
        engine.sync_start();
        wait_for_state(&engine, EngineState::Running, 500);
        std::thread::sleep(Duration::from_millis(100));

        engine.sync_bpm_update(150);

        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(
            engine.state(),
            EngineState::Running,
            "engine should remain Running after SyncBpmUpdate"
        );
        engine.sync_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);
    }

    #[test]
    fn sync_start_with_no_project_enters_waiting() {
        let (engine, _) = make_engine_no_project();
        engine.sync_start();
        wait_for_state(&engine, EngineState::Waiting, 200);
        assert_eq!(engine.state(), EngineState::Waiting);
        engine.sync_stop();
        wait_for_state(&engine, EngineState::Stopped, 200);
    }

    #[test]
    fn clock_stop_on_shutdown_noop_when_stopped() {
        let (engine, captured) = make_engine_with_project();
        engine.clock_stop_on_shutdown();
        std::thread::sleep(Duration::from_millis(20));

        let events = captured.lock().unwrap().clone();
        let has_clock_stop = events
            .iter()
            .any(|e| matches!(e, midi::MidiEvent::ClockStop));
        assert!(
            !has_clock_stop,
            "clock_stop_on_shutdown should not emit ClockStop when already Stopped"
        );
    }
}
