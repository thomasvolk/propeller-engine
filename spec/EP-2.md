# EP-2 · Project Model — PRD

## Overview

A project is the central data structure that defines what MIDI signals the engine will send. It consists of a header (tempo and time signature) and a list of tracks, each containing bars of notes. Projects can be created and modified at runtime via the engine's runtime interface; updates always take effect on a bar boundary so the current bar plays to completion. The engine holds exactly one active project at a time.

**Confidence Level:** 92% — All core requirements are fully specified. The error response format in F-15 ("structured error") is intentionally deferred to EP-4.

---

## User Journeys

### UJ-1 · Creating a new project

A musician uses the runtime interface to define a new project: they provide a BPM, a time signature, and one or more tracks. Each track has a name, a MIDI channel, a MIDI instrument, and a sequence of bars filled with notes and rests. Once submitted, the engine begins using the project as its active project.

### UJ-2 · Modifying a running project at runtime

While the engine is looping through an active project, a musician sends a runtime command to add a track, change a note, or update the BPM. The running bar plays to completion; the change takes effect from the next bar boundary onward.

### UJ-3 · Replacing the active project

A musician loads a completely different project while the engine is running. The engine discards the previous project and adopts the new one, again waiting for the current bar to complete before switching.

### UJ-4 · Polymetric project with tracks of different lengths

A musician creates a project with two tracks: a bass track with 2 bars and a melody track with 4 bars. The engine plays both tracks simultaneously. The bass track loops every 2 bars while the melody track plays its full 4-bar phrase once. After 4 bars the project cycle restarts from the beginning of both tracks.

---

## Functional Requirements

| ID | Requirement |
|----|-------------|
| F-1 | A project consists of a header and an ordered list of tracks. |
| F-2 | The project header defines the BPM (tempo) and the time signature. |
| F-3 | A track has a name, a MIDI channel (1–16), a MIDI instrument (0–127), and an ordered list of bars. |
| F-4 | A bar contains an ordered list of notes. |
| F-5 | Every bar in a project has the same length, determined by the time signature. |
| F-6 | A note has a pitch (MIDI standard, 0–127) and a velocity (MIDI, 0–127). |
| F-7 | A note has a duration expressed in ticks (see F-16) that must be greater than zero and cannot exceed the length of one bar. |
| F-8 | A note can be a rest; a rest occupies duration without emitting a MIDI note-on event. |
| F-9 | The time signature upper numeral is a positive integer indicating how many note values constitute a bar. |
| F-10 | The time signature lower numeral indicates the note value and must be a power of 2 (2, 4, 8, or 16). |
| F-11 | A project can be created via the runtime interface while the engine is running. |
| F-12 | An existing project can be modified via the runtime interface while the engine is running. |
| F-13 | Project updates (create or modify) take effect only at the next bar boundary; the current bar always plays to completion first. The updated project must begin immediately when that bar ends — there must be no timing gap between the last tick of the current bar and the first tick of the updated project. |
| F-14 | The engine holds at most one active project at a time; loading a new project replaces the previous one. |
| F-15 | The engine validates a project on receipt and rejects invalid projects with a structured error. The error format is defined by EP-4. |
| F-16 | Note duration is expressed as an integer number of ticks. The engine defines a fixed PPQN (pulses per quarter note) constant; one tick is the smallest schedulable time unit in the project model. |
| F-17 | A bar may be partially filled; any remaining duration after the last note in a bar is treated as an implicit rest with no MIDI output. |
| F-18 | All tracks in a project play simultaneously; there is no per-track mute or solo facility. |
| F-19 | A project with zero tracks is valid; the engine accepts it and produces no MIDI output while it is active. |
| F-20 | The engine's fixed PPQN constant is 480. The tick length of the note value indicated by the time signature's lower numeral D is `480 × 4 / D` ticks. The tick length of one bar for a time signature N/D is `N × (480 × 4 / D)` ticks. |
| F-21 | The BPM value in the project header must be a whole number (integer) in the range 20–300 inclusive. |
| F-22 | Tracks within a project may have different numbers of bars. The project's total length is the bar count of the track with the most bars. When a track exhausts its bars before the project cycle ends, it restarts from its first bar on the next bar boundary and continues until the project cycle completes. |

---

## Non-Functional Requirements

| ID | Requirement |
|----|-------------|
| NF-1 | Validation must complete synchronously before the project is accepted or rejected; a rejected project must not partially replace the current active project. |
| NF-2 | The project data model must be defined as a first-class type in the engine's domain layer, independent of any serialization format. |
| NF-3 | Project updates must take effect at the bar boundary with no perceptible timing gap. The swap must be atomic from the engine's scheduler perspective: the updated content must begin on the very next tick after the current bar ends. |
| NF-4 | The PPQN resolution must be a fixed, documented integer constant; all tick arithmetic in the project model must use integer values with no floating-point intermediate steps. |
| NF-5 | The active project is held in memory only; it is not persisted to disk and does not survive a daemon restart. |

---

## Acceptance Criteria

| ID | Given | When | Then |
|----|-------|------|------|
| AC-1 | A valid project definition | the project is submitted to the engine | the engine accepts it and holds it as the active project |
| AC-2 | The engine is looping an active project | a runtime modification is submitted | the running bar plays to completion and the change takes effect from the next bar boundary with no timing gap |
| AC-3 | A note with a tick duration exceeding the bar's total tick length | the project containing it is submitted | the engine rejects the project with a validation error |
| AC-4 | A time signature with a lower numeral that is not a power of 2 | the project is submitted | the engine rejects the project with a validation error |
| AC-5 | The engine has an active project | a new project is submitted | the previous project is discarded and the new one becomes active from the next bar boundary |
| AC-6 | A bar containing a rest note | the bar is played | no MIDI note-on event is emitted for the rest's duration |
| AC-7 | A project with a MIDI channel outside 1–16 or an instrument outside 0–127 | the project is submitted | the engine rejects it with a validation error |
| AC-8 | The engine is looping an active project | a runtime modification is submitted and the bar boundary is reached | the first tick of the updated project follows immediately after the last tick of the previous bar with no silent gap |
| AC-9 | A bar containing notes whose tick durations sum to less than the bar's total tick length | the project is submitted | the engine accepts it and treats the remaining ticks as silence |
| AC-10 | A project with an empty track list (zero tracks) | it is submitted | the engine accepts it as the active project and emits no MIDI note-on events |
| AC-11 | A 4/4 time signature (PPQN = 480, bar length = 1920 ticks) | a note with a tick duration of 1920 is submitted | the engine accepts it as valid; a note with a tick duration of 1921 is rejected with a validation error |
| AC-12 | A project header with BPM = 19 or BPM = 301 | the project is submitted | the engine rejects it with a validation error |
| AC-13 | A project with track A (2 bars) and track B (4 bars) | the engine plays one full project cycle | track A plays its 2 bars twice and track B plays its 4 bars once, both ending simultaneously at the project cycle boundary |
| AC-14 | A project header with a non-integer BPM value (e.g., 120.5) | the project is submitted | the engine rejects it with a validation error |

---

## Open Questions

None. All questions have been reconciled.

---

## Refinement Log

### Cycle 1 — Confidence: 50%
- Reconciled: nothing (PRD created from roadmap)
- Added: Q1 (note duration representation), Q2 (bar fill constraint), Q3 (project persistence), Q4 (multi-track playback), Q5 (empty project behaviour)

### Cycle 2 — Confidence: 50%
- Reconciled: briefing "Important requirements" → F-13 updated (no timing gap on project update), NF-3 added (atomic bar-boundary swap), AC-2 updated (no timing gap), AC-8 added (gap-free transition test)
- Added: none — open questions Q1–Q5 remain unanswered

### Cycle 3 — Confidence: 72%
- Reconciled: Q1 → F-16 (tick-based duration), NF-4 (integer PPQN arithmetic); Q2 → F-17 (under-filled bars allowed), AC-9 (under-filled bar accepted); Q3 → NF-5 (memory-only persistence); Q4 → F-18 (all tracks play simultaneously); Q5 → F-19 (zero-track project valid), AC-10 (zero-track emits no MIDI)
- Added: Q6 (PPQN constant value), Q7 (BPM valid range), Q8 (track bar count)

### Cycle 4 — Confidence: 85%
- Reconciled: Q6 → F-20 (PPQN = 480, bar tick formula), AC-11 (tick boundary validation); Q7 → F-21 (BPM range 20–300), AC-12 (out-of-range BPM rejected); Q8 → F-22 (independent track looping, longest determines cycle), UJ-4 (polymetric project journey), AC-13 (2-bar vs 4-bar track looping)
- Added: Q9 (BPM precision — integer vs. fractional)

### Cycle 5 — Confidence: 92%
- Reconciled: Q9 → F-21 updated (BPM must be a whole number integer), AC-14 (non-integer BPM rejected)
- Added: none — confidence 92%, PRD is complete
