# EP-2 · Project Model — PRD

## Overview

A project is the central data structure that defines what MIDI signals the engine will send. It consists of a header (tempo and time signature) and a list of tracks, each containing bars of notes. Projects can be created and modified at runtime via the engine's runtime interface; updates always take effect on a bar boundary so the current bar plays to completion. The engine holds exactly one active project at a time.

**Confidence Level:** 50% — Core structure is clear from the roadmap, but note duration representation, project persistence, bar fill rules, and validation error contracts are all undefined.

---

## User Journeys

### UJ-1 · Creating a new project

A musician uses the runtime interface to define a new project: they provide a BPM, a time signature, and one or more tracks. Each track has a name, a MIDI channel, a MIDI instrument, and a sequence of bars filled with notes and rests. Once submitted, the engine begins using the project as its active project.

### UJ-2 · Modifying a running project at runtime

While the engine is looping through an active project, a musician sends a runtime command to add a track, change a note, or update the BPM. The running bar plays to completion; the change takes effect from the next bar boundary onward.

### UJ-3 · Replacing the active project

A musician loads a completely different project while the engine is running. The engine discards the previous project and adopts the new one, again waiting for the current bar to complete before switching.

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
| F-7 | A note has a duration that must be greater than zero and cannot exceed the length of one bar. |
| F-8 | A note can be a rest; a rest occupies duration without emitting a MIDI note-on event. |
| F-9 | The time signature upper numeral is a positive integer indicating how many note values constitute a bar. |
| F-10 | The time signature lower numeral indicates the note value and must be a power of 2 (2, 4, 8, or 16). |
| F-11 | A project can be created via the runtime interface while the engine is running. |
| F-12 | An existing project can be modified via the runtime interface while the engine is running. |
| F-13 | Project updates (create or modify) take effect only at the next bar boundary; the current bar always plays to completion first. The updated project must begin immediately when that bar ends — there must be no timing gap between the last tick of the current bar and the first tick of the updated project. |
| F-14 | The engine holds at most one active project at a time; loading a new project replaces the previous one. |
| F-15 | The engine validates a project on receipt and rejects invalid projects with a structured error. |

---

## Non-Functional Requirements

| ID | Requirement |
|----|-------------|
| NF-1 | Validation must complete synchronously before the project is accepted or rejected; a rejected project must not partially replace the current active project. |
| NF-2 | The project data model must be defined as a first-class type in the engine's domain layer, independent of any serialization format. |
| NF-3 | Project updates must take effect at the bar boundary with no perceptible timing gap. The swap must be atomic from the engine's scheduler perspective: the updated content must begin on the very next tick after the current bar ends. |

---

## Acceptance Criteria

| ID | Given | When | Then |
|----|-------|------|------|
| AC-1 | A valid project definition | the project is submitted to the engine | the engine accepts it and holds it as the active project |
| AC-2 | The engine is looping an active project | a runtime modification is submitted | the running bar plays to completion and the change takes effect from the next bar boundary with no timing gap |
| AC-3 | A note with a duration exceeding the bar length | the project containing it is submitted | the engine rejects the project with a validation error |
| AC-4 | A time signature with a lower numeral that is not a power of 2 | the project is submitted | the engine rejects the project with a validation error |
| AC-5 | The engine has an active project | a new project is submitted | the previous project is discarded and the new one becomes active from the next bar boundary |
| AC-6 | A bar containing a rest note | the bar is played | no MIDI note-on event is emitted for the rest's duration |
| AC-7 | A project with a MIDI channel outside 1–16 or an instrument outside 0–127 | the project is submitted | the engine rejects it with a validation error |
| AC-8 | The engine is looping an active project | a runtime modification is submitted and the bar boundary is reached | the first tick of the updated project follows immediately after the last tick of the previous bar with no silent gap |

---

## Open Questions

### Q1 · Note duration representation

How should note durations be expressed in the data model?

**Options**
- A. Rational fraction of a bar (e.g., `1/4` means one quarter of the bar) — portable but requires rational arithmetic
- B. Number of ticks where one beat = N ticks (e.g., PPQN-style) — standard in MIDI, familiar to musicians *(recommended — aligns with MIDI standards and simplifies conversion to MIDI events)*
- C. Enumerated note value (whole, half, quarter, eighth, sixteenth) — simple but limits expressiveness

**Answer:**

---

### Q2 · Bar fill constraint

Must the notes in a bar sum exactly to the bar's total duration, or may a bar be partially filled (with the remainder treated as implicit silence)?

**Options**
- A. Notes must sum to exactly the bar length; validation rejects under-filled bars — strict, predictable
- B. Under-filled bars are allowed; the remainder is treated as an implicit rest *(recommended — more ergonomic for live editing, matches common DAW behaviour)*
- C. Under-filled bars are allowed but only at the end of a bar (no gaps between notes)

**Answer:**

---

### Q3 · Project persistence

Is the active project persisted to disk (survives daemon restart), or is it held in memory only?

**Options**
- A. Memory-only; the client must re-submit the project after a restart — simpler, no file I/O in this epic
- B. Persisted to a file automatically whenever a project is set or modified *(recommended — prevents data loss on crash and matches user expectation for a "live environment")*
- C. Persisted on explicit client request only (a separate "save" command)

**Answer:**

---

### Q4 · Multiple tracks playback relationship

Do all tracks in a project play simultaneously, or can individual tracks be muted/soloed?

**Options**
- A. All tracks always play simultaneously; no per-track mute/solo in this epic *(recommended — keeps EP-2 focused; mute/solo can be a later epic)*
- B. Each track has an enabled flag; disabled tracks are silent

**Answer:**

---

### Q5 · Empty project behaviour

What should the engine do when a project with zero tracks is submitted?

**Options**
- A. Accept it as valid; the engine loops silently with no MIDI output *(recommended — clean separation between "no project" and "empty project"; both are valid states)*
- B. Reject it with a validation error; a project must have at least one track

**Answer:**

---

## Refinement Log

### Cycle 1 — Confidence: 50%
- Reconciled: nothing (PRD created from roadmap)
- Added: Q1 (note duration representation), Q2 (bar fill constraint), Q3 (project persistence), Q4 (multi-track playback), Q5 (empty project behaviour)

### Cycle 2 — Confidence: 50%
- Reconciled: briefing "Important requirements" → F-13 updated (no timing gap on project update), NF-3 added (atomic bar-boundary swap), AC-2 updated (no timing gap), AC-8 added (gap-free transition test)
- Added: none — open questions Q1–Q5 remain unanswered
