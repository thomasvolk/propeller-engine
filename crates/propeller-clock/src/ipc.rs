// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::oneshot;
use tracing::error;

use crate::engine::{ClockEngine, ClockState};

#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
pub enum Command {
    Start,
    Pause,
    Resume,
    Stop,
    Status,
    Bpm { bpm: u32 },
}

pub struct ClockSettings {
    /// Name of the MIDI port in use: either the created virtual "propeller-clock" port,
    /// or the external port named by PROPELLER_CLOCK_PORT.
    pub port_name: String,
}

fn ok_response() -> Value {
    json!({"status": "ok"})
}

fn error_response(code: &str, message: &str) -> Value {
    json!({"status": "error", "code": code, "message": message})
}

fn state_str(state: ClockState) -> &'static str {
    match state {
        ClockState::Stopped => "stopped",
        ClockState::Running => "running",
        ClockState::Paused => "paused",
    }
}

fn dispatch(line: &str, engine: &Arc<ClockEngine>, settings: &ClockSettings) -> Value {
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
        Command::Start => {
            engine.start();
            ok_response()
        }
        Command::Pause => {
            if engine.state() != ClockState::Running {
                return error_response("invalid_state", "pause requires the clock to be running");
            }
            engine.pause();
            ok_response()
        }
        Command::Resume => {
            if engine.state() != ClockState::Paused {
                return error_response("invalid_state", "resume requires the clock to be paused");
            }
            engine.resume();
            ok_response()
        }
        Command::Stop => {
            engine.stop();
            ok_response()
        }
        Command::Bpm { bpm } => {
            if !(20..=300).contains(&bpm) {
                return error_response("bpm_out_of_range", "BPM must be between 20 and 300");
            }
            engine.set_bpm(bpm as f64);
            ok_response()
        }
        Command::Status => json!({
            "status": "ok",
            "state": state_str(engine.state()),
            "bpm": engine.bpm(),
            "port": settings.port_name,
            "tick": engine.pulse_count(),
        }),
    }
}

pub async fn connection_handler(
    stream: UnixStream,
    engine: Arc<ClockEngine>,
    settings: Arc<ClockSettings>,
    shutdown_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();

    match reader.read_line(&mut line).await {
        Ok(0) => return,
        Ok(_) => {}
        Err(_) => return,
    }

    let is_stop = serde_json::from_str::<Value>(line.trim_end_matches('\n'))
        .ok()
        .and_then(|v| {
            v.get("command")
                .and_then(|c| c.as_str())
                .map(str::to_string)
        })
        == Some("stop".to_string());

    let response = dispatch(line.trim_end_matches('\n'), &engine, &settings);

    let mut stream = reader.into_inner();
    let response_str = serde_json::to_string(&response).unwrap() + "\n";
    let _ = stream.write_all(response_str.as_bytes()).await;

    if is_stop {
        let _ = stream.flush().await;
        let mut tx_guard = shutdown_tx.lock().unwrap();
        if let Some(tx) = tx_guard.take() {
            let _ = tx.send(());
        }
    }
}

pub async fn run_ipc_server(
    listener: UnixListener,
    engine: Arc<ClockEngine>,
    settings: Arc<ClockSettings>,
    shutdown_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let engine = Arc::clone(&engine);
                let settings = Arc::clone(&settings);
                let shutdown_tx = Arc::clone(&shutdown_tx);
                tokio::spawn(async move {
                    connection_handler(stream, engine, settings, shutdown_tx).await;
                });
            }
            Err(e) => {
                error!("accept error: {e}");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi::CapturingClockOutput;
    use tokio::io::AsyncReadExt;

    fn make_engine() -> Arc<ClockEngine> {
        let (output, _) = CapturingClockOutput::new();
        Arc::new(ClockEngine::new(Box::new(output), 120.0))
    }

    fn make_settings() -> Arc<ClockSettings> {
        Arc::new(ClockSettings {
            port_name: "propeller-clock".to_string(),
        })
    }

    fn make_shutdown() -> Arc<Mutex<Option<oneshot::Sender<()>>>> {
        let (tx, _rx) = oneshot::channel();
        Arc::new(Mutex::new(Some(tx)))
    }

    async fn send_and_get_response(command_json: &str) -> String {
        let engine = make_engine();
        let settings = make_settings();
        let shutdown_tx = make_shutdown();
        let (client, server) = UnixStream::pair().unwrap();
        let cmd = command_json.to_string() + "\n";

        tokio::spawn(async move {
            connection_handler(server, engine, settings, shutdown_tx).await;
        });

        let mut client = client;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();
        response
    }

    #[test]
    fn deserialize_start() {
        let cmd: Command = serde_json::from_str(r#"{"command":"start"}"#).unwrap();
        assert!(matches!(cmd, Command::Start));
    }

    #[test]
    fn deserialize_bpm() {
        let cmd: Command = serde_json::from_str(r#"{"command":"bpm","bpm":140}"#).unwrap();
        match cmd {
            Command::Bpm { bpm } => assert_eq!(bpm, 140),
            _ => panic!("expected Bpm"),
        }
    }

    #[test]
    fn deserialize_missing_command_fails() {
        let result: Result<Command, _> = serde_json::from_str(r#"{"bpm":120}"#);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_unknown_command_fails() {
        let result: Result<Command, _> = serde_json::from_str(r#"{"command":"unknownxyz"}"#);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn start_returns_ok() {
        let response = send_and_get_response(r#"{"command":"start"}"#).await;
        let v: Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "ok");
    }

    #[tokio::test]
    async fn malformed_json_returns_parse_error() {
        let response = send_and_get_response("not json").await;
        let v: Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "parse_error");
    }

    #[tokio::test]
    async fn missing_command_returns_error() {
        let response = send_and_get_response(r#"{"bpm":120}"#).await;
        let v: Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["code"], "missing_command");
    }

    #[tokio::test]
    async fn pause_without_running_returns_invalid_state() {
        let response = send_and_get_response(r#"{"command":"pause"}"#).await;
        let v: Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "invalid_state");
    }

    #[tokio::test]
    async fn resume_without_paused_returns_invalid_state() {
        let response = send_and_get_response(r#"{"command":"resume"}"#).await;
        let v: Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "invalid_state");
    }

    #[tokio::test]
    async fn bpm_out_of_range_low() {
        let response = send_and_get_response(r#"{"command":"bpm","bpm":19}"#).await;
        let v: Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["code"], "bpm_out_of_range");
    }

    #[tokio::test]
    async fn bpm_out_of_range_high() {
        let response = send_and_get_response(r#"{"command":"bpm","bpm":301}"#).await;
        let v: Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["code"], "bpm_out_of_range");
    }

    #[tokio::test]
    async fn bpm_valid_updates_engine() {
        let engine = make_engine();
        let settings = make_settings();
        let shutdown_tx = make_shutdown();
        let (client, server) = UnixStream::pair().unwrap();
        let engine_clone = Arc::clone(&engine);

        tokio::spawn(async move {
            connection_handler(server, engine_clone, settings, shutdown_tx).await;
        });

        let cmd = r#"{"command":"bpm","bpm":150}"#.to_string() + "\n";
        let mut client = client;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();

        let v: Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(engine.bpm(), 150.0);
    }

    #[tokio::test]
    async fn status_reports_expected_fields() {
        let response = send_and_get_response(r#"{"command":"status"}"#).await;
        let v: Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(v["state"], "stopped");
        assert_eq!(v["bpm"], 120.0);
        assert_eq!(v["port"], "propeller-clock");
        assert_eq!(v["tick"], 0);
    }

    #[tokio::test]
    async fn stop_command_signals_shutdown() {
        let engine = make_engine();
        let settings = make_settings();
        let (tx, mut rx) = oneshot::channel::<()>();
        let shutdown_tx = Arc::new(Mutex::new(Some(tx)));

        let (client, server) = UnixStream::pair().unwrap();
        tokio::spawn(async move {
            connection_handler(server, engine, settings, shutdown_tx).await;
        });

        let cmd = r#"{"command":"stop"}"#.to_string() + "\n";
        let mut client = client;
        client.write_all(cmd.as_bytes()).await.unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).await.unwrap();

        let v: Value = serde_json::from_str(resp.trim()).unwrap();
        assert_eq!(v["status"], "ok");
        assert!(rx.try_recv().is_ok());
    }

    #[tokio::test]
    async fn empty_stream_no_response() {
        let engine = make_engine();
        let settings = make_settings();
        let shutdown_tx = make_shutdown();
        let (client, server) = UnixStream::pair().unwrap();

        tokio::spawn(async move {
            connection_handler(server, engine, settings, shutdown_tx).await;
        });

        drop(client);
    }
}
