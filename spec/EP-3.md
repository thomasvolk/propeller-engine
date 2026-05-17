# EP-3 · Loop Engine — PRD

## Overview

The engine plays back the active project in a continuous loop. Notes are emitted on their designated MIDI channel and instrument at the tick positions defined in the project. The loop repeats seamlessly from end back to start, and project updates take effect at bar boundaries without any timing gap. Timing precision is the highest-priority concern; drift and gaps are not acceptable.

**Confidence Level:** 92% — All roadmap requirements are covered and all open questions are reconciled. The PRD is complete.

---

## User Journeys

### UJ-1 · Continuous playback of an active project

A performer loads a project. The loop engine begins playing all tracks simultaneously, sending MIDI note-on (and note-off) events at the tick positions defined in each bar. The loop runs without any further action from the performer until explicitly stopped.

### UJ-2 · Seamless loop repeat

The project reaches its last bar. When the final tick of the last bar passes, the engine immediately resumes from the first tick of the first bar with no audible gap or timing glitch.

### UJ-3 · Live BPM adjustment

While the loop is running a performer changes the BPM value in the project header. The engine adjusts its tick-scheduling rate immediately so subsequent notes arrive at the new tempo without stopping or resetting the loop.

### UJ-4 · Seamless project update mid-loop

The performer sends a project modification while the loop is running. The engine finishes the current bar, then — at the exact tick boundary — begins playing the updated content. There is no silent gap between the old bar's last tick and the new content's first tick.

### UJ-5 · Explicit start after project load

A performer loads a project. No sound is produced yet. The performer then issues an explicit start command. The loop begins playing from the first tick of the first bar.

### UJ-6 · Start command issued before project load

A performer issues a start command before any project is loaded. The engine enters a started/waiting state silently. The performer then loads a project. Playback begins immediately from the first tick without requiring a second start command.

---

## Functional Requirements

| ID | Requirement |
|----|-------------|
| F-1 | The engine continuously repeats the active project from start to finish in an endless loop. |
| F-2 | On each loop iteration the engine plays all tracks simultaneously, emitting MIDI events on the track's designated MIDI channel and instrument at the tick positions defined in the project. |
| F-3 | Tempo (BPM) and time signature are read from the project header and govern the conversion of ticks to wall-clock time. |
| F-4 | A change to BPM in the project header takes effect on the running loop. |
| F-5 | When the project reaches the end of its last bar, playback resumes immediately from the first tick of the first bar; there must be no timing gap at the loop boundary. |
| F-6 | When the active project is updated (per EP-2 F-13), the updated content begins at the next bar boundary with no timing gap between the last tick of the current bar and the first tick of the updated project. |
| F-7 | For each non-rest note the engine sends a MIDI note-on event at the note's start tick and a MIDI note-off event after the note's tick duration has elapsed. |
| F-8 | Rest notes produce no MIDI output; the engine advances the schedule by the rest's tick duration without emitting any MIDI events. |
| F-9 | The loop engine maintains an explicit running/stopped state. Playback begins only when an explicit start command is issued; loading or updating a project does not automatically begin playback. |
| F-10 | An explicit stop command halts the loop engine; when stopped the engine emits no MIDI output. |
| F-11 | A BPM change takes effect immediately: the scheduler recalculates the next scheduled tick deadline using the new BPM as soon as the change is received, without waiting for a bar or note boundary. |
| F-12 | The engine sends a MIDI Program Change message on each track's MIDI channel immediately before the first note of that track at loop start, and again whenever the track's instrument value changes. |
| F-13 | When no active project is present and the engine is in a started state, the engine idles silently with no MIDI output. |
| F-14 | When the stop command is issued, the engine sends a MIDI note-off message for every currently-sounding note before halting, to prevent stuck notes on the MIDI device. |
| F-15 | When a start command is issued and no active project is present, the engine enters a started/waiting state. Playback begins automatically from the first tick of the first bar as soon as a project is subsequently loaded. |

---

## Non-Functional Requirements

| ID | Requirement |
|----|-------------|
| NF-1 | Timing precision is the highest-priority requirement; all other concerns are secondary. |
| NF-2 | The engine must not accumulate timing drift over time; the schedule must remain anchored to a monotonic clock reference. |
| NF-3 | Timing jitter (deviation of an actual MIDI event from its ideal scheduled tick time) must not exceed 5 ms. |
| NF-4 | The scheduler must use a monotonic high-resolution clock to maintain the < 5 ms jitter target. |
| NF-5 | While idling with no active project, the engine must consume minimal CPU consistent with the idle target defined in EP-1 NF-2. |

---

## Acceptance Criteria

| ID | Given | When | Then |
|----|-------|------|------|
| AC-1 | An active project with at least one note | the loop engine is running | MIDI note-on events are emitted for each non-rest note at its scheduled tick position |
| AC-2 | An active project | the loop reaches the end of the project's last bar | playback resumes from the first tick of the first bar with no audible gap |
| AC-3 | The loop is running | BPM is changed in the project header | subsequent notes are scheduled at the new tempo without stopping the loop |
| AC-4 | The loop is running and a project update is submitted | the current bar plays to completion | the updated project begins on the very next tick after the bar boundary with no silent gap |
| AC-5 | A rest note in a bar | the bar is played | no MIDI note-on or note-off event is emitted for the rest's duration |
| AC-6 | A non-rest note | the bar is played | a MIDI note-off event is emitted after the note's tick duration has elapsed |
| AC-7 | An active project is loaded and the loop is in a stopped state | an explicit start command is issued | playback begins from the first tick of the first bar |
| AC-8 | The loop is running | a stop command is issued | MIDI output ceases |
| AC-9 | The loop is running | MIDI event timing is measured over a sustained period | no event deviates from its ideal scheduled tick time by more than 5 ms |
| AC-10 | The loop is running at a given BPM | the BPM is changed via the project header | the interval to the next scheduled tick is recalculated using the new BPM immediately, without a bar or note boundary wait |
| AC-11 | A track with instrument X on MIDI channel N | the loop starts | a MIDI Program Change message for instrument X is sent on channel N before any note-on event from that track |
| AC-12 | No active project is present and the loop is started | the engine is running | no MIDI events are emitted |
| AC-13 | The loop is running with one or more notes currently sounding | the stop command is issued | a MIDI note-off event is emitted for each active note before the engine halts |
| AC-14 | The engine is in a started/waiting state with no active project | a project is loaded | playback begins automatically from the first tick of the first bar without requiring a second start command |

---

## Open Questions

No open questions. All questions have been reconciled.

---

## Refinement Log

### Cycle 1 — Confidence: 55%
- Reconciled: nothing (PRD created from roadmap)
- Added: Q-1 (loop start behavior), Q-2 (quantitative timing precision), Q-3 (BPM change timing), Q-4 (MIDI program change), Q-5 (no-project behavior)

### Cycle 2 — Confidence: 78%
- Reconciled: Q-1 → F-9/F-10 (explicit start/stop state), UJ-5, AC-7/AC-8; Q-2 → NF-3 updated (< 5 ms jitter), NF-4 (monotonic clock), AC-9; Q-3 → F-11 (immediate BPM effect), AC-10; Q-4 → F-12 (MIDI Program Change at start and on instrument change), AC-11; Q-5 → F-13 (idle silently when no project), NF-5, AC-12
- Added: Q-6 (stuck note handling on stop), Q-7 (start with no active project), Q-8 (project loaded in started state)

### Cycle 3 — Confidence: 92%
- Reconciled: Q-6 → F-14 (note-off all active notes before halt), AC-13; Q-7 → F-15 (start with no project enters waiting state), UJ-6, AC-14; Q-8 → covered by F-15 and AC-14 (same implication as Q-7-A)
- Added: none — confidence 92%, PRD is complete
