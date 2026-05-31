// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::midi_clock::SyncClockState;

#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
pub enum Command {
    CreateProject { header: WireHeader, tracks: Vec<WireTrack> },
    ModifyProject { header: WireHeader, tracks: Vec<WireTrack> },
    SetBpm { bpm: f64 },
    SetMode { mode: String },
    LoopStart,
    LoopStop,
    ClockStart,
    ClockPause,
    ClockResume,
    ClockStop,
    ListMidiPorts,
    Status,
    Stop,
}

#[derive(Debug, Deserialize)]
pub struct WireHeader {
    pub bpm: f64,
    pub time_signature: WireTimeSignature,
}

#[derive(Debug, Deserialize)]
pub struct WireTimeSignature {
    pub numerator: u32,
    pub denominator: u32,
}

#[derive(Debug, Deserialize)]
pub struct WireTrack {
    pub name: String,
    pub channel: u8,
    pub instrument: u8,
    pub bars: Vec<WireBar>,
}

#[derive(Debug, Deserialize)]
pub struct WireBar {
    pub notes: Vec<WireNote>,
}

#[derive(Debug, Deserialize)]
pub struct WireNote {
    pub rest: Option<bool>,
    pub pitch: Option<u8>,
    pub velocity: Option<u8>,
    pub duration_ticks: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EngineMode {
    Standalone,
    Clock,
    Sync,
}

impl EngineMode {
    pub fn from_str(s: &str) -> Option<EngineMode> {
        match s {
            "standalone" => Some(EngineMode::Standalone),
            "clock" => Some(EngineMode::Clock),
            "sync" => Some(EngineMode::Sync),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            EngineMode::Standalone => "standalone",
            EngineMode::Clock => "clock",
            EngineMode::Sync => "sync",
        }
    }
}

pub struct EngineSettings {
    pub mode: EngineMode,
    pub bpm: u32,
    /// Present only when the daemon was started with --sync; used by Status and SetMode handlers.
    pub sync_clock_state: Option<Arc<Mutex<SyncClockState>>>,
}

impl EngineSettings {
    pub fn new() -> Self {
        EngineSettings { mode: EngineMode::Standalone, bpm: 120, sync_clock_state: None }
    }
}

pub fn ok_response() -> Value {
    json!({"status": "ok"})
}

pub fn error_response(code: &str, message: &str) -> Value {
    json!({"status": "error", "code": code, "message": message})
}

#[cfg(test)]
mod tests {
    use super::*;

    // T-1: deserialize loop-start and set-bpm commands
    #[test]
    fn deserialize_loop_start() {
        let cmd: Command = serde_json::from_str(r#"{"command":"loop-start"}"#).unwrap();
        assert!(matches!(cmd, Command::LoopStart));
    }

    #[test]
    fn deserialize_set_bpm() {
        let cmd: Command = serde_json::from_str(r#"{"command":"set-bpm","bpm":120}"#).unwrap();
        match cmd {
            Command::SetBpm { bpm } => assert_eq!(bpm, 120.0),
            _ => panic!("expected SetBpm"),
        }
    }

    // T-3: missing "command" field → serde error
    #[test]
    fn deserialize_missing_command_field_fails() {
        let result: Result<Command, _> = serde_json::from_str(r#"{"bpm":120}"#);
        assert!(result.is_err());
    }

    // T-4: unknown command → serde error
    #[test]
    fn deserialize_unknown_command_fails() {
        let result: Result<Command, _> = serde_json::from_str(r#"{"command":"unknownxyz"}"#);
        assert!(result.is_err());
    }

    // T-12: list-midi-ports deserialises to Command::ListMidiPorts
    #[test]
    fn deserialize_list_midi_ports() {
        let cmd: Command = serde_json::from_str(r#"{"command":"list-midi-ports"}"#).unwrap();
        assert!(matches!(cmd, Command::ListMidiPorts));
    }

    // T-11 (EP-5): clock command variants deserialise correctly
    #[test]
    fn deserialize_clock_start() {
        let cmd: Command = serde_json::from_str(r#"{"command":"clock-start"}"#).unwrap();
        assert!(matches!(cmd, Command::ClockStart));
    }

    #[test]
    fn deserialize_clock_pause() {
        let cmd: Command = serde_json::from_str(r#"{"command":"clock-pause"}"#).unwrap();
        assert!(matches!(cmd, Command::ClockPause));
    }

    #[test]
    fn deserialize_clock_resume() {
        let cmd: Command = serde_json::from_str(r#"{"command":"clock-resume"}"#).unwrap();
        assert!(matches!(cmd, Command::ClockResume));
    }

    #[test]
    fn deserialize_clock_stop() {
        let cmd: Command = serde_json::from_str(r#"{"command":"clock-stop"}"#).unwrap();
        assert!(matches!(cmd, Command::ClockStop));
    }

    // T-1 (EP-7): EngineSettings::new() defaults to Standalone mode (F-1, F-10, AC-1)
    #[test]
    fn engine_settings_new_default_mode_is_standalone() {
        let settings = EngineSettings::new();
        assert_eq!(settings.mode, EngineMode::Standalone);
    }

    // T-5: response helpers serialise correctly
    #[test]
    fn ok_response_shape() {
        let v = ok_response();
        assert_eq!(v["status"], "ok");
    }

    #[test]
    fn error_response_shape() {
        let v = error_response("parse_error", "bad json");
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "parse_error");
        assert_eq!(v["message"], "bad json");
    }

    // EP-6: EngineMode::Sync serialises to "sync"
    #[test]
    fn engine_mode_sync_as_str() {
        assert_eq!(EngineMode::Sync.as_str(), "sync");
    }

    #[test]
    fn engine_mode_sync_from_str() {
        assert_eq!(EngineMode::from_str("sync"), Some(EngineMode::Sync));
    }
}
