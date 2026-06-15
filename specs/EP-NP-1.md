# EP-NP-1 · Domain Model Refactoring — PRD

## Overview

Replace the bar-based domain model in `src/domain/project.rs` with a flat,
tick-addressed note list. The `TimeSignature`, `Bar`, and `NoteEvent` types are
removed along with their derived methods. `Header` gains a `loop_duration: u32`
field that becomes the single source of loop length for the entire system.
`Track` gains a flat `notes: Vec<Note>` list, and `Note` is redesigned as a
plain data struct holding `start_tick`, `duration`, `pitch`, and `velocity`.
This epic is the foundation for all other new-protocol epics; nothing else can
merge until it lands.

**Confidence Level:** 92% — All open questions answered; all roadmap items
covered. Minor residual uncertainty around unchanged `Project` struct fields not
explicitly listed in the roadmap.

---

## User Journeys

### UJ-1 · Engine reads loop length from the active project

The player needs to know how many ticks make up one loop iteration. It reads
`project.header.loop_duration` directly — no derived method or formula is
required.

### UJ-2 · Engine iterates over a track's notes

When building the event list for a loop iteration, the player iterates
`track.notes` and reads `note.start_tick`, `note.duration`, `note.pitch`, and
`note.velocity` directly from each note struct.

### UJ-3 · IPC handler converts wire notes into domain notes

The handler receives a raw JSON array `[start_tick, duration, pitch, velocity]`
per note, constructs `Note` structs, and pushes them into `Track.notes`. No
bar or rest concept is involved.

### UJ-4 · Validation layer inspects notes against loop length

The validator reads `project.header.loop_duration` and each
`note.start_tick + note.duration` to check for out-of-range values. No
time-signature arithmetic is needed.

---

## Functional Requirements

| ID   | Requirement                                                                                                    |
| ---- | -------------------------------------------------------------------------------------------------------------- |
| F-1  | `Header` contains exactly two fields: `bpm: u32` and `loop_duration: u32`.                                    |
| F-2  | `Track` contains `name: String`, `channel: u8`, `instrument: u8`, and `notes: Vec<Note>`. `bars` is removed.  |
| F-3  | `Note` is a named struct: `struct Note { start_tick: u32, duration: u32, pitch: u8, velocity: u8 }`. Tuple-struct and type-alias forms are rejected. |
| F-4  | `TimeSignature` is deleted from the codebase with no replacement in the domain layer.                          |
| F-5  | `Bar` is deleted from the codebase with no replacement in the domain layer.                                    |
| F-6  | `NoteEvent` is deleted from the codebase. The `Rest` concept no longer exists in the domain model.             |
| F-7  | `bar_ticks()` method is deleted. No equivalent method is added to `Header` or any other type.                  |
| F-8  | `cycle_length()` method on `Project` is deleted. No replacement is added.                                      |
| F-9  | `bar_at()` method on `Track` is deleted. No replacement is added.                                              |
| F-10 | `pub const PPQN: u32 = 480` remains in `src/domain/project.rs` and stays public.                              |
| F-11 | An empty `notes` list on a `Track` is a valid domain value; the validation layer (EP-NP-4) enforces any constraints. |
| F-12 | A `loop_duration` of zero is a valid domain value at the model level; the validation layer enforces the minimum. |
| F-13 | `Project`, `Header`, and `Track` expose no computed instance methods; all values are read directly from public fields. |

---

## Non-Functional Requirements

| ID   | Requirement                                                                                                            |
| ---- | ---------------------------------------------------------------------------------------------------------------------- |
| NF-1 | The change must not introduce any compiler warnings (`cargo build` and `cargo test` must produce zero warnings).       |
| NF-2 | All pre-existing unit tests are either updated to use the new types or replaced with equivalent tests for new behaviour. No test may be silently deleted without a replacement. |
| NF-3 | `cargo test` must pass after this epic lands, independently of later epics.                                            |
| NF-4 | The domain model is a pure data layer; it must not import from `ipc`, `loop_engine`, or any other higher-level crate module. |

---

## Acceptance Criteria

| ID   | Given                                               | When                                              | Then                                                                              |
| ---- | --------------------------------------------------- | ------------------------------------------------- | --------------------------------------------------------------------------------- |
| AC-1 | A `Header` is constructed                           | `bpm` and `loop_duration` are supplied            | Both fields are stored and readable; no `time_signature` field exists             |
| AC-2 | A `Note` is constructed with named-field syntax     | All four fields are supplied                      | `note.start_tick`, `note.duration`, `note.pitch`, `note.velocity` are readable    |
| AC-3 | A `Track` is constructed with a non-empty note list | `notes` is assigned                               | `track.notes` returns the same slice; no `bars` field exists                      |
| AC-4 | A `Track` is constructed with an empty note list    | `notes` is `vec![]`                               | Construction succeeds; the domain model does not panic or error                   |
| AC-5 | The codebase is compiled                            | `cargo build` runs                                | `TimeSignature`, `Bar`, `NoteEvent`, `bar_ticks`, `cycle_length`, `bar_at` are absent and the build succeeds |
| AC-6 | `cargo test` is run                                 | After this epic lands with no other epics applied | All tests pass; zero compiler warnings                                            |
| AC-7 | `PPQN` is referenced from another module            | `use crate::domain::PPQN`                         | It resolves to `480u32`                                                           |
| AC-8 | The public API of `Project`, `Header`, and `Track` is inspected | After this epic lands                | No computed instance methods exist on any of these types; all values are read via public fields |

---

## Open Questions

No open questions remain. The PRD is complete and ready for implementation.

---

## Refinement Log

### Cycle 1 — Confidence: 72%
- Reconciled: none (PRD created from roadmap; no prior answered questions)
- Added: Q1 (loop_duration type), Q2 (Note layout), Q3 (helper methods)

### Cycle 2 — Confidence: 72%
- Reconciled: none (Q1, Q2, Q3 remain unanswered)
- Added: none (Q1–Q3 already cover all identified gaps; no new ambiguities found)

### Cycle 3 — Confidence: 75%
- Reconciled: Q3 (helper methods → A, pure data struct) → F-13 (no computed methods on Project/Header/Track), AC-8 (API surface verified method-free)
- Added: none (Q1 and Q2 remain the only unresolved gaps)

### Cycle 4 — Confidence: 82%
- Reconciled: Q1 (loop_duration type → A, u32) → confirms F-1 already correct; no new requirement needed
- Added: none (Q2 is the sole remaining open question)

### Cycle 5 — Confidence: 92%
- Reconciled: Q2 (Note layout → A, named struct) → F-3 updated to explicitly specify named-struct form and reject tuple-struct/alias alternatives; AC-2 updated to use named-field access syntax
- Added: none (no open questions remain; PRD is complete)
