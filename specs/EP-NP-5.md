# EP-NP-5 · Documentation & Spec — PRD

## Overview

Update all user-facing and specification documents to reflect the new tick-based
protocol. `docs/json-socket-interface.md` must replace every `time_signature`,
`bars`, rest-note, and bar-boundary reference with the new flat note model
(`loop_duration`, `notes: [[start_tick, duration, pitch, velocity], ...]`).
`specs/propeller.allium` must align entity definitions and rules with the same
domain model, removing `TimeSignature`, `BarSpec`, and `NoteSpec`, and adding the
`loop_duration` field, the flat `notes` list, and cross-loop carry-over semantics.
This epic is purely textual and can land at any point after EP-NP-1 is understood.

**Confidence Level:** 92% — all roadmap requirements are covered and ACs are testable; the only residual uncertainty is the exact new identifier names chosen for renamed allium invariants, which the engineer can derive from context.

---

## User Journeys

### UJ-1 · Operator reads the docs to create a project with the new protocol

Operator opens `docs/json-socket-interface.md` to understand how to send a
`create-project` command. The docs show a header with `loop_duration` and a flat
`notes` array of four-element integer tuples. The operator assembles a valid payload
without encountering any reference to bars or time signatures.

### UJ-2 · Operator uses overlapping notes

Operator reads the overlapping notes section and understands that multiple notes can
share the same `start_tick` on the same channel. They construct a chord using three
entries with the same start tick and successfully load the project.

### UJ-3 · Operator uses a cross-loop note

Operator reads the cross-loop notes section and understands that a note whose
`start_tick + duration > loop_duration` carries into the next iteration, subject to
the `duration ≤ 2 × loop_duration` constraint. The docs example makes the carry-over
semantics unambiguous.

### UJ-4 · Operator uses `modify-project` to change a project while playing

Operator reads the `modify-project` description and sees that changes take effect at
the next **loop** boundary. They understand the timing semantics without any
bar-boundary confusion.

### UJ-5 · Developer validates the allium spec

Developer runs `tend` against `specs/propeller.allium` and receives no errors. The
spec reflects the new domain model with no legacy entities.

---

## Functional Requirements

| ID   | Requirement                                                                                                                                                                                                              |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| F-1  | `docs/json-socket-interface.md` removes all occurrences of `time_signature` from command examples, field reference tables, and the status response description.                                                          |
| F-2  | `docs/json-socket-interface.md` adds `loop_duration: integer` to the header field reference and to all `create-project` / `modify-project` examples.                                                                    |
| F-3  | `docs/json-socket-interface.md` replaces the `bars` track field with `notes`, and replaces the note field table (`pitch`, `velocity`, `duration_ticks`, `rest`) with a note-tuple table (`[start_tick, duration, pitch, velocity]`). |
| F-4  | `docs/json-socket-interface.md` removes the `rest` concept entirely; no mention of `"rest": true` remains.                                                                                                               |
| F-5  | `docs/json-socket-interface.md` updates `modify-project` semantics: changes take effect at the next **loop** boundary, not bar boundary.                                                                                 |
| F-6  | `docs/json-socket-interface.md` updates the `set-bpm` description: the new tempo takes effect at the next loop boundary.                                                                                                |
| F-7  | `docs/json-socket-interface.md` updates the status response field table: the `time_signature` field is removed; `loop_duration` is added (present when a project is loaded, absent when no project is loaded).          |
| F-8  | `docs/json-socket-interface.md` adds an "Overlapping notes" section explaining that multiple notes with the same `start_tick` on the same channel are valid.                                                             |
| F-9  | `docs/json-socket-interface.md` adds a "Cross-loop notes" section explaining that a note with `start_tick + duration > loop_duration` carries into the next iteration, with duration bounded at `2 × loop_duration`.    |
| F-10 | `docs/json-socket-interface.md` updates the error code table: removes any bar-based validation error codes; adds `loop_duration_zero`, `note_start_tick_out_of_range`, `note_duration_zero`, and `note_duration_exceeds_limit` as distinct top-level error codes. |
| F-11 | `docs/json-socket-interface.md` updates all shell examples in the step-by-step guide and the "Load and play" example section to use the new wire format.                                                                |
| F-12 | `specs/propeller.allium` removes the `TimeSignature` value type.                                                                                                                                                         |
| F-13 | `specs/propeller.allium` removes the `NoteSpec` value type (including its `rest` field).                                                                                                                                 |
| F-14 | `specs/propeller.allium` removes the `BarSpec` value type.                                                                                                                                                               |
| F-15 | `specs/propeller.allium` replaces `ProjectData.time_signature: TimeSignature` with `ProjectData.loop_duration: Integer`.                                                                                                 |
| F-16 | `specs/propeller.allium` removes the `cycle_length` computed field from `ProjectData`.                                                                                                                                   |
| F-17 | `specs/propeller.allium` replaces `TrackSpec.bars: List<BarSpec>` with `TrackSpec.notes` using a four-element integer tuple type for `[start_tick, duration, pitch, velocity]`.                                         |
| F-18 | `specs/propeller.allium` updates the `CreateProject` and `ModifyProject` rule `requires` clauses: removes `time_signature` and bar-based conditions; adds tick-based validation (`loop_duration > 0`, `note.start_tick < loop_duration`, `note.duration > 0`, `note.start_tick + note.duration ≤ 2 × loop_duration`). |
| F-19 | `specs/propeller.allium` adds a rule or invariant documenting cross-loop note carry-over: notes with `start_tick + duration > loop_duration` produce a NoteOff in the next loop iteration.                               |
| F-20 | `specs/propeller.allium` renames and updates all invariants and rule comments that reference bar terminology (`PendingAppliedAtBarBoundary`, `SyncRestartResetsBarIndex`, `ProgramChangeBeforeFirstNoteOn`, `SetBpm` comment, `ClockStop` description, etc.) to use loop terminology. |

---

## Non-Functional Requirements

| ID   | Requirement                                                                                                                                                    |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| NF-1 | All JSON code examples in `docs/json-socket-interface.md` must be syntactically valid JSON.                                                                    |
| NF-2 | No shell example in `docs/json-socket-interface.md` may reference old wire fields (`bars`, `time_signature`, `duration_ticks`, `rest`).                        |
| NF-3 | `specs/propeller.allium` must pass `tend` linter validation with zero errors after the changes.                                                                |
| NF-4 | This epic may be merged independently of EP-NP-4; the error code table entries (`loop_duration_zero` etc.) may be added as documentation ahead of the implementation landing. |

---

## Acceptance Criteria

| ID    | Given                                    | When                                                                                           | Then                                                                                                          |
| ----- | ---------------------------------------- | ---------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| AC-1  | `docs/json-socket-interface.md` updated  | A text search for `time_signature`, `bars`, `duration_ticks`, `rest`, and `bar boundary` runs  | No matches are found                                                                                          |
| AC-2  | `docs/json-socket-interface.md` updated  | The `create-project` example is read                                                           | The header contains `loop_duration` and the track contains a `notes` array of four-element integer arrays     |
| AC-3  | `docs/json-socket-interface.md` updated  | The status response field table is read                                                        | `loop_duration` appears (with a note that it is absent when no project is loaded) and `time_signature` does not |
| AC-4  | `docs/json-socket-interface.md` updated  | The overlapping notes section is read                                                          | The docs explain that multiple notes sharing the same `start_tick` on the same channel are valid              |
| AC-5  | `docs/json-socket-interface.md` updated  | The cross-loop notes section is read                                                           | The docs explain the carry-over rule and the `duration ≤ 2 × loop_duration` constraint with an example        |
| AC-6  | `specs/propeller.allium` updated         | A text search for `TimeSignature`, `BarSpec`, `NoteSpec`, `time_signature`, `bars`, `rest` runs | No matches are found                                                                                        |
| AC-7  | `specs/propeller.allium` updated         | The `CreateProject` and `ModifyProject` rule `requires` clauses are read                       | They reference `loop_duration`, `start_tick`, `duration`, and no time-signature or bar fields                 |
| AC-8  | `specs/propeller.allium` updated         | `tend` validation is run                                                                       | Zero errors are reported                                                                                      |
| AC-9  | `docs/json-socket-interface.md` updated  | The error code table is read                                                                   | `loop_duration_zero`, `note_start_tick_out_of_range`, `note_duration_zero`, and `note_duration_exceeds_limit` are present as distinct entries; bar-based error codes are absent |
| AC-10 | `specs/propeller.allium` updated         | A text search for `BarBoundary`, `BarIndex`, `bar boundary`, and `bar index` runs              | No matches are found                                                                                          |

---

## Open Questions

No open questions.

---

## Refinement Log

### Cycle 1 — Confidence: 62%

- Reconciled: none (PRD created from roadmap; no prior answered questions)
- Added: Q1 (loop_duration in status response), Q2 (validation error wire names), Q3 (allium invariant terminology scope)

### Cycle 2 — Confidence: 92%

- Reconciled: Q1 → F-7 updated (absent not null when no project), AC-3 updated; Q2 → F-10 updated with specific wire names (`loop_duration_zero` etc.), AC-9 added (error code table content); Q3 → F-20 confirmed in scope, AC-10 added (allium bar-terminology search)
- Added: none — PRD is complete
