# EP-2 · Play Back Pitch Bend Events in Real Time — PRD

## Overview
While a project with pitch bend events is playing, the system sends a MIDI pitch bend message
on the track's channel at the exact tick each event occurs, with the same timing precision as
note-on and note-off events. Pitch bend events on multiple tracks, and pitch bend events that
land on the same tick as notes, all play back together without any of them being dropped,
delayed, or reordered relative to each other.

**Confidence Level:** 92% — All four roadmap-level scenarios are covered, and every gap raised
during refinement (send order at a shared tick, reset on stop, reset on pause) is now resolved
into a functional requirement, user journey, and acceptance criterion. No open questions
remain; the PRD is complete.

---

## User Journeys

### UJ-1 · Single-track pitch bend playback
A project has one track with pitch-bend events interleaved among its notes. As playback
advances tick by tick, each time the running tick matches a pitch-bend event's tick, the
system sends a pitch bend message on that track's channel carrying the event's value, with
the same timing precision as a note-on at that tick.

### UJ-2 · Pitch bend and notes coincide on the same tick
A track has both a note event and a pitch-bend event scheduled at the same tick. When
playback reaches that tick, the system sends both messages — neither is dropped, delayed to a
later tick, or reordered relative to messages from other tracks at the same tick.

### UJ-3 · Multi-track simultaneous pitch bend
Two tracks on different MIDI channels each have a pitch-bend event at the same tick. When
playback reaches that tick, the system sends a pitch bend message on each track's own channel;
neither track's message interferes with or suppresses the other's.

### UJ-4 · Pitch bend replays across a loop boundary
A project with pitch-bend events loops back to its start after reaching the end of its
duration. On the new pass, the same pitch-bend events fire again at their respective ticks,
consistent with how notes are replayed on every loop pass.

### UJ-5 · Stopping or pausing resets pitch bend to center
A project with pitch-bend events is stopped or paused mid-playback while a channel is bent
away from center. The system sends a pitch bend message resetting that channel to 8192,
leaving the device in the same neutral state it already leaves for notes on stop/pause.

---

## Functional Requirements
| ID | Requirement |
|-----|-------------|
| F-1 | When active playback reaches a tick containing one or more pitch-bend events, the system sends a MIDI pitch bend message on each event's track channel, carrying the event's value. |
| F-2 | Pitch-bend messages share the same per-tick dispatch as note-on and note-off messages: every message scheduled for a tick is sent before playback proceeds to the next scheduled tick. |
| F-3 | Pitch-bend events on different tracks/channels that share the same tick are each sent on their own channel independently; one track's pitch-bend message never suppresses or delays another track's message at the same tick. |
| F-4 | On each loop pass, pitch-bend events fire again at their original ticks, using the same replay mechanism as notes. |
| F-5 | When a pitch-bend event and a note-off and/or note-on event share the same tick on the same track, the system sends them in the order: note-off, then pitch-bend, then note-on. |
| F-6 | When playback stops (Stop, ClockStop, or SyncStop), the system sends a pitch bend message resetting every channel that has pitch-bend events to center (8192), the same way it already flushes note-off for any still-sounding note on stop. |
| F-7 | Pausing playback resets every channel that has pitch-bend events to center (8192) as well, the same as a full stop, since pause already flushes note-off for sounding notes via the same code path. |

---

## Non-Functional Requirements
| ID | Requirement |
|-----|-------------|
| NF-1 | Sending a pitch-bend message follows the same bounded-latency dispatch path as existing note-on/note-off/clock-pulse messages and introduces no measurable delay to the timing of the next scheduled clock tick or note event. |
| NF-2 | Pitch-bend playback reuses the existing MIDI output mechanism and per-tick event scheduling used for notes and clock pulses; it does not introduce a separate playback path or timing source. |

---

## Acceptance Criteria
| ID | Given | When | Then |
|------|-------|------|------|
| AC-1 | An active project reaches the tick of a pitch bend event | playback processes that tick | the system sends a pitch bend message on that track's MIDI channel carrying the event's value |
| AC-2 | A pitch bend event and one or more note events fall on the same tick | playback processes that tick | the system sends all of them without dropping or delaying any |
| AC-3 | A project loops back to its start | playback continues into the new pass | pitch bend events are sent again on each pass, consistent with how notes are replayed across loop boundaries |
| AC-4 | Pitch bend events are being sent during playback | subsequent clock ticks and note events are due | doing so introduces no observable delay to their timing |
| AC-5 | A pitch-bend event and a note-on event fall on the same tick | playback processes that tick | the pitch-bend message is sent after any note-off message and before the note-on message at that tick |
| AC-6 | A project with pitch-bend events is stopped (Stop, ClockStop, or SyncStop) | the stop is processed | every channel that has pitch-bend events receives a pitch bend message resetting it to center (8192) |
| AC-7 | A project with pitch-bend events is paused | the pause is processed | every channel that has pitch-bend events receives a pitch bend message resetting it to center (8192) |

---

## Open Questions

None. The PRD is complete.

---

## Refinement Log

### Cycle 1 — Confidence: 65%
- Created initial PRD from the EP-2 roadmap entry, cross-checked against the existing
  playback engine's tick-scheduling and priority-ordering model (`src/loop_engine/player.rs`)
  and MIDI output trait (`src/loop_engine/midi.rs`).
- Added: Q1 (send order between pitch-bend and note-on/note-off at a shared tick), Q2
  (whether to reset pitch bend to center on stop).

### Cycle 2 — Confidence: 85%
- Reconciled: Q1 → F-5 & AC-5 (note-off, pitch-bend, note-on send order), Q2 → F-6 & AC-6
  (reset every pitch-bend channel to center on Stop/ClockStop/SyncStop)
- Added: Q3 (whether the same reset applies on pause, not just full stop)

### Cycle 3 — Confidence: 92%
- Reconciled: Q3 → F-7, AC-7, UJ-5 (pause also resets every pitch-bend channel to center)
- No open questions remain. PRD is complete.
