// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

pub mod tracker;

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use crate::loop_engine::LoopEngine;
use crate::loop_engine::midi::MidiOutput;
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
    ///
    /// `forward_output` is the same output connection the engine sends notes on. When
    /// `forwarding_enabled` is true, every recognized incoming clock message is relayed to
    /// it as a raw MIDI Thru, written directly from this input callback thread (not routed
    /// through the player loop's command channel) so downstream devices see the lowest
    /// possible added latency. When false, incoming messages are still classified and used
    /// to drive engine sync (start/stop/tempo tracking) as normal, but nothing is written to
    /// the output port — including the clock-loss Stop below.
    pub fn new(
        port_name: &str,
        engine: Arc<LoopEngine>,
        forward_output: Arc<Mutex<Box<dyn MidiOutput>>>,
        forwarding_enabled: bool,
    ) -> Result<Self, String> {
        use midir::MidiInput;
        let midi_in = MidiInput::new("propeller-sync").map_err(|e| e.to_string())?;
        let ports = midi_in.ports();
        let port = ports
            .iter()
            .find(|p| midi_in.port_name(p).as_deref() == Ok(port_name))
            .ok_or_else(|| format!("MIDI input port {:?} not found", port_name))?
            .clone();
        let (tx, rx) = mpsc::channel::<ClockMessage>();
        let forward_output_cb = Arc::clone(&forward_output);
        let conn = midi_in
            .connect(
                &port,
                "propeller-sync-input",
                move |_stamp, msg, _| {
                    let m = if forwarding_enabled {
                        let mut out = forward_output_cb.lock().unwrap();
                        forward_clock_message(msg, out.as_mut())
                    } else {
                        classify_clock_message(msg)
                    };
                    if let Some(m) = m {
                        let _ = tx.send(m);
                    }
                },
                (),
            )
            .map_err(|e| e.to_string())?;

        let state = Arc::new(Mutex::new(SyncClockState::Waiting));
        let state_clone = Arc::clone(&state);
        std::thread::spawn(move || {
            run_receiver(rx, engine, state_clone, forward_output, forwarding_enabled);
        });
        Ok(MidiClockReceiver {
            state,
            _midi_conn: Some(Box::new(conn)),
        })
    }

    /// Test constructor: caller provides the channel receiver directly.
    #[cfg(test)]
    pub fn new_for_test(
        rx: mpsc::Receiver<ClockMessage>,
        engine: Arc<LoopEngine>,
        forward_output: Arc<Mutex<Box<dyn MidiOutput>>>,
        forwarding_enabled: bool,
    ) -> Self {
        let state = Arc::new(Mutex::new(SyncClockState::Waiting));
        let state_clone = Arc::clone(&state);
        std::thread::spawn(move || {
            run_receiver(rx, engine, state_clone, forward_output, forwarding_enabled);
        });
        MidiClockReceiver {
            state,
            _midi_conn: None,
        }
    }

    #[cfg(test)]
    pub fn sync_clock_state(&self) -> SyncClockState {
        self.state.lock().unwrap().clone()
    }

    pub fn state_arc(&self) -> Arc<Mutex<SyncClockState>> {
        Arc::clone(&self.state)
    }
}

// Classifies a raw incoming MIDI Realtime byte, without touching any output.
fn classify_clock_message(bytes: &[u8]) -> Option<ClockMessage> {
    match bytes {
        [0xF8] => Some(ClockMessage::Pulse),
        [0xFA] => Some(ClockMessage::Start),
        [0xFB] => Some(ClockMessage::Continue),
        [0xFC] => Some(ClockMessage::Stop),
        _ => None,
    }
}

// Classifies a raw incoming MIDI Realtime byte and, if recognized, forwards the matching
// clock message to `output` before returning it for the engine-tracking side to consume.
fn forward_clock_message(bytes: &[u8], output: &mut dyn MidiOutput) -> Option<ClockMessage> {
    let message = classify_clock_message(bytes)?;
    let _ = match message {
        ClockMessage::Pulse => output.clock_tick(),
        ClockMessage::Start => output.clock_start(),
        ClockMessage::Continue => output.clock_continue(),
        ClockMessage::Stop => output.clock_stop(),
    };
    Some(message)
}

fn run_receiver(
    rx: mpsc::Receiver<ClockMessage>,
    engine: Arc<LoopEngine>,
    state: Arc<Mutex<SyncClockState>>,
    forward_output: Arc<Mutex<Box<dyn MidiOutput>>>,
    forwarding_enabled: bool,
) {
    let mut pulse_tracker = PulseTracker::new();
    let mut last_bpm: Option<u32> = None;

    loop {
        let timeout = pulse_tracker
            .timeout_duration()
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
                // Sequencers commonly stop emitting clock pulses while paused, so a
                // stale last-pulse timestamp does not mean the device is gone. Only
                // reject Continue when no clock was ever established at all.
                if pulse_tracker.timeout_duration().is_some() {
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
                    // Nothing was received to relay, so send an explicit Stop of our own —
                    // downstream devices chained off the output port should pause too.
                    if forwarding_enabled {
                        let _ = forward_output.lock().unwrap().clock_stop();
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Header, Note, Project, ProjectStore, Track};
    use crate::loop_engine::midi::{CapturingMidiOutput, MidiEvent, MockMidiOutput};
    use crate::loop_engine::{EngineState, LoopEngine};
    use std::sync::{Arc, RwLock};
    use std::time::Duration;

    fn mock_forward_output() -> Arc<Mutex<Box<dyn MidiOutput>>> {
        let boxed: Box<dyn MidiOutput> = Box::new(MockMidiOutput::new());
        Arc::new(Mutex::new(boxed))
    }

    fn make_engine_with_project() -> Arc<LoopEngine> {
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        {
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
                    pitch_bends: vec![],
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

    #[test]
    fn pulse_messages_transition_to_tracking() {
        let engine = make_engine_no_project();
        let (tx, rx) = mpsc::channel::<ClockMessage>();
        let receiver =
            MidiClockReceiver::new_for_test(rx, Arc::clone(&engine), mock_forward_output(), true);

        assert_eq!(receiver.sync_clock_state(), SyncClockState::Waiting);

        // Feed 25 pulses at 120 BPM intervals (20_833 μs each)
        for _ in 0..25 {
            tx.send(ClockMessage::Pulse).unwrap();
            std::thread::sleep(Duration::from_micros(20_833));
        }

        wait_for_sync_state(&receiver, SyncClockState::Tracking, 2000);
        assert_eq!(receiver.sync_clock_state(), SyncClockState::Tracking);
    }

    #[test]
    fn start_message_calls_sync_start_and_sets_tracking() {
        let engine = make_engine_with_project();
        let (tx, rx) = mpsc::channel::<ClockMessage>();
        let receiver =
            MidiClockReceiver::new_for_test(rx, Arc::clone(&engine), mock_forward_output(), true);

        tx.send(ClockMessage::Start).unwrap();

        wait_for_sync_state(&receiver, SyncClockState::Tracking, 500);
        assert_eq!(receiver.sync_clock_state(), SyncClockState::Tracking);

        // engine.sync_start() → Running (project present)
        wait_for_engine_state(&engine, EngineState::Running, 500);
        assert_eq!(engine.state(), EngineState::Running);
        engine.sync_stop();
    }

    #[test]
    fn continue_when_clock_active_calls_sync_continue() {
        let engine = make_engine_with_project();
        let (tx, rx) = mpsc::channel::<ClockMessage>();
        let receiver =
            MidiClockReceiver::new_for_test(rx, Arc::clone(&engine), mock_forward_output(), true);

        // Prime the pulse tracker with a few pulses to establish a tempo
        for _ in 0..3 {
            tx.send(ClockMessage::Pulse).unwrap();
            std::thread::sleep(Duration::from_micros(20_833));
        }
        wait_for_sync_state(&receiver, SyncClockState::Tracking, 500);

        // Now Stop then Continue shortly after
        tx.send(ClockMessage::Stop).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        tx.send(ClockMessage::Continue).unwrap();

        wait_for_engine_state(&engine, EngineState::Running, 500);
        assert_eq!(engine.state(), EngineState::Running);
        engine.sync_stop();
    }

    #[test]
    fn continue_after_long_pause_still_resumes_engine() {
        // Regression test: a sequencer that stops emitting clock pulses while
        // paused must still be able to resume playback via an explicit
        // Continue (0xFB), even though the pulse tracker's last-seen pulse is
        // long stale by the time Continue arrives.
        let engine = make_engine_with_project();
        let (tx, rx) = mpsc::channel::<ClockMessage>();
        let receiver =
            MidiClockReceiver::new_for_test(rx, Arc::clone(&engine), mock_forward_output(), true);

        // Establish tracking with a few pulses at 120 BPM (~20_833 μs interval,
        // ~73 ms clock-loss timeout).
        for _ in 0..3 {
            tx.send(ClockMessage::Pulse).unwrap();
            std::thread::sleep(Duration::from_micros(20_833));
        }
        wait_for_sync_state(&receiver, SyncClockState::Tracking, 500);

        // Sequencer pauses: sends Stop and then falls silent for well over the
        // clock-loss timeout, as many real sequencers do while paused.
        tx.send(ClockMessage::Stop).unwrap();
        wait_for_engine_state(&engine, EngineState::Stopped, 500);
        std::thread::sleep(Duration::from_millis(150));

        // User resumes playback on the sequencer: it sends Continue before any
        // new pulses arrive.
        tx.send(ClockMessage::Continue).unwrap();

        wait_for_engine_state(&engine, EngineState::Running, 500);
        assert_eq!(
            engine.state(),
            EngineState::Running,
            "Continue must resume playback even after a long pause with no pulses"
        );
        engine.sync_stop();
    }

    #[test]
    fn continue_when_clock_not_active_is_ignored() {
        let engine = make_engine_with_project();
        let (tx, rx) = mpsc::channel::<ClockMessage>();
        let receiver =
            MidiClockReceiver::new_for_test(rx, Arc::clone(&engine), mock_forward_output(), true);

        // No pulses — clock not active; send Continue
        tx.send(ClockMessage::Continue).unwrap();
        std::thread::sleep(Duration::from_millis(50));

        // Engine should stay Stopped (sync_continue not called)
        assert_eq!(engine.state(), EngineState::Stopped);
        drop(receiver);
    }

    #[test]
    fn stop_message_pauses_engine_and_sets_sync_waiting() {
        // MIDI Stop (0xFC) pauses the engine (retaining Song Position for a later
        // Continue), it does not hard-stop it. The receiver's own SyncClockState still
        // reports Waiting, since it is tracking clock activity, not playback position.
        let engine = make_engine_with_project();
        let (tx, rx) = mpsc::channel::<ClockMessage>();
        let receiver =
            MidiClockReceiver::new_for_test(rx, Arc::clone(&engine), mock_forward_output(), true);

        // Start first
        tx.send(ClockMessage::Start).unwrap();
        wait_for_engine_state(&engine, EngineState::Running, 500);

        // Then stop
        tx.send(ClockMessage::Stop).unwrap();
        wait_for_engine_state(&engine, EngineState::Paused, 500);
        wait_for_sync_state(&receiver, SyncClockState::Waiting, 500);

        assert_eq!(engine.state(), EngineState::Paused);
        assert_eq!(receiver.sync_clock_state(), SyncClockState::Waiting);
    }

    #[test]
    fn timeout_declares_clock_lost_and_pulse_resumes_tracking_without_start() {
        // Prime with very high-frequency pulses (≈30_000 BPM → 2 ms interval → 7 ms timeout)
        let engine = make_engine_with_project();
        let (tx, rx) = mpsc::channel::<ClockMessage>();
        let receiver =
            MidiClockReceiver::new_for_test(rx, Arc::clone(&engine), mock_forward_output(), true);

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
        // Engine should be Paused (sync_stop was called; position is retained for Continue)
        wait_for_engine_state(&engine, EngineState::Paused, 500);
        assert_eq!(engine.state(), EngineState::Paused);

        // Now send pulses again — state should go to Tracking but engine stays Paused
        for _ in 0..5 {
            tx.send(ClockMessage::Pulse).unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        wait_for_sync_state(&receiver, SyncClockState::Tracking, 500);
        assert_eq!(receiver.sync_clock_state(), SyncClockState::Tracking);
        // Engine must NOT have restarted (no sync_start called)
        assert_eq!(
            engine.state(),
            EngineState::Paused,
            "engine must not restart after clock recovery without Start/Continue"
        );
    }

    #[test]
    fn clock_resume_then_start_restarts_engine() {
        let engine = make_engine_with_project();
        let (tx, rx) = mpsc::channel::<ClockMessage>();
        let receiver =
            MidiClockReceiver::new_for_test(rx, Arc::clone(&engine), mock_forward_output(), true);

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

        // Resume pulses (tracking restored, engine still paused)
        for _ in 0..5 {
            tx.send(ClockMessage::Pulse).unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        wait_for_sync_state(&receiver, SyncClockState::Tracking, 500);
        assert_eq!(engine.state(), EngineState::Paused);

        // Now send Start — engine should restart
        tx.send(ClockMessage::Start).unwrap();
        wait_for_engine_state(&engine, EngineState::Running, 500);
        assert_eq!(engine.state(), EngineState::Running);
        engine.sync_stop();
    }

    #[test]
    fn forward_clock_message_dispatches_each_recognized_type() {
        let mut m = MockMidiOutput::new();
        assert!(matches!(
            forward_clock_message(&[0xF8], &mut m),
            Some(ClockMessage::Pulse)
        ));
        assert!(matches!(
            forward_clock_message(&[0xFA], &mut m),
            Some(ClockMessage::Start)
        ));
        assert!(matches!(
            forward_clock_message(&[0xFB], &mut m),
            Some(ClockMessage::Continue)
        ));
        assert!(matches!(
            forward_clock_message(&[0xFC], &mut m),
            Some(ClockMessage::Stop)
        ));
        assert_eq!(
            m.events,
            vec![
                MidiEvent::ClockTick,
                MidiEvent::ClockStart,
                MidiEvent::ClockContinue,
                MidiEvent::ClockStop,
            ]
        );
    }

    #[test]
    fn forward_clock_message_ignores_unrecognized_bytes() {
        let mut m = MockMidiOutput::new();
        assert!(forward_clock_message(&[0x90, 60, 80], &mut m).is_none());
        assert!(m.events.is_empty());
    }

    // Regression guard for the propeller-controlled part of the sub-millisecond latency
    // requirement: classifying a byte and writing it to the output must be near-instant on
    // its own. The full path also crosses two real CoreMIDI hops (external send -> our
    // input callback, our output write -> a listener), which this cannot measure; see the
    // #[ignore]-gated live test below for that.
    #[test]
    fn forward_clock_message_completes_in_well_under_a_millisecond() {
        let mut m = MockMidiOutput::new();
        for bytes in [[0xF8].as_slice(), &[0xFA], &[0xFB], &[0xFC]] {
            let start = Instant::now();
            forward_clock_message(bytes, &mut m);
            let elapsed = start.elapsed();
            assert!(
                elapsed < Duration::from_millis(1),
                "forwarding took {elapsed:?}, expected well under 1ms"
            );
        }
    }

    #[test]
    fn clock_loss_forwards_an_explicit_stop_to_the_output_port() {
        // Prime with very high-frequency pulses (≈30_000 BPM → 2 ms interval → 7 ms timeout)
        // so the clock-loss timeout fires quickly.
        let engine = make_engine_with_project();
        let (tx, rx) = mpsc::channel::<ClockMessage>();
        let events: Arc<Mutex<Vec<MidiEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let forward_output: Arc<Mutex<Box<dyn MidiOutput>>> = Arc::new(Mutex::new(Box::new(
            CapturingMidiOutput::new(Arc::clone(&events)),
        )));
        let receiver = MidiClockReceiver::new_for_test(
            rx,
            Arc::clone(&engine),
            Arc::clone(&forward_output),
            true,
        );

        tx.send(ClockMessage::Start).unwrap();
        wait_for_engine_state(&engine, EngineState::Running, 500);

        for _ in 0..25 {
            tx.send(ClockMessage::Pulse).unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        std::thread::sleep(Duration::from_millis(50));

        wait_for_sync_state(&receiver, SyncClockState::Lost, 500);

        let recorded = events.lock().unwrap().clone();
        assert!(
            recorded.contains(&MidiEvent::ClockStop),
            "expected an explicit ClockStop forwarded to the output port on clock loss, got {recorded:?}"
        );
    }

    #[test]
    fn disabled_forwarding_writes_nothing_to_the_output_port_including_on_clock_loss() {
        // Same scenario as clock_loss_forwards_an_explicit_stop_to_the_output_port, but with
        // forwarding_enabled = false: engine sync (start/stop/tempo tracking) must still work
        // normally, while the output port sees nothing at all — neither the relayed live
        // messages nor the synthesized clock-loss Stop.
        let engine = make_engine_with_project();
        let (tx, rx) = mpsc::channel::<ClockMessage>();
        let events: Arc<Mutex<Vec<MidiEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let forward_output: Arc<Mutex<Box<dyn MidiOutput>>> = Arc::new(Mutex::new(Box::new(
            CapturingMidiOutput::new(Arc::clone(&events)),
        )));
        let receiver = MidiClockReceiver::new_for_test(
            rx,
            Arc::clone(&engine),
            Arc::clone(&forward_output),
            false,
        );

        tx.send(ClockMessage::Start).unwrap();
        wait_for_engine_state(&engine, EngineState::Running, 500);

        for _ in 0..25 {
            tx.send(ClockMessage::Pulse).unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        std::thread::sleep(Duration::from_millis(50));

        wait_for_sync_state(&receiver, SyncClockState::Lost, 500);
        // Engine sync tracking is unaffected by the flag: it still paused on clock loss.
        wait_for_engine_state(&engine, EngineState::Paused, 500);
        assert_eq!(engine.state(), EngineState::Paused);

        let recorded = events.lock().unwrap().clone();
        assert!(
            recorded.is_empty(),
            "expected no output-port writes with forwarding disabled, got {recorded:?}"
        );
    }

    // Live end-to-end check using real virtual CoreMIDI ports: an incoming clock message on
    // the sync input port must show up as the matching message on the output port, with the
    // full round trip (external send -> our receive -> our forward -> listener receive)
    // under 1ms. Ignored by default since it needs a real MIDI subsystem; run with
    // `cargo test -- --ignored`.
    type ReceivedMessages = Arc<Mutex<Vec<(Instant, Vec<u8>)>>>;

    #[test]
    #[ignore]
    fn sync_receiver_forwards_clock_to_output_port_within_one_millisecond() {
        use crate::midi_port;
        use midir::os::unix::VirtualOutput;

        let seq_client = midir::MidiOutput::new("propeller-test-seq").unwrap();
        let mut seq_conn = seq_client
            .create_virtual("propeller-test-sync-src")
            .unwrap();

        let fwd_output = midi_port::open_virtual_named("propeller-test-fwd-out").unwrap();
        let forward_output: Arc<Mutex<Box<dyn MidiOutput>>> =
            Arc::new(Mutex::new(Box::new(fwd_output)));

        let received: ReceivedMessages = Arc::new(Mutex::new(Vec::new()));
        let received_clone = Arc::clone(&received);
        let listen_client = midir::MidiInput::new("propeller-test-listener").unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let listen_ports = listen_client.ports();
        let listen_port = listen_ports
            .iter()
            .find(|p| listen_client.port_name(p).unwrap_or_default() == "propeller-test-fwd-out")
            .expect("forward-output virtual port not found");
        let _listen_conn = listen_client
            .connect(
                listen_port,
                "propeller-test-listen-in",
                move |_stamp, data, _| {
                    received_clone
                        .lock()
                        .unwrap()
                        .push((Instant::now(), data.to_vec()));
                },
                (),
            )
            .unwrap();

        let engine = make_engine_no_project();

        std::thread::sleep(Duration::from_millis(50));
        let _receiver = MidiClockReceiver::new(
            "propeller-test-sync-src",
            Arc::clone(&engine),
            Arc::clone(&forward_output),
            true,
        )
        .expect("failed to start clock receiver on virtual sync port");
        std::thread::sleep(Duration::from_millis(50));

        let messages: [(&[u8], &str); 4] = [
            (&[0xFA], "Start"),
            (&[0xF8], "Tick"),
            (&[0xFB], "Continue"),
            (&[0xFC], "Stop"),
        ];

        let mut send_times = Vec::new();
        for (bytes, _label) in messages.iter() {
            send_times.push(Instant::now());
            seq_conn.send(bytes).unwrap();
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(50));

        let observed = received.lock().unwrap().clone();
        assert_eq!(
            observed.len(),
            4,
            "expected all 4 forwarded messages, got {observed:?}"
        );

        for (i, (sent_at, (observed_at, bytes))) in
            send_times.iter().zip(observed.iter()).enumerate()
        {
            let (expected_bytes, label) = messages[i];
            assert_eq!(
                bytes.as_slice(),
                expected_bytes,
                "message {i} ({label}) forwarded incorrectly"
            );
            let latency = observed_at.duration_since(*sent_at);
            assert!(
                latency < Duration::from_millis(1),
                "{label} forwarding latency was {latency:?}, expected <1ms"
            );
        }
    }
}
