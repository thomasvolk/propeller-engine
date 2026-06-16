# EP-NP-5 · Documentation & Spec — Technical Specification

## Overview

This epic edits two text files with no Rust code changes: `docs/json-socket-interface.md`
(the user-facing JSON API reference) and `specs/propeller.allium` (the domain
specification). Both files must be updated to replace all bar- and time-signature-based
concepts with the new tick-based model: `loop_duration` replaces `time_signature`,
a flat `notes` list replaces `bars`, and the `rest` concept is removed entirely.
The allium spec additionally requires new and renamed invariants to cover cross-loop
note carry-over and loop-boundary semantics.

**Confidence Level:** 95% — all decisions resolved and all PRD requirements covered; minor residual uncertainty is the exact allium constraint-annotation syntax within the new `value Note` block.

---

## Architecture Overview

There is no runtime component to this epic. The work is two independent textual edits:

1. **`docs/json-socket-interface.md`** — Replace all wire-format references. Every JSON
   example, field table, and prose description that mentions `time_signature`, `bars`,
   `duration_ticks`, `rest`, or `bar boundary` must be updated. Two new sections
   ("Overlapping notes" and "Cross-loop notes") are added. The error code table gains
   four new entries and loses any bar-based codes. The status response table gains
   `loop_duration` and loses `time_signature`.

2. **`specs/propeller.allium`** — Remove the three legacy value types (`TimeSignature`,
   `NoteSpec`, `BarSpec`). Update `ProjectData` and `TrackSpec`. Add a named
   `value Note { start_tick, duration, pitch, velocity }` block. Update the `requires`
   clauses of `CreateProject` and `ModifyProject`.
   Add a `CrossLoopNoteCarryOver` invariant. Rename `PendingAppliedAtBarBoundary` and
   `SyncRestartResetsBarIndex`; update all bar-terminology comments in rules and
   invariants. The file must pass `tend` linter validation with zero errors after all
   changes.

The two edits are independent and can be done in any order or simultaneously.

---

## Components

### docs/json-socket-interface.md

**Sections changed:**

| Section                    | Change                                                                              |
| -------------------------- | ----------------------------------------------------------------------------------- |
| Step-by-step guide (step 3) | Status response example: remove `time_signature`, add `loop_duration`              |
| Step-by-step guide (step 4) | create-project example: new wire format with `loop_duration` and notes tuple       |
| `create-project` reference  | JSON example: new format                                                            |
| `modify-project` reference  | Prose: "next bar boundary" → "next loop boundary"                                  |
| `set-bpm` reference         | Prose: "next bar boundary" → "next loop boundary"                                  |
| `clock-pause` reference     | Prose: "mid-bar" → "mid-loop", "bar position" → "loop position"                   |
| `clock-stop` reference      | Prose: "resets the bar index to 0" → "resets the loop position"                   |
| Header field reference      | Remove `time_signature.*` rows; add `loop_duration` row                             |
| Track field reference       | `bars` row → `notes` row                                                            |
| Note field reference        | Replace four old rows with note-tuple row `[start_tick, duration, pitch, velocity]` |
| Status response fields      | Remove `time_signature` row; add `loop_duration` row                                |
| Error codes                 | Add four new codes; remove bar-based codes                                          |
| New: Overlapping notes      | New section placed after Field reference, before Error codes                        |
| New: Cross-loop notes       | New section placed after Overlapping notes, before Error codes                      |
| Load-and-play example       | Updated to new wire format                                                          |

### specs/propeller.allium

**Value types:**

| Symbol       | Change                                                                                   |
| ------------ | ---------------------------------------------------------------------------------------- |
| `TimeSignature` | Removed entirely                                                                      |
| `NoteSpec`   | Removed entirely                                                                         |
| `BarSpec`    | Removed entirely                                                                         |
| `Note`       | Added as named value type: `value Note { start_tick: Integer, duration: Integer, pitch: Integer, velocity: Integer }` |
| `TrackSpec`  | `bars: List<BarSpec>` → `notes: List<Note>`                                              |
| `ProjectData` | `time_signature: TimeSignature` → `loop_duration: Integer`; `cycle_length` removed      |

**Rules updated:**

| Rule            | Change                                                                                       |
| --------------- | -------------------------------------------------------------------------------------------- |
| `CreateProject` | `requires` clauses: remove time_signature conditions; add `loop_duration > 0`, note tick-bounds |
| `ModifyProject` | Same as `CreateProject`; update "bar boundary" comment to "loop boundary"                    |
| `SetBpm`        | Comment: "next bar boundary" → "next loop boundary"                                          |
| `ExternalClockStart` | Comment: "bar 0" → "loop start"                                                         |
| `ExternalClockContinue` | Comment: "bar position" → "loop position"                                            |

**Invariants:**

| Symbol                      | Change                                                                              |
| --------------------------- | ----------------------------------------------------------------------------------- |
| `PendingAppliedAtBarBoundary` | Renamed `PendingAppliedAtLoopBoundary`; body text updated (bar → loop)            |
| `SyncRestartResetsBarIndex` | Renamed `SyncRestartResetsLoopPosition`; body text updated                          |
| `ProgramChangeBeforeFirstNoteOn` | Body text updated: "each bar" → "each loop"                                   |
| `CrossLoopNoteCarryOver`    | New invariant documenting carry-over semantics                                      |

---

## Data Model

| Type / Field                     | New Value                    | Notes                                                              |
| -------------------------------- | ---------------------------- | ------------------------------------------------------------------ |
| `ProjectData.loop_duration`      | `Integer`                    | Replaces `time_signature`; must be > 0                             |
| `ProjectData.cycle_length`       | removed                      | Derived from time_signature; no longer needed                      |
| `TrackSpec.notes`                | `List<Note>`                 | Replaces `bars`; may be empty                                      |
| `Note.start_tick`                | `Integer`                    | 0 ≤ start_tick < loop_duration                                     |
| `Note.duration`                  | `Integer`                    | > 0; start_tick + duration ≤ 2 × loop_duration                     |
| `Note.pitch`                     | `Integer`                    | 0–127                                                              |
| `Note.velocity`                  | `Integer`                    | 0–127                                                              |
| Wire note tuple                  | `[start_tick, duration, pitch, velocity]` | Documented in docs as four-element integer array    |
| `loop_duration_zero`             | new error code               | Replaces bar-based validation errors in docs error table           |
| `note_start_tick_out_of_range`   | new error code               | New error code                                                     |
| `note_duration_zero`             | new error code               | New error code                                                     |
| `note_duration_exceeds_limit`    | new error code               | New error code                                                     |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID   | Task                                                                                                    | Type | PRD ref               | Depends on          |
| ---- | ------------------------------------------------------------------------------------------------------- | ---- | --------------------- | ------------------- |
| T-1  | Confirm legacy terms exist in docs: grep for `time_signature`, `bars`, `duration_ticks`, `rest`, `bar boundary` | test | AC-1, NF-2  | —                   |
| T-2  | Replace header/track/note field reference tables and all wire-format examples in docs                   | impl | F-1,F-2,F-3,F-4,F-11 | T-1                 |
| T-3  | Verify create-project command reference uses `loop_duration` and notes tuple                            | test | AC-2                  | T-2                 |
| T-4  | Update modify-project and set-bpm descriptions: "bar boundary" → "loop boundary"                       | impl | F-5,F-6               | T-3                 |
| T-5  | Verify status response field table: `loop_duration` present, `time_signature` absent                   | test | AC-3                  | T-4                 |
| T-6  | Update status response field table                                                                      | impl | F-7                   | T-5                 |
| T-7  | Verify overlapping-notes and cross-loop-notes sections are absent (expect failure)                     | test | AC-4, AC-5            | T-6                 |
| T-8  | Add overlapping notes section and cross-loop notes section with examples (after Field reference, before Error codes) | impl | F-8, F-9 | T-7 |
| T-9  | Verify error code table is missing new validation error codes (expect failure)                          | test | AC-9                  | T-8                 |
| T-10 | Update error code table: add four new codes, remove bar-based codes                                     | impl | F-10                  | T-9                 |
| T-11 | Final grep check on docs: confirm all legacy terms are absent                                           | test | AC-1                  | T-10                |
| T-12 | Confirm legacy value types exist in allium: grep for `TimeSignature`, `NoteSpec`, `BarSpec`, `bars`    | test | AC-6                  | —                   |
| T-13 | Remove `TimeSignature`, `NoteSpec`, `BarSpec` value types from allium                                  | impl | F-12,F-13,F-14        | T-12                |
| T-14 | Replace `ProjectData.time_signature` with `loop_duration`; remove `cycle_length`; replace `TrackSpec.bars` with `notes` | impl | F-15,F-16,F-17 | T-13     |
| T-15 | Verify `CreateProject`/`ModifyProject` requires clauses still reference `time_signature` (expect failure) | test | AC-7                | T-14                |
| T-16 | Update `CreateProject` and `ModifyProject` requires clauses to tick-based validation                    | impl | F-18                  | T-15                |
| T-17 | Verify `CrossLoopNoteCarryOver` invariant is absent from allium (expect failure)                       | test | F-19                  | T-16                |
| T-18 | Add `CrossLoopNoteCarryOver` invariant                                                                  | impl | F-19                  | T-17                |
| T-19 | Verify old bar-terminology invariant names exist: `PendingAppliedAtBarBoundary`, `SyncRestartResetsBarIndex` | test | AC-10            | T-18                |
| T-20 | Rename invariants and update all bar-terminology in rule comments throughout allium                     | impl | F-20                  | T-19                |
| T-21 | Run `tend` validation on allium; assert zero errors                                                     | test | AC-8, NF-3            | T-20                |
| T-22 | Fix any remaining `tend` validation errors                                                              | impl | NF-3                  | T-21                |

---

## Open Questions

No open questions.

---

## Open Decisions

No open decisions.

---

## Revision Log

### Cycle 1 — Confidence: 82%

- Reconciled: none (spec created from PRD; no prior decisions or questions)
- Added: D-1 (Note representation in allium), D-2 (new section placement in docs)

### Cycle 2 — Confidence: 95%

- Reconciled: D-1 → named `value Note` type adopted; Components and Data Model updated; T-13 dependency cleaned; D-2 → new sections placed after Field reference before Error codes; Components and T-8 updated
- Added: none — specification is complete
