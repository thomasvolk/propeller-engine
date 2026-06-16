// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

#[allow(dead_code)]
pub const PPQN: u32 = 480;

pub struct Project {
    pub header: Header,
    pub tracks: Vec<Track>,
}

pub struct Header {
    pub bpm: u32,
    pub loop_duration: u32,
}

pub struct Track {
    pub name: String,
    pub channel: u8,
    pub instrument: u8,
    pub notes: Vec<Note>,
}

pub struct Note {
    pub start_tick: u32,
    pub duration: u32,
    pub pitch: u8,
    pub velocity: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_fields() {
        let h = Header {
            bpm: 120,
            loop_duration: 1920,
        };
        assert_eq!(h.bpm, 120);
        assert_eq!(h.loop_duration, 1920);
    }

    #[test]
    fn test_header_zero_loop_duration() {
        let h = Header {
            bpm: 120,
            loop_duration: 0,
        };
        assert_eq!(h.loop_duration, 0);
    }

    #[test]
    fn test_note_fields() {
        let n = Note {
            start_tick: 0,
            duration: 480,
            pitch: 60,
            velocity: 80,
        };
        assert_eq!(n.start_tick, 0);
        assert_eq!(n.duration, 480);
        assert_eq!(n.pitch, 60);
        assert_eq!(n.velocity, 80);
    }

    #[test]
    fn test_track_with_notes() {
        let track = Track {
            name: "piano".to_string(),
            channel: 1,
            instrument: 0,
            notes: vec![
                Note {
                    start_tick: 0,
                    duration: 480,
                    pitch: 60,
                    velocity: 80,
                },
                Note {
                    start_tick: 480,
                    duration: 480,
                    pitch: 62,
                    velocity: 80,
                },
            ],
        };
        assert_eq!(track.notes.len(), 2);
        assert_eq!(track.notes[0].pitch, 60);
        assert_eq!(track.notes[1].pitch, 62);
    }

    #[test]
    fn test_track_empty_notes() {
        let track = Track {
            name: "empty".to_string(),
            channel: 1,
            instrument: 0,
            notes: vec![],
        };
        assert_eq!(track.notes.len(), 0);
    }

    #[test]
    fn test_ppqn_value() {
        assert_eq!(PPQN, 480u32);
    }
}
