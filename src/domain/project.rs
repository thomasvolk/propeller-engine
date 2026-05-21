pub const PPQN: u32 = 480;

pub struct Project {
    pub header: Header,
    pub tracks: Vec<Track>,
}

impl Project {
    pub fn cycle_length(&self) -> usize {
        self.tracks.iter().map(|t| t.bars.len()).max().unwrap_or(0)
    }
}

pub struct Header {
    pub bpm: u32,
    pub time_signature: TimeSignature,
}

pub struct TimeSignature {
    pub numerator: u32,
    pub denominator: u32,
}

impl TimeSignature {
    pub fn bar_ticks(&self) -> u32 {
        self.numerator * (PPQN * 4 / self.denominator)
    }
}

pub struct Track {
    pub name: String,
    pub channel: u8,
    pub instrument: u8,
    pub bars: Vec<Bar>,
}

impl Track {
    pub fn bar_at(&self, cycle_pos: usize) -> &Bar {
        &self.bars[cycle_pos % self.bars.len()]
    }
}

pub struct Bar {
    pub notes: Vec<Note>,
}

pub struct Note {
    pub event: NoteEvent,
    pub duration_ticks: u32,
}

pub enum NoteEvent {
    Note { pitch: u8, velocity: u8 },
    Rest,
}

#[cfg(test)]
mod tests {
    use super::*;

    // T-1: construct all domain types and assert fields stored correctly; assert PPQN = 480
    #[test]
    fn test_construct_project() {
        let note = Note {
            event: NoteEvent::Note { pitch: 60, velocity: 80 },
            duration_ticks: 480,
        };
        let rest = Note {
            event: NoteEvent::Rest,
            duration_ticks: 480,
        };
        let bar = Bar { notes: vec![note, rest] };
        let track = Track {
            name: "Piano".to_string(),
            channel: 1,
            instrument: 0,
            bars: vec![bar],
        };
        let header = Header {
            bpm: 120,
            time_signature: TimeSignature { numerator: 4, denominator: 4 },
        };
        let project = Project { header, tracks: vec![track] };

        assert_eq!(project.header.bpm, 120);
        assert_eq!(project.header.time_signature.numerator, 4);
        assert_eq!(project.header.time_signature.denominator, 4);
        assert_eq!(project.tracks.len(), 1);
        assert_eq!(project.tracks[0].name, "Piano");
        assert_eq!(project.tracks[0].channel, 1);
        assert_eq!(project.tracks[0].instrument, 0);
        assert_eq!(project.tracks[0].bars.len(), 1);
        assert_eq!(project.tracks[0].bars[0].notes.len(), 2);
        match &project.tracks[0].bars[0].notes[0].event {
            NoteEvent::Note { pitch, velocity } => {
                assert_eq!(*pitch, 60);
                assert_eq!(*velocity, 80);
            }
            _ => panic!("expected Note event"),
        }
        match &project.tracks[0].bars[0].notes[1].event {
            NoteEvent::Rest => {}
            _ => panic!("expected Rest event"),
        }
        assert_eq!(PPQN, 480);
    }

    // T-3: bar_ticks() returns correct values for common time signatures
    #[test]
    fn test_bar_ticks() {
        assert_eq!(TimeSignature { numerator: 4, denominator: 4 }.bar_ticks(), 1920);
        assert_eq!(TimeSignature { numerator: 3, denominator: 4 }.bar_ticks(), 1440);
        assert_eq!(TimeSignature { numerator: 6, denominator: 8 }.bar_ticks(), 1440);
        assert_eq!(TimeSignature { numerator: 1, denominator: 4 }.bar_ticks(), 480);
        assert_eq!(TimeSignature { numerator: 1, denominator: 8 }.bar_ticks(), 240);
    }

    // T-26: cycle_length() returns max bar count; returns 0 for zero-track project
    #[test]
    fn test_cycle_length() {
        let make_track = |bar_count: usize| Track {
            name: "t".to_string(),
            channel: 1,
            instrument: 0,
            bars: (0..bar_count).map(|_| Bar { notes: vec![] }).collect(),
        };

        let project = Project {
            header: Header {
                bpm: 120,
                time_signature: TimeSignature { numerator: 4, denominator: 4 },
            },
            tracks: vec![make_track(1), make_track(2), make_track(4)],
        };
        assert_eq!(project.cycle_length(), 4);

        let empty_project = Project {
            header: Header {
                bpm: 120,
                time_signature: TimeSignature { numerator: 4, denominator: 4 },
            },
            tracks: vec![],
        };
        assert_eq!(empty_project.cycle_length(), 0);
    }

    // T-27: bar_at() wraps correctly for a 2-bar track at positions 0–3
    #[test]
    fn test_bar_at() {
        let bars = vec![
            Bar {
                notes: vec![Note {
                    event: NoteEvent::Note { pitch: 60, velocity: 80 },
                    duration_ticks: 480,
                }],
            },
            Bar {
                notes: vec![Note {
                    event: NoteEvent::Note { pitch: 62, velocity: 80 },
                    duration_ticks: 480,
                }],
            },
        ];
        let track = Track { name: "t".to_string(), channel: 1, instrument: 0, bars };

        let pitches: Vec<u8> = (0..4)
            .map(|i| match &track.bar_at(i).notes[0].event {
                NoteEvent::Note { pitch, .. } => *pitch,
                _ => panic!("expected Note"),
            })
            .collect();
        assert_eq!(pitches, vec![60, 62, 60, 62]);
    }
}
