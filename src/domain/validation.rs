// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use super::project::Project;

#[derive(Debug, PartialEq)]
pub enum ValidationError {
    BpmOutOfRange { actual: u32 },
    InvalidTimeSignatureNumerator,
    InvalidTimeSignatureDenominator { actual: u32 },
    InvalidMidiChannel { track: usize, actual: u8 },
    InvalidMidiInstrument { track: usize, actual: u8 },
    EmptyTrackBars { track: usize },
    NoteDurationZero { track: usize, bar: usize, note: usize },
    NoteDurationExceedsBar { track: usize, bar: usize, note: usize, duration: u32, bar_ticks: u32 },
}

pub fn validate(project: &Project) -> Result<(), ValidationError> {
    if project.header.bpm < 20 || project.header.bpm > 300 {
        return Err(ValidationError::BpmOutOfRange { actual: project.header.bpm });
    }

    if project.header.time_signature.numerator < 1 {
        return Err(ValidationError::InvalidTimeSignatureNumerator);
    }

    if ![2u32, 4, 8, 16].contains(&project.header.time_signature.denominator) {
        return Err(ValidationError::InvalidTimeSignatureDenominator {
            actual: project.header.time_signature.denominator,
        });
    }

    let bar_ticks = project.header.time_signature.bar_ticks();

    for (ti, track) in project.tracks.iter().enumerate() {
        if track.channel < 1 || track.channel > 16 {
            return Err(ValidationError::InvalidMidiChannel { track: ti, actual: track.channel });
        }
        if track.instrument > 127 {
            return Err(ValidationError::InvalidMidiInstrument { track: ti, actual: track.instrument });
        }
        if track.bars.is_empty() {
            return Err(ValidationError::EmptyTrackBars { track: ti });
        }
        for (bi, bar) in track.bars.iter().enumerate() {
            for (ni, note) in bar.notes.iter().enumerate() {
                if note.duration_ticks == 0 {
                    return Err(ValidationError::NoteDurationZero { track: ti, bar: bi, note: ni });
                }
                if note.duration_ticks > bar_ticks {
                    return Err(ValidationError::NoteDurationExceedsBar {
                        track: ti,
                        bar: bi,
                        note: ni,
                        duration: note.duration_ticks,
                        bar_ticks,
                    });
                }
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
            header: Header {
                bpm: 120,
                time_signature: TimeSignature { numerator: 4, denominator: 4 },
            },
            tracks: vec![Track {
                name: "piano".to_string(),
                channel: 1,
                instrument: 0,
                bars: vec![Bar {
                    notes: vec![Note {
                        event: NoteEvent::Note { pitch: 60, velocity: 80 },
                        duration_ticks: 480,
                    }],
                }],
            }],
        }
    }

    // T-5: validate returns Ok for a well-formed 4/4 project with BPM 120
    #[test]
    fn test_validate_ok() {
        assert_eq!(validate(&make_valid_project()), Ok(()));
    }

    // T-6: validate returns BpmOutOfRange for BPM 19 and 301
    #[test]
    fn test_validate_bpm_out_of_range() {
        let mut p = make_valid_project();
        p.header.bpm = 19;
        assert!(matches!(validate(&p), Err(ValidationError::BpmOutOfRange { actual: 19 })));
        p.header.bpm = 301;
        assert!(matches!(validate(&p), Err(ValidationError::BpmOutOfRange { actual: 301 })));
    }

    // T-7: validate returns InvalidTimeSignatureDenominator for denominator 3 and 5
    #[test]
    fn test_validate_invalid_denominator() {
        let mut p = make_valid_project();
        p.header.time_signature.denominator = 3;
        assert!(matches!(
            validate(&p),
            Err(ValidationError::InvalidTimeSignatureDenominator { actual: 3 })
        ));
        p.header.time_signature.denominator = 5;
        assert!(matches!(
            validate(&p),
            Err(ValidationError::InvalidTimeSignatureDenominator { actual: 5 })
        ));
    }

    // T-8: validate returns NoteDurationZero for duration_ticks = 0
    #[test]
    fn test_validate_note_duration_zero() {
        let mut p = make_valid_project();
        p.tracks[0].bars[0].notes[0].duration_ticks = 0;
        assert!(matches!(validate(&p), Err(ValidationError::NoteDurationZero { .. })));
    }

    // T-9: validate returns NoteDurationExceedsBar for duration > 1920 in 4/4
    #[test]
    fn test_validate_note_duration_exceeds_bar() {
        let mut p = make_valid_project();
        p.tracks[0].bars[0].notes[0].duration_ticks = 1921;
        assert!(matches!(validate(&p), Err(ValidationError::NoteDurationExceedsBar { .. })));
    }

    // T-10: validate returns Ok for duration_ticks exactly equal to bar_ticks (1920 for 4/4)
    #[test]
    fn test_validate_note_duration_exact_bar() {
        let mut p = make_valid_project();
        p.tracks[0].bars[0].notes[0].duration_ticks = 1920;
        assert_eq!(validate(&p), Ok(()));
    }

    // T-11: validate returns InvalidMidiChannel for channel 0 and channel 17
    #[test]
    fn test_validate_invalid_channel() {
        let mut p = make_valid_project();
        p.tracks[0].channel = 0;
        assert!(matches!(validate(&p), Err(ValidationError::InvalidMidiChannel { actual: 0, .. })));
        p.tracks[0].channel = 17;
        assert!(matches!(validate(&p), Err(ValidationError::InvalidMidiChannel { actual: 17, .. })));
    }

    // T-12: validate returns InvalidMidiInstrument for instrument 128
    #[test]
    fn test_validate_invalid_instrument() {
        let mut p = make_valid_project();
        p.tracks[0].instrument = 128;
        assert!(matches!(
            validate(&p),
            Err(ValidationError::InvalidMidiInstrument { actual: 128, .. })
        ));
    }

    // T-13: validate returns Ok for zero-track project
    #[test]
    fn test_validate_zero_tracks() {
        let p = Project {
            header: Header {
                bpm: 120,
                time_signature: TimeSignature { numerator: 4, denominator: 4 },
            },
            tracks: vec![],
        };
        assert_eq!(validate(&p), Ok(()));
    }

    // T-14: validate returns Ok when note tick-sum is less than bar_ticks
    #[test]
    fn test_validate_underfilled_bar() {
        let mut p = make_valid_project();
        p.tracks[0].bars[0].notes[0].duration_ticks = 480;
        assert_eq!(validate(&p), Ok(()));
    }

    // T-15: validate returns InvalidTimeSignatureNumerator for numerator 0
    #[test]
    fn test_validate_invalid_numerator() {
        let p = Project {
            header: Header {
                bpm: 120,
                time_signature: TimeSignature { numerator: 0, denominator: 4 },
            },
            tracks: vec![],
        };
        assert!(matches!(validate(&p), Err(ValidationError::InvalidTimeSignatureNumerator)));
    }

    // T-29: validate returns EmptyTrackBars for a track with zero bars
    #[test]
    fn test_validate_empty_track_bars() {
        let p = Project {
            header: Header {
                bpm: 120,
                time_signature: TimeSignature { numerator: 4, denominator: 4 },
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                bars: vec![],
            }],
        };
        assert!(matches!(validate(&p), Err(ValidationError::EmptyTrackBars { track: 0 })));
    }
}
