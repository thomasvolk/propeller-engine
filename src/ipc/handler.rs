use std::sync::{Arc, Mutex, RwLock};

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::oneshot;

use crate::domain::{
    Bar, Header, Note, NoteEvent, Project, ProjectStore, TimeSignature, Track, ValidationError,
};
use crate::loop_engine::{EngineState, LoopEngine};
use crate::midi_clock::SyncClockState;

use super::types::{
    Command, EngineMode, EngineSettings, WireBar, WireHeader, WireNote, WireTrack,
    error_response, ok_response,
};

pub async fn connection_handler(
    stream: UnixStream,
    store: Arc<RwLock<ProjectStore>>,
    engine: Arc<LoopEngine>,
    settings: Arc<Mutex<EngineSettings>>,
    shutdown_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    sync_clock_state: Option<Arc<Mutex<SyncClockState>>>,
) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();

    match reader.read_line(&mut line).await {
        Ok(0) => return, // AC-13: client disconnected with no data
        Ok(_) => {}
        Err(_) => return,
    }

    let response = dispatch(&line.trim_end_matches('\n'), &store, &engine, &settings, sync_clock_state.as_ref()).await;

    let mut stream = reader.into_inner();

    if let Some(Command::Stop) = parse_command_tag(&line) {
        let response_str = serde_json::to_string(&response).unwrap() + "\n";
        let _ = stream.write_all(response_str.as_bytes()).await;
        let _ = stream.flush().await;
        let mut tx_guard = shutdown_tx.lock().unwrap();
        if let Some(tx) = tx_guard.take() {
            let _ = tx.send(());
        }
    } else {
        let response_str = serde_json::to_string(&response).unwrap() + "\n";
        let _ = stream.write_all(response_str.as_bytes()).await;
    }
}

fn parse_command_tag(line: &str) -> Option<Command> {
    serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|v| if v.get("command").and_then(|c| c.as_str()) == Some("stop") { Some(Command::Stop) } else { None })
}

async fn dispatch(
    line: &str,
    store: &Arc<RwLock<ProjectStore>>,
    engine: &Arc<LoopEngine>,
    settings: &Arc<Mutex<EngineSettings>>,
    sync_clock_state: Option<&Arc<Mutex<SyncClockState>>>,
) -> Value {
    let parsed: serde_json::Result<Value> = serde_json::from_str(line);

    let raw = match parsed {
        Err(_) => return error_response("parse_error", "malformed JSON"),
        Ok(v) => v,
    };

    if raw.get("command").is_none() {
        return error_response("missing_command", "request must include a \"command\" field");
    }

    let cmd: Result<Command, _> = serde_json::from_str(line);
    let cmd = match cmd {
        Err(_) => return error_response("unknown_command", "unrecognised command value"),
        Ok(c) => c,
    };

    match cmd {
        Command::CreateProject { header, tracks } => handle_create_project(header, tracks, store),
        Command::ModifyProject { header, tracks } => handle_project(header, tracks, store),
        Command::SetBpm { bpm } => handle_set_bpm(bpm, store, settings),
        Command::SetMode { mode } => handle_set_mode(&mode, settings, engine, sync_clock_state),
        Command::LoopStart => {
            engine.start();
            ok_response()
        }
        Command::LoopStop => {
            engine.stop();
            ok_response()
        }
        // T-12 (EP-5): clock IPC commands
        Command::ClockStart => {
            if store.read().unwrap().active().is_none() {
                return error_response("no_project", "clock-start requires an active project");
            }
            engine.clock_start();
            ok_response()
        }
        Command::ClockPause => {
            engine.clock_pause();
            ok_response()
        }
        Command::ClockResume => {
            engine.clock_resume();
            ok_response()
        }
        Command::ClockStop => {
            engine.clock_stop();
            ok_response()
        }
        Command::ListMidiPorts => {
            let ports = crate::midi_port::list_ports();
            json!({"status": "ok", "ports": ports})
        }
        Command::Status => handle_status(store, engine, settings, sync_clock_state),
        Command::Stop => ok_response(),
    }
}

fn build_domain_project(header: WireHeader, tracks: Vec<WireTrack>) -> Result<Project, Value> {
    if header.bpm.fract() != 0.0 {
        return Err(error_response("bpm_non_integer", "BPM must be a whole number"));
    }
    let bpm = header.bpm as u32;
    Ok(Project {
        header: Header {
            bpm,
            time_signature: TimeSignature {
                numerator: header.time_signature.numerator,
                denominator: header.time_signature.denominator,
            },
        },
        tracks: tracks.into_iter().map(wire_track_to_domain).collect(),
    })
}

fn handle_create_project(
    header: WireHeader,
    tracks: Vec<WireTrack>,
    store: &Arc<RwLock<ProjectStore>>,
) -> Value {
    let project = match build_domain_project(header, tracks) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let mut store_w = store.write().unwrap();
    match store_w.set_pending(project) {
        Ok(()) => {
            store_w.commit_pending();
            ok_response()
        }
        Err(e) => validation_error_response(e),
    }
}

fn handle_project(
    header: WireHeader,
    tracks: Vec<WireTrack>,
    store: &Arc<RwLock<ProjectStore>>,
) -> Value {
    let project = match build_domain_project(header, tracks) {
        Ok(p) => p,
        Err(e) => return e,
    };
    match store.write().unwrap().set_pending(project) {
        Ok(()) => ok_response(),
        Err(e) => validation_error_response(e),
    }
}

fn handle_set_bpm(
    bpm: f64,
    store: &Arc<RwLock<ProjectStore>>,
    settings: &Arc<Mutex<EngineSettings>>,
) -> Value {
    if settings.lock().unwrap().mode == EngineMode::Sync {
        return error_response("sync_mode_active", "cannot change BPM in sync mode");
    }
    if bpm.fract() != 0.0 {
        return error_response("bpm_non_integer", "BPM must be a whole number");
    }
    let bpm_u32 = bpm as u32;
    if bpm_u32 < 20 || bpm_u32 > 300 {
        return error_response("bpm_out_of_range", "BPM must be between 20 and 300");
    }

    settings.lock().unwrap().bpm = bpm_u32;

    if let Some(active) = store.read().unwrap().active() {
        let new_project = Project {
            header: Header {
                bpm: bpm_u32,
                time_signature: TimeSignature {
                    numerator: active.header.time_signature.numerator,
                    denominator: active.header.time_signature.denominator,
                },
            },
            tracks: active.tracks.iter().map(|t| Track {
                name: t.name.clone(),
                channel: t.channel,
                instrument: t.instrument,
                bars: t.bars.iter().map(|b| Bar {
                    notes: b.notes.iter().map(|n| Note {
                        event: match &n.event {
                            NoteEvent::Note { pitch, velocity } => NoteEvent::Note { pitch: *pitch, velocity: *velocity },
                            NoteEvent::Rest => NoteEvent::Rest,
                        },
                        duration_ticks: n.duration_ticks,
                    }).collect(),
                }).collect(),
            }).collect(),
        };
        let _ = store.write().unwrap().set_pending(new_project);
    }

    ok_response()
}

fn handle_set_mode(
    mode_str: &str,
    settings: &Arc<Mutex<EngineSettings>>,
    engine: &Arc<LoopEngine>,
    sync_clock_state: Option<&Arc<Mutex<SyncClockState>>>,
) -> Value {
    match EngineMode::from_str(mode_str) {
        Some(EngineMode::Sync) if sync_clock_state.is_none() => {
            error_response("sync_requires_port", "sync mode requires --sync-port at startup")
        }
        Some(new_mode) => {
            let current_mode = settings.lock().unwrap().mode.clone();
            let engine_state = engine.state();

            if current_mode == EngineMode::Clock && new_mode != EngineMode::Clock {
                if engine_state == EngineState::Running || engine_state == EngineState::Paused {
                    engine.clock_stop();
                }
            } else if new_mode == EngineMode::Sync && current_mode != EngineMode::Sync {
                if engine_state == EngineState::Running || engine_state == EngineState::Paused {
                    engine.stop();
                }
            }

            settings.lock().unwrap().mode = new_mode;
            ok_response()
        }
        None => error_response("invalid_mode", "unrecognised mode; use standalone, clock, or sync"),
    }
}

fn handle_status(
    store: &Arc<RwLock<ProjectStore>>,
    engine: &Arc<LoopEngine>,
    settings: &Arc<Mutex<EngineSettings>>,
    sync_clock_state: Option<&Arc<Mutex<SyncClockState>>>,
) -> Value {
    let store_read = store.read().unwrap();
    let active = store_read.active();
    let settings_guard = settings.lock().unwrap();

    let clock_state = match engine.state() {
        EngineState::Running | EngineState::Waiting | EngineState::Paused => "started",
        EngineState::Stopped => "stopped",
    };

    let bpm = active
        .map(|p| p.header.bpm)
        .unwrap_or(settings_guard.bpm);

    let time_signature = active.map(|p| {
        json!({
            "numerator": p.header.time_signature.numerator,
            "denominator": p.header.time_signature.denominator,
        })
    });

    let mut resp = json!({
        "status": "ok",
        "mode": settings_guard.mode.as_str(),
        "bpm": bpm,
        "time_signature": time_signature,
        "clock_state": clock_state,
        "project_present": active.is_some(),
    });

    if settings_guard.mode == EngineMode::Sync {
        if let Some(state_arc) = sync_clock_state {
            let state_str = match *state_arc.lock().unwrap() {
                SyncClockState::Waiting => "waiting",
                SyncClockState::Tracking => "tracking",
                SyncClockState::Lost => "lost",
            };
            resp["sync_clock_state"] = json!(state_str);
        }
    }

    resp
}

fn wire_track_to_domain(t: WireTrack) -> Track {
    Track {
        name: t.name,
        channel: t.channel,
        instrument: t.instrument,
        bars: t.bars.into_iter().map(wire_bar_to_domain).collect(),
    }
}

fn wire_bar_to_domain(b: WireBar) -> Bar {
    Bar {
        notes: b.notes.into_iter().map(wire_note_to_domain).collect(),
    }
}

fn wire_note_to_domain(n: WireNote) -> Note {
    let event = if n.rest == Some(true) {
        NoteEvent::Rest
    } else {
        NoteEvent::Note {
            pitch: n.pitch.unwrap_or(60),
            velocity: n.velocity.unwrap_or(64),
        }
    };
    Note { event, duration_ticks: n.duration_ticks }
}

fn validation_error_response(e: ValidationError) -> Value {
    let message = match &e {
        ValidationError::BpmOutOfRange { actual } => format!("BPM {actual} is out of range (20–300)"),
        ValidationError::InvalidTimeSignatureNumerator => "time signature numerator must be ≥ 1".to_string(),
        ValidationError::InvalidTimeSignatureDenominator { actual } => format!("time signature denominator {actual} must be 2, 4, 8, or 16"),
        ValidationError::InvalidMidiChannel { track, actual } => format!("track {track}: MIDI channel {actual} is out of range (1–16)"),
        ValidationError::InvalidMidiInstrument { track, actual } => format!("track {track}: instrument {actual} is out of range (0–127)"),
        ValidationError::EmptyTrackBars { track } => format!("track {track}: must have at least one bar"),
        ValidationError::NoteDurationZero { track, bar, note } => format!("track {track} bar {bar} note {note}: duration must be > 0"),
        ValidationError::NoteDurationExceedsBar { track, bar, note, duration, bar_ticks } => {
            format!("track {track} bar {bar} note {note}: duration {duration} exceeds bar ticks {bar_ticks}")
        }
    };
    error_response("validation_error", &message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loop_engine::midi::MockMidiOutput;
    use std::sync::{Arc, Mutex, RwLock};
    use tokio::io::AsyncReadExt;
    use tokio::net::UnixStream;
    use tokio::sync::oneshot;

    fn make_shared_state() -> (
        Arc<RwLock<ProjectStore>>,
        Arc<LoopEngine>,
        Arc<Mutex<EngineSettings>>,
        Arc<Mutex<Option<oneshot::Sender<()>>>>,
    ) {
        let (tx, _rx) = oneshot::channel();
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        let engine = Arc::new(LoopEngine::new(Arc::clone(&store), Box::new(MockMidiOutput::new())));
        (
            store,
            engine,
            Arc::new(Mutex::new(EngineSettings::new())),
            Arc::new(Mutex::new(Some(tx))),
        )
    }

    async fn send_command_get_response(command_json: &str) -> String {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        let (client, server) = UnixStream::pair().unwrap();
        let cmd = command_json.to_string() + "\n";

        tokio::spawn(async move {
            connection_handler(server, store, engine, settings, shutdown_tx, None).await;
        });

        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();
        response
    }

    // T-7: loop-start → {"status":"ok"}\n, stream closed after response
    #[tokio::test]
    async fn loop_start_returns_ok() {
        let response = send_command_get_response(r#"{"command":"loop-start"}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert!(response.ends_with('\n'));
    }

    // T-8: client writes nothing → no response
    #[tokio::test]
    async fn empty_stream_no_response() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        let (client, server) = UnixStream::pair().unwrap();

        tokio::spawn(async move {
            connection_handler(server, store, engine, settings, shutdown_tx, None).await;
        });

        drop(client); // close without sending anything — handler should return silently
    }

    // T-9: malformed JSON → parse_error
    #[tokio::test]
    async fn malformed_json_returns_parse_error() {
        let response = send_command_get_response("not json at all").await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "parse_error");
    }

    // T-10: valid JSON, no "command" field → missing_command
    #[tokio::test]
    async fn missing_command_field_returns_error() {
        let response = send_command_get_response(r#"{"bpm":120}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "missing_command");
    }

    // T-13: valid create-project → ok, store.active() is Some
    #[tokio::test]
    async fn create_project_stores_project() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        let (client, server) = UnixStream::pair().unwrap();

        let store_clone = Arc::clone(&store);
        tokio::spawn(async move {
            connection_handler(server, store_clone, engine, settings, shutdown_tx, None).await;
        });

        let cmd = r#"{"command":"create-project","header":{"bpm":120,"time_signature":{"numerator":4,"denominator":4}},"tracks":[{"name":"piano","channel":1,"instrument":0,"bars":[{"notes":[{"pitch":60,"velocity":80,"duration_ticks":480}]}]}]}"# .to_string() + "\n";

        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        drop(client);

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(store.read().unwrap().active().is_some());
    }

    // T-14: create-project with bpm 301 → validation_error
    #[tokio::test]
    async fn create_project_bpm_out_of_range() {
        let response = send_command_get_response(
            r#"{"command":"create-project","header":{"bpm":301,"time_signature":{"numerator":4,"denominator":4}},"tracks":[{"name":"t","channel":1,"instrument":0,"bars":[{"notes":[{"pitch":60,"velocity":80,"duration_ticks":480}]}]}]}"#
        ).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "validation_error");
        assert!(!v["message"].as_str().unwrap().is_empty());
    }

    // T-15: create-project with bpm 120.5 → bpm_non_integer
    #[tokio::test]
    async fn create_project_bpm_non_integer() {
        let response = send_command_get_response(
            r#"{"command":"create-project","header":{"bpm":120.5,"time_signature":{"numerator":4,"denominator":4}},"tracks":[]}"#
        ).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "bpm_non_integer");
    }

    // T-17: modify-project → ok, store pending updated
    #[tokio::test]
    async fn modify_project_returns_ok() {
        let response = send_command_get_response(
            r#"{"command":"modify-project","header":{"bpm":140,"time_signature":{"numerator":4,"denominator":4}},"tracks":[{"name":"t","channel":1,"instrument":0,"bars":[{"notes":[{"pitch":60,"velocity":80,"duration_ticks":480}]}]}]}"#
        ).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "ok");
    }

    // T-19: set-bpm 150 → ok, settings.bpm = 150
    #[tokio::test]
    async fn set_bpm_valid() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        let (client, server) = UnixStream::pair().unwrap();
        let settings_clone = Arc::clone(&settings);

        tokio::spawn(async move {
            connection_handler(server, store, engine, settings_clone, shutdown_tx, None).await;
        });

        let cmd = r#"{"command":"set-bpm","bpm":150}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();

        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();

        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(settings.lock().unwrap().bpm, 150);
    }

    // T-20: set-bpm 19 → bpm_out_of_range; set-bpm 120.5 → bpm_non_integer
    #[tokio::test]
    async fn set_bpm_out_of_range() {
        let response = send_command_get_response(r#"{"command":"set-bpm","bpm":19}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["code"], "bpm_out_of_range");
    }

    #[tokio::test]
    async fn set_bpm_non_integer() {
        let response = send_command_get_response(r#"{"command":"set-bpm","bpm":120.5}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["code"], "bpm_non_integer");
    }

    // T-22: set-mode "clock" → ok, settings.mode = Clock
    #[tokio::test]
    async fn set_mode_clock() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        let (client, server) = UnixStream::pair().unwrap();
        let settings_clone = Arc::clone(&settings);

        tokio::spawn(async move {
            connection_handler(server, store, engine, settings_clone, shutdown_tx, None).await;
        });

        let cmd = r#"{"command":"set-mode","mode":"clock"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();

        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(settings.lock().unwrap().mode, EngineMode::Clock);
    }

    // T-23: set-mode with unrecognised string → invalid_mode
    #[tokio::test]
    async fn set_mode_invalid() {
        let response = send_command_get_response(r#"{"command":"set-mode","mode":"turbo"}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["code"], "invalid_mode");
    }

    // T-25: loop-start → ok, engine state Running or Waiting
    #[tokio::test]
    async fn loop_start_changes_engine_state() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        let (client, server) = UnixStream::pair().unwrap();
        let engine_clone = Arc::clone(&engine);

        tokio::spawn(async move {
            connection_handler(server, store, engine_clone, settings, shutdown_tx, None).await;
        });

        let cmd = r#"{"command":"loop-start"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        let state = engine.state();
        assert!(
            state == EngineState::Running || state == EngineState::Waiting,
            "expected Running or Waiting, got {:?}", state
        );
    }

    // T-27: loop-stop → ok, engine state Stopped
    #[tokio::test]
    async fn loop_stop_changes_engine_state() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        let engine_clone = Arc::clone(&engine);
        engine_clone.start();
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;

        let (client, server) = UnixStream::pair().unwrap();

        tokio::spawn(async move {
            connection_handler(server, store, engine_clone, settings, shutdown_tx, None).await;
        });

        let cmd = r#"{"command":"loop-stop"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert_eq!(engine.state(), EngineState::Stopped);
    }

    // T-29: status with active project, loop stopped
    #[tokio::test]
    async fn status_with_project_stopped() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();

        // Load a project
        {
            use crate::domain::*;
            let project = Project {
                header: Header { bpm: 120, time_signature: TimeSignature { numerator: 4, denominator: 4 } },
                tracks: vec![Track {
                    name: "t".to_string(), channel: 1, instrument: 0,
                    bars: vec![Bar { notes: vec![Note { event: NoteEvent::Note { pitch: 60, velocity: 80 }, duration_ticks: 480 }] }],
                }],
            };
            store.write().unwrap().set_pending(project).unwrap();
            store.write().unwrap().commit_pending();
        }

        let (client, server) = UnixStream::pair().unwrap();
        let store_clone = Arc::clone(&store);
        let engine_clone = Arc::clone(&engine);
        let settings_clone = Arc::clone(&settings);

        tokio::spawn(async move {
            connection_handler(server, store_clone, engine_clone, settings_clone, shutdown_tx, None).await;
        });

        let cmd = r#"{"command":"status"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();

        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert!(v.get("mode").is_some());
        assert!(v.get("bpm").is_some());
        assert!(v.get("time_signature").is_some());
        assert_eq!(v["clock_state"], "stopped");
        assert_eq!(v["project_present"], true);
    }

    // T-30: status with loop running → clock_state "started"
    #[tokio::test]
    async fn status_loop_running_clock_state_started() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        engine.start();
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;

        let (client, server) = UnixStream::pair().unwrap();
        let store_clone = Arc::clone(&store);
        let engine_clone = Arc::clone(&engine);
        let settings_clone = Arc::clone(&settings);

        tokio::spawn(async move {
            connection_handler(server, store_clone, engine_clone, settings_clone, shutdown_tx, None).await;
        });

        let cmd = r#"{"command":"status"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();

        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["clock_state"], "started");
    }

    // T-31: status with no active project → project_present false, time_signature null
    #[tokio::test]
    async fn status_no_project() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        settings.lock().unwrap().bpm = 99;

        let (client, server) = UnixStream::pair().unwrap();
        let store_clone = Arc::clone(&store);
        let engine_clone = Arc::clone(&engine);
        let settings_clone = Arc::clone(&settings);

        tokio::spawn(async move {
            connection_handler(server, store_clone, engine_clone, settings_clone, shutdown_tx, None).await;
        });

        let cmd = r#"{"command":"status"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();

        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["project_present"], false);
        assert!(v["time_signature"].is_null());
        assert_eq!(v["bpm"], 99);
    }

    // T-15: list-midi-ports over live socket → {"status":"ok","ports":[...]}
    #[tokio::test]
    async fn list_midi_ports_returns_ok_with_ports_array() {
        let response = send_command_get_response(r#"{"command":"list-midi-ports"}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert!(v["ports"].is_array(), "ports should be a JSON array");
    }

    // T-11 (EP-5): clock-start with no active project → error no_project; engine stays Stopped
    #[tokio::test]
    async fn clock_start_without_project_returns_no_project_error() {
        let response = send_command_get_response(r#"{"command":"clock-start"}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "no_project");
    }

    // T-11 variant: clock-start with active project → ok
    #[tokio::test]
    async fn clock_start_with_project_returns_ok() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        {
            use crate::domain::*;
            let project = Project {
                header: Header { bpm: 120, time_signature: TimeSignature { numerator: 4, denominator: 4 } },
                tracks: vec![Track {
                    name: "t".to_string(), channel: 1, instrument: 0,
                    bars: vec![Bar { notes: vec![Note {
                        event: NoteEvent::Note { pitch: 60, velocity: 80 }, duration_ticks: 480,
                    }] }],
                }],
            };
            store.write().unwrap().set_pending(project).unwrap();
            store.write().unwrap().commit_pending();
        }

        let (client, server) = tokio::net::UnixStream::pair().unwrap();
        tokio::spawn(async move {
            connection_handler(server, store, engine, settings, shutdown_tx, None).await;
        });

        let cmd = r#"{"command":"clock-start"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "ok");
    }

    // T-35: stop command → ok response, shutdown_tx receives signal
    #[tokio::test]
    async fn stop_command_signals_shutdown() {
        let (store, engine, settings, _) = make_shared_state();
        let (shutdown_tx_chan, mut shutdown_rx) = oneshot::channel::<()>();
        let shutdown_tx = Arc::new(Mutex::new(Some(shutdown_tx_chan)));

        let (client, server) = UnixStream::pair().unwrap();
        tokio::spawn(async move {
            connection_handler(server, store, engine, settings, shutdown_tx, None).await;
        });

        let cmd = r#"{"command":"stop"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();

        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert!(shutdown_rx.try_recv().is_ok());
    }

    // T-23 (EP-6): set-bpm in sync mode → sync_mode_active error
    #[tokio::test]
    async fn set_bpm_in_sync_mode_returns_error() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        settings.lock().unwrap().mode = EngineMode::Sync;
        let settings_clone = Arc::clone(&settings);

        let (client, server) = UnixStream::pair().unwrap();
        tokio::spawn(async move {
            connection_handler(server, store, engine, settings_clone, shutdown_tx, None).await;
        });

        let cmd = r#"{"command":"set-bpm","bpm":140}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "sync_mode_active");
        // BPM must not be changed
        assert_eq!(settings.lock().unwrap().bpm, 120);
    }

    // T-37 (EP-6): status with mode=Sync and SyncClockState → response has sync_clock_state field
    #[tokio::test]
    async fn status_sync_mode_includes_sync_clock_state() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        settings.lock().unwrap().mode = EngineMode::Sync;
        let settings_clone = Arc::clone(&settings);

        let sync_state = Arc::new(Mutex::new(SyncClockState::Tracking));
        let sync_state_clone = Arc::clone(&sync_state);

        let (client, server) = UnixStream::pair().unwrap();
        tokio::spawn(async move {
            connection_handler(server, store, engine, settings_clone, shutdown_tx, Some(sync_state_clone)).await;
        });

        let cmd = r#"{"command":"status"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["sync_clock_state"], "tracking");

        // Also test with Lost state
        *sync_state.lock().unwrap() = SyncClockState::Lost;
        let (store2, engine2, settings2, shutdown_tx2) = make_shared_state();
        settings2.lock().unwrap().mode = EngineMode::Sync;
        let sync_state2 = Arc::clone(&sync_state);
        let (client2, server2) = UnixStream::pair().unwrap();
        tokio::spawn(async move {
            connection_handler(server2, store2, engine2, settings2, shutdown_tx2, Some(sync_state2)).await;
        });
        let mut client2 = client2;
        client2.write_all((r#"{"command":"status"}"#.to_string() + "\n").as_bytes()).await.unwrap();
        let mut resp2 = String::new();
        client2.read_to_string(&mut resp2).await.unwrap();
        let v2: serde_json::Value = serde_json::from_str(resp2.trim()).unwrap();
        assert_eq!(v2["sync_clock_state"], "lost");
    }

    // T-39 (EP-6): status with mode=Standalone → no sync_clock_state field
    #[tokio::test]
    async fn status_standalone_mode_no_sync_clock_state() {
        let response = send_command_get_response(r#"{"command":"status"}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert!(v.get("sync_clock_state").is_none(), "sync_clock_state should not be present in standalone mode");
    }

    // T-41 (EP-6): set-mode sync without sync_clock_state → sync_requires_port error
    #[tokio::test]
    async fn set_mode_sync_without_receiver_returns_error() {
        // No sync_clock_state (None) → should return error
        let response = send_command_get_response(r#"{"command":"set-mode","mode":"sync"}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "sync_requires_port");
    }

    // T-3 (EP-7): status response includes "mode" field (F-2, AC-2)
    #[tokio::test]
    async fn status_response_includes_mode_field() {
        let response = send_command_get_response(r#"{"command":"status"}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert!(v.get("mode").is_some(), "status response must include mode field");
        assert_eq!(v["mode"], "standalone");
    }

    // T-9 (EP-7): set-mode clock while standalone with loop Running → engine stays Running (F-6, F-11, AC-3)
    #[tokio::test]
    async fn set_mode_clock_while_loop_running_does_not_stop_engine() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        let engine_clone = Arc::clone(&engine);
        engine_clone.start();
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;

        let (client, server) = UnixStream::pair().unwrap();
        let s = Arc::clone(&store);
        let e = Arc::clone(&engine);
        let se = Arc::clone(&settings);
        let st = Arc::clone(&shutdown_tx);
        tokio::spawn(async move {
            connection_handler(server, s, e, se, st, None).await;
        });

        let cmd = r#"{"command":"set-mode","mode":"clock"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "ok");

        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        let state = engine_clone.state();
        assert!(
            state == EngineState::Running || state == EngineState::Waiting,
            "loop must remain running after standalone→clock switch, got {:?}", state
        );
        engine_clone.stop();
    }

    // T-11 (EP-7): set-mode standalone from clock while Running → engine.clock_stop() called (F-12, AC-9)
    #[tokio::test]
    async fn set_mode_standalone_from_clock_while_running_calls_clock_stop() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        {
            use crate::domain::*;
            let project = Project {
                header: Header { bpm: 300, time_signature: TimeSignature { numerator: 1, denominator: 4 } },
                tracks: vec![Track {
                    name: "t".to_string(), channel: 1, instrument: 0,
                    bars: vec![Bar { notes: vec![Note { event: NoteEvent::Note { pitch: 60, velocity: 80 }, duration_ticks: 480 }] }],
                }],
            };
            store.write().unwrap().set_pending(project).unwrap();
            store.write().unwrap().commit_pending();
        }
        engine.clock_start();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(engine.state(), EngineState::Running);
        settings.lock().unwrap().mode = EngineMode::Clock;

        let (client, server) = UnixStream::pair().unwrap();
        let s = Arc::clone(&store);
        let e = Arc::clone(&engine);
        let se = Arc::clone(&settings);
        let st = Arc::clone(&shutdown_tx);
        tokio::spawn(async move {
            connection_handler(server, s, e, se, st, None).await;
        });

        let cmd = r#"{"command":"set-mode","mode":"standalone"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "ok");

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(engine.state(), EngineState::Stopped, "clock_stop must be called on clock→standalone transition");
        assert_eq!(settings.lock().unwrap().mode, EngineMode::Standalone);
    }

    // T-13 (EP-7): set-mode sync from standalone while Running → engine.stop() called; mode = Sync (F-14, AC-11)
    #[tokio::test]
    async fn set_mode_sync_from_standalone_while_running_calls_stop() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        {
            use crate::domain::*;
            let project = Project {
                header: Header { bpm: 300, time_signature: TimeSignature { numerator: 1, denominator: 4 } },
                tracks: vec![Track {
                    name: "t".to_string(), channel: 1, instrument: 0,
                    bars: vec![Bar { notes: vec![Note { event: NoteEvent::Note { pitch: 60, velocity: 80 }, duration_ticks: 480 }] }],
                }],
            };
            store.write().unwrap().set_pending(project).unwrap();
            store.write().unwrap().commit_pending();
        }
        engine.start();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(engine.state(), EngineState::Running);

        let sync_state = Arc::new(Mutex::new(crate::midi_clock::SyncClockState::Waiting));
        let (client, server) = UnixStream::pair().unwrap();
        let s = Arc::clone(&store);
        let e = Arc::clone(&engine);
        let se = Arc::clone(&settings);
        let st = Arc::clone(&shutdown_tx);
        let sync_c = Arc::clone(&sync_state);
        tokio::spawn(async move {
            connection_handler(server, s, e, se, st, Some(sync_c)).await;
        });

        let cmd = r#"{"command":"set-mode","mode":"sync"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "ok");

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(engine.state(), EngineState::Stopped, "stop must be called on standalone→sync transition");
        assert_eq!(settings.lock().unwrap().mode, EngineMode::Sync);
    }

    // T-15 (EP-7): set-mode sync from clock while Running → engine.clock_stop() called; mode = Sync (F-12, F-14, AC-9, AC-11)
    #[tokio::test]
    async fn set_mode_sync_from_clock_while_running_uses_clock_stop() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        {
            use crate::domain::*;
            let project = Project {
                header: Header { bpm: 300, time_signature: TimeSignature { numerator: 1, denominator: 4 } },
                tracks: vec![Track {
                    name: "t".to_string(), channel: 1, instrument: 0,
                    bars: vec![Bar { notes: vec![Note { event: NoteEvent::Note { pitch: 60, velocity: 80 }, duration_ticks: 480 }] }],
                }],
            };
            store.write().unwrap().set_pending(project).unwrap();
            store.write().unwrap().commit_pending();
        }
        engine.clock_start();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(engine.state(), EngineState::Running);
        settings.lock().unwrap().mode = EngineMode::Clock;

        let sync_state = Arc::new(Mutex::new(crate::midi_clock::SyncClockState::Waiting));
        let (client, server) = UnixStream::pair().unwrap();
        let s = Arc::clone(&store);
        let e = Arc::clone(&engine);
        let se = Arc::clone(&settings);
        let st = Arc::clone(&shutdown_tx);
        let sync_c = Arc::clone(&sync_state);
        tokio::spawn(async move {
            connection_handler(server, s, e, se, st, Some(sync_c)).await;
        });

        let cmd = r#"{"command":"set-mode","mode":"sync"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "ok");

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(engine.state(), EngineState::Stopped, "clock_stop must be called on clock→sync transition");
        assert_eq!(settings.lock().unwrap().mode, EngineMode::Sync);
    }

    // T-17 (EP-7): set-mode sync while already in sync with loop Running → no stop; mode stays Sync (F-9, AC-10)
    #[tokio::test]
    async fn set_mode_sync_while_already_sync_does_not_stop_engine() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        engine.start();
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        settings.lock().unwrap().mode = EngineMode::Sync;

        let sync_state = Arc::new(Mutex::new(crate::midi_clock::SyncClockState::Waiting));
        let (client, server) = UnixStream::pair().unwrap();
        let s = Arc::clone(&store);
        let e = Arc::clone(&engine);
        let se = Arc::clone(&settings);
        let st = Arc::clone(&shutdown_tx);
        let sync_c = Arc::clone(&sync_state);
        tokio::spawn(async move {
            connection_handler(server, s, e, se, st, Some(sync_c)).await;
        });

        let cmd = r#"{"command":"set-mode","mode":"sync"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "ok");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let state = engine.state();
        assert!(
            state == EngineState::Running || state == EngineState::Waiting,
            "sync→sync must not stop the engine, got {:?}", state
        );
        assert_eq!(settings.lock().unwrap().mode, EngineMode::Sync);
        engine.stop();
    }

    // T-21 (EP-7): set-mode standalone from sync, then set-bpm → BPM updated (F-5, AC-5)
    #[tokio::test]
    async fn set_mode_standalone_from_sync_re_enables_bpm() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        settings.lock().unwrap().mode = EngineMode::Sync;

        // First IPC call: set-mode standalone
        {
            let (client, server) = UnixStream::pair().unwrap();
            let s = Arc::clone(&store);
            let e = Arc::clone(&engine);
            let se = Arc::clone(&settings);
            let st = Arc::clone(&shutdown_tx);
            tokio::spawn(async move {
                connection_handler(server, s, e, se, st, None).await;
            });
            let mut client = client;
            use tokio::io::AsyncWriteExt;
            client.write_all((r#"{"command":"set-mode","mode":"standalone"}"#.to_string() + "\n").as_bytes()).await.unwrap();
            let mut resp = String::new();
            client.read_to_string(&mut resp).await.unwrap();
            let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
            assert_eq!(v["status"], "ok");
        }
        assert_eq!(settings.lock().unwrap().mode, EngineMode::Standalone);

        // Second IPC call: set-bpm → must succeed since sync mode is no longer active
        {
            let (client, server) = UnixStream::pair().unwrap();
            let s = Arc::clone(&store);
            let e = Arc::clone(&engine);
            let se = Arc::clone(&settings);
            let st = Arc::clone(&shutdown_tx);
            tokio::spawn(async move {
                connection_handler(server, s, e, se, st, None).await;
            });
            let mut client = client;
            use tokio::io::AsyncWriteExt;
            client.write_all((r#"{"command":"set-bpm","bpm":150}"#.to_string() + "\n").as_bytes()).await.unwrap();
            let mut resp = String::new();
            client.read_to_string(&mut resp).await.unwrap();
            let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
            assert_eq!(v["status"], "ok");
        }
        assert_eq!(settings.lock().unwrap().bpm, 150);
    }
}
