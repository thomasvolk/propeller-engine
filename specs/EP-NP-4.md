# EP-NP-4 · IPC Handler & Validation — PRD

## Overview

Update `src/ipc/handler.rs` and `src/domain/validation.rs` to work with the new
tick-based protocol introduced by EP-NP-1 (domain model) and EP-NP-2 (wire
types). The bar- and time-signature-scoped validation rules are removed and
replaced with three new tick-level rules: `LoopDurationZero`,
`NoteStartTickOutOfRange`, and `NoteDurationExceedsLimit`. The handler functions
`build_domain_project()`, `wire_track_to_domain()`, `handle_set_bpm()`,
`handle_status()`, and `validation_error_response()` are updated to use
`loop_duration` throughout; `wire_bar_to_domain()` and `wire_note_to_domain()`
are deleted. This epic depends on EP-NP-1 and EP-NP-2 and may land in parallel
with EP-NP-3 and EP-NP-5.

**Confidence Level:** 95% — all requirements, journeys, and acceptance criteria are specified; no open ambiguities remain.

---

## User Journeys

### UJ-1 · Client submits a create-project with loop_duration zero

A client sends `{"command":"create-project","header":{"bpm":120,"loop_duration":0},"tracks":[]}`.
The handler calls `build_domain_project()`, constructs a `Project`, passes it to
the validator, which returns `LoopDurationZero`. The handler maps this to a
`validation_error` response with a human-readable message. No project is stored.

### UJ-2 · Client submits a note whose start_tick equals or exceeds loop_duration

A client sends a note tuple where `start_tick >= loop_duration`. After conversion
in `wire_track_to_domain()`, the validator catches the out-of-range position and
returns `NoteStartTickOutOfRange`. The handler returns a `validation_error`
response identifying the track and note index, and including the offending
`start_tick` and `loop_duration` values. No project is stored.

### UJ-3 · Client submits a note whose carry-over duration exceeds the limit

A client sends a note where `start_tick + duration > 2 * loop_duration`. The
validator returns `NoteDurationExceedsLimit`. The handler returns a
`validation_error` response including the offending duration and the limit. No
project is stored.

### UJ-4 · Client submits valid overlapping notes and they are accepted

Two notes in the same track share the same `start_tick`. The validator does not
treat overlap as an error. The handler stores the project and returns `{"status":"ok"}`.

### UJ-5 · Client issues set-bpm while a project is active

`handle_set_bpm()` reconstructs the active project with the new BPM value,
carrying `loop_duration` from `header.loop_duration` (not from a
`time_signature`). The reconstructed project is stored as pending.

### UJ-6 · Client queries status with an active project

`handle_status()` returns a JSON object that includes `loop_duration` from the
active project's header and omits any `time_signature` field.

---

## Functional Requirements

| ID   | Requirement                                                                                                                                   |
| ---- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| F-1  | `ValidationError` removes `InvalidTimeSignatureNumerator`, `InvalidTimeSignatureDenominator`, and `EmptyTrackBars` variants entirely.         |
| F-2  | `ValidationError` removes `NoteDurationExceedsBar` and replaces it with `NoteDurationExceedsLimit { track: usize, note: usize, duration: u32, limit: u32 }`. |
| F-3  | `ValidationError` adds `LoopDurationZero` as a unit variant (no fields), triggered when `header.loop_duration == 0`.                          |
| F-4  | `ValidationError` adds `NoteStartTickOutOfRange { track: usize, note: usize, start_tick: u32, loop_duration: u32 }`, triggered when `note.start_tick >= loop_duration` for any note in any track. |
| F-5  | `ValidationError` updates `NoteDurationZero` to use `{ track: usize, note: usize }` (the `bar` field is removed).                            |
| F-6  | The validator checks `LoopDurationZero` before iterating tracks or notes.                                                                     |
| F-7  | Per note, the validator checks `NoteDurationZero` first, then `NoteStartTickOutOfRange`, then `NoteDurationExceedsLimit`.                     |
| F-8  | An empty `notes` list on a track is valid; the validator does not return an error for it.                                                     |
| F-9  | `build_domain_project()` reads `WireHeader.loop_duration` (a `u32`) and sets `Header.loop_duration`; no `time_signature` field is read.       |
| F-10 | `wire_track_to_domain()` converts `WireTrack.notes: Vec<[u32; 4]>` by mapping each tuple `[start_tick, duration, pitch, velocity]` to a `Note` struct; it no longer calls `wire_bar_to_domain()`. |
| F-11 | `wire_bar_to_domain()` and `wire_note_to_domain()` are deleted from `src/ipc/handler.rs`.                                                    |
| F-12 | `handle_set_bpm()` reconstructs the active project carrying `header.loop_duration` from the existing project; no `time_signature` field is used. |
| F-13 | `handle_status()` includes `loop_duration` from the active project when a project is loaded, and omits the field entirely when no project is loaded. |
| F-14 | `handle_status()` does not include a `time_signature` field in its response under any circumstances.                                           |
| F-15 | `validation_error_response()` maps each new `ValidationError` variant to a human-readable `message` string and an `"error"` status response.  |
| F-16 | `validation_error_response()` removes match arms for `InvalidTimeSignatureNumerator`, `InvalidTimeSignatureDenominator`, `EmptyTrackBars`, and `NoteDurationExceedsBar`. |

---

## Non-Functional Requirements

| ID   | Requirement                                                                                                                     |
| ---- | ------------------------------------------------------------------------------------------------------------------------------- |
| NF-1 | `cargo build` and `cargo test` must produce zero compiler warnings after this epic lands.                                       |
| NF-2 | All existing handler and validation tests are updated to use the new wire format or replaced with equivalent tick-based tests.  |
| NF-3 | This epic must not modify `src/loop_engine/` or `src/domain/project.rs` — those belong to EP-NP-3 and EP-NP-1 respectively.   |
| NF-4 | `cargo test` must pass independently after this epic lands (i.e., EP-NP-1 and EP-NP-2 must be merged first).                  |

---

## Acceptance Criteria

| ID    | Given                                                                                       | When                                    | Then                                                                                                                         |
| ----- | ------------------------------------------------------------------------------------------- | --------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| AC-1  | `create-project` payload with `loop_duration: 0`                                            | handler processes the request           | Response is `validation_error`; no project stored                                                                            |
| AC-2  | `create-project` payload with a note where `start_tick >= loop_duration`                    | handler processes the request           | Response is `validation_error`; no project stored                                                                            |
| AC-3  | `create-project` payload with `note.duration = 0`                                           | handler processes the request           | Response is `validation_error`; no project stored                                                                            |
| AC-4  | `create-project` payload with `start_tick + duration > 2 * loop_duration`                   | handler processes the request           | Response is `validation_error`; no project stored                                                                            |
| AC-5  | `create-project` payload with valid overlapping notes (same `start_tick`, same channel)     | handler processes the request           | Response is `{"status":"ok"}`; project is stored as active                                                                   |
| AC-6  | `create-project` payload with valid note where `start_tick + duration == 2 * loop_duration` | handler processes the request           | Response is `{"status":"ok"}`; project stored (boundary is inclusive)                                                        |
| AC-7  | `handle_set_bpm` with an active project                                                     | BPM changes via `set-bpm`              | Reconstructed project retains `loop_duration`; no `time_signature` field on reconstructed header                             |
| AC-8  | `status` command with an active project                                                     | handler processes the request           | Response includes `loop_duration` matching the active project's value; no `time_signature` key present                       |
| AC-9  | `status` command with no active project                                                     | handler processes the request           | Response does not include `time_signature`; `loop_duration` key is absent from the JSON                                      |
| AC-10 | `validation_error_response` receives `LoopDurationZero`                                     | called                                  | Returns `{"status":"error","code":"validation_error","message":"..."}` with non-empty message                                |
| AC-11 | `validation_error_response` receives `NoteStartTickOutOfRange`                              | called                                  | Returns `{"status":"error","code":"validation_error","message":"..."}` with non-empty message that includes the offending `start_tick` and `loop_duration` values |
| AC-12 | `validation_error_response` receives `NoteDurationExceedsLimit`                             | called                                  | Returns `{"status":"error","code":"validation_error","message":"..."}` including the offending duration and the limit        |
| AC-13 | `cargo test` is run after EP-NP-1 and EP-NP-2 are merged                                   | all handler and validation tests run    | All tests pass; zero compiler warnings                                                                                       |
| AC-14 | `create-project` payload with a note where `duration == 0` and `start_tick >= loop_duration` | handler processes the request           | Response is `validation_error` for `NoteDurationZero`, not `NoteStartTickOutOfRange`                                         |

---

## Open Questions

No open questions.

---

## Refinement Log

### Cycle 1 — Confidence: 72%

- Reconciled: none (PRD created from roadmap; no prior answered questions)
- Added: Q1 (NoteStartTickOutOfRange fields), Q2 (LoopDurationZero variant shape), Q3 (loop_duration in status when no project)

### Cycle 2 — Confidence: 85%

- Reconciled: Q1 → F-4, AC-11 updated (NoteStartTickOutOfRange carries `start_tick` and `loop_duration`); Q2 → F-3 clarified as unit variant; Q3 → F-13, AC-9 updated (loop_duration omitted entirely when no project)
- Added: Q4 (per-note check order for NoteDurationZero vs NoteStartTickOutOfRange)

### Cycle 3 — Confidence: 95%

- Reconciled: Q4 → F-7 confirmed as-is (NoteDurationZero first), AC-14 added (priority conflict test case)
- Added: none — PRD is complete
