// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::midi_clock::SyncClockState;

#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
pub enum Command {
    CreateProject {
        header: WireHeader,
        tracks: Vec<WireTrack>,
    },
    ModifyProject {
        header: WireHeader,
        tracks: Vec<WireTrack>,
    },
    SetBpm {
        bpm: f64,
    },
    SetMode {
        mode: String,
    },
    LoopStart,
    LoopStop,
    ClockStart,
    ClockPause,
    ClockResume,
    ClockStop,
    ListMidiPorts,
    Status,
    Project,
    Stop,
    GetPosition,
}

#[derive(Debug, Deserialize)]
pub struct WireHeader {
    pub bpm: u32,
    pub loop_duration: u32,
}

#[derive(Debug, Deserialize)]
pub struct WireTrack {
    pub name: String,
    pub channel: u8,
    pub instrument: u8,
    pub notes: Vec<[u32; 4]>,
    #[serde(default, rename = "pitch-bends")]
    pub pitch_bends: Vec<[u32; 2]>,
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
    /// Name of the configured MIDI output port; None when using the fallback virtual port.
    pub midi_port_name: Option<String>,
    /// Name of the MIDI input port used for clock sync; present only when the sync receiver started.
    pub sync_port_name: Option<String>,
}

impl EngineSettings {
    pub fn new() -> Self {
        EngineSettings {
            mode: EngineMode::Standalone,
            bpm: 120,
            sync_clock_state: None,
            midi_port_name: None,
            sync_port_name: None,
        }
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

    // T-5: {"command":"project"} deserialises to Command::Project (F-6)
    #[test]
    fn deserialize_project() {
        let cmd: Command = serde_json::from_str(r#"{"command":"project"}"#).unwrap();
        assert!(matches!(cmd, Command::Project));
    }

    #[test]
    fn deserialize_missing_command_field_fails() {
        let result: Result<Command, _> = serde_json::from_str(r#"{"bpm":120}"#);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_unknown_command_fails() {
        let result: Result<Command, _> = serde_json::from_str(r#"{"command":"unknownxyz"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_list_midi_ports() {
        let cmd: Command = serde_json::from_str(r#"{"command":"list-midi-ports"}"#).unwrap();
        assert!(matches!(cmd, Command::ListMidiPorts));
    }

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

    #[test]
    fn engine_settings_new_default_mode_is_standalone() {
        let settings = EngineSettings::new();
        assert_eq!(settings.mode, EngineMode::Standalone);
    }

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

    #[test]
    fn engine_mode_sync_as_str() {
        assert_eq!(EngineMode::Sync.as_str(), "sync");
    }

    #[test]
    fn engine_mode_sync_from_str() {
        assert_eq!(EngineMode::from_str("sync"), Some(EngineMode::Sync));
    }

    // EP-NP-2: WireHeader deserialises with bpm and loop_duration
    #[test]
    fn wire_header_deserialises() {
        let h: WireHeader = serde_json::from_str(r#"{"bpm":120,"loop_duration":1920}"#).unwrap();
        assert_eq!(h.bpm, 120);
        assert_eq!(h.loop_duration, 1920);
    }

    // EP-NP-2: WireTrack notes are arrays of four u32
    #[test]
    fn wire_track_notes_deserialise() {
        let t: WireTrack = serde_json::from_str(
            r#"{"name":"piano","channel":1,"instrument":0,"notes":[[0,480,60,80]]}"#,
        )
        .unwrap();
        assert_eq!(t.notes.len(), 1);
        assert_eq!(t.notes[0], [0, 480, 60, 80]);
    }

    // EP-2: GetPosition deserialises from the "command"-tagged wire format
    #[test]
    fn deserialize_get_position() {
        let cmd: Command = serde_json::from_str(r#"{"command":"get-position"}"#).unwrap();
        assert!(matches!(cmd, Command::GetPosition));
    }

    // T-3: WireTrack deserialises "pitch-bends" and defaults to empty when absent (F-1, F-9)
    #[test]
    fn wire_track_pitch_bends_deserialise() {
        let t: WireTrack = serde_json::from_str(
            r#"{"name":"piano","channel":1,"instrument":0,"notes":[],"pitch-bends":[[0,8192],[240,0]]}"#,
        )
        .unwrap();
        assert_eq!(t.pitch_bends.len(), 2);
        assert_eq!(t.pitch_bends[0], [0, 8192]);
        assert_eq!(t.pitch_bends[1], [240, 0]);
    }

    #[test]
    fn wire_track_pitch_bends_absent_defaults_to_empty() {
        let t: WireTrack =
            serde_json::from_str(r#"{"name":"piano","channel":1,"instrument":0,"notes":[]}"#)
                .unwrap();
        assert_eq!(t.pitch_bends.len(), 0);
    }

    // EP-NP-2: create-project with new wire format deserialises correctly
    #[test]
    fn deserialize_create_project_new_format() {
        let cmd: Command = serde_json::from_str(
            r#"{"command":"create-project","header":{"bpm":120,"loop_duration":1920},"tracks":[{"name":"p","channel":1,"instrument":0,"notes":[[0,480,60,80]]}]}"#,
        )
        .unwrap();
        match cmd {
            Command::CreateProject { header, tracks } => {
                assert_eq!(header.bpm, 120);
                assert_eq!(header.loop_duration, 1920);
                assert_eq!(tracks.len(), 1);
                assert_eq!(tracks[0].notes[0], [0, 480, 60, 80]);
            }
            _ => panic!("expected CreateProject"),
        }
    }
}
