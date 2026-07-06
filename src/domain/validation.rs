// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use super::project::Project;

#[derive(Debug, PartialEq)]
pub enum ValidationError {
    BpmOutOfRange {
        actual: u32,
    },
    LoopDurationZero,
    InvalidMidiChannel {
        track: usize,
        actual: u8,
    },
    InvalidMidiInstrument {
        track: usize,
        actual: u8,
    },
    NoteDurationZero {
        track: usize,
        note: usize,
    },
    NoteStartTickOutOfRange {
        track: usize,
        note: usize,
        start_tick: u32,
        loop_duration: u32,
    },
    NoteDurationExceedsLimit {
        track: usize,
        note: usize,
        duration: u32,
        limit: u32,
    },
    PitchBendValueOutOfRange {
        track: usize,
        event: usize,
        actual: u32,
    },
    PitchBendTickOutOfRange {
        track: usize,
        event: usize,
        tick: u32,
        loop_duration: u32,
    },
}

pub fn validate(project: &Project) -> Result<(), ValidationError> {
    if project.header.bpm < 20 || project.header.bpm > 300 {
        return Err(ValidationError::BpmOutOfRange {
            actual: project.header.bpm,
        });
    }

    // F-6: check LoopDurationZero before any track or note iteration.
    if project.header.loop_duration == 0 {
        return Err(ValidationError::LoopDurationZero);
    }

    let loop_duration = project.header.loop_duration;
    let limit = 2 * loop_duration as u64;

    for (ti, track) in project.tracks.iter().enumerate() {
        if track.channel < 1 || track.channel > 16 {
            return Err(ValidationError::InvalidMidiChannel {
                track: ti,
                actual: track.channel,
            });
        }
        if track.instrument > 127 {
            return Err(ValidationError::InvalidMidiInstrument {
                track: ti,
                actual: track.instrument,
            });
        }
        // F-7: per-note order: NoteDurationZero, NoteStartTickOutOfRange, NoteDurationExceedsLimit.
        for (ni, note) in track.notes.iter().enumerate() {
            if note.duration == 0 {
                return Err(ValidationError::NoteDurationZero {
                    track: ti,
                    note: ni,
                });
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

        // F-8: pitch-bends are validated only after all of this track's notes pass.
        for (ei, event) in track.pitch_bends.iter().enumerate() {
            if event.value > 16383 {
                return Err(ValidationError::PitchBendValueOutOfRange {
                    track: ti,
                    event: ei,
                    actual: event.value,
                });
            }
            if event.tick >= loop_duration {
                return Err(ValidationError::PitchBendTickOutOfRange {
                    track: ti,
                    event: ei,
                    tick: event.tick,
                    loop_duration,
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::project::*;
    use super::*;

    fn make_valid_project() -> Project {
        Project {
            header: Header {
                bpm: 120,
                loop_duration: 1920,
            },
            tracks: vec![Track {
                name: "piano".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![Note {
                    start_tick: 0,
                    duration: 480,
                    pitch: 60,
                    velocity: 80,
                }],
                pitch_bends: vec![],
            }],
        }
    }

    #[test]
    fn test_validate_ok() {
        assert_eq!(validate(&make_valid_project()), Ok(()));
    }

    #[test]
    fn test_validate_bpm_out_of_range() {
        let mut p = make_valid_project();
        p.header.bpm = 19;
        assert!(matches!(
            validate(&p),
            Err(ValidationError::BpmOutOfRange { actual: 19 })
        ));
        p.header.bpm = 301;
        assert!(matches!(
            validate(&p),
            Err(ValidationError::BpmOutOfRange { actual: 301 })
        ));
    }

    #[test]
    fn test_validate_invalid_channel() {
        let mut p = make_valid_project();
        p.tracks[0].channel = 0;
        assert!(matches!(
            validate(&p),
            Err(ValidationError::InvalidMidiChannel { actual: 0, .. })
        ));
        p.tracks[0].channel = 17;
        assert!(matches!(
            validate(&p),
            Err(ValidationError::InvalidMidiChannel { actual: 17, .. })
        ));
    }

    #[test]
    fn test_validate_invalid_instrument() {
        let mut p = make_valid_project();
        p.tracks[0].instrument = 128;
        assert!(matches!(
            validate(&p),
            Err(ValidationError::InvalidMidiInstrument { actual: 128, .. })
        ));
    }

    #[test]
    fn test_validate_note_duration_zero() {
        let mut p = make_valid_project();
        p.tracks[0].notes[0].duration = 0;
        assert!(matches!(
            validate(&p),
            Err(ValidationError::NoteDurationZero { track: 0, note: 0 })
        ));
    }

    #[test]
    fn test_validate_zero_tracks() {
        let p = Project {
            header: Header {
                bpm: 120,
                loop_duration: 1920,
            },
            tracks: vec![],
        };
        assert_eq!(validate(&p), Ok(()));
    }

    #[test]
    fn test_validate_loop_duration_zero() {
        let p = Project {
            header: Header {
                bpm: 120,
                loop_duration: 0,
            },
            tracks: vec![],
        };
        assert_eq!(validate(&p), Err(ValidationError::LoopDurationZero));
    }

    #[test]
    fn test_validate_loop_duration_zero_before_channel() {
        let p = Project {
            header: Header {
                bpm: 120,
                loop_duration: 0,
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 0, // invalid
                instrument: 0,
                notes: vec![],
                pitch_bends: vec![],
            }],
        };
        assert_eq!(validate(&p), Err(ValidationError::LoopDurationZero));
    }

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

    #[test]
    fn test_validate_note_boundary_ok() {
        let mut p = make_valid_project();
        p.tracks[0].notes[0].start_tick = 0;
        p.tracks[0].notes[0].duration = 3840; // 0 + 3840 == 3840 == 2 * 1920
        assert_eq!(validate(&p), Ok(()));
    }

    #[test]
    fn test_validate_overlapping_notes_ok() {
        let p = Project {
            header: Header {
                bpm: 120,
                loop_duration: 1920,
            },
            tracks: vec![Track {
                name: "t".to_string(),
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
                        start_tick: 0,
                        duration: 480,
                        pitch: 64,
                        velocity: 80,
                    },
                ],
                pitch_bends: vec![],
            }],
        };
        assert_eq!(validate(&p), Ok(()));
    }

    #[test]
    fn test_validate_empty_notes_ok() {
        let p = Project {
            header: Header {
                bpm: 120,
                loop_duration: 1920,
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![],
                pitch_bends: vec![],
            }],
        };
        assert_eq!(validate(&p), Ok(()));
    }

    #[test]
    fn test_validate_priority_duration_zero_wins() {
        let p = Project {
            header: Header {
                bpm: 120,
                loop_duration: 1920,
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![Note {
                    start_tick: 2000,
                    duration: 0,
                    pitch: 60,
                    velocity: 80,
                }],
                pitch_bends: vec![],
            }],
        };
        assert_eq!(
            validate(&p),
            Err(ValidationError::NoteDurationZero { track: 0, note: 0 })
        );
    }

    #[test]
    fn test_validate_note_start_tick_out_of_range_fields() {
        let p = Project {
            header: Header {
                bpm: 120,
                loop_duration: 100,
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![Note {
                    start_tick: 150,
                    duration: 10,
                    pitch: 60,
                    velocity: 80,
                }],
                pitch_bends: vec![],
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

    // T-7: validate() accepts pitch-bend values 0, 8192, 16383 and rejects out-of-range
    // values, identifying the failing track/event index (F-2, F-6, AC-2, AC-3, AC-4, AC-5).
    #[test]
    fn test_validate_pitch_bend_value_boundaries_ok() {
        for value in [0u32, 8192, 16383] {
            let mut p = make_valid_project();
            p.tracks[0].pitch_bends = vec![PitchBend { tick: 0, value }];
            assert_eq!(validate(&p), Ok(()));
        }
    }

    #[test]
    fn test_validate_pitch_bend_value_out_of_range() {
        let mut p = make_valid_project();
        p.tracks[0].pitch_bends = vec![PitchBend {
            tick: 0,
            value: 16384,
        }];
        assert_eq!(
            validate(&p),
            Err(ValidationError::PitchBendValueOutOfRange {
                track: 0,
                event: 0,
                actual: 16384,
            })
        );
    }

    #[test]
    fn test_validate_pitch_bend_value_out_of_range_second_event() {
        let mut p = make_valid_project();
        p.tracks[0].pitch_bends = vec![
            PitchBend {
                tick: 0,
                value: 8192,
            },
            PitchBend {
                tick: 1,
                value: 99999,
            },
        ];
        assert_eq!(
            validate(&p),
            Err(ValidationError::PitchBendValueOutOfRange {
                track: 0,
                event: 1,
                actual: 99999,
            })
        );
    }

    // T-9: validate() accepts a tick < loop_duration and rejects a tick >= loop_duration,
    // identifying the failing track/event index (F-3, F-6, AC-6).
    #[test]
    fn test_validate_pitch_bend_tick_below_loop_duration_ok() {
        let mut p = make_valid_project();
        p.tracks[0].pitch_bends = vec![PitchBend {
            tick: 1919,
            value: 8192,
        }];
        assert_eq!(validate(&p), Ok(()));
    }

    #[test]
    fn test_validate_pitch_bend_tick_at_loop_duration_rejected() {
        let mut p = make_valid_project();
        p.tracks[0].pitch_bends = vec![PitchBend {
            tick: 1920,
            value: 8192,
        }];
        assert_eq!(
            validate(&p),
            Err(ValidationError::PitchBendTickOutOfRange {
                track: 0,
                event: 0,
                tick: 1920,
                loop_duration: 1920,
            })
        );
    }

    #[test]
    fn test_validate_pitch_bend_tick_exceeds_loop_duration_rejected() {
        let mut p = make_valid_project();
        p.tracks[0].pitch_bends = vec![PitchBend {
            tick: 5000,
            value: 8192,
        }];
        assert_eq!(
            validate(&p),
            Err(ValidationError::PitchBendTickOutOfRange {
                track: 0,
                event: 0,
                tick: 5000,
                loop_duration: 1920,
            })
        );
    }

    // T-11: validate() applies no ordering, dedup, or count limit to pitch-bend events
    // (F-7, F-10).
    #[test]
    fn test_validate_pitch_bend_out_of_order_and_repeated_ticks_ok() {
        let mut p = make_valid_project();
        p.tracks[0].pitch_bends = vec![
            PitchBend {
                tick: 500,
                value: 100,
            },
            PitchBend {
                tick: 100,
                value: 200,
            },
            PitchBend {
                tick: 100,
                value: 300,
            },
        ];
        assert_eq!(validate(&p), Ok(()));
    }

    #[test]
    fn test_validate_pitch_bend_large_event_count_ok() {
        let mut p = make_valid_project();
        p.tracks[0].pitch_bends = (0..10_000)
            .map(|i| PitchBend {
                tick: i % 1920,
                value: 8192,
            })
            .collect();
        assert_eq!(validate(&p), Ok(()));
    }

    // T-13: given a track with both an invalid note and an invalid pitch-bend event,
    // validate() reports the note's error, not the pitch-bend's (F-8).
    #[test]
    fn test_validate_note_error_wins_over_pitch_bend_error() {
        let mut p = make_valid_project();
        p.tracks[0].notes[0].duration = 0;
        p.tracks[0].pitch_bends = vec![PitchBend {
            tick: 0,
            value: 99999,
        }];
        assert_eq!(
            validate(&p),
            Err(ValidationError::NoteDurationZero { track: 0, note: 0 })
        );
    }
}
