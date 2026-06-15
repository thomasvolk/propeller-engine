# EP-NP-3 · Loop Engine Refactoring — PRD

## Overview

Replace bar-by-bar iteration in `src/loop_engine/player.rs` with full-loop event
scheduling. The `PlayerLoop` struct loses its `bar_index` and `last_bar_ticks`
fields; `build_normal_bar()` and `advance_bar()` are replaced by
`build_loop_events()` and `advance_loop()` which operate over the full
`loop_duration` read directly from the active project. Notes are scheduled by
absolute `start_tick` within the loop; notes whose `start_tick + duration`
exceeds `loop_duration` carry their `NoteOff` into the next loop iteration. This
epic depends on EP-NP-1 (domain model) and can be worked in parallel with EP-NP-2
and EP-NP-5.

**Confidence Level:** 92% — all roadmap requirements are covered and ACs are
concrete; residual uncertainty is minor implementation-ordering detail only.

---

## User Journeys

### UJ-1 · Engine plays a full-loop project

The operator loads a project that uses the new flat note list. When the engine
starts, `build_loop_events()` iterates `track.notes`, emits a `NoteOn` at each
`note.start_tick` and a `NoteOff` at `note.start_tick + note.duration`, sorts
all events by `(tick, priority)`, and hands the list to `play_events()`. At the
end of the loop, `advance_loop()` advances the anchor by `loop_duration` ticks
and commits any pending project, and the cycle repeats indefinitely.

### UJ-2 · Cross-loop note carry-over

The operator writes a note whose `start_tick + duration` exceeds `loop_duration`.
The `NoteOn` fires at `note.start_tick` in the current loop. The `NoteOff` is
placed in `PlayerLoop.carry_over` by `advance_loop()` at the tick offset
`(start_tick + duration) - loop_duration`. At the start of the next loop,
`build_loop_events()` prepends those carry-over events before new loop events.

### UJ-3 · Pause and resume mid-loop

The operator pauses clock mode while the loop is partway through. The player
flushes active notes, stores the remaining unplayed events (with tick offsets
relative to loop start) in `PauseContext`, and enters the `Paused` state. On
resume, those events play from where they were interrupted; no events are lost
or duplicated.

### UJ-4 · BPM change or project modification at loop boundary

While the engine is running, the operator submits a modified project (new BPM or
new note layout). The change is committed by `advance_loop()` at the next loop
boundary via `ProjectStore::commit_pending()`, ensuring the running loop is never
interrupted mid-flight.

### UJ-5 · SyncContinue resumes mid-loop

The engine is running and has advanced `loop_elapsed_ticks` ticks into the
current loop when a `SyncContinue` message arrives. Rather than restarting from
tick 0, `do_sync_continue()` uses `loop_elapsed_ticks` to resume playback at the
correct position within the current loop's `remaining_events`.

---

## Functional Requirements

| ID   | Requirement                                                                                                                                                                                                                                               |
| ---- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| F-1  | `PlayerLoop.bar_index: usize` is removed. The loop has no concept of sub-bar position.                                                                                                                                                                   |
| F-2  | `PlayerLoop.last_bar_ticks: u64` is replaced by `PlayerLoop.loop_duration: u64`, whose value is read from `project.header.loop_duration` at the start of each loop.                                                                                      |
| F-3  | `build_normal_bar()` is replaced by `build_loop_events()`. The new method iterates `track.notes` directly, emitting `NoteOn` at `note.start_tick` and `NoteOff` at `note.start_tick + note.duration`.                                                    |
| F-4  | Notes sharing the same `start_tick` on the same channel are each emitted as independent events; they are not merged or de-duplicated.                                                                                                                     |
| F-5  | If `note.start_tick + note.duration > loop_duration`, the `NoteOff` event is carried over to the next loop iteration at tick offset `(note.start_tick + note.duration) - loop_duration`.                                                                 |
| F-6  | Carried-over `NoteOff` events are emitted at the start of each new loop iteration, before any `NoteOn` or other events from the new loop. They are sorted by tick offset among themselves before prepending.                                              |
| F-7  | `advance_bar()` is replaced by `advance_loop()`, which advances the anchor by `loop_duration` ticks and calls `ProjectStore::commit_pending()`.                                                                                                           |
| F-8  | `init_running_from_project()` reads `header.loop_duration` instead of `time_signature.bar_ticks()`.                                                                                                                                                      |
| F-9  | `PauseContext.bar_index` is removed. `PauseContext.bar_ticks` is replaced by a loop-duration value. `remaining_events` holds tick offsets relative to loop start.                                                                                        |
| F-10 | Clock pulses are inserted every 20 ticks across the full `loop_duration` in clock mode, unchanged in principle from the bar-based implementation.                                                                                                         |
| F-11 | BPM changes (both via `modify-project` and via `SyncBpmUpdate`) are applied by `advance_loop()` at the loop boundary, not mid-loop.                                                                                                                       |
| F-12 | `do_stop()` and `do_sync_stop()` no longer reset `bar_index`; they continue to flush active notes and update engine state.                                                                                                                                |
| F-13 | `do_sync_restart()` no longer resets `bar_index`; it flushes notes, clears `last_instruments`, and resets the anchor.                                                                                                                                    |
| F-14 | `PlayerLoop` gains a `carry_over: Vec<LoopEvent>` field. `advance_loop()` populates it by collecting all note-offs from the just-completed loop whose absolute tick exceeds `loop_duration`, converting each to a tick offset. `build_loop_events()` prepends these (sorted by tick offset) before new loop events and then clears the field. |
| F-15 | `PlayerLoop.loop_duration` is a cached field. It is updated from `project.header.loop_duration` at the start of each loop. When no project is active (e.g., after `ProjectStore::clear()`), the last-cached value is used for clock-pulse emission, preserving existing clock-without-project behaviour. |
| F-16 | `PlayerLoop` gains a `loop_elapsed_ticks: u64` field that tracks the current position within the active loop iteration. It resets to 0 in `advance_loop()` at the start of each new loop.                                                                |
| F-17 | `do_sync_continue()` uses `loop_elapsed_ticks` to filter `remaining_events`, retaining only events whose tick offset is ≥ `loop_elapsed_ticks`, so playback resumes from the correct mid-loop position.                                                  |
| F-18 | Private enum types `BarEvent` and `BarOutcome` are renamed to `LoopEvent` and `LoopOutcome` throughout `src/loop_engine/`.                                                                                                                                |
| F-19 | `do_stop()`, `do_sync_stop()`, and `do_sync_restart()` all clear the `carry_over` field after flushing active notes, so that stale `NoteOff` events from a previous playback session cannot fire on the next start.                                      |

---

## Non-Functional Requirements

| ID   | Requirement                                                                                                                   |
| ---- | ----------------------------------------------------------------------------------------------------------------------------- |
| NF-1 | The refactoring must not introduce any compiler warnings (`cargo build` and `cargo test` must produce zero warnings).        |
| NF-2 | All existing tests are updated or replaced to use the new domain types; no test may be silently deleted without a replacement. |
| NF-3 | `cargo test` must pass after this epic lands, independently of other parallel epics.                                         |
| NF-4 | Clock timing accuracy is unchanged: clock pulses continue to be emitted every 20 ticks with the same polling resolution.     |

---

## Acceptance Criteria

| ID   | Given                                                                                                              | When                                                        | Then                                                                                                                              |
| ---- | ------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| AC-1 | A project with `loop_duration = 1920` and a note `[0, 480, 60, 80]`                                               | The engine runs one iteration                               | `NoteOn` is emitted at tick 0 and `NoteOff` at tick 480; no bar subdivisions exist                                               |
| AC-2 | A project with two notes `[0, 480, 60, 80]` and `[0, 480, 64, 80]`                                                | The engine runs one iteration                               | Both `NoteOn` events are emitted at tick 0 and both `NoteOff` events at tick 480                                                  |
| AC-3 | A project with `loop_duration = 1920` and a note `[0, 1921, 60, 80]`                                              | The engine completes loop 1 and starts loop 2               | `NoteOn` is emitted at tick 0 of loop 1; `NoteOff` is emitted at tick 1 of loop 2 (carry-over offset = 1921 − 1920 = 1)          |
| AC-4 | Clock mode is active with `loop_duration = 1920`                                                                   | The engine runs one iteration                               | Clock pulses appear at ticks 0, 20, 40, …, 1900 (96 pulses total)                                                                |
| AC-5 | The engine is paused mid-loop with unplayed events remaining                                                       | The engine resumes via `clock_resume()`                     | The remaining events play from where they were interrupted; no events are skipped or doubled                                      |
| AC-6 | A project modification is submitted while the engine is running                                                    | The current loop completes                                  | The new project (or BPM) is applied at the loop boundary; the running loop is not disrupted                                       |
| AC-7 | `cargo test` is run after this epic lands                                                                          | With no other new-protocol epics applied                    | All tests pass; zero compiler warnings                                                                                            |
| AC-8 | The engine is mid-loop with `loop_elapsed_ticks = 480` and has pending events at ticks 480, 960, and 1440         | A `SyncContinue` message is received                        | Events at ticks 480, 960, and 1440 play in order; no events from ticks 0–479 are replayed                                        |
| AC-9 | The engine has a carry-over `NoteOff` pending and `do_stop()` is called                                           | A subsequent `do_start()` / `do_sync_start()` is called     | No ghost `NoteOff` events from the previous session are emitted; `carry_over` is empty at the start of the new playback session   |

---

## Open Questions

None. All questions have been answered and reconciled.

---

## Refinement Log

### Cycle 1 — Confidence: 55%

- Reconciled: none (PRD created from roadmap; no prior answered questions)
- Added: Q1 (carry-over NoteOff data structure), Q2 (fallback loop_duration in clock mode without project), Q3 (SyncContinue semantics after bar_index removal), Q4 (rename BarEvent/BarOutcome)

### Cycle 2 — Confidence: 78%

- Reconciled: Q1 → F-14 (carry_over field on PlayerLoop; advance_loop populates, build_loop_events prepends); Q2 → F-15 (loop_duration cached for clock mode without project); Q3 → F-16 (loop_elapsed_ticks field), F-17 (SyncContinue uses loop_elapsed_ticks), AC-8 (SyncContinue resumes at correct mid-loop position), UJ-5; Q4 → F-18 (rename BarEvent → LoopEvent, BarOutcome → LoopOutcome)
- Added: Q5 (carry_over cleanup on stop), Q6 (AC-3 carry-over example mathematical inconsistency)

### Cycle 3 — Confidence: 78%

- Reconciled: none (Q5 and Q6 had no answers yet)
- Added: none (existing open questions already covered the remaining gaps)

### Cycle 4 — Confidence: 92%

- Reconciled: Q5 → F-19 (do_stop/do_sync_stop/do_sync_restart clear carry_over), AC-9 (no ghost NoteOff after stop+start); Q6 → AC-3 updated to note [0, 1921, 60, 80] with NoteOff at tick 1 of loop 2
- Added: none (confidence ≥ 90%; PRD is complete)
