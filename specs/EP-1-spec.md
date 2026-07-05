# EP-1 · Tick Position State — Technical Specification

## Overview

This epic adds a `current_tick` counter to the engine that tracks the current playback position
within the active loop. The counter is an `Arc<AtomicU64>` shared between the `LoopEngine` public
handle and the `PlayerLoop` background thread. The player thread writes it on the hot path; any
other thread (IPC handlers, tests) reads it lock-free via `LoopEngine::current_tick()`. The
Allium spec gains corresponding entity fields and invariants. EP-2 will consume `current_tick()`
to serve `get_position` IPC requests.

**Confidence Level:** 92% — all PRD requirements are mapped to tasks; minor implementation-time
details (exact test timing strategies for AC-2 and AC-7) are constrained but not prescribed.

---

## Architecture Overview

A single `Arc<AtomicU64>` is created in `LoopEngine::new()` with an initial value of 0. One clone
is passed into `run_player_loop` and stored on `PlayerLoop`; the original is kept on `LoopEngine`.

**Write path (player thread only):**

| Trigger | Method | Value written |
|---------|--------|---------------|
| Each event fires in the timing loop | `play_events()` | Tick of the event |
| Loop boundary completes | `advance_loop()` | 0 |
| Stop (standalone mode) | `do_stop()` | 0 |
| Stop (clock mode) | `do_clock_stop()` | 0 |
| Stop (sync mode) | `do_sync_stop()` | 0 |
| SyncStart mid-loop (0xFA) | `do_sync_restart()` | 0 |

**Non-write paths (the freeze and zero-by-default invariants follow from these omissions):**

- `handle_paused()` — no write; counter retains its last running value (F-5)
- `do_sync_continue()` — no write; counter resumes from frozen position (F-10)
- `handle_waiting()` — no write; counter is 0 (already reset by the preceding stop)
- `handle_stopped(SyncStart)` — no write needed; counter is 0 (reset by whichever stop preceded)

**Read path:**

`LoopEngine::current_tick()` loads the atomic with `Ordering::Relaxed`. No lock is held.
`Relaxed` is sufficient because the value is advisory position data — it does not gate any
shared state transition, so no happens-before relationship with other threads is required.

---

## Components

### `LoopEngine` (`src/loop_engine/mod.rs`)

- Add field: `current_tick: Arc<AtomicU64>`
- In `new()`: create `Arc::new(AtomicU64::new(0))`, clone it for `run_player_loop`, store the
  original on `self`
- Add method: `pub fn current_tick(&self) -> u64 { self.current_tick.load(Ordering::Relaxed) }`
- Update the `std::sync` import to include `atomic::{AtomicU64, Ordering}`

The test `dropping_loop_engine_exits_thread` constructs `run_player_loop` directly and must be
updated to pass a fresh `Arc::new(AtomicU64::new(0))`.

### `PlayerLoop` (`src/loop_engine/player.rs`)

- Add field: `current_tick: Arc<AtomicU64>`
- Update `PlayerLoop::new()` to accept `current_tick: Arc<AtomicU64>` and store it
- Update `std::sync` import to include `atomic::{AtomicU64, Ordering}`

Write-point changes (one line each):

```rust
// play_events() — after self.loop_elapsed_ticks = tick;
self.current_tick.store(tick, Ordering::Relaxed);

// advance_loop() — after self.loop_elapsed_ticks = 0;
self.current_tick.store(0, Ordering::Relaxed);

// do_stop(), do_clock_stop(), do_sync_stop() — at the top of each method
self.current_tick.store(0, Ordering::Relaxed);

// do_sync_restart() — at the top of the method
self.current_tick.store(0, Ordering::Relaxed);
```

Add a comment to `handle_paused()` and `do_sync_continue()` each:

```rust
// current_tick is intentionally not written here: the counter freezes during pause
// and resumes from its frozen value on Continue (F-5 / F-10).
```

### `run_player_loop` (`src/loop_engine/player.rs`)

- Add parameter: `current_tick: Arc<AtomicU64>`
- Forward to `PlayerLoop::new()`

### Allium Specification (`specs/propeller.allium`)

Add to the `Engine` entity:

```
current_tick: Integer  -- 0 ≤ current_tick < loop_duration; 0 when stopped or paused
```

Add five new invariants after `CrossLoopNoteCarryOver`:

```
invariant TickAdvances {
    -- While engine.state = running, engine.current_tick equals the tick of the most
    -- recently processed event (NoteOn, NoteOff, or ClockPulse).
}

invariant TickResetsAtLoopBoundary {
    -- engine.current_tick is set to 0 at every loop boundary, before the first event
    -- of the new iteration is processed.
}

invariant TickResetsOnStop {
    -- Any transition to engine.state = stopped sets engine.current_tick to 0.
    -- Covers LoopStop, ClockStop, ExternalClockStop, DaemonStop, and ExternalClockLost.
}

invariant TickResetsOnExternalClockStart {
    -- ExternalClockStart (MIDI 0xFA) sets engine.current_tick to 0 regardless of the
    -- prior state. This also applies to LoopStart / ClockStart which restart from
    -- tick 0 by design.
}

invariant TickFreezesDuringPause {
    -- While engine.state = paused, engine.current_tick is not modified.
    -- ExternalClockContinue (MIDI 0xFB) does not reset engine.current_tick; playback
    -- resumes from the frozen value.
}
```

---

## Data Model

| Type               | Fields / Changes                            | Notes                                                             |
|--------------------|---------------------------------------------|-------------------------------------------------------------------|
| `LoopEngine`       | + `current_tick: Arc<AtomicU64>`            | Public read handle; `current_tick()` loads with `Relaxed`         |
| `PlayerLoop`       | + `current_tick: Arc<AtomicU64>`            | Written at event ticks and reset points; never written in paused  |
| `run_player_loop`  | + param `current_tick: Arc<AtomicU64>`      | Forwarded to `PlayerLoop::new()`                                  |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID   | Task                                                                                                                                                                                          | Type | PRD ref         | Depends on |
|------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|------|-----------------|------------|
| T-1  | Unit test: newly created `LoopEngine` returns `current_tick() == 0`                                                                                                                          | test | F-1, F-8        | —          |
| T-2  | Add `Arc<AtomicU64>` field to `LoopEngine` and `PlayerLoop`; add `current_tick` param to `run_player_loop`; implement `LoopEngine::current_tick()`; update `dropping_loop_engine_exits_thread` test | impl | F-1, F-7, F-8   | T-1        |
| T-3  | Integration test: `current_tick()` > 0 after the engine has been running in standalone mode for at least one full loop with a project loaded                                                  | test | F-2, F-9, AC-1  | T-2        |
| T-4  | In `play_events()`, store the event tick into the atomic immediately before emitting each event (after the sleep, before `emit_event`)                                                         | impl | F-2, F-9        | T-3        |
| T-5  | Integration test: `current_tick()` == 0 after a loop boundary is crossed; strategy — run for 2+ loops at high BPM, collect samples, assert at least one sample is 0                          | test | F-3, AC-2       | T-4        |
| T-6  | In `advance_loop()`, store 0 into the atomic after resetting `loop_elapsed_ticks`                                                                                                             | impl | F-3             | T-5        |
| T-7  | Integration test: `current_tick()` == 0 after `stop()` (wait for `Stopped` state); repeat for clock-stop and sync-stop paths                                                                  | test | F-6, AC-3, AC-6 | T-6        |
| T-8  | Store 0 at the top of `do_stop()`, `do_clock_stop()`, and `do_sync_stop()`                                                                                                                   | impl | F-6             | T-7        |
| T-9  | Integration test: sample `current_tick()` twice with a short sleep while engine is paused; assert both values are equal                                                                        | test | F-5, AC-4       | T-8        |
| T-10 | Confirm `handle_paused()` makes no writes to the atomic; add the intentional-omission comment to `handle_paused()`                                                                            | impl | F-5             | T-9        |
| T-11 | Integration test: let `current_tick()` advance to > 0 in sync mode, call `sync_start()`, wait for engine to re-enter `Running`, assert `current_tick() == 0`                                 | test | F-4, AC-5       | T-10       |
| T-12 | Store 0 at the top of `do_sync_restart()`                                                                                                                                                    | impl | F-4             | T-11       |
| T-13 | Integration test: record T = `current_tick()` while running in sync mode (T > 0), call `sync_continue()`, verify `current_tick()` >= T after the engine continues playing                    | test | F-10, AC-7      | T-12       |
| T-14 | Confirm `do_sync_continue()` makes no writes to the atomic; add the intentional-omission comment to `do_sync_continue()`                                                                     | impl | F-10            | T-13       |
| T-15 | Add `current_tick: Integer` to the `Engine` entity in `specs/propeller.allium`; add invariants `TickAdvances`, `TickResetsAtLoopBoundary`, `TickResetsOnStop`, `TickResetsOnExternalClockStart`, `TickFreezesDuringPause` | impl | roadmap         | T-14       |

---

## Open Questions

None.

---

## Open Decisions

None.

---

## Revision Log

### Cycle 1 — Confidence: 92%

- Reconciled: nothing (spec created fresh from PRD)
- Added: nothing (confidence >= 90%; no open questions or decisions)
