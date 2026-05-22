pub trait MidiOutput: Send + 'static {
    fn note_on(&mut self, channel: u8, pitch: u8, velocity: u8);
    fn note_off(&mut self, channel: u8, pitch: u8);
    fn program_change(&mut self, channel: u8, program: u8);
}

#[derive(Debug, Clone, PartialEq)]
pub enum MidiEvent {
    NoteOn { channel: u8, pitch: u8, velocity: u8 },
    NoteOff { channel: u8, pitch: u8 },
    ProgramChange { channel: u8, program: u8 },
}

pub struct MockMidiOutput {
    pub events: Vec<MidiEvent>,
}

impl MockMidiOutput {
    pub fn new() -> MockMidiOutput {
        MockMidiOutput { events: Vec::new() }
    }
}

impl MidiOutput for MockMidiOutput {
    fn note_on(&mut self, channel: u8, pitch: u8, velocity: u8) {
        self.events.push(MidiEvent::NoteOn { channel, pitch, velocity });
    }

    fn note_off(&mut self, channel: u8, pitch: u8) {
        self.events.push(MidiEvent::NoteOff { channel, pitch });
    }

    fn program_change(&mut self, channel: u8, program: u8) {
        self.events.push(MidiEvent::ProgramChange { channel, program });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // T-9: MockMidiOutput records NoteOn, NoteOff, ProgramChange in insertion order
    #[test]
    fn mock_records_events_in_order() {
        let mut m = MockMidiOutput::new();
        m.note_on(1, 60, 80);
        m.note_off(1, 60);
        m.program_change(1, 42);
        assert_eq!(m.events, vec![
            MidiEvent::NoteOn { channel: 1, pitch: 60, velocity: 80 },
            MidiEvent::NoteOff { channel: 1, pitch: 60 },
            MidiEvent::ProgramChange { channel: 1, program: 42 },
        ]);
    }
}
