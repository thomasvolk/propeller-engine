use midir::os::unix::VirtualOutput;
use serde::Serialize;

use crate::loop_engine::midi::MidiOutput;

#[derive(Debug, Serialize, Clone)]
pub struct MidiPortInfo {
    pub index: usize,
    pub name: String,
}

#[derive(Debug)]
pub enum MidiPortError {
    NotFound { requested: String, available: Vec<String> },
    ConnectionFailed(midir::ConnectError<midir::MidiOutput>),
    InitFailed(midir::InitError),
}

impl std::fmt::Display for MidiPortError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MidiPortError::NotFound { requested, available } => {
                write!(
                    f,
                    "MIDI port {:?} not found; available ports: [{}]",
                    requested,
                    available.join(", ")
                )
            }
            MidiPortError::ConnectionFailed(e) => write!(f, "MIDI connection failed: {e}"),
            MidiPortError::InitFailed(e) => write!(f, "MIDI init failed: {e}"),
        }
    }
}

pub struct MidiPortOutput(midir::MidiOutputConnection);

impl MidiOutput for MidiPortOutput {
    fn note_on(&mut self, channel: u8, pitch: u8, velocity: u8) {
        let bytes = note_on_bytes(channel, pitch, velocity);
        let _ = self.0.send(&bytes);
    }

    fn note_off(&mut self, channel: u8, pitch: u8) {
        let bytes = note_off_bytes(channel, pitch);
        let _ = self.0.send(&bytes);
    }

    fn program_change(&mut self, channel: u8, program: u8) {
        let bytes = program_change_bytes(channel, program);
        let _ = self.0.send(&bytes);
    }
}

pub fn find_port_by_name(names: &[String], target: &str) -> Option<usize> {
    names.iter().position(|n| n == target)
}

fn note_on_bytes(channel: u8, pitch: u8, velocity: u8) -> [u8; 3] {
    [0x90 | (channel - 1), pitch, velocity]
}

fn note_off_bytes(channel: u8, pitch: u8) -> [u8; 3] {
    [0x80 | (channel - 1), pitch, 0]
}

fn program_change_bytes(channel: u8, program: u8) -> [u8; 2] {
    [0xC0 | (channel - 1), program]
}

pub fn list_ports() -> Vec<MidiPortInfo> {
    let output = match midir::MidiOutput::new("propeller-list") {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    let ports = output.ports();
    ports
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            output.port_name(p).ok().map(|name| MidiPortInfo { index: i, name })
        })
        .collect()
}

pub fn open_port(name: &str) -> Result<MidiPortOutput, MidiPortError> {
    let output = midir::MidiOutput::new("propeller").map_err(MidiPortError::InitFailed)?;
    let ports = output.ports();
    let names: Vec<String> = ports
        .iter()
        .filter_map(|p| output.port_name(p).ok())
        .collect();

    match find_port_by_name(&names, name) {
        Some(idx) => {
            let conn = output
                .connect(&ports[idx], "propeller")
                .map_err(MidiPortError::ConnectionFailed)?;
            Ok(MidiPortOutput(conn))
        }
        None => Err(MidiPortError::NotFound {
            requested: name.to_string(),
            available: names,
        }),
    }
}

pub fn open_virtual() -> Result<MidiPortOutput, MidiPortError> {
    let output = midir::MidiOutput::new("propeller").map_err(MidiPortError::InitFailed)?;
    let conn = output
        .create_virtual("propeller")
        .map_err(MidiPortError::ConnectionFailed)?;
    Ok(MidiPortOutput(conn))
}

#[cfg(test)]
mod tests {
    use super::*;

    // T-2: find_port_by_name exact match, order matters
    #[test]
    fn find_port_exact_match_first() {
        let names: Vec<String> = vec!["Surge XT".into(), "Surge".into()];
        assert_eq!(find_port_by_name(&names, "Surge XT"), Some(0));
    }

    #[test]
    fn find_port_exact_match_second() {
        let names: Vec<String> = vec!["Surge".into(), "Surge XT".into()];
        assert_eq!(find_port_by_name(&names, "Surge XT"), Some(1));
    }

    // T-3: prefix alone does not match
    #[test]
    fn find_port_prefix_not_matched() {
        let names: Vec<String> = vec!["Surge XT".into()];
        assert_eq!(find_port_by_name(&names, "Surge"), None);
    }

    // T-4: empty slice → None
    #[test]
    fn find_port_empty_slice() {
        assert_eq!(find_port_by_name(&[], "anything"), None);
    }

    // T-6: note_on_bytes
    #[test]
    fn note_on_bytes_ch1() {
        assert_eq!(note_on_bytes(1, 60, 80), [0x90, 60, 80]);
    }

    #[test]
    fn note_on_bytes_ch16() {
        assert_eq!(note_on_bytes(16, 60, 80), [0x9F, 60, 80]);
    }

    // T-7: note_off_bytes
    #[test]
    fn note_off_bytes_ch1() {
        assert_eq!(note_off_bytes(1, 60), [0x80, 60, 0]);
    }

    #[test]
    fn note_off_bytes_ch16() {
        assert_eq!(note_off_bytes(16, 60), [0x8F, 60, 0]);
    }

    // T-8: program_change_bytes
    #[test]
    fn program_change_bytes_ch1() {
        assert_eq!(program_change_bytes(1, 42), [0xC0, 42]);
    }

    #[test]
    fn program_change_bytes_ch2() {
        assert_eq!(program_change_bytes(2, 0), [0xC1, 0]);
    }

    // T-10: MidiPortInfo serialises to {"index":0,"name":"Surge XT"}
    #[test]
    fn midi_port_info_serialises() {
        let info = MidiPortInfo { index: 0, name: "Surge XT".into() };
        let s = serde_json::to_string(&info).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["index"], 0);
        assert_eq!(v["name"], "Surge XT");
    }

    // T-19: open_port with non-existent name → NotFound error
    #[test]
    fn open_port_not_found() {
        let result = open_port("__propeller_nonexistent__");
        match result {
            Err(MidiPortError::NotFound { requested, available: _ }) => {
                assert_eq!(requested, "__propeller_nonexistent__");
            }
            Err(MidiPortError::InitFailed(_)) => {
                // acceptable on systems without a MIDI subsystem
            }
            _ => panic!("expected NotFound or InitFailed"),
        }
    }

    // T-17: #[ignore] integration — MidiPortOutput loopback via virtual port
    #[test]
    #[ignore]
    fn midi_port_output_loopback() {
        use std::sync::{Arc, Mutex};

        let received: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
        let received_clone = Arc::clone(&received);

        let midi_in = midir::MidiInput::new("propeller-test-in").unwrap();
        let midi_out = midir::MidiOutput::new("propeller-test-out").unwrap();

        // Create virtual output
        let out_conn = midi_out.create_virtual("propeller-test").unwrap();
        let mut port_out = MidiPortOutput(out_conn);

        // Small delay so the virtual port appears
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Find and connect input to the virtual output
        let in_ports = midi_in.ports();
        let test_port = in_ports
            .iter()
            .find(|p| midi_in.port_name(p).unwrap_or_default() == "propeller-test")
            .expect("virtual port not found");

        let _in_conn = midi_in
            .connect(
                test_port,
                "propeller-test-in",
                move |_, data, _| {
                    received_clone.lock().unwrap().push(data.to_vec());
                },
                (),
            )
            .unwrap();

        port_out.note_on(1, 60, 80);
        port_out.note_off(1, 60);
        port_out.program_change(1, 42);

        std::thread::sleep(std::time::Duration::from_millis(100));

        let msgs = received.lock().unwrap().clone();
        assert!(msgs.iter().any(|m| m == &[0x90, 60, 80]), "note_on not received");
        assert!(msgs.iter().any(|m| m == &[0x80, 60, 0]), "note_off not received");
        assert!(msgs.iter().any(|m| m == &[0xC0, 42]), "program_change not received");
    }

    // T-21: #[ignore] integration — open_virtual returns Ok and port is visible
    #[test]
    #[ignore]
    fn open_virtual_creates_port() {
        let result = open_virtual();
        assert!(result.is_ok(), "open_virtual failed: {:?}", result.err());

        // The virtual port named "propeller" should appear in list_ports
        std::thread::sleep(std::time::Duration::from_millis(50));
        let ports = list_ports();
        assert!(
            ports.iter().any(|p| p.name.contains("propeller")),
            "virtual port not found in list: {:?}",
            ports
        );
    }
}
