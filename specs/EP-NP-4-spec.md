# EP-NP-4 · IPC Handler & Validation — Technical Specification

## Overview

This epic updates `src/domain/validation.rs` and `src/ipc/handler.rs` to work with
the new tick-based domain model (EP-NP-1) and wire types (EP-NP-2). The
`ValidationError` enum gains three new tick-level variants (`LoopDurationZero`,
`NoteStartTickOutOfRange`, `NoteDurationExceedsLimit`) and loses four bar/time-signature
variants that were deleted in EP-NP-1. The handler functions `build_domain_project()`,
`wire_track_to_domain()`, `handle_set_bpm()`, `handle_status()`, and
`validation_error_response()` are updated to use `loop_duration` throughout.

**Confidence Level:** 93% — all functional requirements, acceptance criteria, and data
model details are fully specified; no open questions or decisions remain.

---

## Architecture Overview

Two files change in this epic; all other source files are untouched (NF-3).

`src/domain/validation.rs` owns the `ValidationError` enum and the `validate()`
function. After EP-NP-1 the enum already lacks `InvalidTimeSignatureNumerator`,
`InvalidTimeSignatureDenominator`, `EmptyTrackBars`, and `NoteDurationExceedsBar`, and
`NoteDurationZero` already carries only `{ track, note }`. EP-NP-4 adds three new
variants and rewrites the per-note check logic.

`src/ipc/handler.rs` owns the IPC command dispatch. After EP-NP-2 the handler already
operates against `WireHeader { bpm: u32, loop_duration: u32 }` and
`WireTrack { notes: Vec<[u32; 4]> }`; the stub implementations of `build_domain_project()`
and `wire_track_to_domain()` are replaced with their full implementations in this epic.
The dead `bpm.fract()` guard in `build_domain_project()` is removed (EP-NP-2 moved
integer enforcement to the serde boundary). `handle_set_bpm()` and `handle_status()` are
updated to carry `loop_duration` instead of `time_signature`.

**Validation check order in `validate()`:**

1. `BpmOutOfRange` (existing)
2. `LoopDurationZero` (new — before any track or note iteration, per F-6)
3. For each track: `InvalidMidiChannel`, `InvalidMidiInstrument`
4. For each note in the track: `NoteDurationZero`, `NoteStartTickOutOfRange`,
   `NoteDurationExceedsLimit` (in this order, per F-7)

**Per-note condition table:**

| Condition                                    | Error                     |
| -------------------------------------------- | ------------------------- |
| `note.duration == 0`                         | `NoteDurationZero`        |
| `note.start_tick >= loop_duration`           | `NoteStartTickOutOfRange` |
| `note.start_tick + note.duration > 2 * loop_duration` | `NoteDurationExceedsLimit` |
| otherwise                                    | ok                        |

Because `NoteDurationZero` is checked first, a note with both `duration == 0` and
`start_tick >= loop_duration` returns `NoteDurationZero` (AC-14).

---

## Components

### `src/domain/validation.rs`

`ValidationError` receives three new variants (F-2, F-3, F-4) and confirms the deletion
of four variants already removed in EP-NP-1 (F-1). `validate()` is updated to check
`LoopDurationZero` immediately after `BpmOutOfRange` (F-6) and to apply the per-note
check order (F-7). An empty `notes` list on a track is valid; no error is returned (F-8).
When constructing `NoteDurationExceedsLimit`, `limit` is set to `2 * loop_duration` —
the absolute upper bound for `note.start_tick + note.duration`.

### `src/ipc/handler.rs`

`build_domain_project()` reads `WireHeader.loop_duration` directly and sets
`Header.loop_duration`; it no longer computes the shim formula from a time signature
and no longer guards against fractional BPM (F-9).

`wire_track_to_domain()` maps each `[u32; 4]` element as `[start_tick, duration, pitch,
velocity]` to a `Note` struct; `pitch` and `velocity` are cast from `u32` to `u8` (F-10).
`wire_bar_to_domain()` and `wire_note_to_domain()` are confirmed absent (F-11; deleted
in EP-NP-2).

`handle_set_bpm()` reconstructs the active project by cloning `header.loop_duration`
from the existing project; no `TimeSignature` field is referenced (F-12).

`handle_status()` emits a top-level `"loop_duration"` key when a project is active
(AC-8); the key is omitted entirely when no project is loaded (AC-9). The
`"time_signature"` key is never emitted under any circumstances (F-14).

`validation_error_response()` is extended with match arms for all three new
`ValidationError` variants (F-15) and the dead arms for deleted variants are removed
(F-16).

---

## Data Model

| Type                                    | Fields                                                                               | Notes                                                                       |
| --------------------------------------- | ------------------------------------------------------------------------------------ | --------------------------------------------------------------------------- |
| `ValidationError::LoopDurationZero`     | —                                                                                    | Unit variant; `header.loop_duration == 0`                                   |
| `ValidationError::NoteStartTickOutOfRange` | `track: usize`, `note: usize`, `start_tick: u32`, `loop_duration: u32`           | `note.start_tick >= loop_duration`                                          |
| `ValidationError::NoteDurationExceedsLimit` | `track: usize`, `note: usize`, `duration: u32`, `limit: u32`                   | `start_tick + duration > 2 * loop_duration`; `limit = 2 * loop_duration`   |
| `ValidationError::NoteDurationZero`     | `track: usize`, `note: usize`                                                        | Kept from EP-NP-1; `bar` field already removed                              |
| `ValidationError::BpmOutOfRange`        | `actual: u32`                                                                        | Kept unchanged                                                              |
| `ValidationError::InvalidMidiChannel`   | `track: usize`, `actual: u8`                                                         | Kept unchanged                                                              |
| `ValidationError::InvalidMidiInstrument`| `track: usize`, `actual: u8`                                                         | Kept unchanged                                                              |

**Confirmed-deleted variants (removed in EP-NP-1, absent at start of this epic):**

| Variant                                 | Removed by |
| --------------------------------------- | ---------- |
| `InvalidTimeSignatureNumerator`         | EP-NP-1    |
| `InvalidTimeSignatureDenominator`       | EP-NP-1    |
| `EmptyTrackBars`                        | EP-NP-1    |
| `NoteDurationExceedsBar`               | EP-NP-1    |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID   | Task                                                                                                                                                                           | Type | PRD ref                            | Depends on      |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---- | ---------------------------------- | --------------- |
| T-1  | Write failing tests: match against `ValidationError::LoopDurationZero`; assert `validate()` returns `LoopDurationZero` when `loop_duration == 0`; assert the check precedes track iteration (e.g., invalid channel + zero loop_duration → `LoopDurationZero`) | test | F-3, F-6, AC-1       | —               |
| T-2  | Add `LoopDurationZero` variant to `ValidationError`; insert `LoopDurationZero` check in `validate()` after `BpmOutOfRange` and before the track loop                          | impl | F-3, F-6                           | T-1             |
| T-3  | Write failing tests: `NoteStartTickOutOfRange` for `start_tick >= loop_duration` (AC-2); `NoteDurationExceedsLimit` for `start_tick + duration > 2 * loop_duration` (AC-4) with `limit == 2 * loop_duration`; boundary note `start_tick + duration == 2 * loop_duration` → Ok (AC-6); overlapping notes → Ok (AC-5); empty notes list → Ok (F-8); priority: `duration == 0` AND `start_tick >= loop_duration` → `NoteDurationZero` not `NoteStartTickOutOfRange` (AC-14); assert `start_tick` and `loop_duration` fields on `NoteStartTickOutOfRange` are correct | test | F-4, F-5, F-7, F-8, AC-2, AC-3, AC-4, AC-5, AC-6, AC-14 | T-2 |
| T-4  | Add `NoteStartTickOutOfRange` and `NoteDurationExceedsLimit` variants to `ValidationError`; update `validate()` per-note checks                                                | impl | F-2, F-4, F-7, F-8                 | T-3             |
| T-5  | Write failing tests for `validation_error_response()`: `LoopDurationZero` → non-empty message (AC-10); `NoteStartTickOutOfRange` → message includes the `start_tick` and `loop_duration` integer values (AC-11); `NoteDurationExceedsLimit` → message includes the `duration` value and the `limit` value (`== 2 * loop_duration`) (AC-12); all three return `status: "error"` and `code: "validation_error"` | test | F-15, AC-10, AC-11, AC-12          | T-4             |
| T-6  | Update `validation_error_response()`: add match arms for `LoopDurationZero`, `NoteStartTickOutOfRange`, `NoteDurationExceedsLimit`; remove arms for the four deleted variants (F-16) | impl | F-15, F-16                         | T-5             |
| T-7  | Write failing handler integration tests: `create-project` with `loop_duration: 0` → validation_error (AC-1); note with `start_tick >= loop_duration` → validation_error (AC-2); `duration == 0` → validation_error (AC-3); `start_tick + duration > 2 * loop_duration` → validation_error (AC-4); overlapping notes → ok (AC-5); boundary note → ok (AC-6); priority note → `NoteDurationZero` (AC-14); all using new wire format `{"bpm": u32, "loop_duration": u32}` and `"notes": [[u32; 4]]` | test | F-9, F-10, AC-1–AC-6, AC-13, AC-14 | T-4, T-6       |
| T-8  | Update `build_domain_project()`: read `header.loop_duration` directly; remove dead `bpm.fract()` guard; construct `Header { bpm, loop_duration }`. Update `wire_track_to_domain()`: map `[start_tick, duration, pitch, velocity]` tuple to `Note { start_tick, duration, pitch: pitch as u8, velocity: velocity as u8 }`. Confirm `wire_bar_to_domain()` and `wire_note_to_domain()` are absent | impl | F-9, F-10, F-11                    | T-7             |
| T-9  | Write failing handler integration test: `set-bpm` while a project with known `loop_duration` is active → reconstructed project retains `loop_duration`; response `status: "ok"` (AC-7) | test | F-12, AC-7                         | T-8             |
| T-10 | Update `handle_set_bpm()`: clone `active.header.loop_duration` into reconstructed project; remove all `TimeSignature` references                                                | impl | F-12                               | T-9             |
| T-11 | Write failing handler integration tests: `status` with active project → response includes top-level `"loop_duration"` key matching the project's `loop_duration` (AC-8); `status` with no project → `"loop_duration"` key is absent from response (AC-9); neither response contains a `"time_signature"` key (F-14) | test | F-13, F-14, AC-8, AC-9             | T-8             |
| T-12 | Update `handle_status()`: emit `"loop_duration": active.header.loop_duration` when a project is active; omit the key entirely when no project; remove all `time_signature` JSON construction | impl | F-13, F-14                         | T-11            |

---

## Open Questions

No open questions.

---

## Open Decisions

No open decisions.

---

## Revision Log

### Cycle 1 — Confidence: 82%
- Reconciled: none (spec created from PRD and full codebase analysis of `validation.rs`, `handler.rs`, `EP-NP-1-spec.md`, `EP-NP-2-spec.md`)
- Added: Q-1 (`NoteDurationExceedsLimit.limit` value)

### Cycle 2 — Confidence: 82%
- Reconciled: none (Q-1 not yet answered; no checked decisions)
- Added: nothing — no new ambiguities identified

### Cycle 3 — Confidence: 93%
- Reconciled: Q-1 (A selected) → Data Model updated (`limit = 2 * loop_duration`); Components/validation.rs updated (limit computation made explicit); T-3 and T-5 updated to assert specific `limit` value; Q-1 removed from Open Questions
- Added: nothing — specification is complete
