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
                    engine.sync_bpm_update(bpm);
                }
            }
            Ok(ClockMessage::Start) => {
                pulse_tracker.reset();
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
    use crate::loop_engine::midi::{
        CapturingMidiOutput, MidiEvent, MockMidiOutput, SharedMidiOutput,
    };
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

    // Empirical regression check for the sync-mode drift/wobble bug: runs a real daemon
    // stack (LoopEngine + MidiClockReceiver) against a synthetic external clock device
    // over real CoreMIDI virtual ports for an extended, real-time session, and measures
    // whether propeller's actual NoteOn timing on its output port stays phase-locked to
    // the clock device's own precise send schedule — the thing a human would perceive as
    // "does the downbeat keep landing on time." Parametrized over clock-forwarding, since
    // that's a config axis the reported regression had not been isolated against.
    // Ignored by default (needs a real MIDI subsystem and ~2.5 minutes per variant); run
    // with `cargo test -- --ignored sync_mode_note_timing`.
    fn run_sync_note_timing_session(forwarding_enabled: bool, port_prefix: &str) {
        use crate::midi_port;
        use midir::os::unix::VirtualOutput;

        // Deliberately non-integer BPM: a real external clock's true tempo essentially
        // never lands on a whole number, which is exactly the case that exposed the
        // truncation bias this fix addresses.
        const BPM: f64 = 119.7;
        const PULSES_PER_QUARTER: f64 = 24.0;
        const SESSION_SECS: f64 = 150.0;

        let pulse_interval_secs = 60.0 / (BPM * PULSES_PER_QUARTER);
        let beat_interval_secs = pulse_interval_secs * PULSES_PER_QUARTER;

        let clockdev_port_name = format!("{port_prefix}-clockdev");
        let output_port_name = format!("{port_prefix}-out");

        // Synthetic clock-giving device: a virtual MIDI source we drive by hand with a
        // precise, anchor-based send schedule (never cumulative sleeps), so any drift
        // observed downstream is attributable to propeller, not to this test's own timer.
        let seq_client = midir::MidiOutput::new(&format!("{port_prefix}-seq")).unwrap();
        let mut seq_conn = seq_client.create_virtual(&clockdev_port_name).unwrap();

        // Propeller's real output port, and a listener on it capturing every NoteOn with
        // its arrival Instant.
        let output_port = midi_port::open_virtual_named(&output_port_name).unwrap();
        let output: Arc<Mutex<Box<dyn MidiOutput>>> = Arc::new(Mutex::new(Box::new(output_port)));

        let captured: Arc<Mutex<Vec<Instant>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = Arc::clone(&captured);
        let listen_client = midir::MidiInput::new(&format!("{port_prefix}-listener")).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let listen_ports = listen_client.ports();
        let listen_port = listen_ports
            .iter()
            .find(|p| listen_client.port_name(p).unwrap_or_default() == output_port_name)
            .expect("propeller output virtual port not found");
        let _listen_conn = listen_client
            .connect(
                listen_port,
                &format!("{port_prefix}-listen-in"),
                move |_stamp, data, _| {
                    // NoteOn, channel 1, velocity > 0.
                    if data.len() == 3 && data[0] == 0x90 && data[2] > 0 {
                        captured_clone.lock().unwrap().push(Instant::now());
                    }
                },
                (),
            )
            .unwrap();

        // One note per loop, loop_duration = one quarter note: this maximises how often
        // the sync-tempo-update code path (applied once per loop pass) runs relative to
        // session length, i.e. the worst case for the bug under test.
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        {
            let project = Project {
                header: Header {
                    bpm: BPM.round() as u32,
                    loop_duration: 480,
                },
                tracks: vec![Track {
                    name: "t".to_string(),
                    channel: 1,
                    instrument: 0,
                    notes: vec![Note {
                        start_tick: 0,
                        duration: 240,
                        pitch: 60,
                        velocity: 80,
                    }],
                    pitch_bends: vec![],
                }],
            };
            store.write().unwrap().set_pending(project).unwrap();
            store.write().unwrap().commit_pending();
        }
        let engine = Arc::new(LoopEngine::new(
            store,
            Box::new(SharedMidiOutput(Arc::clone(&output))),
        ));

        std::thread::sleep(Duration::from_millis(50));
        let receiver = MidiClockReceiver::new(
            &clockdev_port_name,
            Arc::clone(&engine),
            Arc::clone(&output),
            forwarding_enabled,
        )
        .expect("failed to start clock receiver on virtual sync port");
        std::thread::sleep(Duration::from_millis(50));

        // Drive the synthetic clock: Start, then steady 24-ppqn pulses on a precise
        // anchor-based schedule for SESSION_SECS.
        let session_start = Instant::now();
        seq_conn.send(&[0xFA]).unwrap();
        let mut pulse_index: u64 = 0;
        loop {
            let elapsed = session_start.elapsed().as_secs_f64();
            if elapsed >= SESSION_SECS {
                break;
            }
            let target =
                session_start + Duration::from_secs_f64(pulse_index as f64 * pulse_interval_secs);
            let now = Instant::now();
            if target > now {
                std::thread::sleep(target - now);
            }
            seq_conn.send(&[0xF8]).unwrap();
            pulse_index += 1;
        }
        // Let the last loop pass fully complete and its NoteOn be captured.
        std::thread::sleep(Duration::from_millis(500));
        engine.sync_stop();

        let notes = captured.lock().unwrap().clone();
        assert!(
            notes.len() > 20,
            "expected a substantial number of captured NoteOns, got {}",
            notes.len()
        );

        // Distinguish a genuine full-length session from a premature dropout (the sync
        // clock getting spuriously declared Lost mid-session, pausing playback for good
        // since nothing here ever resends Start/Continue) — this is a separate concern
        // from drift/wobble, and a truncated session must not be allowed to silently pass
        // the drift/wobble checks below on incomplete data.
        let last_elapsed = notes
            .last()
            .unwrap()
            .duration_since(session_start)
            .as_secs_f64();
        eprintln!(
            "(forwarding_enabled={forwarding_enabled}) last captured note at {last_elapsed:.1}s into a {SESSION_SECS:.1}s session; engine state at end: {:?}; sync_clock_state at end: {:?}",
            engine.state(),
            receiver.sync_clock_state(),
        );
        assert!(
            last_elapsed > SESSION_SECS * 0.9,
            "(forwarding_enabled={forwarding_enabled}) playback stopped early: last captured note at {last_elapsed:.1}s of a {SESSION_SECS:.1}s session (engine {:?}, sync {:?}) — looks like a premature clock-loss dropout, not the deliberate end-of-test stop",
            engine.state(),
            receiver.sync_clock_state(),
        );

        // Offset of each captured note from where it "should" land, per the clock
        // device's own precise, drift-free send schedule. A constant offset (fixed
        // startup/IO latency) is expected and fine; a *growing* offset over the session
        // is the drift bug, and a large jump between one note and the next is the wobble
        // bug.
        let offsets: Vec<f64> = notes
            .iter()
            .enumerate()
            .map(|(k, t)| {
                let expected =
                    session_start + Duration::from_secs_f64(k as f64 * beat_interval_secs);
                t.duration_since(expected).as_secs_f64()
            })
            .collect();

        // The first several notes carry a one-off settling transient (thread/CPU
        // scheduling warm-up, the pulse tracker's moving average filling its window)
        // that is unrelated to steady-state drift and would otherwise swamp it in a
        // short synthetic session; exclude them from the drift comparison rather than
        // let that transient masquerade as (or mask) real drift.
        let warmup = (offsets.len() / 10).max(15).min(offsets.len() / 3);
        let steady = &offsets[warmup..];
        assert!(
            steady.len() >= 20,
            "not enough post-warmup samples ({}) to assess steady-state drift",
            steady.len()
        );

        let median = |xs: &mut [f64]| {
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            xs[xs.len() / 2]
        };
        let window = (steady.len() / 5).max(10);
        let mut first_window = steady[..window].to_vec();
        let mut last_window = steady[steady.len() - window..].to_vec();
        let first_median = median(&mut first_window);
        let last_median = median(&mut last_window);
        let drift_growth_ms = (last_median - first_median) * 1000.0;

        let mut max_step_ms: f64 = 0.0;
        for pair in steady.windows(2) {
            let step_ms = (pair[1] - pair[0]).abs() * 1000.0;
            if step_ms > max_step_ms {
                max_step_ms = step_ms;
            }
        }

        eprintln!(
            "forwarding_enabled={forwarding_enabled}: captured {} notes over {:.1}s ({} excluded as warmup); post-warmup first-window offset {:.2}ms, last-window offset {:.2}ms, drift growth {:.2}ms, max step-to-step change {:.2}ms",
            notes.len(),
            SESSION_SECS,
            warmup,
            first_median * 1000.0,
            last_median * 1000.0,
            drift_growth_ms,
            max_step_ms,
        );

        // Thresholds are calibrated against the original bug's magnitude (drift growing
        // to a 1/16-1/8 note, i.e. ~100-250ms, over a session) versus the run-to-run
        // measurement noise actually observed from this synthetic harness (single digits
        // up to ~20ms) — wide enough to absorb that noise, narrow enough to still clearly
        // fail if the original bug reappeared.
        assert!(
            drift_growth_ms.abs() < 50.0,
            "(forwarding_enabled={forwarding_enabled}) note timing drifted {drift_growth_ms:.2}ms over the post-warmup session (first-window offset {:.2}ms -> last-window offset {:.2}ms); expected it to stay locked to the external clock",
            first_median * 1000.0,
            last_median * 1000.0,
        );
        assert!(
            max_step_ms < 30.0,
            "(forwarding_enabled={forwarding_enabled}) note timing jumped {max_step_ms:.2}ms between two consecutive notes; expected smooth, inaudible rate tracking with no discrete steps"
        );
    }

    #[test]
    #[ignore]
    fn sync_mode_note_timing_stays_locked_with_clock_forwarding_enabled() {
        run_sync_note_timing_session(true, "propeller-drift-test-fwd-on");
    }

    #[test]
    #[ignore]
    fn sync_mode_note_timing_stays_locked_with_clock_forwarding_disabled() {
        run_sync_note_timing_session(false, "propeller-drift-test-fwd-off");
    }

    // One-off manual diagnostic against a real external clock device (not a portable
    // regression test — depends on specific hardware being connected and running).
    // Point PROPELLER_TEST_REAL_SYNC_PORT at the device's MIDI output port name and run
    // with `cargo test -- --ignored --nocapture sync_mode_note_timing_against_real_device`.
    // Unlike the synthetic tests above, this doesn't drive the clock itself — it listens
    // passively on the same input port propeller uses, records every raw realtime byte
    // (Start/Stop/Pulse) with its arrival time as ground truth, and derives each beat's
    // "true" timestamp directly from those observed pulses (not a nominal/assumed
    // schedule), since a real device's actual timing and jitter aren't known in advance.
    // Requires the device to be actively sending clock, and to send (or be made to send,
    // e.g. by stopping and restarting its transport) a MIDI Start during the run.
    #[test]
    #[ignore]
    fn sync_mode_note_timing_against_real_device() {
        let port_name = std::env::var("PROPELLER_TEST_REAL_SYNC_PORT")
            .expect("set PROPELLER_TEST_REAL_SYNC_PORT to the clock device's MIDI port name");
        const SESSION_SECS: f64 = 150.0;
        const PULSES_PER_BEAT: u32 = 24;

        // Raw ground-truth listener: every realtime status byte with its arrival Instant.
        let raw: Arc<Mutex<Vec<(Instant, u8)>>> = Arc::new(Mutex::new(Vec::new()));
        let raw_clone = Arc::clone(&raw);
        let raw_client = midir::MidiInput::new("propeller-real-test-raw").unwrap();
        let raw_ports = raw_client.ports();
        let raw_port = raw_ports
            .iter()
            .find(|p| raw_client.port_name(p).as_deref() == Ok(port_name.as_str()))
            .unwrap_or_else(|| panic!("MIDI input port {port_name:?} not found"));
        let _raw_conn = raw_client
            .connect(
                raw_port,
                "propeller-real-test-raw-in",
                move |_stamp, data, _| {
                    if let [byte] = data {
                        if matches!(byte, 0xF8 | 0xFA | 0xFB | 0xFC) {
                            raw_clone.lock().unwrap().push((Instant::now(), *byte));
                        }
                    }
                },
                (),
            )
            .unwrap();

        // Propeller's real output port, and a listener on it capturing every NoteOn.
        let output_port = crate::midi_port::open_virtual_named("propeller-real-test-out").unwrap();
        let output: Arc<Mutex<Box<dyn MidiOutput>>> = Arc::new(Mutex::new(Box::new(output_port)));
        let captured: Arc<Mutex<Vec<Instant>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = Arc::clone(&captured);
        let listen_client = midir::MidiInput::new("propeller-real-test-listener").unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let listen_ports = listen_client.ports();
        let listen_port = listen_ports
            .iter()
            .find(|p| listen_client.port_name(p).unwrap_or_default() == "propeller-real-test-out")
            .expect("propeller output virtual port not found");
        let _listen_conn = listen_client
            .connect(
                listen_port,
                "propeller-real-test-listen-in",
                move |_stamp, data, _| {
                    if data.len() == 3 && data[0] == 0x90 && data[2] > 0 {
                        captured_clone.lock().unwrap().push(Instant::now());
                    }
                },
                (),
            )
            .unwrap();

        // One note per loop, loop_duration = one quarter note, as with the synthetic
        // tests: maximises how often the sync-tempo-update path runs relative to session
        // length.
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        {
            let project = Project {
                header: Header {
                    bpm: 120,
                    loop_duration: 480,
                },
                tracks: vec![Track {
                    name: "t".to_string(),
                    channel: 1,
                    instrument: 0,
                    notes: vec![Note {
                        start_tick: 0,
                        duration: 240,
                        pitch: 60,
                        velocity: 80,
                    }],
                    pitch_bends: vec![],
                }],
            };
            store.write().unwrap().set_pending(project).unwrap();
            store.write().unwrap().commit_pending();
        }
        let engine = Arc::new(LoopEngine::new(
            store,
            Box::new(SharedMidiOutput(Arc::clone(&output))),
        ));

        std::thread::sleep(Duration::from_millis(50));
        let receiver =
            MidiClockReceiver::new(&port_name, Arc::clone(&engine), Arc::clone(&output), true)
                .expect("failed to start clock receiver on the real sync port");

        eprintln!(
            "listening on {port_name:?} for {SESSION_SECS:.0}s — stop and restart the device's transport now so it sends a fresh MIDI Start"
        );
        let session_start = Instant::now();
        while session_start.elapsed().as_secs_f64() < SESSION_SECS {
            std::thread::sleep(Duration::from_secs(5));
            eprintln!(
                "  {:.0}s elapsed; engine {:?}; sync {:?}; {} notes, {} raw clock bytes captured so far",
                session_start.elapsed().as_secs_f64(),
                engine.state(),
                receiver.sync_clock_state(),
                captured.lock().unwrap().len(),
                raw.lock().unwrap().len(),
            );
        }
        engine.sync_stop();

        // Derive beat-boundary ground truth from the raw log: index pulses since the most
        // recent Start, and take the timestamp of every 24th pulse as that beat's true time.
        let raw_log = raw.lock().unwrap().clone();
        let mut beat_times: Vec<Instant> = Vec::new();
        let mut pulses_since_start: Option<u32> = None;
        for (t, byte) in &raw_log {
            match *byte {
                0xFA | 0xFB => pulses_since_start = Some(0),
                0xF8 => {
                    if let Some(n) = pulses_since_start.as_mut() {
                        if *n % PULSES_PER_BEAT == 0 {
                            beat_times.push(*t);
                        }
                        *n += 1;
                    }
                }
                _ => {}
            }
        }

        let notes = captured.lock().unwrap().clone();
        eprintln!(
            "captured {} NoteOns and derived {} beat boundaries from {} raw clock bytes over {SESSION_SECS:.0}s",
            notes.len(),
            beat_times.len(),
            raw_log.len(),
        );
        assert!(
            !beat_times.is_empty(),
            "no beat boundaries derived — did the device send a MIDI Start during the run?"
        );
        assert!(
            !notes.is_empty(),
            "no NoteOns captured — engine state {:?}, sync state {:?}",
            engine.state(),
            receiver.sync_clock_state(),
        );

        let n = notes.len().min(beat_times.len());
        let offsets: Vec<f64> = (0..n)
            .map(|k| notes[k].duration_since(beat_times[k]).as_secs_f64())
            .collect();

        for (k, offset) in offsets.iter().enumerate() {
            eprintln!("  note {k}: offset {:.2}ms", offset * 1000.0);
        }

        let warmup = (offsets.len() / 10)
            .max(5)
            .min(offsets.len().saturating_sub(10));
        let steady = &offsets[warmup..];
        if steady.len() >= 10 {
            let median = |xs: &mut [f64]| {
                xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
                xs[xs.len() / 2]
            };
            let window = (steady.len() / 5).max(5);
            let mut first_window = steady[..window].to_vec();
            let mut last_window = steady[steady.len() - window..].to_vec();
            let first_median = median(&mut first_window);
            let last_median = median(&mut last_window);
            let mut max_step_ms: f64 = 0.0;
            for pair in steady.windows(2) {
                max_step_ms = max_step_ms.max((pair[1] - pair[0]).abs() * 1000.0);
            }
            eprintln!(
                "post-warmup ({} excluded): first-window offset {:.2}ms, last-window offset {:.2}ms, drift growth {:.2}ms, max step-to-step change {:.2}ms",
                warmup,
                first_median * 1000.0,
                last_median * 1000.0,
                (last_median - first_median) * 1000.0,
                max_step_ms,
            );
        } else {
            eprintln!(
                "only {} post-warmup samples ({} total notes matched to beats) — not enough for a first/last comparison; see the per-note offsets above",
                steady.len(),
                offsets.len(),
            );
        }
    }
}
