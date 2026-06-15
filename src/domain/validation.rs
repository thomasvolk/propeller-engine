// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use super::project::Project;

#[derive(Debug, PartialEq)]
pub enum ValidationError {
    BpmOutOfRange { actual: u32 },
    LoopDurationZero,
    InvalidMidiChannel { track: usize, actual: u8 },
    InvalidMidiInstrument { track: usize, actual: u8 },
    NoteDurationZero { track: usize, note: usize },
    NoteStartTickOutOfRange { track: usize, note: usize, start_tick: u32, loop_duration: u32 },
    NoteDurationExceedsLimit { track: usize, note: usize, duration: u32, limit: u32 },
}

pub fn validate(project: &Project) -> Result<(), ValidationError> {
    if project.header.bpm < 20 || project.header.bpm > 300 {
        return Err(ValidationError::BpmOutOfRange { actual: project.header.bpm });
    }

    // F-6: check LoopDurationZero before any track or note iteration.
    if project.header.loop_duration == 0 {
        return Err(ValidationError::LoopDurationZero);
    }

    let loop_duration = project.header.loop_duration;
    let limit = 2 * loop_duration as u64;

    for (ti, track) in project.tracks.iter().enumerate() {
        if track.channel < 1 || track.channel > 16 {
            return Err(ValidationError::InvalidMidiChannel { track: ti, actual: track.channel });
        }
        if track.instrument > 127 {
            return Err(ValidationError::InvalidMidiInstrument { track: ti, actual: track.instrument });
        }
        // F-7: per-note order: NoteDurationZero, NoteStartTickOutOfRange, NoteDurationExceedsLimit.
        for (ni, note) in track.notes.iter().enumerate() {
            if note.duration == 0 {
                return Err(ValidationError::NoteDurationZero { track: ti, note: ni });
            }
            if note.start_tick >= loop_duration {
                return Err(ValidationError::NoteStartTickOutOfRange {
                    track: ti,
                    note: ni,
                    start_tick: note.start_tick,
                    loop_duration,
                });
            }
            if (note.start_tick as u64) + (note.duration as u64) > limit {
                return Err(ValidationError::NoteDurationExceedsLimit {
                    track: ti,
                    note: ni,
                    duration: note.duration,
                    limit: 2 * loop_duration,
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::project::*;

    fn make_valid_project() -> Project {
        Project {
            header: Header { bpm: 120, loop_duration: 1920 },
            tracks: vec![Track {
                name: "piano".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![Note { start_tick: 0, duration: 480, pitch: 60, velocity: 80 }],
            }],
        }
    }

    // T-9 (rewrite): validate returns Ok for a well-formed project with BPM 120
    #[test]
    fn test_validate_ok() {
        assert_eq!(validate(&make_valid_project()), Ok(()));
    }

    // T-9 (rewrite): validate returns BpmOutOfRange for BPM 19 and 301
    #[test]
    fn test_validate_bpm_out_of_range() {
        let mut p = make_valid_project();
        p.header.bpm = 19;
        assert!(matches!(validate(&p), Err(ValidationError::BpmOutOfRange { actual: 19 })));
        p.header.bpm = 301;
        assert!(matches!(validate(&p), Err(ValidationError::BpmOutOfRange { actual: 301 })));
    }

    // T-9 (rewrite): validate returns InvalidMidiChannel for channel 0 and 17
    #[test]
    fn test_validate_invalid_channel() {
        let mut p = make_valid_project();
        p.tracks[0].channel = 0;
        assert!(matches!(validate(&p), Err(ValidationError::InvalidMidiChannel { actual: 0, .. })));
        p.tracks[0].channel = 17;
        assert!(matches!(validate(&p), Err(ValidationError::InvalidMidiChannel { actual: 17, .. })));
    }

    // T-9 (rewrite): validate returns InvalidMidiInstrument for instrument 128
    #[test]
    fn test_validate_invalid_instrument() {
        let mut p = make_valid_project();
        p.tracks[0].instrument = 128;
        assert!(matches!(
            validate(&p),
            Err(ValidationError::InvalidMidiInstrument { actual: 128, .. })
        ));
    }

    // T-9 (rewrite): validate returns NoteDurationZero for note.duration == 0
    #[test]
    fn test_validate_note_duration_zero() {
        let mut p = make_valid_project();
        p.tracks[0].notes[0].duration = 0;
        assert!(matches!(
            validate(&p),
            Err(ValidationError::NoteDurationZero { track: 0, note: 0 })
        ));
    }

    // T-9 (rewrite): validate returns Ok for zero-track project
    #[test]
    fn test_validate_zero_tracks() {
        let p = Project {
            header: Header { bpm: 120, loop_duration: 1920 },
            tracks: vec![],
        };
        assert_eq!(validate(&p), Ok(()));
    }

    // T-1: LoopDurationZero returned when loop_duration == 0 (F-3, AC-1)
    #[test]
    fn test_validate_loop_duration_zero() {
        let p = Project {
            header: Header { bpm: 120, loop_duration: 0 },
            tracks: vec![],
        };
        assert_eq!(validate(&p), Err(ValidationError::LoopDurationZero));
    }

    // T-1: LoopDurationZero is checked before track iteration (F-6)
    #[test]
    fn test_validate_loop_duration_zero_before_channel() {
        let p = Project {
            header: Header { bpm: 120, loop_duration: 0 },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 0, // invalid
                instrument: 0,
                notes: vec![],
            }],
        };
        assert_eq!(validate(&p), Err(ValidationError::LoopDurationZero));
    }

    // T-3: NoteStartTickOutOfRange when start_tick == loop_duration (AC-2)
    #[test]
    fn test_validate_note_start_tick_at_boundary() {
        let mut p = make_valid_project();
        p.tracks[0].notes[0].start_tick = 1920; // == loop_duration
        assert_eq!(
            validate(&p),
            Err(ValidationError::NoteStartTickOutOfRange {
                track: 0,
                note: 0,
                start_tick: 1920,
                loop_duration: 1920,
            })
        );
    }

    // T-3: NoteStartTickOutOfRange when start_tick > loop_duration (AC-2)
    #[test]
    fn test_validate_note_start_tick_exceeds() {
        let mut p = make_valid_project();
        p.tracks[0].notes[0].start_tick = 2000;
        assert_eq!(
            validate(&p),
            Err(ValidationError::NoteStartTickOutOfRange {
                track: 0,
                note: 0,
                start_tick: 2000,
                loop_duration: 1920,
            })
        );
    }

    // T-3: NoteDurationExceedsLimit when start_tick + duration > 2 * loop_duration (AC-4)
    #[test]
    fn test_validate_note_duration_exceeds_limit() {
        let mut p = make_valid_project();
        p.tracks[0].notes[0].start_tick = 0;
        p.tracks[0].notes[0].duration = 3841; // 0 + 3841 > 3840
        assert_eq!(
            validate(&p),
            Err(ValidationError::NoteDurationExceedsLimit {
                track: 0,
                note: 0,
                duration: 3841,
                limit: 3840,
            })
        );
    }

    // T-3: boundary note start_tick + duration == 2 * loop_duration → Ok (AC-6)
    #[test]
    fn test_validate_note_boundary_ok() {
        let mut p = make_valid_project();
        p.tracks[0].notes[0].start_tick = 0;
        p.tracks[0].notes[0].duration = 3840; // 0 + 3840 == 3840 == 2 * 1920
        assert_eq!(validate(&p), Ok(()));
    }

    // T-3: overlapping notes (same start_tick) are valid (AC-5)
    #[test]
    fn test_validate_overlapping_notes_ok() {
        let p = Project {
            header: Header { bpm: 120, loop_duration: 1920 },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![
                    Note { start_tick: 0, duration: 480, pitch: 60, velocity: 80 },
                    Note { start_tick: 0, duration: 480, pitch: 64, velocity: 80 },
                ],
            }],
        };
        assert_eq!(validate(&p), Ok(()));
    }

    // T-3: empty notes list on a track is valid (F-8)
    #[test]
    fn test_validate_empty_notes_ok() {
        let p = Project {
            header: Header { bpm: 120, loop_duration: 1920 },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![],
            }],
        };
        assert_eq!(validate(&p), Ok(()));
    }

    // T-3: duration == 0 AND start_tick >= loop_duration → NoteDurationZero, not NoteStartTickOutOfRange (AC-14, F-7)
    #[test]
    fn test_validate_priority_duration_zero_wins() {
        let p = Project {
            header: Header { bpm: 120, loop_duration: 1920 },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![Note { start_tick: 2000, duration: 0, pitch: 60, velocity: 80 }],
            }],
        };
        assert_eq!(
            validate(&p),
            Err(ValidationError::NoteDurationZero { track: 0, note: 0 })
        );
    }

    // T-3: NoteStartTickOutOfRange fields are correct (AC-11 partial)
    #[test]
    fn test_validate_note_start_tick_out_of_range_fields() {
        let p = Project {
            header: Header { bpm: 120, loop_duration: 100 },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![Note { start_tick: 150, duration: 10, pitch: 60, velocity: 80 }],
            }],
        };
        assert_eq!(
            validate(&p),
            Err(ValidationError::NoteStartTickOutOfRange {
                track: 0,
                note: 0,
                start_tick: 150,
                loop_duration: 100,
            })
        );
    }
}
