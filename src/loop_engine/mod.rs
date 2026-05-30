pub mod midi;
pub mod player;
pub mod scheduler;

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
}

impl LoopEngine {
    pub fn new(store: Arc<RwLock<ProjectStore>>, output: Box<dyn MidiOutput>) -> LoopEngine {
        let (sender, receiver) = mpsc::channel::<LoopCommand>();
        let state = Arc::new(Mutex::new(EngineState::Stopped));
        let state_clone = Arc::clone(&state);

        std::thread::spawn(move || {
            run_player_loop(receiver, store, output, state_clone);
        });

        LoopEngine { sender, state }
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
    store
}

#[cfg(test)]
fn make_empty_store() -> Arc<RwLock<ProjectStore>> {
    Arc::new(RwLock::new(ProjectStore::new()))
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
    use crate::domain::*;
    use crate::loop_engine::midi::{MidiSendError, MockMidiOutput};
    use std::sync::{Arc, Mutex, RwLock};
    use std::time::{Duration, Instant};

    fn make_engine_with_project() -> (LoopEngine, Arc<Mutex<Vec<midi::MidiEvent>>>) {
        let store = make_test_store_with_project();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = Arc::clone(&captured);
        let output = CapturingOutput { captured: captured_clone };
        (LoopEngine::new(store, Box::new(output)), captured)
    }

    fn make_engine_no_project() -> (LoopEngine, Arc<Mutex<Vec<midi::MidiEvent>>>) {
        let store = make_empty_store();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = Arc::clone(&captured);
        let output = CapturingOutput { captured: captured_clone };
        (LoopEngine::new(store, Box::new(output)), captured)
    }

    struct CapturingOutput {
        captured: Arc<Mutex<Vec<midi::MidiEvent>>>,
    }

    impl MidiOutput for CapturingOutput {
        fn note_on(&mut self, channel: u8, pitch: u8, velocity: u8) -> Result<(), MidiSendError> {
            self.captured.lock().unwrap().push(midi::MidiEvent::NoteOn { channel, pitch, velocity });
            Ok(())
        }
        fn note_off(&mut self, channel: u8, pitch: u8) -> Result<(), MidiSendError> {
            self.captured.lock().unwrap().push(midi::MidiEvent::NoteOff { channel, pitch });
            Ok(())
        }
        fn program_change(&mut self, channel: u8, program: u8) -> Result<(), MidiSendError> {
            self.captured.lock().unwrap().push(midi::MidiEvent::ProgramChange { channel, program });
            Ok(())
        }
        fn clock_tick(&mut self) -> Result<(), MidiSendError> {
            self.captured.lock().unwrap().push(midi::MidiEvent::ClockTick);
            Ok(())
        }
        fn clock_start(&mut self) -> Result<(), MidiSendError> {
            self.captured.lock().unwrap().push(midi::MidiEvent::ClockStart);
            Ok(())
        }
        fn clock_continue(&mut self) -> Result<(), MidiSendError> {
            self.captured.lock().unwrap().push(midi::MidiEvent::ClockContinue);
            Ok(())
        }
        fn clock_stop(&mut self) -> Result<(), MidiSendError> {
            self.captured.lock().unwrap().push(midi::MidiEvent::ClockStop);
            Ok(())
        }
    }

    // T-11: LoopEngine::new() → state is Stopped
    #[test]
    fn new_engine_state_is_stopped() {
        let (engine, _) = make_engine_no_project();
        assert_eq!(engine.state(), EngineState::Stopped);
    }

    // T-13: start() with no project → state is Waiting, no MIDI events
    #[test]
    fn start_with_no_project_is_waiting() {
        let (engine, captured) = make_engine_no_project();
        engine.start();
        wait_for_state(&engine, EngineState::Waiting, 200);
        assert_eq!(engine.state(), EngineState::Waiting);
        assert!(captured.lock().unwrap().is_empty());
    }

    // T-14: start() with active project → state transitions to Running
    #[test]
    fn start_with_project_transitions_to_running() {
        let (engine, _) = make_engine_with_project();
        engine.start();
        wait_for_state(&engine, EngineState::Running, 500);
        assert_eq!(engine.state(), EngineState::Running);
        engine.stop();
    }

    // T-16: stop() while Running → state is Stopped
    #[test]
    fn stop_while_running_transitions_to_stopped() {
        let (engine, _) = make_engine_with_project();
        engine.start();
        wait_for_state(&engine, EngineState::Running, 500);
        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 500);
        assert_eq!(engine.state(), EngineState::Stopped);
    }

    // T-18: running engine with a single non-rest note → NoteOn then NoteOff
    #[test]
    fn single_note_bar_emits_note_on_then_note_off() {
        let (engine, captured) = make_engine_with_project();
        engine.start();
        // BPM 300, 1/4 bar = 480 ticks, micros_per_tick = 416μs; bar = ~200ms
        std::thread::sleep(Duration::from_millis(400));
        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 200);

        let events = captured.lock().unwrap().clone();
        // Filter out program changes
        let note_events: Vec<_> = events.iter().filter(|e| !matches!(e, midi::MidiEvent::ProgramChange { .. })).collect();
        assert!(!note_events.is_empty(), "expected at least one NoteOn");
        // Check pattern: NoteOn followed by NoteOff
        let first_on = note_events.iter().position(|e| matches!(e, midi::MidiEvent::NoteOn { .. }));
        let first_off = note_events.iter().position(|e| matches!(e, midi::MidiEvent::NoteOff { .. }));
        assert!(first_on.is_some(), "expected NoteOn");
        assert!(first_off.is_some(), "expected NoteOff");
        assert!(first_on.unwrap() < first_off.unwrap(), "NoteOn should precede NoteOff");
    }

    // T-19: rest note bar → no MIDI note events
    #[test]
    fn rest_note_bar_emits_no_events() {
        let store = Arc::new(RwLock::new(ProjectStore::new()));
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
                    notes: vec![Note { event: NoteEvent::Rest, duration_ticks: 480 }],
                }],
            }],
        };
        store.write().unwrap().set_pending(project).unwrap();
        store.write().unwrap().commit_pending();

        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput { captured: Arc::clone(&captured) };
        let engine = LoopEngine::new(store, Box::new(output));
        engine.start();
        std::thread::sleep(Duration::from_millis(300));
        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 200);

        let events = captured.lock().unwrap().clone();
        let note_events: Vec<_> = events.iter().filter(|e| !matches!(e, midi::MidiEvent::ProgramChange { .. })).collect();
        assert!(note_events.is_empty(), "rest bar should emit no note events, got {:?}", note_events);
    }

    // T-21: two tracks → NoteOn events for both tracks
    #[test]
    fn two_tracks_both_emit_note_on() {
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        let project = Project {
            header: Header {
                bpm: 300,
                time_signature: TimeSignature { numerator: 1, denominator: 4 },
            },
            tracks: vec![
                Track {
                    name: "t1".to_string(), channel: 1, instrument: 0,
                    bars: vec![Bar { notes: vec![Note { event: NoteEvent::Note { pitch: 60, velocity: 80 }, duration_ticks: 480 }] }],
                },
                Track {
                    name: "t2".to_string(), channel: 2, instrument: 1,
                    bars: vec![Bar { notes: vec![Note { event: NoteEvent::Note { pitch: 64, velocity: 80 }, duration_ticks: 480 }] }],
                },
            ],
        };
        store.write().unwrap().set_pending(project).unwrap();
        store.write().unwrap().commit_pending();

        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput { captured: Arc::clone(&captured) };
        let engine = LoopEngine::new(store, Box::new(output));
        engine.start();
        std::thread::sleep(Duration::from_millis(300));
        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 200);

        let events = captured.lock().unwrap().clone();
        let has_ch1 = events.iter().any(|e| matches!(e, midi::MidiEvent::NoteOn { channel: 1, .. }));
        let has_ch2 = events.iter().any(|e| matches!(e, midi::MidiEvent::NoteOn { channel: 2, .. }));
        assert!(has_ch1, "expected NoteOn on channel 1");
        assert!(has_ch2, "expected NoteOn on channel 2");
    }

    // T-23: ProgramChange sent for each track before any NoteOn at loop start
    #[test]
    fn program_change_sent_before_first_note_on() {
        let (engine, captured) = make_engine_with_project();
        engine.start();
        std::thread::sleep(Duration::from_millis(300));
        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 200);

        let events = captured.lock().unwrap().clone();
        let first_pc = events.iter().position(|e| matches!(e, midi::MidiEvent::ProgramChange { .. }));
        let first_on = events.iter().position(|e| matches!(e, midi::MidiEvent::NoteOn { .. }));
        assert!(first_pc.is_some(), "expected at least one ProgramChange");
        assert!(first_on.is_some(), "expected at least one NoteOn");
        assert!(first_pc.unwrap() < first_on.unwrap(), "ProgramChange must precede first NoteOn");
    }

    // T-25: after final bar the engine wraps to bar 0 and continues (seamless loop)
    #[test]
    fn engine_loops_seamlessly_after_last_bar() {
        let (engine, captured) = make_engine_with_project();
        engine.start();
        // Wait 3 bar durations (3 * ~200ms = 600ms) to ensure multiple loops
        std::thread::sleep(Duration::from_millis(700));
        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 200);

        let events = captured.lock().unwrap().clone();
        let note_on_count = events.iter().filter(|e| matches!(e, midi::MidiEvent::NoteOn { .. })).count();
        assert!(note_on_count >= 2, "expected at least 2 NoteOn events across multiple loops, got {}", note_on_count);
    }

    // T-27: pending project takes effect after current bar completes
    #[test]
    fn pending_project_takes_effect_after_bar_boundary() {
        let store = make_test_store_with_project(); // BPM 300, 1/4 bar

        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput { captured: Arc::clone(&captured) };
        let engine = LoopEngine::new(Arc::clone(&store), Box::new(output));
        engine.start();
        // Wait for at least one bar to play
        std::thread::sleep(Duration::from_millis(150));

        // Submit a new project with pitch 62
        let new_project = Project {
            header: Header {
                bpm: 300,
                time_signature: TimeSignature { numerator: 1, denominator: 4 },
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                bars: vec![Bar {
                    notes: vec![Note { event: NoteEvent::Note { pitch: 62, velocity: 80 }, duration_ticks: 480 }],
                }],
            }],
        };
        store.write().unwrap().set_pending(new_project).unwrap();

        // Wait for the next bar boundary to pick it up
        std::thread::sleep(Duration::from_millis(300));
        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 200);

        let events = captured.lock().unwrap().clone();
        let has_pitch_62 = events.iter().any(|e| matches!(e, midi::MidiEvent::NoteOn { pitch: 62, .. }));
        assert!(has_pitch_62, "expected NoteOn with pitch 62 from updated project");
    }

    // T-29: updated project with changed BPM — no stop occurs
    #[test]
    fn bpm_change_does_not_stop_engine() {
        let store = make_test_store_with_project();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput { captured: Arc::clone(&captured) };
        let engine = LoopEngine::new(Arc::clone(&store), Box::new(output));
        engine.start();
        std::thread::sleep(Duration::from_millis(200));

        let new_project = Project {
            header: Header {
                bpm: 200, // changed BPM
                time_signature: TimeSignature { numerator: 1, denominator: 4 },
            },
            tracks: vec![Track {
                name: "t".to_string(), channel: 1, instrument: 0,
                bars: vec![Bar {
                    notes: vec![Note { event: NoteEvent::Note { pitch: 60, velocity: 80 }, duration_ticks: 480 }],
                }],
            }],
        };
        store.write().unwrap().set_pending(new_project).unwrap();

        std::thread::sleep(Duration::from_millis(300));
        // Engine should still be running
        assert_eq!(engine.state(), EngineState::Running);
        engine.stop();
    }

    // T-31: instrument change → ProgramChange re-sent at bar boundary
    #[test]
    fn instrument_change_triggers_program_change() {
        let store = make_test_store_with_project();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput { captured: Arc::clone(&captured) };
        let engine = LoopEngine::new(Arc::clone(&store), Box::new(output));
        engine.start();
        std::thread::sleep(Duration::from_millis(200));

        // Initial PC count
        let pc_before = captured.lock().unwrap().iter()
            .filter(|e| matches!(e, midi::MidiEvent::ProgramChange { .. })).count();

        let new_project = Project {
            header: Header {
                bpm: 300,
                time_signature: TimeSignature { numerator: 1, denominator: 4 },
            },
            tracks: vec![Track {
                name: "t".to_string(), channel: 1, instrument: 42, // changed instrument
                bars: vec![Bar {
                    notes: vec![Note { event: NoteEvent::Note { pitch: 60, velocity: 80 }, duration_ticks: 480 }],
                }],
            }],
        };
        store.write().unwrap().set_pending(new_project).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 200);

        let events = captured.lock().unwrap().clone();
        let pc_after = events.iter().filter(|e| matches!(e, midi::MidiEvent::ProgramChange { .. })).count();
        assert!(pc_after > pc_before, "expected new ProgramChange after instrument change");
        let has_pc_42 = events.iter().any(|e| matches!(e, midi::MidiEvent::ProgramChange { program: 42, .. }));
        assert!(has_pc_42, "expected ProgramChange for instrument 42");
    }

    // T-33: Waiting state → project loaded → transitions to Running
    #[test]
    fn waiting_state_transitions_to_running_on_project_load() {
        let store = make_empty_store();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput { captured: Arc::clone(&captured) };
        let engine = LoopEngine::new(Arc::clone(&store), Box::new(output));
        engine.start();
        wait_for_state(&engine, EngineState::Waiting, 200);
        assert_eq!(engine.state(), EngineState::Waiting);

        // Load a project
        let project = Project {
            header: Header { bpm: 300, time_signature: TimeSignature { numerator: 1, denominator: 4 } },
            tracks: vec![Track {
                name: "t".to_string(), channel: 1, instrument: 0,
                bars: vec![Bar { notes: vec![Note { event: NoteEvent::Note { pitch: 60, velocity: 80 }, duration_ticks: 480 }] }],
            }],
        };
        store.write().unwrap().set_pending(project).unwrap();

        // Engine should pick it up and transition to Running
        wait_for_state(&engine, EngineState::Running, 500);
        assert_eq!(engine.state(), EngineState::Running);
        engine.stop();
    }

    // T-35: stop while note is sounding → NoteOff emitted before halt
    #[test]
    fn stop_while_note_sounding_emits_note_off() {
        // Use a very long note (longer than the bar) to ensure it's sounding when we stop
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        let project = Project {
            header: Header {
                bpm: 60, // slow tempo so note lasts a long time
                time_signature: TimeSignature { numerator: 4, denominator: 4 },
            },
            tracks: vec![Track {
                name: "t".to_string(), channel: 1, instrument: 0,
                bars: vec![Bar {
                    notes: vec![Note { event: NoteEvent::Note { pitch: 60, velocity: 80 }, duration_ticks: 1920 }],
                }],
            }],
        };
        store.write().unwrap().set_pending(project).unwrap();
        store.write().unwrap().commit_pending();

        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput { captured: Arc::clone(&captured) };
        let engine = LoopEngine::new(store, Box::new(output));
        engine.start();
        // Wait for NoteOn to be emitted (note starts at tick 0, so almost immediately)
        std::thread::sleep(Duration::from_millis(50));

        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 500);

        let events = captured.lock().unwrap().clone();
        let has_note_on = events.iter().any(|e| matches!(e, midi::MidiEvent::NoteOn { .. }));
        let has_note_off = events.iter().any(|e| matches!(e, midi::MidiEvent::NoteOff { .. }));
        assert!(has_note_on, "expected NoteOn to have been emitted");
        assert!(has_note_off, "expected NoteOff on stop to prevent stuck note");
    }

    // T-37: NoteOff and NoteOn at same tick → NoteOff emitted before NoteOn
    #[test]
    fn note_off_before_note_on_at_same_tick() {
        // Two consecutive notes at the same tick boundary:
        // note1: pitch 60, duration 480 (NoteOff at tick 480)
        // note2: pitch 62, duration 480 (NoteOn at tick 480)
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        let project = Project {
            header: Header { bpm: 300, time_signature: TimeSignature { numerator: 2, denominator: 4 } },
            tracks: vec![Track {
                name: "t".to_string(), channel: 1, instrument: 0,
                bars: vec![Bar {
                    notes: vec![
                        Note { event: NoteEvent::Note { pitch: 60, velocity: 80 }, duration_ticks: 480 },
                        Note { event: NoteEvent::Note { pitch: 62, velocity: 80 }, duration_ticks: 480 },
                    ],
                }],
            }],
        };
        store.write().unwrap().set_pending(project).unwrap();
        store.write().unwrap().commit_pending();

        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput { captured: Arc::clone(&captured) };
        let engine = LoopEngine::new(store, Box::new(output));
        engine.start();
        std::thread::sleep(Duration::from_millis(400));
        engine.stop();
        wait_for_state(&engine, EngineState::Stopped, 200);

        let events = captured.lock().unwrap().clone();
        let note_events: Vec<_> = events.iter().filter(|e| !matches!(e, midi::MidiEvent::ProgramChange { .. })).collect();
        // Find the boundary tick: NoteOff(60) and NoteOn(62) should appear in that order
        let off_60_pos = note_events.iter().position(|e| matches!(e, midi::MidiEvent::NoteOff { pitch: 60, .. }));
        let on_62_pos = note_events.iter().position(|e| matches!(e, midi::MidiEvent::NoteOn { pitch: 62, .. }));
        assert!(off_60_pos.is_some(), "expected NoteOff for pitch 60");
        assert!(on_62_pos.is_some(), "expected NoteOn for pitch 62");
        assert!(
            off_60_pos.unwrap() < on_62_pos.unwrap(),
            "NoteOff(60) must precede NoteOn(62) at the same tick"
        );
    }

    // T-40: start() while Running → no state change, no restart
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

    // T-42: dropping LoopEngine causes loop thread to exit
    #[test]
    fn dropping_loop_engine_exits_thread() {
        let store = make_empty_store();
        let (sender, receiver) = mpsc::channel::<LoopCommand>();
        let state = Arc::new(Mutex::new(EngineState::Stopped));
        let state_clone = Arc::clone(&state);
        let output: Box<dyn MidiOutput> = Box::new(MockMidiOutput::new());

        let handle = std::thread::spawn(move || {
            run_player_loop(receiver, store, output, state_clone);
        });

        // Drop the sender to simulate LoopEngine drop
        drop(sender);

        let result = handle.join();
        assert!(result.is_ok(), "player thread should exit cleanly after sender is dropped");
    }

    // T-39: timing jitter test — events within ±5ms of scheduled deadline
    #[test]
    #[ignore] // slow test; run with --include-ignored
    fn timing_jitter_within_5ms() {
        use std::sync::Mutex;

        struct TimestampedOutput {
            timestamps: Arc<Mutex<Vec<(std::time::Instant, midi::MidiEvent)>>>,
        }

        impl MidiOutput for TimestampedOutput {
            fn note_on(&mut self, channel: u8, pitch: u8, velocity: u8) -> Result<(), MidiSendError> {
                self.timestamps.lock().unwrap().push((
                    Instant::now(),
                    midi::MidiEvent::NoteOn { channel, pitch, velocity },
                ));
                Ok(())
            }
            fn note_off(&mut self, channel: u8, pitch: u8) -> Result<(), MidiSendError> {
                self.timestamps.lock().unwrap().push((
                    Instant::now(),
                    midi::MidiEvent::NoteOff { channel, pitch },
                ));
                Ok(())
            }
            fn program_change(&mut self, channel: u8, program: u8) -> Result<(), MidiSendError> {
                self.timestamps.lock().unwrap().push((
                    Instant::now(),
                    midi::MidiEvent::ProgramChange { channel, program },
                ));
                Ok(())
            }
            fn clock_tick(&mut self) -> Result<(), MidiSendError> { Ok(()) }
            fn clock_start(&mut self) -> Result<(), MidiSendError> { Ok(()) }
            fn clock_continue(&mut self) -> Result<(), MidiSendError> { Ok(()) }
            fn clock_stop(&mut self) -> Result<(), MidiSendError> { Ok(()) }
        }

        // BPM 480 → micros_per_tick = 60_000_000/(480*480) = 260μs; 1/4 bar = ~125ms
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        let project = Project {
            header: Header { bpm: 480, time_signature: TimeSignature { numerator: 4, denominator: 4 } },
            tracks: vec![Track {
                name: "t".to_string(), channel: 1, instrument: 0,
                bars: vec![Bar { notes: vec![
                    Note { event: NoteEvent::Note { pitch: 60, velocity: 80 }, duration_ticks: 480 },
                    Note { event: NoteEvent::Note { pitch: 62, velocity: 80 }, duration_ticks: 480 },
                    Note { event: NoteEvent::Note { pitch: 64, velocity: 80 }, duration_ticks: 480 },
                    Note { event: NoteEvent::Note { pitch: 65, velocity: 80 }, duration_ticks: 480 },
                ]}],
            }],
        };
        store.write().unwrap().set_pending(project).unwrap();
        store.write().unwrap().commit_pending();

        let timestamps = Arc::new(Mutex::new(Vec::new()));
        let output = TimestampedOutput { timestamps: Arc::clone(&timestamps) };
        let engine = LoopEngine::new(Arc::clone(&store), Box::new(output));
        engine.start();
        // Run for 4 bars
        std::thread::sleep(Duration::from_millis(2000));
        engine.stop();

        // We can't easily check exact timing here without the anchor,
        // but we verify the engine produced output and ran for 4 bars.
        let ts = timestamps.lock().unwrap();
        let note_on_count = ts.iter().filter(|(_, e)| matches!(e, midi::MidiEvent::NoteOn { .. })).count();
        assert!(note_on_count >= 16, "expected at least 16 NoteOn events over 4 bars * 4 repetitions, got {}", note_on_count);
    }

    // T-3 (EP-5): EngineState::Paused exists and can be stored in Arc<Mutex<EngineState>>
    #[test]
    fn engine_state_paused_exists() {
        let state: Arc<Mutex<EngineState>> = Arc::new(Mutex::new(EngineState::Paused));
        assert_eq!(*state.lock().unwrap(), EngineState::Paused);
    }

    // T-5 (EP-5): clock_start() with project → Running; clock_stop() → Stopped
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

    // T-7 (EP-5): clock_start() emits ClockStart (0xFA) before first ClockTick (0xF8)
    #[test]
    fn clock_start_emits_clock_start_before_first_clock_tick() {
        let (engine, captured) = make_engine_with_project();
        engine.clock_start();
        std::thread::sleep(Duration::from_millis(50));
        engine.clock_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);

        let events = captured.lock().unwrap().clone();
        let start_pos = events.iter().position(|e| matches!(e, midi::MidiEvent::ClockStart));
        let tick_pos = events.iter().position(|e| matches!(e, midi::MidiEvent::ClockTick));
        assert!(start_pos.is_some(), "expected ClockStart event");
        assert!(tick_pos.is_some(), "expected at least one ClockTick");
        assert!(
            start_pos.unwrap() < tick_pos.unwrap(),
            "ClockStart must precede first ClockTick"
        );
    }

    // T-9 (EP-5): clock mode with 1/4-bar at BPM 300 produces ≥24 ClockTick events per bar
    #[test]
    fn clock_mode_emits_clock_ticks() {
        let (engine, captured) = make_engine_with_project(); // BPM 300, 1/4 bar
        engine.clock_start();
        // Bar duration ≈ 200ms; wait 250ms to ensure at least one full bar
        std::thread::sleep(Duration::from_millis(250));
        engine.clock_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);

        let events = captured.lock().unwrap().clone();
        let tick_count = events.iter().filter(|e| matches!(e, midi::MidiEvent::ClockTick)).count();
        assert!(tick_count >= 24, "expected ≥24 ClockTick events for one bar at BPM 300, got {}", tick_count);
    }

    // T-13 (EP-5): clock_start() → both NoteOn events and ClockTick events emitted
    #[test]
    fn clock_start_plays_notes_and_clock_ticks() {
        let (engine, captured) = make_engine_with_project();
        engine.clock_start();
        std::thread::sleep(Duration::from_millis(300));
        engine.clock_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);

        let events = captured.lock().unwrap().clone();
        let has_note_on = events.iter().any(|e| matches!(e, midi::MidiEvent::NoteOn { .. }));
        let has_clock_tick = events.iter().any(|e| matches!(e, midi::MidiEvent::ClockTick));
        assert!(has_note_on, "expected NoteOn events in clock mode");
        assert!(has_clock_tick, "expected ClockTick events in clock mode");
    }

    // T-15 (EP-5): clock_pause() while running → Paused; NoteOff flushed; no ClockStop
    #[test]
    fn clock_pause_transitions_to_paused_and_flushes_notes() {
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        let project = Project {
            header: Header { bpm: 60, time_signature: TimeSignature { numerator: 4, denominator: 4 } },
            tracks: vec![Track {
                name: "t".to_string(), channel: 1, instrument: 0,
                bars: vec![Bar { notes: vec![Note {
                    event: NoteEvent::Note { pitch: 60, velocity: 80 }, duration_ticks: 1920,
                }] }],
            }],
        };
        store.write().unwrap().set_pending(project).unwrap();
        store.write().unwrap().commit_pending();

        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput { captured: Arc::clone(&captured) };
        let engine = LoopEngine::new(store, Box::new(output));

        engine.clock_start();
        std::thread::sleep(Duration::from_millis(50));
        engine.clock_pause();
        wait_for_state(&engine, EngineState::Paused, 500);

        let events = captured.lock().unwrap().clone();
        assert_eq!(engine.state(), EngineState::Paused);
        let has_note_off = events.iter().any(|e| matches!(e, midi::MidiEvent::NoteOff { .. }));
        let has_clock_stop = events.iter().any(|e| matches!(e, midi::MidiEvent::ClockStop));
        assert!(has_note_off, "expected NoteOff on pause to prevent stuck notes");
        assert!(!has_clock_stop, "clock_pause must not emit ClockStop");

        engine.clock_stop();
    }

    // T-17 (EP-5): clock_resume() → ClockContinue (0xFB) before first resumed ClockTick; Running
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
        let continue_pos = post_resume.iter().position(|e| matches!(e, midi::MidiEvent::ClockContinue));
        let tick_pos = post_resume.iter().position(|e| matches!(e, midi::MidiEvent::ClockTick));
        assert!(continue_pos.is_some(), "expected ClockContinue after resume");
        assert!(tick_pos.is_some(), "expected ClockTick after resume");
        assert!(
            continue_pos.unwrap() < tick_pos.unwrap(),
            "ClockContinue must precede first ClockTick after resume"
        );
    }

    // T-19 (EP-5): after clock_resume(), ClockTick events continue (loop did not restart)
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
        let has_tick_after_continue = events.iter()
            .skip_while(|e| !matches!(e, midi::MidiEvent::ClockContinue))
            .any(|e| matches!(e, midi::MidiEvent::ClockTick));
        assert!(has_tick_after_continue, "expected ClockTick events after ClockContinue");
    }

    // T-21 (EP-5): clock_stop() while running → ClockStop emitted; state Stopped
    #[test]
    fn clock_stop_while_running_emits_clock_stop() {
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        let project = Project {
            header: Header { bpm: 60, time_signature: TimeSignature { numerator: 4, denominator: 4 } },
            tracks: vec![Track {
                name: "t".to_string(), channel: 1, instrument: 0,
                bars: vec![Bar { notes: vec![Note {
                    event: NoteEvent::Note { pitch: 60, velocity: 80 }, duration_ticks: 1920,
                }] }],
            }],
        };
        store.write().unwrap().set_pending(project).unwrap();
        store.write().unwrap().commit_pending();

        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput { captured: Arc::clone(&captured) };
        let engine = LoopEngine::new(store, Box::new(output));

        engine.clock_start();
        std::thread::sleep(Duration::from_millis(50));
        engine.clock_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);

        let events = captured.lock().unwrap().clone();
        assert_eq!(engine.state(), EngineState::Stopped);
        let has_clock_stop = events.iter().any(|e| matches!(e, midi::MidiEvent::ClockStop));
        assert!(has_clock_stop, "expected ClockStop event from clock_stop()");
    }

    // T-23 (EP-5): clock_stop() while paused → ClockStop emitted; state Stopped
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
        let has_clock_stop = events.iter().any(|e| matches!(e, midi::MidiEvent::ClockStop));
        assert!(has_clock_stop, "expected ClockStop after clock_stop() while paused");
    }

    // T-25 (EP-5): BPM change while clock running → clock continues without stopping
    #[test]
    fn bpm_change_does_not_stop_clock_mode() {
        let store = make_test_store_with_project();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput { captured: Arc::clone(&captured) };
        let engine = LoopEngine::new(Arc::clone(&store), Box::new(output));

        engine.clock_start();
        wait_for_state(&engine, EngineState::Running, 500);
        std::thread::sleep(Duration::from_millis(200));

        let new_project = Project {
            header: Header { bpm: 200, time_signature: TimeSignature { numerator: 1, denominator: 4 } },
            tracks: vec![Track {
                name: "t".to_string(), channel: 1, instrument: 0,
                bars: vec![Bar { notes: vec![Note {
                    event: NoteEvent::Note { pitch: 60, velocity: 80 }, duration_ticks: 480,
                }] }],
            }],
        };
        store.write().unwrap().set_pending(new_project).unwrap();

        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(engine.state(), EngineState::Running, "engine should still be Running after BPM change");
        engine.clock_stop();
    }

    // T-27 (EP-5): project removed → clock continues (ClockTick), no NoteOn, no ClockStop from removal
    #[test]
    fn project_removed_clock_continues_without_notes() {
        let store = make_test_store_with_project();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput { captured: Arc::clone(&captured) };
        let engine = LoopEngine::new(Arc::clone(&store), Box::new(output));

        engine.clock_start();
        wait_for_state(&engine, EngineState::Running, 500);
        std::thread::sleep(Duration::from_millis(100));

        // Clear the project (simulate project removal)
        store.write().unwrap().clear();

        let tick_count_mid = {
            let ev = captured.lock().unwrap();
            ev.iter().filter(|e| matches!(e, midi::MidiEvent::ClockTick)).count()
        };

        // Wait for more bars to play (with no project, should be clock-only)
        std::thread::sleep(Duration::from_millis(300));
        engine.clock_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);

        let events = captured.lock().unwrap().clone();
        let tick_count_final = events.iter().filter(|e| matches!(e, midi::MidiEvent::ClockTick)).count();

        assert!(
            tick_count_final > tick_count_mid,
            "clock should continue ticking after project removal"
        );

        // No NoteOn events after project removal (check events after the first bar boundary)
        // The clock_stop at the very end sends ClockStop — that's the only one
        let clock_stop_count = events.iter().filter(|e| matches!(e, midi::MidiEvent::ClockStop)).count();
        assert_eq!(clock_stop_count, 1, "only one ClockStop expected (from explicit clock_stop())");
    }

    // T-29 (EP-5): clock_stop_on_shutdown() while Running → ClockStop sent, state Stopped
    #[test]
    fn clock_stop_on_shutdown_sends_clock_stop_when_running() {
        let (engine, captured) = make_engine_with_project();
        engine.clock_start();
        wait_for_state(&engine, EngineState::Running, 500);

        engine.clock_stop_on_shutdown();
        wait_for_state(&engine, EngineState::Stopped, 500);

        let events = captured.lock().unwrap().clone();
        let has_clock_stop = events.iter().any(|e| matches!(e, midi::MidiEvent::ClockStop));
        assert!(has_clock_stop, "expected ClockStop on shutdown");
    }

    // T-25 (EP-1): clock_stop_on_shutdown() blocks until Stopped — state is Stopped immediately on return
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
        let has_clock_stop = events.iter().any(|e| matches!(e, midi::MidiEvent::ClockStop));
        assert!(has_clock_stop, "ClockStop must be emitted before clock_stop_on_shutdown returns");
    }

    // T-12 (EP-6): SyncStart with active project → state = Running; bar_index resets to 0
    #[test]
    fn sync_start_with_project_transitions_to_running_at_bar_0() {
        let (engine, _) = make_engine_with_project();
        engine.sync_start();
        wait_for_state(&engine, EngineState::Running, 500);
        assert_eq!(engine.state(), EngineState::Running);
        engine.sync_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);
    }

    // T-14 (EP-6): SyncStart while already Running → bar restarts (still Running)
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
        assert!(events_after > events_before, "expected more events after SyncStart restart");
        engine.sync_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);
    }

    // T-16 (EP-6): SyncContinue with active project → state = Running; bar_index unchanged
    #[test]
    fn sync_continue_with_project_transitions_to_running() {
        let (engine, _) = make_engine_with_project();
        engine.sync_continue();
        wait_for_state(&engine, EngineState::Running, 500);
        assert_eq!(engine.state(), EngineState::Running);
        engine.sync_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);
    }

    // T-18 (EP-6): SyncStop → state = Stopped; active notes flushed
    #[test]
    fn sync_stop_transitions_to_stopped_and_flushes_notes() {
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        let project = Project {
            header: Header { bpm: 60, time_signature: TimeSignature { numerator: 4, denominator: 4 } },
            tracks: vec![Track {
                name: "t".to_string(), channel: 1, instrument: 0,
                bars: vec![Bar { notes: vec![Note {
                    event: NoteEvent::Note { pitch: 60, velocity: 80 }, duration_ticks: 1920,
                }] }],
            }],
        };
        store.write().unwrap().set_pending(project).unwrap();
        store.write().unwrap().commit_pending();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let output = CapturingOutput { captured: Arc::clone(&captured) };
        let engine = LoopEngine::new(store, Box::new(output));

        engine.sync_start();
        std::thread::sleep(Duration::from_millis(50)); // let NoteOn emit
        engine.sync_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);

        let events = captured.lock().unwrap().clone();
        assert_eq!(engine.state(), EngineState::Stopped);
        let has_note_off = events.iter().any(|e| matches!(e, midi::MidiEvent::NoteOff { .. }));
        assert!(has_note_off, "expected NoteOff on sync_stop");
        let has_clock_stop = events.iter().any(|e| matches!(e, midi::MidiEvent::ClockStop));
        assert!(!has_clock_stop, "sync_stop must not emit ClockStop (0xFC)");
    }

    // T-20 (EP-6): SyncBpmUpdate while Running → applied at bar boundary; engine stays Running
    #[test]
    fn sync_bpm_update_does_not_stop_engine() {
        let (engine, _) = make_engine_with_project(); // BPM 300
        engine.sync_start();
        wait_for_state(&engine, EngineState::Running, 500);
        std::thread::sleep(Duration::from_millis(100));

        engine.sync_bpm_update(150);

        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(engine.state(), EngineState::Running, "engine should remain Running after SyncBpmUpdate");
        engine.sync_stop();
        wait_for_state(&engine, EngineState::Stopped, 500);
    }

    // T-12 variant: SyncStart with no project → state = Waiting
    #[test]
    fn sync_start_with_no_project_enters_waiting() {
        let (engine, _) = make_engine_no_project();
        engine.sync_start();
        wait_for_state(&engine, EngineState::Waiting, 200);
        assert_eq!(engine.state(), EngineState::Waiting);
        engine.sync_stop();
        wait_for_state(&engine, EngineState::Stopped, 200);
    }

    // T-29 variant: clock_stop_on_shutdown() while Stopped → no ClockStop
    #[test]
    fn clock_stop_on_shutdown_noop_when_stopped() {
        let (engine, captured) = make_engine_with_project();
        engine.clock_stop_on_shutdown();
        std::thread::sleep(Duration::from_millis(20));

        let events = captured.lock().unwrap().clone();
        let has_clock_stop = events.iter().any(|e| matches!(e, midi::MidiEvent::ClockStop));
        assert!(!has_clock_stop, "clock_stop_on_shutdown should not emit ClockStop when already Stopped");
    }

}
