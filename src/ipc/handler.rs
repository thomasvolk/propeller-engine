// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use std::sync::{Arc, Mutex, RwLock};

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::oneshot;

use crate::domain::{Header, Note, PitchBend, Project, ProjectStore, Track, ValidationError};
use crate::loop_engine::{EngineState, LoopEngine};
use crate::midi_clock::SyncClockState;

use super::types::{
    Command, EngineMode, EngineSettings, WireHeader, WireTrack, error_response, ok_response,
};

pub async fn connection_handler(
    stream: UnixStream,
    store: Arc<RwLock<ProjectStore>>,
    engine: Arc<LoopEngine>,
    settings: Arc<Mutex<EngineSettings>>,
    shutdown_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();

    match reader.read_line(&mut line).await {
        Ok(0) => return, // AC-13: client disconnected with no data
        Ok(_) => {}
        Err(_) => return,
    }

    let response = dispatch(&line.trim_end_matches('\n'), &store, &engine, &settings).await;

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
    serde_json::from_str::<Value>(line).ok().and_then(|v| {
        if v.get("command").and_then(|c| c.as_str()) == Some("stop") {
            Some(Command::Stop)
        } else {
            None
        }
    })
}

async fn dispatch(
    line: &str,
    store: &Arc<RwLock<ProjectStore>>,
    engine: &Arc<LoopEngine>,
    settings: &Arc<Mutex<EngineSettings>>,
) -> Value {
    let parsed: serde_json::Result<Value> = serde_json::from_str(line);

    let raw = match parsed {
        Err(_) => return error_response("parse_error", "malformed JSON"),
        Ok(v) => v,
    };

    if raw.get("command").is_none() {
        return error_response(
            "missing_command",
            "request must include a \"command\" field",
        );
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
        Command::SetMode { mode } => handle_set_mode(&mode, settings, engine),
        Command::LoopStart => {
            let mode = settings.lock().unwrap().mode.clone();
            match mode {
                EngineMode::Clock => {
                    if store.read().unwrap().active().is_none() {
                        return error_response(
                            "no_project",
                            "clock-start requires an active project",
                        );
                    }
                    engine.clock_start();
                }
                EngineMode::Sync => {
                    return error_response(
                        "sync_mode_active",
                        "in sync mode playback is controlled by the external clock; send MIDI Start (0xFA) from your device",
                    );
                }
                EngineMode::Standalone => {
                    engine.start();
                }
            }
            ok_response()
        }
        Command::LoopStop => {
            let mode = settings.lock().unwrap().mode.clone();
            match mode {
                EngineMode::Clock => engine.clock_stop(),
                EngineMode::Sync => {
                    return error_response(
                        "sync_mode_active",
                        "in sync mode playback is controlled by the external clock; send MIDI Stop (0xFC) from your device",
                    );
                }
                EngineMode::Standalone => engine.stop(),
            }
            ok_response()
        }
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
        Command::Status => handle_status(store, engine, settings),
        Command::Project => handle_get_project(store),
        Command::Stop => ok_response(),
        Command::GetPosition => handle_get_position(engine),
    }
}

fn handle_get_position(engine: &Arc<LoopEngine>) -> Value {
    let tick = engine.current_tick();
    let raw_dur = engine.loop_duration_ticks();
    let loop_duration = if raw_dur == 0 { None } else { Some(raw_dur) };
    let loop_count = engine.loop_count();
    json!({"tick": tick, "loop_duration": loop_duration, "loop_count": loop_count})
}

fn build_domain_project(header: WireHeader, tracks: Vec<WireTrack>) -> Project {
    Project {
        header: Header {
            bpm: header.bpm,
            loop_duration: header.loop_duration,
        },
        tracks: tracks.into_iter().map(wire_track_to_domain).collect(),
    }
}

fn project_to_json(project: &Project) -> Value {
    let tracks: Vec<Value> = project
        .tracks
        .iter()
        .map(|t| {
            let notes: Vec<Value> = t
                .notes
                .iter()
                .map(|n| json!([n.start_tick, n.duration, n.pitch, n.velocity]))
                .collect();
            let pitch_bends: Vec<Value> = t
                .pitch_bends
                .iter()
                .map(|pb| json!([pb.tick, pb.value]))
                .collect();
            json!({
                "name": t.name,
                "channel": t.channel,
                "instrument": t.instrument,
                "notes": notes,
                "pitch-bends": pitch_bends,
            })
        })
        .collect();

    json!({
        "header": {
            "bpm": project.header.bpm,
            "loop_duration": project.header.loop_duration,
        },
        "tracks": tracks,
    })
}

fn handle_create_project(
    header: WireHeader,
    tracks: Vec<WireTrack>,
    store: &Arc<RwLock<ProjectStore>>,
) -> Value {
    let project = build_domain_project(header, tracks);
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
    let project = build_domain_project(header, tracks);
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
        return error_response(
            "sync_mode_active",
            "set-bpm is not allowed in sync mode; tempo is controlled by the external clock",
        );
    }

    if bpm.fract() != 0.0 {
        return error_response("bpm_non_integer", "BPM must be a whole number");
    }
    let bpm_u32 = bpm as u32;
    if bpm_u32 < 20 || bpm_u32 > 300 {
        return error_response("bpm_out_of_range", "BPM must be between 20 and 300");
    }

    settings.lock().unwrap().bpm = bpm_u32;

    // Read active project data and release the read lock before acquiring the write lock
    // to avoid holding both simultaneously (which would deadlock on std::sync::RwLock).
    let rebuild = {
        let guard = store.read().unwrap();
        guard.active().map(|p| {
            let tracks = p
                .tracks
                .iter()
                .map(|t| Track {
                    name: t.name.clone(),
                    channel: t.channel,
                    instrument: t.instrument,
                    notes: t
                        .notes
                        .iter()
                        .map(|n| Note {
                            start_tick: n.start_tick,
                            duration: n.duration,
                            pitch: n.pitch,
                            velocity: n.velocity,
                        })
                        .collect(),
                    pitch_bends: t
                        .pitch_bends
                        .iter()
                        .map(|pb| PitchBend {
                            tick: pb.tick,
                            value: pb.value,
                        })
                        .collect(),
                })
                .collect::<Vec<_>>();
            (p.header.loop_duration, tracks)
        })
    };
    if let Some((loop_duration, tracks)) = rebuild {
        let new_project = Project {
            header: Header {
                bpm: bpm_u32,
                loop_duration,
            },
            tracks,
        };
        let _ = store.write().unwrap().set_pending(new_project);
    }

    ok_response()
}

fn handle_set_mode(
    mode_str: &str,
    settings: &Arc<Mutex<EngineSettings>>,
    engine: &Arc<LoopEngine>,
) -> Value {
    match EngineMode::from_str(mode_str) {
        Some(new_mode) => {
            if new_mode == EngineMode::Sync {
                let has_receiver = settings.lock().unwrap().sync_clock_state.is_some();
                if !has_receiver {
                    return error_response(
                        "sync_requires_port",
                        "sync mode requires --sync at startup",
                    );
                }
            }

            let current_mode = settings.lock().unwrap().mode.clone();
            let engine_state = engine.state();

            if current_mode == EngineMode::Clock && new_mode != EngineMode::Clock {
                if engine_state == EngineState::Running || engine_state == EngineState::Paused {
                    engine.clock_stop();
                }
            }

            settings.lock().unwrap().mode = new_mode;
            ok_response()
        }
        None => error_response(
            "invalid_mode",
            "unrecognised mode; use standalone, clock, or sync",
        ),
    }
}

fn handle_status(
    store: &Arc<RwLock<ProjectStore>>,
    engine: &Arc<LoopEngine>,
    settings: &Arc<Mutex<EngineSettings>>,
) -> Value {
    let store_read = store.read().unwrap();
    let active = store_read.active();
    let settings_guard = settings.lock().unwrap();

    let clock_state = match engine.state() {
        EngineState::Running | EngineState::Waiting | EngineState::Paused => "started",
        EngineState::Stopped => "stopped",
    };

    let bpm = active.map(|p| p.header.bpm).unwrap_or(settings_guard.bpm);

    let mut resp = json!({
        "status": "ok",
        "mode": settings_guard.mode.as_str(),
        "bpm": bpm,
        "clock_state": clock_state,
        "project_present": active.is_some(),
    });

    // F-13: include loop_duration when a project is active; omit entirely when no project.
    if let Some(p) = active {
        resp["loop_duration"] = json!(p.header.loop_duration);
    }

    if settings_guard.mode == EngineMode::Sync {
        if let Some(ref arc) = settings_guard.sync_clock_state {
            let sync_state = arc.lock().unwrap().clone();
            let label = match sync_state {
                SyncClockState::Waiting => "waiting",
                SyncClockState::Tracking => "tracking",
                SyncClockState::Lost => "lost",
            };
            resp["sync_clock_state"] = json!(label);
        }
    }

    resp
}

fn handle_get_project(store: &Arc<RwLock<ProjectStore>>) -> Value {
    let store_read = store.read().unwrap();
    let mut resp = json!({"status": "ok"});

    if let Some(p) = store_read.active() {
        resp["current"] = project_to_json(p);
    }
    if let Some(p) = store_read.pending() {
        resp["pending"] = project_to_json(p);
    }

    resp
}

fn wire_track_to_domain(t: WireTrack) -> Track {
    Track {
        name: t.name,
        channel: t.channel,
        instrument: t.instrument,
        notes: t
            .notes
            .into_iter()
            .map(|[start_tick, duration, pitch, velocity]| Note {
                start_tick,
                duration,
                pitch: pitch as u8,
                velocity: velocity as u8,
            })
            .collect(),
        pitch_bends: t
            .pitch_bends
            .into_iter()
            .map(|[tick, value]| PitchBend { tick, value })
            .collect(),
    }
}

fn validation_error_response(e: ValidationError) -> Value {
    let message = match &e {
        ValidationError::BpmOutOfRange { actual } => {
            format!("BPM {actual} is out of range (20–300)")
        }
        ValidationError::LoopDurationZero => "loop_duration must be greater than 0".to_string(),
        ValidationError::InvalidMidiChannel { track, actual } => {
            format!("track {track}: MIDI channel {actual} is out of range (1–16)")
        }
        ValidationError::InvalidMidiInstrument { track, actual } => {
            format!("track {track}: instrument {actual} is out of range (0–127)")
        }
        ValidationError::NoteDurationZero { track, note } => {
            format!("track {track} note {note}: duration must be > 0")
        }
        ValidationError::NoteStartTickOutOfRange {
            track,
            note,
            start_tick,
            loop_duration,
        } => {
            format!(
                "track {track} note {note}: start_tick {start_tick} is out of range \
                 (must be < loop_duration {loop_duration})"
            )
        }
        ValidationError::NoteDurationExceedsLimit {
            track,
            note,
            duration,
            limit,
        } => {
            format!(
                "track {track} note {note}: duration {duration} exceeds limit {limit} \
                 (2 * loop_duration)"
            )
        }
        ValidationError::PitchBendValueOutOfRange {
            track,
            event,
            actual,
        } => {
            format!("track {track} pitch-bend {event}: value {actual} is out of range (0–16383)")
        }
        ValidationError::PitchBendTickOutOfRange {
            track,
            event,
            tick,
            loop_duration,
        } => {
            format!(
                "track {track} pitch-bend {event}: tick {tick} is out of range \
                 (must be < loop_duration {loop_duration})"
            )
        }
    };
    error_response("validation_error", &message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loop_engine::midi::MockMidiOutput;
    use crate::midi_clock::SyncClockState;
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
        let engine = Arc::new(LoopEngine::new(
            Arc::clone(&store),
            Box::new(MockMidiOutput::new()),
        ));
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
            connection_handler(server, store, engine, settings, shutdown_tx).await;
        });

        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();
        response
    }

    #[tokio::test]
    async fn loop_start_returns_ok() {
        let response = send_command_get_response(r#"{"command":"loop-start"}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert!(response.ends_with('\n'));
    }

    #[tokio::test]
    async fn empty_stream_no_response() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        let (client, server) = UnixStream::pair().unwrap();

        tokio::spawn(async move {
            connection_handler(server, store, engine, settings, shutdown_tx).await;
        });

        drop(client); // close without sending anything — handler should return silently
    }

    #[tokio::test]
    async fn malformed_json_returns_parse_error() {
        let response = send_command_get_response("not json at all").await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "parse_error");
    }

    #[tokio::test]
    async fn missing_command_field_returns_error() {
        let response = send_command_get_response(r#"{"bpm":120}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "missing_command");
    }

    #[tokio::test]
    async fn create_project_stores_project() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        let (client, server) = UnixStream::pair().unwrap();

        let store_clone = Arc::clone(&store);
        tokio::spawn(async move {
            connection_handler(server, store_clone, engine, settings, shutdown_tx).await;
        });

        let cmd = r#"{"command":"create-project","header":{"bpm":120,"loop_duration":1920},"tracks":[{"name":"piano","channel":1,"instrument":0,"notes":[[0,480,60,80]]}]}"#.to_string() + "\n";

        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        drop(client);

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(store.read().unwrap().active().is_some());
    }

    #[tokio::test]
    async fn create_project_bpm_out_of_range() {
        let response = send_command_get_response(
            r#"{"command":"create-project","header":{"bpm":301,"loop_duration":1920},"tracks":[]}"#,
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "validation_error");
        assert!(!v["message"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn modify_project_returns_ok() {
        let response = send_command_get_response(
            r#"{"command":"modify-project","header":{"bpm":140,"loop_duration":1920},"tracks":[{"name":"t","channel":1,"instrument":0,"notes":[[0,480,60,80]]}]}"#,
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "ok");
    }

    #[tokio::test]
    async fn set_bpm_valid() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        let (client, server) = UnixStream::pair().unwrap();
        let settings_clone = Arc::clone(&settings);

        tokio::spawn(async move {
            connection_handler(server, store, engine, settings_clone, shutdown_tx).await;
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

    #[tokio::test]
    async fn set_mode_clock() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        let (client, server) = UnixStream::pair().unwrap();
        let settings_clone = Arc::clone(&settings);

        tokio::spawn(async move {
            connection_handler(server, store, engine, settings_clone, shutdown_tx).await;
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

    #[tokio::test]
    async fn set_mode_invalid() {
        let response = send_command_get_response(r#"{"command":"set-mode","mode":"turbo"}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["code"], "invalid_mode");
    }

    #[tokio::test]
    async fn loop_start_changes_engine_state() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        let (client, server) = UnixStream::pair().unwrap();
        let engine_clone = Arc::clone(&engine);

        tokio::spawn(async move {
            connection_handler(server, store, engine_clone, settings, shutdown_tx).await;
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
            "expected Running or Waiting, got {:?}",
            state
        );
    }

    #[tokio::test]
    async fn loop_stop_changes_engine_state() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        let engine_clone = Arc::clone(&engine);
        engine_clone.start();
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;

        let (client, server) = UnixStream::pair().unwrap();

        tokio::spawn(async move {
            connection_handler(server, store, engine_clone, settings, shutdown_tx).await;
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

    #[tokio::test]
    async fn status_with_project_stopped() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();

        {
            use crate::domain::*;
            let project = Project {
                header: Header {
                    bpm: 120,
                    loop_duration: 1920,
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

        let (client, server) = UnixStream::pair().unwrap();
        let store_clone = Arc::clone(&store);
        let engine_clone = Arc::clone(&engine);
        let settings_clone = Arc::clone(&settings);

        tokio::spawn(async move {
            connection_handler(
                server,
                store_clone,
                engine_clone,
                settings_clone,
                shutdown_tx,
            )
            .await;
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
        assert_eq!(v["loop_duration"], 1920);
        assert!(v.get("time_signature").is_none());
        assert_eq!(v["clock_state"], "stopped");
        assert_eq!(v["project_present"], true);
    }

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
            connection_handler(
                server,
                store_clone,
                engine_clone,
                settings_clone,
                shutdown_tx,
            )
            .await;
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

    //        loop_duration absent (AC-9, F-14)
    #[tokio::test]
    async fn status_no_project() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        settings.lock().unwrap().bpm = 99;

        let (client, server) = UnixStream::pair().unwrap();
        let store_clone = Arc::clone(&store);
        let engine_clone = Arc::clone(&engine);
        let settings_clone = Arc::clone(&settings);

        tokio::spawn(async move {
            connection_handler(
                server,
                store_clone,
                engine_clone,
                settings_clone,
                shutdown_tx,
            )
            .await;
        });

        let cmd = r#"{"command":"status"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();

        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["project_present"], false);
        assert!(v.get("time_signature").is_none());
        assert!(
            v.get("loop_duration").is_none(),
            "loop_duration must be absent when no project"
        );
        assert_eq!(v["bpm"], 99);
    }

    #[tokio::test]
    async fn list_midi_ports_returns_ok_with_ports_array() {
        let response = send_command_get_response(r#"{"command":"list-midi-ports"}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert!(v["ports"].is_array(), "ports should be a JSON array");
    }

    #[tokio::test]
    async fn clock_start_without_project_returns_no_project_error() {
        let response = send_command_get_response(r#"{"command":"clock-start"}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "no_project");
    }

    #[tokio::test]
    async fn clock_start_with_project_returns_ok() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        {
            use crate::domain::*;
            let project = Project {
                header: Header {
                    bpm: 120,
                    loop_duration: 1920,
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

        let (client, server) = tokio::net::UnixStream::pair().unwrap();
        tokio::spawn(async move {
            connection_handler(server, store, engine, settings, shutdown_tx).await;
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

    #[tokio::test]
    async fn stop_command_signals_shutdown() {
        let (store, engine, settings, _) = make_shared_state();
        let (shutdown_tx_chan, mut shutdown_rx) = oneshot::channel::<()>();
        let shutdown_tx = Arc::new(Mutex::new(Some(shutdown_tx_chan)));

        let (client, server) = UnixStream::pair().unwrap();
        tokio::spawn(async move {
            connection_handler(server, store, engine, settings, shutdown_tx).await;
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

    #[tokio::test]
    async fn status_response_includes_mode_field() {
        let response = send_command_get_response(r#"{"command":"status"}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert!(
            v.get("mode").is_some(),
            "status response must include mode field"
        );
        assert_eq!(v["mode"], "standalone");
    }

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
            connection_handler(server, s, e, se, st).await;
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
            "loop must remain running after standalone→clock switch, got {:?}",
            state
        );
        engine_clone.stop();
    }

    #[tokio::test]
    async fn set_mode_standalone_from_clock_while_running_calls_clock_stop() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        {
            use crate::domain::*;
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
            connection_handler(server, s, e, se, st).await;
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
        assert_eq!(
            engine.state(),
            EngineState::Stopped,
            "clock_stop must be called on clock→standalone transition"
        );
        assert_eq!(settings.lock().unwrap().mode, EngineMode::Standalone);
    }

    #[tokio::test]
    async fn set_bpm_in_sync_mode_returns_error() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        settings.lock().unwrap().mode = EngineMode::Sync;

        let (client, server) = UnixStream::pair().unwrap();
        tokio::spawn(async move {
            connection_handler(server, store, engine, settings, shutdown_tx).await;
        });

        let cmd = r#"{"command":"set-bpm","bpm":120}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "sync_mode_active");
    }

    #[tokio::test]
    async fn status_in_sync_mode_includes_sync_clock_state_tracking() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        let sync_state = Arc::new(Mutex::new(SyncClockState::Tracking));
        {
            let mut s = settings.lock().unwrap();
            s.mode = EngineMode::Sync;
            s.sync_clock_state = Some(Arc::clone(&sync_state));
        }

        let (client, server) = UnixStream::pair().unwrap();
        tokio::spawn(async move {
            connection_handler(server, store, engine, settings, shutdown_tx).await;
        });

        let cmd = r#"{"command":"status"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(v["sync_clock_state"], "tracking");
    }

    #[tokio::test]
    async fn status_in_sync_mode_includes_sync_clock_state_lost() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        let sync_state = Arc::new(Mutex::new(SyncClockState::Lost));
        {
            let mut s = settings.lock().unwrap();
            s.mode = EngineMode::Sync;
            s.sync_clock_state = Some(Arc::clone(&sync_state));
        }

        let (client, server) = UnixStream::pair().unwrap();
        tokio::spawn(async move {
            connection_handler(server, store, engine, settings, shutdown_tx).await;
        });

        let cmd = r#"{"command":"status"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["sync_clock_state"], "lost");
    }

    #[tokio::test]
    async fn status_in_standalone_mode_excludes_sync_clock_state() {
        let response = send_command_get_response(r#"{"command":"status"}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(v["mode"], "standalone");
        assert!(
            v.get("sync_clock_state").is_none(),
            "sync_clock_state must not appear in standalone mode"
        );
    }

    #[tokio::test]
    async fn loop_start_in_sync_mode_returns_error() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        settings.lock().unwrap().mode = EngineMode::Sync;

        let (client, server) = UnixStream::pair().unwrap();
        tokio::spawn(async move {
            connection_handler(server, store, engine, settings, shutdown_tx).await;
        });

        let cmd = r#"{"command":"loop-start"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "sync_mode_active");
    }

    #[tokio::test]
    async fn loop_stop_in_sync_mode_returns_error() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        settings.lock().unwrap().mode = EngineMode::Sync;

        let (client, server) = UnixStream::pair().unwrap();
        tokio::spawn(async move {
            connection_handler(server, store, engine, settings, shutdown_tx).await;
        });

        let cmd = r#"{"command":"loop-stop"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "sync_mode_active");
    }

    #[tokio::test]
    async fn set_mode_sync_without_receiver_returns_error() {
        let response = send_command_get_response(r#"{"command":"set-mode","mode":"sync"}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "sync_requires_port");
    }

    // T-7: no project ever active or pending -> "current"/"pending" both omitted
    // (F-2, F-4, AC-2, AC-4)
    #[tokio::test]
    async fn project_query_no_project_omits_current_and_pending() {
        let response = send_command_get_response(r#"{"command":"project"}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert!(v.get("current").is_none());
        assert!(v.get("pending").is_none());
    }

    // T-8: only an active project exists -> "current" present with correct data,
    // "pending" omitted (F-1, F-4, AC-1, AC-4)
    #[tokio::test]
    async fn project_query_active_only_includes_current_omits_pending() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        {
            use crate::domain::*;
            let project = Project {
                header: Header {
                    bpm: 120,
                    loop_duration: 1920,
                },
                tracks: vec![Track {
                    name: "piano".to_string(),
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

        let (client, server) = UnixStream::pair().unwrap();
        tokio::spawn(async move {
            connection_handler(server, store, engine, settings, shutdown_tx).await;
        });

        let cmd = r#"{"command":"project"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();

        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(v["current"]["header"]["bpm"], 120);
        assert_eq!(v["current"]["tracks"][0]["name"], "piano");
        assert!(v.get("pending").is_none());
    }

    // T-9: an active project exists and a second project is staged without
    // committing -> both "current" (original) and "pending" (staged) present with
    // distinct data (F-3, F-5, AC-3, AC-5)
    #[tokio::test]
    async fn project_query_active_and_pending_both_present_and_distinct() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        {
            use crate::domain::*;
            let active_project = Project {
                header: Header {
                    bpm: 120,
                    loop_duration: 1920,
                },
                tracks: vec![],
            };
            store.write().unwrap().set_pending(active_project).unwrap();
            store.write().unwrap().commit_pending();

            let staged_project = Project {
                header: Header {
                    bpm: 140,
                    loop_duration: 960,
                },
                tracks: vec![],
            };
            store.write().unwrap().set_pending(staged_project).unwrap();
        }

        let (client, server) = UnixStream::pair().unwrap();
        tokio::spawn(async move {
            connection_handler(server, store, engine, settings, shutdown_tx).await;
        });

        let cmd = r#"{"command":"project"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();

        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(v["current"]["header"]["bpm"], 120);
        assert_eq!(v["pending"]["header"]["bpm"], 140);
    }

    // T-12: identical ProjectStore state queried once in Standalone mode and once
    // in Sync mode yields byte-identical responses (F-7, AC-6, AC-7, NF-3).
    #[tokio::test]
    async fn project_query_response_identical_across_modes() {
        async fn query_with_mode(mode: EngineMode) -> String {
            let (store, engine, settings, shutdown_tx) = make_shared_state();
            {
                use crate::domain::*;
                let active_project = Project {
                    header: Header {
                        bpm: 120,
                        loop_duration: 1920,
                    },
                    tracks: vec![],
                };
                store.write().unwrap().set_pending(active_project).unwrap();
                store.write().unwrap().commit_pending();

                let staged_project = Project {
                    header: Header {
                        bpm: 140,
                        loop_duration: 960,
                    },
                    tracks: vec![],
                };
                store.write().unwrap().set_pending(staged_project).unwrap();
            }
            settings.lock().unwrap().mode = mode;

            let (client, server) = UnixStream::pair().unwrap();
            tokio::spawn(async move {
                connection_handler(server, store, engine, settings, shutdown_tx).await;
            });

            let cmd = r#"{"command":"project"}"#.to_string() + "\n";
            let mut client = client;
            use tokio::io::AsyncWriteExt;
            client.write_all(cmd.as_bytes()).await.unwrap();
            let mut resp = String::new();
            client.read_to_string(&mut resp).await.unwrap();
            resp
        }

        let standalone_resp = query_with_mode(EngineMode::Standalone).await;
        let sync_resp = query_with_mode(EngineMode::Sync).await;

        assert_eq!(
            standalone_resp, sync_resp,
            "project query response must be identical regardless of daemon mode"
        );
    }

    // T-13: invoking the "project" query (once, and repeated) never mutates
    // ProjectStore::active()/pending() (NF-1).
    #[tokio::test]
    async fn project_query_does_not_mutate_store() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        {
            use crate::domain::*;
            let active_project = Project {
                header: Header {
                    bpm: 120,
                    loop_duration: 1920,
                },
                tracks: vec![],
            };
            store.write().unwrap().set_pending(active_project).unwrap();
            store.write().unwrap().commit_pending();

            let staged_project = Project {
                header: Header {
                    bpm: 140,
                    loop_duration: 960,
                },
                tracks: vec![],
            };
            store.write().unwrap().set_pending(staged_project).unwrap();
        }

        for _ in 0..3 {
            let (client, server) = UnixStream::pair().unwrap();
            let s = Arc::clone(&store);
            let e = Arc::clone(&engine);
            let se = Arc::clone(&settings);
            let st = Arc::clone(&shutdown_tx);
            tokio::spawn(async move {
                connection_handler(server, s, e, se, st).await;
            });

            let cmd = r#"{"command":"project"}"#.to_string() + "\n";
            let mut client = client;
            use tokio::io::AsyncWriteExt;
            client.write_all(cmd.as_bytes()).await.unwrap();
            let mut resp = String::new();
            client.read_to_string(&mut resp).await.unwrap();
        }

        let store_r = store.read().unwrap();
        assert_eq!(store_r.active().unwrap().header.bpm, 120);
        assert_eq!(store_r.pending().unwrap().header.bpm, 140);
    }

    // T-3: project_to_json maps a Project to the WireHeader/WireTrack-mirrored JSON
    // shape, including array-tuple notes and the "pitch-bends" key (F-1, F-3).
    #[test]
    fn project_to_json_maps_full_shape() {
        let project = Project {
            header: Header {
                bpm: 120,
                loop_duration: 1920,
            },
            tracks: vec![Track {
                name: "piano".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![Note {
                    start_tick: 0,
                    duration: 480,
                    pitch: 60,
                    velocity: 80,
                }],
                pitch_bends: vec![PitchBend {
                    tick: 240,
                    value: 8192,
                }],
            }],
        };

        let v = project_to_json(&project);

        assert_eq!(v["header"]["bpm"], 120);
        assert_eq!(v["header"]["loop_duration"], 1920);
        assert_eq!(v["tracks"][0]["name"], "piano");
        assert_eq!(v["tracks"][0]["channel"], 1);
        assert_eq!(v["tracks"][0]["instrument"], 0);
        assert_eq!(v["tracks"][0]["notes"][0], json!([0, 480, 60, 80]));
        assert_eq!(v["tracks"][0]["pitch-bends"][0], json!([240, 8192]));
    }

    // T-5: wire_track_to_domain maps WireTrack.pitch_bends [tick, value] pairs (F-1, F-4, AC-1)
    #[test]
    fn wire_track_to_domain_maps_pitch_bends() {
        let wire = WireTrack {
            name: "piano".to_string(),
            channel: 1,
            instrument: 0,
            notes: vec![],
            pitch_bends: vec![[0, 8192], [240, 0]],
        };
        let track = wire_track_to_domain(wire);
        assert_eq!(track.pitch_bends.len(), 2);
        assert_eq!(track.pitch_bends[0].tick, 0);
        assert_eq!(track.pitch_bends[0].value, 8192);
        assert_eq!(track.pitch_bends[1].tick, 240);
        assert_eq!(track.pitch_bends[1].value, 0);
    }

    #[test]
    fn validation_error_response_loop_duration_zero() {
        let v = validation_error_response(ValidationError::LoopDurationZero);
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "validation_error");
        assert!(!v["message"].as_str().unwrap().is_empty());
    }

    #[test]
    fn validation_error_response_note_start_tick_out_of_range() {
        let v = validation_error_response(ValidationError::NoteStartTickOutOfRange {
            track: 0,
            note: 0,
            start_tick: 100,
            loop_duration: 50,
        });
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "validation_error");
        let msg = v["message"].as_str().unwrap();
        assert!(!msg.is_empty());
        assert!(
            msg.contains("100"),
            "message should include start_tick 100, got: {msg}"
        );
        assert!(
            msg.contains("50"),
            "message should include loop_duration 50, got: {msg}"
        );
    }

    #[test]
    fn validation_error_response_note_duration_exceeds_limit() {
        let v = validation_error_response(ValidationError::NoteDurationExceedsLimit {
            track: 0,
            note: 0,
            duration: 5000,
            limit: 3840,
        });
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "validation_error");
        let msg = v["message"].as_str().unwrap();
        assert!(!msg.is_empty());
        assert!(
            msg.contains("5000"),
            "message should include duration 5000, got: {msg}"
        );
        assert!(
            msg.contains("3840"),
            "message should include limit 3840, got: {msg}"
        );
    }

    // T-15: validation_error_response formats both new pitch-bend variants, naming the
    // track and event (NF-2).
    #[test]
    fn validation_error_response_pitch_bend_value_out_of_range() {
        let v = validation_error_response(ValidationError::PitchBendValueOutOfRange {
            track: 2,
            event: 3,
            actual: 20000,
        });
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "validation_error");
        let msg = v["message"].as_str().unwrap();
        assert!(msg.contains('2'), "message should name track 2, got: {msg}");
        assert!(msg.contains('3'), "message should name event 3, got: {msg}");
        assert!(
            msg.contains("20000"),
            "message should include actual value 20000, got: {msg}"
        );
    }

    #[test]
    fn validation_error_response_pitch_bend_tick_out_of_range() {
        let v = validation_error_response(ValidationError::PitchBendTickOutOfRange {
            track: 1,
            event: 4,
            tick: 5000,
            loop_duration: 1920,
        });
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "validation_error");
        let msg = v["message"].as_str().unwrap();
        assert!(
            msg.contains("5000"),
            "message should include tick 5000, got: {msg}"
        );
        assert!(
            msg.contains("1920"),
            "message should include loop_duration 1920, got: {msg}"
        );
    }

    #[tokio::test]
    async fn create_project_loop_duration_zero() {
        let response = send_command_get_response(
            r#"{"command":"create-project","header":{"bpm":120,"loop_duration":0},"tracks":[]}"#,
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "validation_error");
        assert!(!v["message"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_project_note_start_tick_out_of_range() {
        let response = send_command_get_response(
            r#"{"command":"create-project","header":{"bpm":120,"loop_duration":1920},"tracks":[{"name":"t","channel":1,"instrument":0,"notes":[[1920,480,60,80]]}]}"#,
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "validation_error");
    }

    #[tokio::test]
    async fn create_project_note_duration_zero() {
        let response = send_command_get_response(
            r#"{"command":"create-project","header":{"bpm":120,"loop_duration":1920},"tracks":[{"name":"t","channel":1,"instrument":0,"notes":[[0,0,60,80]]}]}"#,
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "validation_error");
    }

    #[tokio::test]
    async fn create_project_note_duration_exceeds_limit() {
        let response = send_command_get_response(
            r#"{"command":"create-project","header":{"bpm":120,"loop_duration":1920},"tracks":[{"name":"t","channel":1,"instrument":0,"notes":[[0,3841,60,80]]}]}"#,
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "validation_error");
    }

    #[tokio::test]
    async fn create_project_overlapping_notes_ok() {
        let response = send_command_get_response(
            r#"{"command":"create-project","header":{"bpm":120,"loop_duration":1920},"tracks":[{"name":"t","channel":1,"instrument":0,"notes":[[0,480,60,80],[0,480,64,80]]}]}"#,
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "ok");
    }

    #[tokio::test]
    async fn create_project_boundary_note_ok() {
        let response = send_command_get_response(
            r#"{"command":"create-project","header":{"bpm":120,"loop_duration":1920},"tracks":[{"name":"t","channel":1,"instrument":0,"notes":[[0,3840,60,80]]}]}"#,
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "ok");
    }

    #[tokio::test]
    async fn create_project_priority_duration_zero_wins() {
        let response = send_command_get_response(
            r#"{"command":"create-project","header":{"bpm":120,"loop_duration":1920},"tracks":[{"name":"t","channel":1,"instrument":0,"notes":[[2000,0,60,80]]}]}"#,
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "validation_error");
        let msg = v["message"].as_str().unwrap();
        assert!(
            msg.contains("duration"),
            "message should mention duration, got: {msg}"
        );
    }

    #[tokio::test]
    async fn set_bpm_retains_loop_duration() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        {
            use crate::domain::*;
            let project = Project {
                header: Header {
                    bpm: 120,
                    loop_duration: 1920,
                },
                tracks: vec![],
            };
            store.write().unwrap().set_pending(project).unwrap();
            store.write().unwrap().commit_pending();
        }

        let store_clone = Arc::clone(&store);
        let (client, server) = UnixStream::pair().unwrap();
        tokio::spawn(async move {
            connection_handler(server, store_clone, engine, settings, shutdown_tx).await;
        });

        let cmd = r#"{"command":"set-bpm","bpm":140}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();

        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "ok");

        // Commit the pending project (created by set-bpm) and verify it retains loop_duration.
        store.write().unwrap().commit_pending();
        let store_r = store.read().unwrap();
        let project = store_r
            .active()
            .expect("project should be active after commit");
        assert_eq!(project.header.bpm, 140);
        assert_eq!(project.header.loop_duration, 1920);
    }

    #[tokio::test]
    async fn status_with_project_includes_loop_duration() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        {
            use crate::domain::*;
            let project = Project {
                header: Header {
                    bpm: 120,
                    loop_duration: 1920,
                },
                tracks: vec![],
            };
            store.write().unwrap().set_pending(project).unwrap();
            store.write().unwrap().commit_pending();
        }

        let (client, server) = UnixStream::pair().unwrap();
        let store_clone = Arc::clone(&store);
        let engine_clone = Arc::clone(&engine);
        let settings_clone = Arc::clone(&settings);

        tokio::spawn(async move {
            connection_handler(
                server,
                store_clone,
                engine_clone,
                settings_clone,
                shutdown_tx,
            )
            .await;
        });

        let cmd = r#"{"command":"status"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();

        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(v["loop_duration"], 1920);
        assert!(v.get("time_signature").is_none());
    }

    // EP-2 AC-3: get_position with no project loaded returns tick 0, loop_duration null
    #[tokio::test]
    async fn get_position_no_project_returns_zero_tick_null_duration() {
        let response = send_command_get_response(r#"{"command":"get-position"}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["tick"], 0);
        assert!(v["loop_duration"].is_null());
        assert_eq!(v["loop_count"], 0);
    }

    // EP-2 AC-1/AC-5: get_position while playing returns tick and matching loop_duration
    #[tokio::test]
    async fn get_position_while_playing_returns_tick_and_loop_duration() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        {
            use crate::domain::*;
            // Two widely-spaced notes so current_tick sits non-zero for a wide real-time
            // window (the player resets the counter as soon as the built event list for
            // the pass is exhausted, not when loop_duration itself is reached).
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
                    pitch_bends: vec![],
                }],
            };
            store.write().unwrap().set_pending(project).unwrap();
            store.write().unwrap().commit_pending();
        }
        engine.start();
        // loop_duration_ticks is only written at a loop boundary (F-9), so wait for the
        // first pass to complete before it reflects the project's loop_duration.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(800);
        while std::time::Instant::now() < deadline && engine.loop_duration_ticks() == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        assert_eq!(
            engine.loop_duration_ticks(),
            960,
            "precondition: loop_duration_ticks must be populated after the first pass"
        );
        // Now in the second pass; poll until an event has fired so tick is > 0 (AC-1).
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(300);
        while std::time::Instant::now() < deadline && engine.current_tick() == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        assert!(
            engine.current_tick() > 0,
            "precondition: tick must have advanced"
        );

        let (client, server) = UnixStream::pair().unwrap();
        let s = Arc::clone(&store);
        let e = Arc::clone(&engine);
        tokio::spawn(async move {
            connection_handler(server, s, e, settings, shutdown_tx).await;
        });

        let cmd = r#"{"command":"get-position"}"#.to_string() + "\n";
        let mut client = client;
        use tokio::io::AsyncWriteExt;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();

        let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
        assert!(
            v["tick"].as_u64().unwrap() > 0,
            "AC-1: tick must be > 0 after ticks have advanced"
        );
        assert_eq!(v["loop_duration"], 960);
        assert!(
            v["loop_count"].as_u64().unwrap() >= 1,
            "expected loop_count >= 1 after the first pass completed"
        );
        engine.stop();
    }

    // EP-2 AC-4: two sequential get_position responses have non-decreasing tick
    #[tokio::test]
    async fn get_position_tick_monotonically_non_decreasing() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        {
            use crate::domain::*;
            // Low BPM + long loop_duration so the loop does not restart between requests.
            let project = Project {
                header: Header {
                    bpm: 60,
                    loop_duration: 480_000,
                },
                tracks: vec![],
            };
            store.write().unwrap().set_pending(project).unwrap();
            store.write().unwrap().commit_pending();
        }
        engine.start();
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;

        async fn send_get_position(
            store: &Arc<RwLock<ProjectStore>>,
            engine: &Arc<LoopEngine>,
            settings: &Arc<Mutex<EngineSettings>>,
            shutdown_tx: &Arc<Mutex<Option<oneshot::Sender<()>>>>,
        ) -> u64 {
            let (client, server) = UnixStream::pair().unwrap();
            let s = Arc::clone(store);
            let e = Arc::clone(engine);
            let se = Arc::clone(settings);
            let st = Arc::clone(shutdown_tx);
            tokio::spawn(async move {
                connection_handler(server, s, e, se, st).await;
            });
            let cmd = r#"{"command":"get-position"}"#.to_string() + "\n";
            let mut client = client;
            use tokio::io::AsyncWriteExt;
            client.write_all(cmd.as_bytes()).await.unwrap();
            let mut resp = String::new();
            client.read_to_string(&mut resp).await.unwrap();
            let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
            v["tick"].as_u64().unwrap()
        }

        let first = send_get_position(&store, &engine, &settings, &shutdown_tx).await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let second = send_get_position(&store, &engine, &settings, &shutdown_tx).await;

        assert!(
            second >= first,
            "expected second tick ({second}) >= first tick ({first})"
        );
        engine.stop();
    }

    // EP-2 AC-6: while paused, tick is frozen across responses and loop_duration is non-null
    #[tokio::test]
    async fn get_position_while_paused_tick_frozen_and_loop_duration_present() {
        let (store, engine, settings, shutdown_tx) = make_shared_state();
        {
            use crate::domain::*;
            // BPM 300, loop_duration 480 => one loop pass ≈ 200ms.
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
        engine.clock_start();
        // Wait past the first loop boundary so loop_duration_ticks has been written (F-9).
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        engine.clock_pause();

        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        while std::time::Instant::now() < deadline && engine.state() != EngineState::Paused {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(engine.state(), EngineState::Paused);

        async fn send_get_position(
            store: &Arc<RwLock<ProjectStore>>,
            engine: &Arc<LoopEngine>,
            settings: &Arc<Mutex<EngineSettings>>,
            shutdown_tx: &Arc<Mutex<Option<oneshot::Sender<()>>>>,
        ) -> serde_json::Value {
            let (client, server) = UnixStream::pair().unwrap();
            let s = Arc::clone(store);
            let e = Arc::clone(engine);
            let se = Arc::clone(settings);
            let st = Arc::clone(shutdown_tx);
            tokio::spawn(async move {
                connection_handler(server, s, e, se, st).await;
            });
            let cmd = r#"{"command":"get-position"}"#.to_string() + "\n";
            let mut client = client;
            use tokio::io::AsyncWriteExt;
            client.write_all(cmd.as_bytes()).await.unwrap();
            let mut resp = String::new();
            client.read_to_string(&mut resp).await.unwrap();
            serde_json::from_str(resp.trim()).unwrap()
        }

        let first = send_get_position(&store, &engine, &settings, &shutdown_tx).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let second = send_get_position(&store, &engine, &settings, &shutdown_tx).await;

        assert_eq!(
            first["tick"], second["tick"],
            "tick must not advance while paused"
        );
        assert!(
            !second["loop_duration"].is_null(),
            "loop_duration must be non-null while paused"
        );
        engine.clock_stop();
    }

    #[tokio::test]
    async fn status_no_project_excludes_loop_duration() {
        let response = send_command_get_response(r#"{"command":"status"}"#).await;
        let v: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert!(
            v.get("loop_duration").is_none(),
            "loop_duration must be absent when no project"
        );
        assert!(v.get("time_signature").is_none());
    }
}
