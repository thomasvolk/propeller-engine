# EP-1 · Correct-Speed Resume — PRD

## Overview
When sync mode playback is paused and then resumed, the loop must resume at its correct
speed. Today, resuming after a pause leaves the loop running noticeably slower than before
the pause. When this epic is done, an operator pausing and resuming sync mode observes the
loop continuing at the same speed it had before the pause, with no lingering slowdown.

**Confidence Level:** 92% — scope, tolerance, root-cause responsibility, and timing are now
pinned down; the only remaining softness is exactly what form the root-cause finding takes
once identified, which is a delivery detail rather than a requirements gap.

---

## User Journeys

### UJ-1 · Operator pauses and resumes sync-mode playback
An operator is running propeller-engine in sync mode, slaved to an external MIDI clock. The
loop is actively playing at the external clock's tempo. The operator (or the external device)
pauses playback, then resumes it a short time later. The operator expects the loop to
continue at the same tempo it had before the pause, with no audible or measurable slowdown.

### UJ-2 · Operator pauses and resumes repeatedly across a session
An operator pauses and resumes sync-mode playback multiple times over the course of a
session (e.g. between song sections). Each resume must return the loop to correct speed;
slowdown must not accumulate across repeated pause/resume cycles.

---

## Functional Requirements

| ID  | Requirement |
|-----|-------------|
| F-1 | When sync-mode playback is paused, the engine loop stops advancing without altering its tracked tempo. |
| F-2 | When sync-mode playback is resumed after a pause, the engine loop continues advancing at the same speed it had immediately before the pause. |
| F-3 | Repeated pause/resume cycles within a session do not cause cumulative slowdown of the loop. |
| F-4 | The pause/resume behaviour covered by this epic is sync-mode pause/resume triggered by the external clock's Stop/Continue messages; standalone clock-pause/clock-resume commands issued outside of sync mode are out of scope. |
| F-5 | This epic includes investigating the root cause of the post-resume slowdown; the fix is not considered complete until the underlying mechanism causing the slowdown has been identified and addressed. |

---

## Non-Functional Requirements

| ID   | Requirement |
|------|-------------|
| NF-1 | The loop's playback speed after resume must not be measurably slower than its speed before pause. |
| NF-2 | The tempo after resume must exactly match the tempo before pause — zero tolerance for measurable drift. |
| NF-3 | Correct tempo must be reflected starting at the very first tick/pulse emitted after resume; no settling window is permitted. |

---

## Acceptance Criteria

| ID   | Given | When | Then |
|------|-------|------|------|
| AC-1 | A running sync-mode loop | it is paused and then resumed | the loop's playback speed immediately after resume matches its speed immediately before the pause |
| AC-2 | A sync-mode loop that has completed a pause/resume cycle | its speed is measured over subsequent repeats | no perceptible or measurable slowdown persists |
| AC-3 | A sync-mode loop that has been paused and resumed multiple times in one session | its speed is measured after each resume | speed matches the pre-pause speed every time, with no cumulative drift across cycles |

---

## Open Questions

None — all open questions from the previous cycle have been answered and reconciled. Confidence is above the 90% threshold.

---

## Refinement Log

### Cycle 1 — Confidence: 55%
- Created PRD from specs/roadmap.md EP-1 section.
- Reconciled: none (new PRD).
- Added: Q1 (pause/resume scope), Q2 (speed-match tolerance), Q3 (root-cause investigation scope), Q4 (timing window for correct speed after resume).

### Cycle 2 — Confidence: 92%
- Reconciled: Q1 → F-4 (sync-mode-only scope), Q2 → NF-2 (exact tolerance, zero drift), Q3 → F-5 (root-cause investigation in scope), Q4 → NF-3 (immediate, no settling window).
- Added: none — confidence at 92%, above the 90% threshold. PRD is complete.
