// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use midir::os::unix::VirtualOutput;

pub trait ClockOutput: Send {
    fn clock_tick(&mut self) -> Result<(), String>;
    fn clock_start(&mut self) -> Result<(), String>;
    fn clock_continue(&mut self) -> Result<(), String>;
    fn clock_stop(&mut self) -> Result<(), String>;
    // Song Position Pointer (0xF2): a 14-bit count of MIDI beats (1 beat = a sixteenth
    // note = 6 clock pulses) since the start of the song.
    fn song_position(&mut self, position: u16) -> Result<(), String>;
}

#[derive(Debug)]
pub enum MidiPortError {
    NotFound {
        requested: String,
        available: Vec<String>,
    },
    ConnectionFailed(midir::ConnectError<midir::MidiOutput>),
    InitFailed(midir::InitError),
}

impl std::fmt::Display for MidiPortError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MidiPortError::NotFound {
                requested,
                available,
            } => {
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

// midir only stamps outgoing CoreMIDI packets with a real host time when the
// coremidi_send_timestamped feature is enabled (see Cargo.toml); otherwise every packet
// goes out with timestamp 0 and downstream devices can silently drop it.
pub struct MidiClockOutput(midir::MidiOutputConnection);

impl ClockOutput for MidiClockOutput {
    fn clock_tick(&mut self) -> Result<(), String> {
        self.0.send(&[0xF8]).map_err(|e| e.to_string())
    }
    fn clock_start(&mut self) -> Result<(), String> {
        self.0.send(&[0xFA]).map_err(|e| e.to_string())
    }
    fn clock_continue(&mut self) -> Result<(), String> {
        self.0.send(&[0xFB]).map_err(|e| e.to_string())
    }
    fn clock_stop(&mut self) -> Result<(), String> {
        self.0.send(&[0xFC]).map_err(|e| e.to_string())
    }
    fn song_position(&mut self, position: u16) -> Result<(), String> {
        self.0
            .send(&song_position_bytes(position))
            .map_err(|e| e.to_string())
    }
}

fn song_position_bytes(position: u16) -> [u8; 3] {
    [
        0xF2,
        (position & 0x7F) as u8,
        ((position >> 7) & 0x7F) as u8,
    ]
}

pub fn find_port_by_name(names: &[String], target: &str) -> Option<usize> {
    names.iter().position(|n| n == target)
}

pub fn list_port_names() -> Vec<String> {
    let output = match midir::MidiOutput::new("propeller-clock-list") {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    let ports = output.ports();
    ports
        .iter()
        .filter_map(|p| output.port_name(p).ok())
        .collect()
}

pub fn open_port(name: &str) -> Result<MidiClockOutput, MidiPortError> {
    let output = midir::MidiOutput::new("propeller-clock").map_err(MidiPortError::InitFailed)?;
    let ports = output.ports();
    let names: Vec<String> = ports
        .iter()
        .filter_map(|p| output.port_name(p).ok())
        .collect();

    match find_port_by_name(&names, name) {
        Some(idx) => {
            let conn = output
                .connect(&ports[idx], "propeller-clock")
                .map_err(MidiPortError::ConnectionFailed)?;
            Ok(MidiClockOutput(conn))
        }
        None => Err(MidiPortError::NotFound {
            requested: name.to_string(),
            available: names,
        }),
    }
}

pub fn open_virtual_named(name: &str) -> Result<MidiClockOutput, MidiPortError> {
    let output = midir::MidiOutput::new("propeller-clock").map_err(MidiPortError::InitFailed)?;
    let conn = output
        .create_virtual(name)
        .map_err(MidiPortError::ConnectionFailed)?;
    Ok(MidiClockOutput(conn))
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub enum ClockEvent {
    Tick,
    Start,
    Continue,
    Stop,
    SongPosition(u16),
}

#[cfg(test)]
pub struct CapturingClockOutput(pub std::sync::Arc<std::sync::Mutex<Vec<ClockEvent>>>);

#[cfg(test)]
impl CapturingClockOutput {
    pub fn new() -> (Self, std::sync::Arc<std::sync::Mutex<Vec<ClockEvent>>>) {
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        (Self(std::sync::Arc::clone(&events)), events)
    }
}

#[cfg(test)]
impl ClockOutput for CapturingClockOutput {
    fn clock_tick(&mut self) -> Result<(), String> {
        self.0.lock().unwrap().push(ClockEvent::Tick);
        Ok(())
    }
    fn clock_start(&mut self) -> Result<(), String> {
        self.0.lock().unwrap().push(ClockEvent::Start);
        Ok(())
    }
    fn clock_continue(&mut self) -> Result<(), String> {
        self.0.lock().unwrap().push(ClockEvent::Continue);
        Ok(())
    }
    fn clock_stop(&mut self) -> Result<(), String> {
        self.0.lock().unwrap().push(ClockEvent::Stop);
        Ok(())
    }
    fn song_position(&mut self, position: u16) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .push(ClockEvent::SongPosition(position));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_port_exact_match() {
        let names: Vec<String> = vec!["Surge XT".into(), "Surge".into()];
        assert_eq!(find_port_by_name(&names, "Surge"), Some(1));
    }

    #[test]
    fn find_port_not_found() {
        let names: Vec<String> = vec!["Surge XT".into()];
        assert_eq!(find_port_by_name(&names, "Nonexistent"), None);
    }

    #[test]
    fn find_port_empty_slice() {
        assert_eq!(find_port_by_name(&[], "anything"), None);
    }

    #[test]
    fn open_port_not_found_returns_error() {
        let result = open_port("__propeller_clock_nonexistent__");
        match result {
            Err(MidiPortError::NotFound { requested, .. }) => {
                assert_eq!(requested, "__propeller_clock_nonexistent__");
            }
            Err(MidiPortError::InitFailed(_)) => {
                // acceptable on systems without a MIDI subsystem
            }
            _ => panic!("expected NotFound or InitFailed"),
        }
    }

    #[test]
    fn capturing_output_records_events_in_order() {
        let (mut output, events) = CapturingClockOutput::new();
        output.clock_start().unwrap();
        output.clock_tick().unwrap();
        output.clock_tick().unwrap();
        output.clock_stop().unwrap();
        assert_eq!(
            events.lock().unwrap().clone(),
            vec![
                ClockEvent::Start,
                ClockEvent::Tick,
                ClockEvent::Tick,
                ClockEvent::Stop,
            ]
        );
    }

    #[test]
    fn song_position_bytes_zero() {
        assert_eq!(song_position_bytes(0), [0xF2, 0x00, 0x00]);
    }

    #[test]
    fn song_position_bytes_max() {
        assert_eq!(song_position_bytes(16383), [0xF2, 0x7F, 0x7F]);
    }

    #[test]
    fn song_position_bytes_mid_value() {
        assert_eq!(song_position_bytes(120), [0xF2, 0x78, 0x00]);
    }

    #[test]
    fn capturing_output_records_song_position() {
        let (mut output, events) = CapturingClockOutput::new();
        output.song_position(120).unwrap();
        assert_eq!(
            events.lock().unwrap().clone(),
            vec![ClockEvent::SongPosition(120)]
        );
    }
}
