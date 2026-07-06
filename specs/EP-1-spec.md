# EP-1 · Accept Pitch Bend Data in Track Definitions — Technical Specification

## Overview
This epic extends the existing project/track/note data model, wire format, and validation
pipeline so a track may also carry a `pitch-bends` list of `[tick, value]` pairs. Accepted
events are stored on the track alongside its notes; out-of-range values or ticks reject the
whole project with the same structured error mechanism used for notes today. This epic is
data-model and validation only — sending pitch-bend MIDI messages during playback is EP-2.

**Confidence Level:** 92% — Every F-x/AC-x maps to a concrete, TDD-ordered task, and the
design is a direct, low-risk extension of existing patterns (`Note`/`ValidationError`/
`WireTrack`) with no genuine architectural trade-offs left open. The only residual
uncertainty is cosmetic (exact wording of new error messages), which does not warrant a
design decision.

---

## Architecture Overview
No new components are introduced. The existing note pipeline is extended in place, at each
of its four existing stages, using the same patterns already established for notes:

1. **Domain model** (`src/domain/project.rs`) — `Track` gains a `pitch_bends: Vec<PitchBend>`
   field, mirroring its existing `notes: Vec<Note>` field.
2. **Wire format** (`src/ipc/types.rs`) — `WireTrack` gains a `pitch_bends` field deserialized
   from the JSON key `pitch-bends`, defaulting to an empty list when the key is absent (F-9).
3. **Wire-to-domain conversion** (`src/ipc/handler.rs::wire_track_to_domain`) — extended to
   map each `[tick, value]` pair into a `PitchBend`.
4. **Validation** (`src/domain/validation.rs::validate`) — extended per track: after the
   existing per-note loop completes (preserving F-8's note-before-pitch-bend order), a second
   loop walks `track.pitch_bends` in submission order (no sorting or dedup, per F-7/F-10)
   checking the value range (F-2) then the tick range (F-3), mirroring the existing note
   checks and reusing the same `track`/`event` index attribution style (F-6).

`loop_engine::player` and MIDI output are untouched by this epic — pitch-bend events are
stored but not yet scheduled or played.

---

## Components

### Domain Model — `src/domain/project.rs`
- New `PitchBend { tick: u32, value: u32 }` struct. Both fields are `u32` (matching the wire
  representation and `Note.start_tick`) rather than a narrower integer type, so an
  out-of-range value can be range-checked in `validate()` before any lossy cast occurs —
  avoiding the silent-truncation risk that `Note.pitch`/`Note.velocity` already have when
  cast down from the wire's `u32`.
- `Track` gains `pitch_bends: Vec<PitchBend>`.

### Wire Format — `src/ipc/types.rs`
- `WireTrack` gains `pitch_bends: Vec<[u32; 2]>`, annotated
  `#[serde(default, rename = "pitch-bends")]`: `default` satisfies F-9 (field may be omitted,
  treated as empty), `rename` maps the hyphenated JSON key from the briefing's example to the
  idiomatic Rust field name.

### Wire-to-Domain Conversion — `src/ipc/handler.rs`
- `wire_track_to_domain` extended with a `.map(|[tick, value]| PitchBend { tick, value })`
  pass over `t.pitch_bends`, identical in shape to the existing `notes` mapping.

### Validation — `src/domain/validation.rs`
- Two new `ValidationError` variants:
  - `PitchBendValueOutOfRange { track: usize, event: usize, actual: u32 }`
  - `PitchBendTickOutOfRange { track: usize, event: usize, tick: u32, loop_duration: u32 }`
- `validate()`: within the existing per-track loop, after the note-validation loop finishes
  (F-8), a new loop over `track.pitch_bends` checks `actual <= 16383` then
  `tick < loop_duration`, returning the corresponding error on the first violation found, in
  submission order (F-7).
- `validation_error_response` gains matching arms formatting both new variants into
  human-readable messages, following the existing `"track {track} note {note}: ..."` style
  (NF-2).

---

## Data Model

| Type | Fields | Notes |
|------|--------|-------|
| `PitchBend` (new) | `tick: u32`, `value: u32` | New domain struct in `src/domain/project.rs`; `value` validated `0..=16383`, `tick` validated `< loop_duration` |
| `Track.pitch_bends` (new field) | `Vec<PitchBend>` | Added alongside existing `notes: Vec<Note>` |
| `WireTrack.pitch_bends` (new field) | `Vec<[u32; 2]>` | `#[serde(default, rename = "pitch-bends")]`; each pair is `[tick, value]` |
| `ValidationError::PitchBendValueOutOfRange` (new variant) | `track: usize`, `event: usize`, `actual: u32` | Mirrors `NoteDurationZero`'s track/note attribution style |
| `ValidationError::PitchBendTickOutOfRange` (new variant) | `track: usize`, `event: usize`, `tick: u32`, `loop_duration: u32` | Mirrors `NoteStartTickOutOfRange` |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID   | Task | Type | PRD ref | Depends on |
|------|------|------|---------|------------|
| T-1  | Unit test: construct a `PitchBend { tick, value }` and a `Track` with a `pitch_bends` list | test | F-1 | — |
| T-2  | Add `PitchBend` struct and `Track.pitch_bends: Vec<PitchBend>` field | impl | F-1 | T-1 |
| T-3  | Unit test: `WireTrack` deserializes `"pitch-bends": [[tick, value], ...]`; a track JSON with no `pitch-bends` key deserializes with an empty list | test | F-1, F-9, AC-7 | — |
| T-4  | Add `WireTrack.pitch_bends: Vec<[u32; 2]>` with `#[serde(default, rename = "pitch-bends")]` | impl | F-1, F-9 | T-3 |
| T-5  | Unit test: `wire_track_to_domain` maps `WireTrack.pitch_bends` `[tick, value]` pairs into `Vec<PitchBend>` on the resulting `Track` | test | F-1, F-4, AC-1 | T-2, T-4 |
| T-6  | Extend `wire_track_to_domain` to convert `pitch_bends` | impl | F-1, F-4, AC-1 | T-5 |
| T-7  | Unit test: `validate()` accepts pitch-bend values 0, 8192, and 16383; rejects a value outside `0..=16383` and identifies the failing track/event index | test | F-2, F-6, AC-2, AC-3, AC-4, AC-5 | T-2 |
| T-8  | Add `ValidationError::PitchBendValueOutOfRange` and the value-range check in `validate()` | impl | F-2, F-6 | T-7 |
| T-9  | Unit test: `validate()` accepts a tick `< loop_duration`; rejects a tick `>= loop_duration` and identifies the failing track/event index | test | F-3, F-6, AC-6 | T-8 |
| T-10 | Add `ValidationError::PitchBendTickOutOfRange` and the tick-range check in `validate()` | impl | F-3, F-6 | T-9 |
| T-11 | Unit test: `validate()` accepts pitch-bend events submitted out of tick order and with repeated ticks, and accepts a track with a large number of pitch-bend events | test | F-7, F-10 | T-10 |
| T-12 | Confirm the pitch-bend validation loop applies no sorting, dedup, or count limit (adjust only if T-11 fails) | impl | F-7, F-10 | T-11 |
| T-13 | Unit test: given a track with both an invalid note and an invalid pitch-bend event, `validate()` reports the note's `ValidationError`, not the pitch-bend one | test | F-8 | T-10 |
| T-14 | Order the per-track validation loop so notes are fully validated before pitch-bends (adjust only if T-13 fails) | impl | F-8 | T-13 |
| T-15 | Unit test: `validation_error_response` formats `PitchBendValueOutOfRange` and `PitchBendTickOutOfRange` into human-readable messages naming the track and event | test | NF-2 | T-8, T-10 |
| T-16 | Add matching arms to `validation_error_response` for both new variants | impl | NF-2 | T-15 |

---

## Open Questions

None. The specification is complete.

---

## Open Decisions

None. This epic is a direct, low-risk extension of existing, well-established patterns
(`Note`, `ValidationError`, `WireTrack`) with no remaining architectural trade-offs.

---

## Revision Log

### Cycle 1 — Confidence: 92%
- Created initial technical specification from the EP-1 PRD (confidence 95%).
- No open questions or decisions — spec derives directly from existing `Note`/
  `ValidationError`/`WireTrack` patterns with no unresolved trade-offs.
