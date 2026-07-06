# EP-1 · Tick Position State — PRD

## Overview

Extend the `Engine` entity with a `current_tick` field that tracks the playback position within
the current loop. The counter increments on every internal clock step while the engine is running,
resets to zero at each loop boundary and on `ExternalClockStart` (MIDI 0xFA), freezes on pause,
and resets on stop/restart. The value is stored as an atomic integer so IPC handlers can read it
without acquiring the engine lock or adding any latency to the timing hot path.

**Confidence Level:** 92% — all major behavioral questions are resolved; remaining margin reflects
minor implementation-time decisions (e.g. exact atomic ordering) that are adequately bounded by
NF-1 through NF-3.

---

## User Journeys

### UJ-1 · Client reads current loop position

A UI client has the propeller daemon running with a project loaded. It queries the IPC socket for
the current tick position at a rate suitable for optical feedback (e.g. 20 Hz). Using the returned
`current_tick` and the known `loop_duration`, it computes fractional progress and highlights the
active step. When the user stops playback the counter reads 0; when playback resumes the counter
advances again from loop start.

---

## Functional Requirements

| ID   | Requirement                                                                                                                                                      |
|------|------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| F-1  | The `Engine` entity gains a `current_tick` field of type `Integer`, initialized to 0.                                                                           |
| F-2  | `current_tick` is updated to the current tick position each time an event is processed in the timing loop (NoteOn, NoteOff, or ClockPulse).                     |
| F-3  | `current_tick` resets to 0 at every loop boundary (i.e. when `advance_loop` completes and the next iteration begins).                                           |
| F-4  | `current_tick` resets to 0 on `ExternalClockStart` (MIDI 0xFA / `SyncStart` command).                                                                          |
| F-5  | `current_tick` is not modified while the engine is in the `paused` state; the last value before pause is retained.                                              |
| F-6  | `current_tick` resets to 0 when the engine transitions to `stopped` (whether via stop, clock-stop, or sync-stop).                                               |
| F-7  | The implementation stores `current_tick` as an `Arc<AtomicU64>` shared between the player thread and `LoopEngine`.                                              |
| F-8  | `LoopEngine` exposes a `current_tick() -> u64` method that reads the atomic counter without taking any lock.                                                     |
| F-9  | In clock mode, `current_tick` advances at ClockPulse events (every 20 ticks); in standalone and sync modes it advances only at NoteOn and NoteOff event ticks. |
| F-10 | `current_tick` is not reset on `ExternalClockContinue` (MIDI 0xFB / `SyncContinue`); the counter resumes incrementing from its frozen value.                   |

---

## Non-Functional Requirements

| ID   | Requirement                                                                                                                        |
|------|------------------------------------------------------------------------------------------------------------------------------------|
| NF-1 | Atomic writes to `current_tick` in the timing loop must use relaxed ordering; they must not introduce any synchronization barrier. |
| NF-2 | Reads of `current_tick` from IPC handlers must be lock-free; no engine mutex may be held during the read.                         |
| NF-3 | Adding `current_tick` must not introduce any additional `sleep` or blocking call to the hot timing loop.                           |

---

## Acceptance Criteria

| ID   | Given                                                                  | When                                               | Then                                                               |
|------|------------------------------------------------------------------------|----------------------------------------------------|--------------------------------------------------------------------|
| AC-1 | Engine is running with a project loaded                                | Sufficient time elapses for events to fire         | `current_tick()` returns a value > 0                              |
| AC-2 | Engine completes one loop iteration                                    | The loop boundary is crossed                       | `current_tick()` returns 0                                        |
| AC-3 | Engine is running                                                      | `stop()` is called then `start()` is called        | `current_tick()` resets to 0 and increments again from 0          |
| AC-4 | Engine is running                                                      | `clock_pause()` is called                          | `current_tick()` does not change while the engine is paused        |
| AC-5 | Engine is in sync mode and running                                     | `SyncStart` (0xFA) arrives                         | `current_tick()` resets to 0                                      |
| AC-6 | Engine is stopped                                                      | `current_tick()` is read                           | Returns 0                                                         |
| AC-7 | Engine is paused in sync mode with `current_tick()` returning T (> 0) | `SyncContinue` (0xFB) arrives and playback resumes | `current_tick()` is >= T immediately after resume (not reset to 0) |

---

## Open Questions

None.

---

## Refinement Log

### Cycle 1 — Confidence: 55%

- Reconciled: nothing (PRD created from roadmap, no prior answers)
- Added: Q1 (update granularity), Q2 (SyncContinue reset semantics), Q3 (tick value on stop)

### Cycle 2 — Confidence: 92%

- Reconciled: Q1 → F-9 (note-event-only update in non-clock modes), Q2 → F-10 + AC-7 (SyncContinue does not reset), Q3 → confirms F-6 + AC-6; AC-2 tightened from "0 or very small" to "0"
- Added: none (confidence >= 90%; PRD is complete)
