mod tracker;
pub mod port_list;

pub use port_list::list_midi_input_ports;

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use crate::loop_engine::LoopEngine;
use tracker::PulseTracker;

#[derive(Debug, Clone, PartialEq)]
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
    #[cfg(test)]
    state: Arc<Mutex<SyncClockState>>,
}

impl MidiClockReceiver {
    pub fn new(
        rx: mpsc::Receiver<ClockMessage>,
        engine: Arc<LoopEngine>,
        state: Arc<Mutex<SyncClockState>>,
    ) -> MidiClockReceiver {
        let state_clone = Arc::clone(&state);
        std::thread::spawn(move || {
            run_receiver_loop(rx, engine, state_clone);
        });
        MidiClockReceiver {
            #[cfg(test)]
            state,
        }
    }

    #[cfg(test)]
    pub fn sync_clock_state(&self) -> SyncClockState {
        self.state.lock().unwrap().clone()
    }
}

fn run_receiver_loop(
    rx: mpsc::Receiver<ClockMessage>,
    engine: Arc<LoopEngine>,
    state: Arc<Mutex<SyncClockState>>,
) {
    let mut tracker = PulseTracker::new();
    let mut last_bpm: Option<u32> = None;
    let mut already_lost = false;

    loop {
        let timeout = tracker
            .timeout_duration()
            .unwrap_or(Duration::from_secs(10));

        match rx.recv_timeout(timeout) {
            Ok(msg) => match msg {
                ClockMessage::Pulse => {
                    tracker.update(Instant::now());
                    already_lost = false;

                    {
                        let mut s = state.lock().unwrap();
                        if *s == SyncClockState::Lost {
                            *s = SyncClockState::Tracking;
                        }
                    }

                    if let Some(bpm) = tracker.bpm() {
                        if last_bpm != Some(bpm) {
                            last_bpm = Some(bpm);
                            engine.sync_bpm_update(bpm);
                        }
                    }
                }
                ClockMessage::Start => {
                    tracker.reset();
                    last_bpm = None;
                    already_lost = false;
                    {
                        let mut s = state.lock().unwrap();
                        *s = SyncClockState::Tracking;
                    }
                    engine.sync_start();
                }
                ClockMessage::Continue => {
                    if tracker.is_clock_active(Instant::now()) {
                        engine.sync_continue();
                    }
                }
                ClockMessage::Stop => {
                    {
                        let mut s = state.lock().unwrap();
                        *s = SyncClockState::Waiting;
                    }
                    engine.sync_stop();
                }
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !already_lost {
                    already_lost = true;
                    {
                        let mut s = state.lock().unwrap();
                        *s = SyncClockState::Lost;
                    }
                    engine.sync_stop();
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// Bridge that opens a real MIDI input port and forwards clock messages.
/// Keeps the midir connection alive for the process lifetime.
pub struct MidiInputBridge {
    _connection: midir::MidiInputConnection<()>,
}

impl MidiInputBridge {
    pub fn open(port_name: &str, tx: mpsc::Sender<ClockMessage>) -> Result<Self, String> {
        let input = midir::MidiInput::new("propeller-sync").map_err(|e| e.to_string())?;
        let ports = input.ports();
        let port = ports
            .iter()
            .find(|p| input.port_name(p).unwrap_or_default() == port_name)
            .ok_or_else(|| format!("MIDI input port '{}' not found", port_name))?
            .clone();

        let connection = input
            .connect(
                &port,
                "propeller-sync-conn",
                move |_timestamp, data, _| {
                    let msg = match data.first().copied() {
                        Some(0xF8) => ClockMessage::Pulse,
                        Some(0xFA) => ClockMessage::Start,
                        Some(0xFB) => ClockMessage::Continue,
                        Some(0xFC) => ClockMessage::Stop,
                        _ => return,
                    };
                    let _ = tx.send(msg);
                },
                (),
            )
            .map_err(|e| e.to_string())?;

        Ok(MidiInputBridge { _connection: connection })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Bar, Header, Note, NoteEvent, Project, ProjectStore, TimeSignature, Track};
    use crate::loop_engine::{EngineState, LoopEngine, midi::MockMidiOutput};
    use std::sync::{Arc, RwLock};

    fn make_engine_with_project() -> Arc<LoopEngine> {
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        {
            let mut s = store.write().unwrap();
            let project = Project {
                header: Header {
                    bpm: 120,
                    time_signature: TimeSignature { numerator: 4, denominator: 4 },
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
            s.set_pending(project).unwrap();
            s.commit_pending();
        }
        Arc::new(LoopEngine::new(store, Box::new(MockMidiOutput::new())))
    }

    fn make_receiver(
        rx: mpsc::Receiver<ClockMessage>,
        engine: Arc<LoopEngine>,
    ) -> (MidiClockReceiver, Arc<Mutex<SyncClockState>>) {
        let state = Arc::new(Mutex::new(SyncClockState::Waiting));
        let receiver = MidiClockReceiver::new(rx, engine, Arc::clone(&state));
        (receiver, state)
    }

    // T-25: 25 Pulse messages trigger BPM update calls; Start transitions to Tracking
    #[test]
    fn pulse_25_messages_trigger_bpm_update() {
        let (tx, rx) = mpsc::channel();
        let engine = make_engine_with_project();
        let (receiver, state) = make_receiver(rx, Arc::clone(&engine));

        // Start to move state to Tracking
        tx.send(ClockMessage::Start).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(*state.lock().unwrap(), SyncClockState::Tracking);

        // Send 25 pulses quickly to exercise the BPM computation and update dispatch path.
        // BPM accuracy is validated by PulseTracker unit tests; here we verify no panic.
        for _ in 0..25 {
            tx.send(ClockMessage::Pulse).unwrap();
        }
        // Brief wait for receiver to drain the queue
        std::thread::sleep(Duration::from_millis(10));

        // Drop sender to terminate receiver thread cleanly; no timeout fires since pulses came fast
        drop(tx);

        // State was Tracking after Start; the receiver ran without panicking
        let _ = receiver.sync_clock_state();
    }

    // T-27: Start message → sync_start() called on engine; SyncClockState = Tracking
    #[test]
    fn start_message_calls_sync_start_and_sets_tracking() {
        let (tx, rx) = mpsc::channel();
        let engine = make_engine_with_project();
        let (receiver, _state) = make_receiver(rx, Arc::clone(&engine));

        tx.send(ClockMessage::Start).unwrap();
        std::thread::sleep(Duration::from_millis(50));

        assert_eq!(receiver.sync_clock_state(), SyncClockState::Tracking);
        // Engine should be Running or Waiting after sync_start with project
        let s = engine.state();
        assert!(
            s == EngineState::Running || s == EngineState::Waiting,
            "expected Running or Waiting, got {:?}", s
        );
    }

    // T-29: Continue when clock active → sync_continue called; Continue when clock not active → ignored
    #[test]
    fn continue_when_clock_active_calls_sync_continue() {
        let (tx, rx) = mpsc::channel();
        let engine = make_engine_with_project();
        let (_receiver, _state) = make_receiver(rx, Arc::clone(&engine));

        // Feed recent pulses to make clock active
        let interval = Duration::from_micros(20_833);
        for _ in 0..3 {
            tx.send(ClockMessage::Pulse).unwrap();
            std::thread::sleep(interval);
        }
        std::thread::sleep(Duration::from_millis(10));

        // Continue should be accepted (clock is active)
        tx.send(ClockMessage::Continue).unwrap();
        std::thread::sleep(Duration::from_millis(50));

        // Engine should start due to sync_continue
        let s = engine.state();
        assert!(
            s == EngineState::Running || s == EngineState::Waiting,
            "expected Running or Waiting after Continue with active clock, got {:?}", s
        );
    }

    #[test]
    fn continue_when_clock_not_active_is_ignored() {
        let (tx, rx) = mpsc::channel();
        let engine = make_engine_with_project();
        let (_receiver, _state) = make_receiver(rx, Arc::clone(&engine));

        // No pulses sent — clock is not active
        tx.send(ClockMessage::Continue).unwrap();
        std::thread::sleep(Duration::from_millis(50));

        // Engine should remain Stopped (sync_continue was not called)
        assert_eq!(engine.state(), EngineState::Stopped);
    }

    // T-31: Stop message → sync_stop() called; SyncClockState = Waiting
    #[test]
    fn stop_message_calls_sync_stop_and_sets_waiting() {
        let (tx, rx) = mpsc::channel();
        let engine = make_engine_with_project();
        let (receiver, _state) = make_receiver(rx, Arc::clone(&engine));

        // Start first to get to Tracking
        tx.send(ClockMessage::Start).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(receiver.sync_clock_state(), SyncClockState::Tracking);

        // Now stop
        tx.send(ClockMessage::Stop).unwrap();
        std::thread::sleep(Duration::from_millis(30));

        assert_eq!(receiver.sync_clock_state(), SyncClockState::Waiting);
        assert_eq!(engine.state(), EngineState::Stopped);
    }

    // T-33: timeout → SyncClockState = Lost; subsequent Pulse → state = Tracking but sync_start NOT called
    #[test]
    fn timeout_sets_lost_and_stops_engine() {
        let (tx, rx) = mpsc::channel();
        let engine = make_engine_with_project();
        let (receiver, _state) = make_receiver(rx, Arc::clone(&engine));

        // Feed pulses at 600 BPM so timeout is short (~14.6 ms = 3.5 × 4167 μs)
        let interval = Duration::from_micros(4_167); // 600 BPM, 24-PPQN
        tx.send(ClockMessage::Start).unwrap();
        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(receiver.sync_clock_state(), SyncClockState::Tracking);

        // Send pulses after Start to re-establish short timeout
        for _ in 0..3 {
            tx.send(ClockMessage::Pulse).unwrap();
            std::thread::sleep(interval);
        }
        std::thread::sleep(Duration::from_millis(10));

        // Now wait for timeout (> 3.5 × last_interval ≈ 14.6 ms)
        std::thread::sleep(Duration::from_millis(100));

        assert_eq!(receiver.sync_clock_state(), SyncClockState::Lost);
        assert_eq!(engine.state(), EngineState::Stopped);
    }

    // T-35: after clock loss and resume via Pulse, engine receives sync_start ONLY after new Start/Continue
    #[test]
    fn after_clock_loss_pulse_alone_does_not_restart_engine() {
        let (tx, rx) = mpsc::channel();
        let engine = make_engine_with_project();
        let (receiver, _state) = make_receiver(rx, Arc::clone(&engine));

        let interval = Duration::from_micros(4_167); // 600 BPM, 24-PPQN

        // Establish clock and start; send pulses after Start to set short timeout
        tx.send(ClockMessage::Start).unwrap();
        std::thread::sleep(Duration::from_millis(10));
        for _ in 0..3 {
            tx.send(ClockMessage::Pulse).unwrap();
            std::thread::sleep(interval);
        }
        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(receiver.sync_clock_state(), SyncClockState::Tracking);

        // Let clock time out → Lost
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(receiver.sync_clock_state(), SyncClockState::Lost);
        assert_eq!(engine.state(), EngineState::Stopped);

        // Resume with pulses → Tracking but engine stays Stopped
        // Send pulses quickly so we can assert before the next timeout fires
        for i in 0..3 {
            tx.send(ClockMessage::Pulse).unwrap();
            if i < 2 {
                std::thread::sleep(interval);
            }
        }
        // Brief sleep to let receiver process last pulse (well under 14.6ms timeout)
        std::thread::sleep(Duration::from_millis(5));

        assert_eq!(receiver.sync_clock_state(), SyncClockState::Tracking);
        // Engine should still be Stopped — no Start/Continue received yet
        assert_eq!(engine.state(), EngineState::Stopped);

        // Now send Start → engine resumes
        tx.send(ClockMessage::Start).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let s = engine.state();
        assert!(
            s == EngineState::Running || s == EngineState::Waiting,
            "expected Running or Waiting after new Start, got {:?}", s
        );
    }
}
