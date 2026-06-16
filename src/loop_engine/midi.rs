// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

#[cfg(test)]
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub struct MidiSendError(String);

impl MidiSendError {
    pub(crate) fn new(msg: impl std::fmt::Display) -> Self {
        MidiSendError(msg.to_string())
    }
}

impl std::fmt::Display for MidiSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for MidiSendError {}

pub trait MidiOutput: Send + 'static {
    fn note_on(&mut self, channel: u8, pitch: u8, velocity: u8) -> Result<(), MidiSendError>;
    fn note_off(&mut self, channel: u8, pitch: u8) -> Result<(), MidiSendError>;
    fn program_change(&mut self, channel: u8, program: u8) -> Result<(), MidiSendError>;
    fn clock_tick(&mut self) -> Result<(), MidiSendError>;
    fn clock_start(&mut self) -> Result<(), MidiSendError>;
    fn clock_continue(&mut self) -> Result<(), MidiSendError>;
    fn clock_stop(&mut self) -> Result<(), MidiSendError>;
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub enum MidiEvent {
    NoteOn {
        channel: u8,
        pitch: u8,
        velocity: u8,
    },
    NoteOff {
        channel: u8,
        pitch: u8,
    },
    ProgramChange {
        channel: u8,
        program: u8,
    },
    ClockTick,
    ClockStart,
    ClockContinue,
    ClockStop,
}

#[cfg(test)]
pub struct MockMidiOutput {
    pub events: Vec<MidiEvent>,
}

#[cfg(test)]
impl MockMidiOutput {
    pub fn new() -> MockMidiOutput {
        MockMidiOutput { events: Vec::new() }
    }
}

#[cfg(test)]
impl MidiOutput for MockMidiOutput {
    fn note_on(&mut self, channel: u8, pitch: u8, velocity: u8) -> Result<(), MidiSendError> {
        self.events.push(MidiEvent::NoteOn {
            channel,
            pitch,
            velocity,
        });
        Ok(())
    }

    fn note_off(&mut self, channel: u8, pitch: u8) -> Result<(), MidiSendError> {
        self.events.push(MidiEvent::NoteOff { channel, pitch });
        Ok(())
    }

    fn program_change(&mut self, channel: u8, program: u8) -> Result<(), MidiSendError> {
        self.events
            .push(MidiEvent::ProgramChange { channel, program });
        Ok(())
    }

    fn clock_tick(&mut self) -> Result<(), MidiSendError> {
        self.events.push(MidiEvent::ClockTick);
        Ok(())
    }

    fn clock_start(&mut self) -> Result<(), MidiSendError> {
        self.events.push(MidiEvent::ClockStart);
        Ok(())
    }

    fn clock_continue(&mut self) -> Result<(), MidiSendError> {
        self.events.push(MidiEvent::ClockContinue);
        Ok(())
    }

    fn clock_stop(&mut self) -> Result<(), MidiSendError> {
        self.events.push(MidiEvent::ClockStop);
        Ok(())
    }
}

#[cfg(test)]
pub struct CapturingMidiOutput {
    events: Arc<Mutex<Vec<MidiEvent>>>,
}

#[cfg(test)]
impl CapturingMidiOutput {
    pub fn new(events: Arc<Mutex<Vec<MidiEvent>>>) -> Self {
        CapturingMidiOutput { events }
    }
}

#[cfg(test)]
impl MidiOutput for CapturingMidiOutput {
    fn note_on(&mut self, channel: u8, pitch: u8, velocity: u8) -> Result<(), MidiSendError> {
        self.events.lock().unwrap().push(MidiEvent::NoteOn {
            channel,
            pitch,
            velocity,
        });
        Ok(())
    }

    fn note_off(&mut self, channel: u8, pitch: u8) -> Result<(), MidiSendError> {
        self.events
            .lock()
            .unwrap()
            .push(MidiEvent::NoteOff { channel, pitch });
        Ok(())
    }

    fn program_change(&mut self, channel: u8, program: u8) -> Result<(), MidiSendError> {
        self.events
            .lock()
            .unwrap()
            .push(MidiEvent::ProgramChange { channel, program });
        Ok(())
    }

    fn clock_tick(&mut self) -> Result<(), MidiSendError> {
        self.events.lock().unwrap().push(MidiEvent::ClockTick);
        Ok(())
    }

    fn clock_start(&mut self) -> Result<(), MidiSendError> {
        self.events.lock().unwrap().push(MidiEvent::ClockStart);
        Ok(())
    }

    fn clock_continue(&mut self) -> Result<(), MidiSendError> {
        self.events.lock().unwrap().push(MidiEvent::ClockContinue);
        Ok(())
    }

    fn clock_stop(&mut self) -> Result<(), MidiSendError> {
        self.events.lock().unwrap().push(MidiEvent::ClockStop);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_records_events_in_order() {
        let mut m = MockMidiOutput::new();
        m.note_on(1, 60, 80).unwrap();
        m.note_off(1, 60).unwrap();
        m.program_change(1, 42).unwrap();
        assert_eq!(
            m.events,
            vec![
                MidiEvent::NoteOn {
                    channel: 1,
                    pitch: 60,
                    velocity: 80
                },
                MidiEvent::NoteOff {
                    channel: 1,
                    pitch: 60
                },
                MidiEvent::ProgramChange {
                    channel: 1,
                    program: 42
                },
            ]
        );
    }

    #[test]
    fn mock_records_clock_events_in_order() {
        let mut m = MockMidiOutput::new();
        m.note_on(1, 60, 80).unwrap();
        m.clock_start().unwrap();
        m.clock_tick().unwrap();
        m.clock_continue().unwrap();
        m.clock_stop().unwrap();
        m.note_off(1, 60).unwrap();
        assert_eq!(
            m.events,
            vec![
                MidiEvent::NoteOn {
                    channel: 1,
                    pitch: 60,
                    velocity: 80
                },
                MidiEvent::ClockStart,
                MidiEvent::ClockTick,
                MidiEvent::ClockContinue,
                MidiEvent::ClockStop,
                MidiEvent::NoteOff {
                    channel: 1,
                    pitch: 60
                },
            ]
        );
    }
}
