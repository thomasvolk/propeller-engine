pub trait MidiOutput: Send + 'static {
    fn note_on(&mut self, channel: u8, pitch: u8, velocity: u8);
    fn note_off(&mut self, channel: u8, pitch: u8);
    fn program_change(&mut self, channel: u8, program: u8);
    fn clock_tick(&mut self);
    fn clock_start(&mut self);
    fn clock_continue(&mut self);
    fn clock_stop(&mut self);
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub enum MidiEvent {
    NoteOn { channel: u8, pitch: u8, velocity: u8 },
    NoteOff { channel: u8, pitch: u8 },
    ProgramChange { channel: u8, program: u8 },
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
    fn note_on(&mut self, channel: u8, pitch: u8, velocity: u8) {
        self.events.push(MidiEvent::NoteOn { channel, pitch, velocity });
    }

    fn note_off(&mut self, channel: u8, pitch: u8) {
        self.events.push(MidiEvent::NoteOff { channel, pitch });
    }

    fn program_change(&mut self, channel: u8, program: u8) {
        self.events.push(MidiEvent::ProgramChange { channel, program });
    }

    fn clock_tick(&mut self) { self.events.push(MidiEvent::ClockTick); }
    fn clock_start(&mut self) { self.events.push(MidiEvent::ClockStart); }
    fn clock_continue(&mut self) { self.events.push(MidiEvent::ClockContinue); }
    fn clock_stop(&mut self) { self.events.push(MidiEvent::ClockStop); }
}

#[cfg(test)]
mod tests {
    use super::*;

    // T-9 (EP-3): MockMidiOutput records NoteOn, NoteOff, ProgramChange in insertion order
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

    // T-1 (EP-5): MockMidiOutput records ClockTick, ClockStart, ClockContinue, ClockStop in order
    #[test]
    fn mock_records_clock_events_in_order() {
        let mut m = MockMidiOutput::new();
        m.note_on(1, 60, 80);
        m.clock_start();
        m.clock_tick();
        m.clock_continue();
        m.clock_stop();
        m.note_off(1, 60);
        assert_eq!(m.events, vec![
            MidiEvent::NoteOn { channel: 1, pitch: 60, velocity: 80 },
            MidiEvent::ClockStart,
            MidiEvent::ClockTick,
            MidiEvent::ClockContinue,
            MidiEvent::ClockStop,
            MidiEvent::NoteOff { channel: 1, pitch: 60 },
        ]);
    }
}
