# EP-7 · Mode Management — Technical Specification

## Overview

EP-7 adds the runtime mode management layer to the engine: orchestrating transitions between `standalone`, `clock`, and `sync` modes. The engine starts in `standalone` mode by default. The `set mode <name>` IPC command is extended to perform mode-specific transition actions — sending a MIDI stop signal before leaving `clock` mode, pausing the loop when entering `sync` mode, and leaving the loop uninterrupted for `standalone`↔`clock` switches. All new logic lives in `src/ipc/handler.rs`. No new top-level modules or data types are introduced; this epic builds on `EngineMode`, `EngineSettings`, and the `LoopEngine` methods defined by EP-3, EP-4, EP-5, and EP-6.

**Confidence Level:** 92% — All decisions reconciled; every F-x and AC-x has a covering task and TDD order is maintained. No open questions remain; spec is implementation-ready.

---

## Architecture Overview

Mode management is handled entirely in the IPC dispatch layer. The authoritative mode value is `settings.mode` (type `EngineMode`, held in `Arc<Mutex<EngineSettings>>`), written by `handle_set_mode` in `src/ipc/handler.rs`.

**Transition decision logic in `handle_set_mode`:**

1. Parse `mode_str` via `EngineMode::from_str`; return structured error if unrecognised (already implemented; F-15, AC-12).
2. Apply EP-6 sync-requires-port guard: if `new_mode == Sync && sync_clock_state.is_none()` → `error_response("sync_requires_port", …)` (EP-6 T-42).
3. If `current_mode == Clock && new_mode != Clock` and engine state ∈ `{Running, Paused}`: call `engine.clock_stop()`. This sends MIDI Stop (0xFC) and stops the player loop (F-12, AC-9). For `clock→sync` transitions this simultaneously satisfies F-14/AC-11.
4. If `new_mode == Sync && current_mode != Sync` and engine state ∈ `{Running, Paused}`: call `engine.stop()`. This pauses the loop immediately (F-14, AC-11). Because clock→sync is handled by step 3, this branch fires only for non-clock → sync.
5. Update `settings.mode = new_mode`.
6. Return `ok_response()`.

For `standalone↔clock` transitions both guards in steps 3 and 4 are false, so no loop action is taken and playback continues uninterrupted (F-6, F-11, AC-3).

**Idempotent transitions:**

The conditions `new_mode != Clock` and `current_mode != Sync` prevent stop methods from being called on same-mode re-sends. A `set mode sync` while already in sync mode (loop playing via EP-6) does not stop the loop.

**BPM guard (F-4, F-5):**

`handle_set_bpm` checks `settings.mode == Sync` and returns `error_response("sync_mode_active", …)` before any state mutation. This guard is introduced by EP-6 (T-24); EP-7 depends on it and verifies it via integration tests.

**Cross-epic dependencies:**

| Requirement | Owner | EP-7 role |
|-------------|-------|-----------|
| F-7: clock-start requires project | EP-5 T-11/T-12 | Inherits; no new code |
| F-8: sync playback requires project + clock | EP-6 T-13–T-19 | Inherits; no new code |
| AC-6: MIDI stop on daemon shutdown in clock mode | EP-5 T-29/T-30 | Inherits; no new code |
| F-4/AC-7: set-bpm rejected in sync mode | EP-6 T-23/T-24 | Depends on; integration test in T-19 |
| sync-requires-port guard | EP-6 T-41/T-42 | Depends on; already wired into handle_set_mode |

---

## Components

### `handle_set_mode` (extended) — `src/ipc/handler.rs`

Extended to accept `engine: &Arc<LoopEngine>` alongside the existing `settings` and `sync_clock_state` parameters. Implements the five-step transition logic described above.

**Signature after EP-7:**

```rust
fn handle_set_mode(
    mode_str: &str,
    settings: &Arc<Mutex<EngineSettings>>,
    engine: &Arc<LoopEngine>,
    sync_clock_state: Option<&Arc<Mutex<SyncClockState>>>,
) -> Value
```

**Transition action matrix:**

| Current mode | Target mode | Engine state | Actions taken |
|---|---|---|---|
| any | same | any | update mode (no-op), return ok |
| Clock | non-Clock | Running / Paused | `engine.clock_stop()`, then update mode |
| Clock | non-Clock | Stopped / Waiting | update mode only |
| non-Clock | Sync | Running / Paused | `engine.stop()`, then update mode |
| non-Clock | Sync | Stopped / Waiting | update mode only |
| non-Sync | non-Sync | any | update mode only (seamless) |
| Sync | non-Sync | any | update mode only (loop already Stopped via EP-6) |

### `dispatch()` — `src/ipc/handler.rs`

Updated to pass `engine` to the `handle_set_mode` call. This is the only change to `dispatch()`.

---

## Data Model

No new types are introduced by EP-7. Relevant existing types:

| Type | Fields | Notes |
|------|--------|-------|
| `EngineMode` | `Standalone`, `Clock`, `Sync` | Defined in `src/ipc/types.rs` (EP-4); `from_str` and `as_str` already implemented |
| `EngineSettings` | `mode: EngineMode`, `bpm: u32` | `new()` initialises `mode = Standalone` (EP-4); shared via `Arc<Mutex<EngineSettings>>` |
| `EngineState` | `Stopped`, `Waiting`, `Running`, `Paused` | `Paused` added by EP-5; read via `engine.state()` |
| `LoopEngine` | methods | EP-7 calls `state()`, `stop()` (EP-3), `clock_stop()` (EP-5) |
| `SyncClockState` | `Waiting`, `Tracking`, `Lost` | EP-6; threaded through dispatch to `handle_set_mode` |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID | Task | Type | PRD ref | Depends on |
|----|------|------|---------|------------|
| T-1 | Write test: `EngineSettings::new()` has `mode = Standalone` | test | F-1, F-10, AC-1 | — |
| T-2 | Impl: (documents) `EngineSettings::new()` default — already implemented; T-1 verifies | impl | F-1, F-10, AC-1 | T-1 |
| T-3 | Write test: `Status` IPC response includes `"mode"` field matching the current `EngineMode` as a string | test | F-2, AC-2 | — |
| T-4 | Impl: (documents) status handler `"mode"` field — already implemented; T-3 verifies | impl | F-2, AC-2 | T-3 |
| T-5 | Write test: `set-mode unknown_name` → `{"status":"error","code":"invalid_mode",…}`, mode unchanged | test | F-15, AC-12 | — |
| T-6 | Impl: (documents) `handle_set_mode` invalid-mode guard — already implemented; T-5 verifies | impl | F-15, AC-12 | T-5 |
| T-7 | Write test: `set-mode clock` (from standalone) → `{"status":"ok"}`, `settings.mode = Clock` | test | F-3, F-13, AC-10 | — |
| T-8 | Impl: extend `handle_set_mode` signature with `engine: &Arc<LoopEngine>`; update `dispatch()` call site; basic mode update logic unchanged | impl | F-3 | T-6, T-7 |
| T-9 | Write test: `set-mode clock` while in standalone with loop Running → engine state remains Running after the mode change | test | F-6, F-11, AC-3 | — |
| T-10 | Impl: standalone→clock path: no loop action; update mode only | impl | F-6, F-11 | T-8, T-9 |
| T-11 | Write test: `set-mode standalone` while in clock mode, engine Running → `engine.clock_stop()` is called | test | F-12, AC-9 | — |
| T-12 | Impl: in `handle_set_mode`, if `current == Clock && new != Clock` and state ∈ `{Running, Paused}` → `engine.clock_stop()` before updating mode | impl | F-12 | T-10, T-11 |
| T-13 | Write test: `set-mode sync` while in standalone, loop Running → `engine.stop()` called; mode = Sync | test | F-14, AC-11 | — |
| T-14 | Impl: in `handle_set_mode`, if `new == Sync && current != Sync` and state ∈ `{Running, Paused}` and current is not Clock → `engine.stop()` before updating mode | impl | F-14 | T-12, T-13 |
| T-15 | Write test: `set-mode sync` while in clock mode, engine Running → `engine.clock_stop()` called (not `engine.stop()`); mode = Sync | test | F-12, F-14, AC-9, AC-11 | — |
| T-16 | Impl: (documents) clock→sync path — `engine.clock_stop()` from T-12 guard stops both the MIDI clock and the player loop, satisfying F-12 and F-14; no additional stop call needed | impl | F-12, F-14 | T-12, T-15 |
| T-17 | Write test: `set-mode sync` while already in sync mode with loop Running → no engine methods called; mode remains Sync | test | F-9, AC-10 | — |
| T-18 | Impl: (documents) idempotent sync→sync path — `current != Sync` condition in T-14 guard prevents spurious `engine.stop()`; T-17 verifies | impl | F-9 | T-14, T-17 |
| T-19 | Write test: `set-bpm` while mode = Sync → `{"status":"error","code":"sync_mode_active",…}` | test | F-4, AC-7 | — |
| T-20 | Impl: `handle_set_bpm` guard (EP-6 T-24); EP-7 locks in the behaviour via T-19 | impl | F-4 | T-19 |
| T-21 | Write test: `set-mode standalone` from sync, then `set-bpm` → success; BPM updated | test | F-5, AC-5 | — |
| T-22 | Impl: (documents) BPM re-enable — guard checks current mode at call time; no new code beyond T-20 | impl | F-5 | T-20, T-21 |

---

## Open Decisions

No open decisions.

---

## Revision Log

### Cycle 1 — Confidence: 75%
- Reconciled: none (initial generation from PRD)
- Added: D-1 (engine parameter passing to handle_set_mode), D-2 (state check before stop methods)

### Cycle 2 — Confidence: 92%
- Reconciled: D-1 → architecture and T-8 already reflect `engine: &Arc<LoopEngine>` parameter; decision removed; D-2 → architecture and task matrix already apply state-check guard before stop calls; decision removed
- Added: none — spec is implementation-ready
