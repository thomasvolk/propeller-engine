# EP-NP-3 · Loop Engine Refactoring — Technical Specification

## Overview

This epic replaces bar-by-bar event scheduling in `src/loop_engine/player.rs` with
full-loop event scheduling. `PlayerLoop` loses `bar_index` and `last_bar_ticks`; gains
`loop_duration`, `carry_over`, and `loop_elapsed_ticks`. `build_normal_bar()` becomes
`build_loop_events()` operating over `track.notes` with absolute tick addresses.
`advance_bar()` becomes `advance_loop()` which commits pending projects and resets the
elapsed counter. Notes whose `start_tick + duration` exceeds `loop_duration` carry their
`NoteOff` into the next iteration. The private enums `BarEvent` and `BarOutcome` are
renamed to `LoopEvent` and `LoopOutcome`. This epic depends on EP-NP-1 (domain model)
and can be worked in parallel with EP-NP-2 and EP-NP-5.

**Confidence Level:** 95% — all functional requirements and ACs are mapped to tasks; all
open questions and decisions have been reconciled; no ambiguities remain.

---

## Architecture Overview

All changes are confined to `src/loop_engine/player.rs`. No other source files require
modification in this epic (assuming EP-NP-1 is already landed and player.rs compiles
against the new domain types).

The refactoring eliminates the concept of "bar" entirely from the player. Instead of
iterating one bar at a time and advancing `bar_index`, the player builds all events for
the complete loop in one pass, then advances by the full `loop_duration` at the end of
each iteration.

Cross-loop notes (those whose `start_tick + duration` exceeds `loop_duration`) are
handled by a carry-over queue. `advance_loop()` collects such note-offs, converts their
absolute ticks to offsets relative to the next loop start, and stores them in
`PlayerLoop.carry_over`. The next call to `build_loop_events()` prepends these carry-over
events (sorted by offset) before the new loop's events.

`loop_elapsed_ticks` tracks the current tick position within the active loop iteration.
It is set to the event's tick value **after waking from the inter-event sleep but before
calling `emit_event()`** in `play_events()`. This ordering ensures that if a command
interrupts playback between the sleep and the emit, the interrupted-tick event is still
included when `SyncContinue` resumes (AC-8). `loop_elapsed_ticks` is reset to 0 by
`advance_loop()`.

`loop_duration` is a cached field on `PlayerLoop`. It is updated from
`project.header.loop_duration` at the start of each loop when a project is active. When
no project is active (e.g., clock-only mode after `ProjectStore::clear()`), the cached
value is used for clock-pulse emission, preserving the existing no-project clock
behaviour.

The positional `bar_ticks`/`loop_duration` parameter previously threaded through
`play_events()`, `handle_sleep_result()`, `handle_command_in_bar()`, and `do_pause()` is
removed. All four methods access `self.loop_duration` directly, removing a redundant
argument from the call chain.

---

## Components

### `src/loop_engine/player.rs`

The sole file changed. The internal structure after this epic:

**Renamed types**

| Old name     | New name      | Reason   |
|--------------|---------------|----------|
| `BarEvent`   | `LoopEvent`   | F-18     |
| `BarOutcome` | `LoopOutcome` | F-18     |

`BuildResult` retains its name; its `Events` variant carries `Vec<(u64, LoopEvent)>`.

**`PlayerLoop` field changes**

| Change  | Field                        | Notes                         |
|---------|------------------------------|-------------------------------|
| Removed | `bar_index: usize`           | F-1                           |
| Removed | `last_bar_ticks: u64`        | F-2                           |
| Added   | `loop_duration: u64`         | F-2, F-15; cached from header |
| Added   | `carry_over: Vec<LoopEvent>` | F-14                          |
| Added   | `loop_elapsed_ticks: u64`    | F-16                          |

**`PauseContext` field changes**

| Change  | Field                              | Notes |
|---------|------------------------------------|-------|
| Removed | `bar_index: usize`                 | F-9   |
| Renamed | `bar_ticks` → `loop_duration: u64` | F-9   |

**Method changes**

| Old                                 | New                              | Notes                                                  |
|-------------------------------------|----------------------------------|--------------------------------------------------------|
| `build_normal_bar() -> BuildResult` | `build_loop_events() -> BuildResult` | F-3                                                |
| `advance_bar(bar_ticks: u64)`       | `advance_loop()`                 | F-7; no parameter — reads `self.loop_duration`         |
| `play_events(bar_ticks: u64, ...)`  | `play_events(...)`               | D-1; uses `self.loop_duration` directly                |
| `handle_sleep_result(bar_ticks, ...)` | `handle_sleep_result(...)`     | D-1; uses `self.loop_duration` directly                |
| `handle_command_in_bar(bar_ticks, ...)` | `handle_command_in_bar(...)` | D-1; uses `self.loop_duration` directly                |
| `do_pause(bar_ticks, ...)`          | `do_pause(...)`                  | D-1; uses `self.loop_duration` directly                |

**`bar_index` reset removal** — All existing `self.bar_index = 0` assignments in
`do_stop()`, `do_clock_stop()`, `do_sync_stop()`, `do_sync_restart()`,
`handle_stopped()`, `handle_waiting()`, and `handle_paused()` are deleted as part of
removing the field (F-1, F-12, F-13).

---

## Data Model

| Type          | Fields                                                                          | Notes                             |
|---------------|---------------------------------------------------------------------------------|-----------------------------------|
| `LoopEvent`   | `NoteOn { channel: u8, pitch: u8, velocity: u8 }`, `NoteOff { channel: u8, pitch: u8 }`, `ClockPulse` | Renamed from `BarEvent` (F-18) |
| `LoopOutcome` | `Complete`, `Stopped`, `Paused`, `SyncRestart`, `Disconnected`                  | Renamed from `BarOutcome` (F-18)  |
| `PlayerLoop`  | +`loop_duration: u64`, +`carry_over: Vec<LoopEvent>`, +`loop_elapsed_ticks: u64`, −`bar_index`, −`last_bar_ticks` | F-1, F-2, F-14, F-15, F-16 |
| `PauseContext`| `remaining_events: Vec<(u64, LoopEvent)>`, `loop_duration: u64`                 | F-9; `bar_index` removed          |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID   | Task                                                                                                                                                                                                  | Type | PRD ref                    | Depends on      |
|------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|------|----------------------------|-----------------|
| T-1  | Write `test_loop_event_priority`: assert `LoopEvent::NoteOff.priority() == 0`, `LoopEvent::NoteOn.priority() == 1`, `LoopEvent::ClockPulse.priority() == 2`                                          | test | F-18                       | —               |
| T-2  | Rename `BarEvent` → `LoopEvent` and `BarOutcome` → `LoopOutcome` throughout `player.rs`; update all existing references (including any EP-NP-1 unit tests in the file) to use new names              | impl | F-18                       | T-1             |
| T-3  | Write `test_player_loop_fields`: verify `PlayerLoop::new()` initialises `loop_duration`, `carry_over`, and `loop_elapsed_ticks`; confirm absence of `bar_index` and `last_bar_ticks` (compile-time)   | test | F-1, F-2, F-14–F-16        | T-2             |
| T-4  | Update `PlayerLoop` fields (remove `bar_index`, `last_bar_ticks`; add `loop_duration`, `carry_over`, `loop_elapsed_ticks`) and update `PauseContext` fields (remove `bar_index`, rename `bar_ticks`); remove all `self.bar_index = 0` assignments | impl | F-1, F-2, F-9, F-12, F-13, F-14–F-16 | T-3 |
| T-5  | Write `test_build_loop_events_single_note`: project with `loop_duration=1920` and one note `[start_tick=0, duration=480, pitch=60, velocity=80]` produces `NoteOn` at tick 0 and `NoteOff` at tick 480 | test | F-3, AC-1                  | T-4             |
| T-6  | Write `test_build_loop_events_two_notes_same_tick`: two notes `[0, 480, 60, 80]` and `[0, 480, 64, 80]` each produce independent NoteOn/NoteOff events at tick 0/480; no merging                     | test | F-4, AC-2                  | T-4             |
| T-7  | Implement `build_loop_events()` replacing `build_normal_bar()`: iterate `track.notes`, emit `NoteOn` at `note.start_tick` and `NoteOff` at `note.start_tick + note.duration`; sort by `(tick, priority())`; update `self.loop_duration` from project header when project is active | impl | F-3, F-4, F-15             | T-5, T-6        |
| T-8  | Write `test_carry_over_collected`: note `[start_tick=0, duration=1921, pitch=60, velocity=80]` with `loop_duration=1920` results in no `NoteOff` in the main event list; after `advance_loop()`, `carry_over` contains one entry at tick offset 1 | test | F-5, AC-3                  | T-7             |
| T-9  | Write `test_carry_over_prepended`: when `carry_over` is pre-populated with `NoteOff` entries, `build_loop_events()` prepends them (sorted by offset) before new loop events and clears `carry_over`   | test | F-6                        | T-7             |
| T-10 | Implement carry-over: in `build_loop_events()` separate NoteOffs whose absolute tick > `loop_duration` into a local vec; in `advance_loop()` convert those to offsets (`tick - loop_duration`) and store in `self.carry_over`; in `build_loop_events()` prepend sorted `carry_over` entries then clear the field | impl | F-5, F-6, F-14             | T-8, T-9        |
| T-11 | Write `test_advance_loop_resets_elapsed_ticks`: after `advance_loop()`, `loop_elapsed_ticks == 0`                                                                                                    | test | F-16                       | T-10            |
| T-12 | Write `test_advance_loop_commits_pending`: `advance_loop()` calls `ProjectStore::commit_pending()` and applies any pending BPM change at the boundary; running loop is not interrupted                  | test | F-7, F-11, AC-6            | T-10            |
| T-13 | Replace `advance_bar()` with `advance_loop()`: advance anchor by `self.loop_duration` ticks; call `commit_pending()`; apply BPM from `pending_sync_bpm` or project header; reset `loop_elapsed_ticks = 0`; update `self.loop_duration` from project when active (keep cached value otherwise) | impl | F-7, F-11, F-15, F-16      | T-11, T-12      |
| T-14 | Write `test_loop_duration_cached_without_project`: after project is cleared and `loop_duration` was set to 1920, `build_loop_events()` still emits clock pulses spanning 1920 ticks with no active project | test | F-15, AC-4 (partial)       | T-13            |
| T-15 | Update `build_loop_events()` no-project path: emit clock pulses using `self.loop_duration` (cached value) instead of a literal default; no other change to no-project behaviour                       | impl | F-15                       | T-14            |
| T-16 | Write `test_clock_pulses_span_loop_duration`: clock mode with `loop_duration=1920` produces exactly 96 `ClockPulse` events at ticks 0, 20, 40, …, 1900                                              | test | F-10, AC-4                 | T-15            |
| T-17 | Update clock pulse generation in `build_loop_events()` to iterate over `self.loop_duration` (replacing the old `bt` / `bar_ticks` variable)                                                          | impl | F-10                       | T-16            |
| T-18 | Write `test_init_running_from_project_reads_loop_duration`: `init_running_from_project()` sets `self.loop_duration` from `project.header.loop_duration` and initialises scheduler from `project.header.bpm` | test | F-8                        | T-17            |
| T-19 | Update `init_running_from_project()` to read `header.loop_duration` instead of `time_signature.bar_ticks()`                                                                                          | impl | F-8                        | T-18            |
| T-20 | Write `test_sync_continue_resumes_mid_loop`: with `loop_elapsed_ticks = 480` and loop events at ticks 0, 480, 960, 1440, `do_sync_continue()` produces events at 480, 960, 1440 only; tick-0 event is absent | test | F-16, F-17, AC-8           | T-19            |
| T-21 | Track `loop_elapsed_ticks` in `play_events()`: set `self.loop_elapsed_ticks = tick` after waking from inter-event sleep but before calling `self.emit_event(event)`; implement `do_sync_continue()` that calls `build_loop_events()` and retains only events with `tick >= self.loop_elapsed_ticks`; drop the `loop_duration` positional parameter from `play_events()`, `handle_sleep_result()`, `handle_command_in_bar()`, and `do_pause()` — access `self.loop_duration` directly in all four | impl | F-16, F-17, D-1, Q-1       | T-20            |
| T-22 | Write `test_do_stop_clears_carry_over`: populate `carry_over` with a `NoteOff` entry, call `do_stop()`, assert `carry_over` is empty                                                                 | test | F-12, F-19, AC-9           | T-21            |
| T-23 | Write `test_do_sync_stop_clears_carry_over`: populate `carry_over`, call `do_sync_stop()`, assert `carry_over` is empty                                                                              | test | F-19                       | T-21            |
| T-24 | Write `test_do_sync_restart_clears_carry_over`: populate `carry_over`, call `do_sync_restart()`, assert `carry_over` is empty; verify `last_instruments` cleared and anchor reset                     | test | F-13, F-19                 | T-21            |
| T-25 | Update `do_stop()`, `do_sync_stop()`, and `do_sync_restart()` to clear `self.carry_over`                                                                                                             | impl | F-12, F-13, F-19           | T-22, T-23, T-24|
| T-26 | Write `test_pause_resume_mid_loop`: start playing a loop, pause mid-loop, verify `PauseContext` stores remaining events with correct `loop_duration`; resume and verify remaining events play in order without skip or duplicate | test | F-9, AC-5                  | T-25            |
| T-27 | Update `do_pause()`, `handle_running()` resume path, and any remaining `handle_command_in_bar()` / `handle_sleep_result()` call sites to use updated `PauseContext` fields (`loop_duration` instead of `bar_ticks`, no `bar_index`); confirm no callers pass `bar_ticks` as a positional argument | impl | F-9                        | T-26            |

---

## Open Questions

None. All questions have been answered and reconciled.

---

## Open Decisions

None. All decisions have been selected and reconciled.

---

## Revision Log

### Cycle 1 — Confidence: 82%
- Reconciled: none (spec created from PRD and full source analysis of `player.rs`)
- Added: Q-1 (loop_elapsed_ticks update timing), D-1 (play_events parameter vs field)

### Cycle 2 — Confidence: 95%
- Reconciled: D-1 → Architecture Overview and Components updated (drop `loop_duration` positional param from `play_events`, `handle_sleep_result`, `handle_command_in_bar`, `do_pause`; use `self.loop_duration` directly); T-21 and T-27 updated accordingly; Q-1 → Architecture Overview updated (set `loop_elapsed_ticks` after sleep wake-up, before `emit_event()`); T-21 updated to reflect ordering
- Added: nothing (confidence ≥ 90%; no open questions or decisions remain)
