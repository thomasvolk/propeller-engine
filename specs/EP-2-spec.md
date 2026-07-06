# EP-2 · Position Query Protocol — Technical Specification

## Overview

This epic adds a `get_position` / `position` request–response pair to the IPC protocol. The
daemon exposes two lock-free read methods on `LoopEngine`: `current_tick()` (defined in EP-1) and
the new `loop_duration_ticks()`. A new `IpcMessage` enum (distinct from the existing `Command`
enum) handles the `{"type":"..."}` wire format. The dispatch function is updated to check for a
`"type"` field before the existing `"command"` field check, so the two protocols coexist without
breaking existing clients.

**Confidence Level:** 92% — all PRD requirements map to tasks; minor residual on the exact
placement of the `loop_duration_ticks` write within `advance_loop()` (before or after the BPM
check) is an implementation-time detail constrained but not prescribed here.

---

## Architecture Overview

**`loop_duration_ticks` atomic (engine side):**

A second `Arc<AtomicU64>` is created in `LoopEngine::new()` with an initial value of 0. One clone
is passed to `run_player_loop` and stored on `PlayerLoop`; the original is kept on `LoopEngine`.
The value encodes the active project's `loop_duration` in ticks, or 0 when no project is loaded.

Write path (player thread only):

| Trigger                                    | Location          | Value written                                       |
|--------------------------------------------|-------------------|-----------------------------------------------------|
| Every loop boundary (including first loop) | `advance_loop()`  | `store.active().map(p.header.loop_duration).unwrap_or(0)` cast to `u64` |

The write always occurs unconditionally after `commit_pending()`, so it reflects the current
`store.active` whether or not a new project was committed this boundary. Stop methods (`do_stop`,
`do_clock_stop`, `do_sync_stop`) do **not** write to `loop_duration_ticks`; stopping the engine
does not change `store.active`, so the value persists correctly across pause/stop while a project
remains loaded.

Read path: `LoopEngine::loop_duration_ticks()` loads with `Ordering::Relaxed`. The same
justification as `current_tick()` applies: advisory position data, no happens-before requirement.

**IPC protocol extension:**

The existing `Command` enum uses `#[serde(tag = "command")]` and handles all existing commands.
A new `IpcMessage` enum uses `#[serde(tag = "type")]` and handles only the new `GetPosition` /
`Position` variants. The `dispatch()` function checks for `raw.get("type")` first; if present it
parses as `IpcMessage` and routes to `handle_get_position()`. Otherwise the existing `"command"`
path runs unchanged.

`handle_get_position()` reads `engine.current_tick()` and `engine.loop_duration_ticks()` in two
independent `Relaxed` loads. No mutex is acquired. The `loop_duration` field is `None` when the
atomic value is 0, `Some(value)` otherwise. The response is serialised as a `serde_json::Value`
and returned to the caller just like any existing response.

---

## Components

### `LoopEngine` (`src/loop_engine/mod.rs`)

- Add field: `loop_duration_ticks: Arc<AtomicU64>`
- In `new()`: create `Arc::new(AtomicU64::new(0))`, clone for `run_player_loop`, store original
- Add method: `pub fn loop_duration_ticks(&self) -> u64 { self.loop_duration_ticks.load(Ordering::Relaxed) }`
- Update import: `atomic::{AtomicU64, Ordering}` (may already be present after EP-1)

The existing `dropping_loop_engine_exits_thread` test calls `run_player_loop` directly and must be
updated to pass a fresh `Arc::new(AtomicU64::new(0))` for the new parameter.

### `PlayerLoop` (`src/loop_engine/player.rs`)

- Add field: `loop_duration_ticks: Arc<AtomicU64>`
- Update `PlayerLoop::new()` to accept and store `loop_duration_ticks: Arc<AtomicU64>`
- Update `run_player_loop()` to accept and forward `loop_duration_ticks: Arc<AtomicU64>`

Write point in `advance_loop()`:

```rust
// After commit_pending() and the BPM-update block:
let loop_dur = self.store.read().unwrap()
    .active()
    .map(|p| p.header.loop_duration as u64)
    .unwrap_or(0);
self.loop_duration_ticks.store(loop_dur, Ordering::Relaxed);
```

### `ipc/types.rs`

Add a new enum alongside `Command`:

```rust
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcMessage {
    GetPosition,
    Position {
        tick: u64,
        loop_duration: Option<u64>,
    },
}
```

`GetPosition` is the inbound request; `Position` is the outbound response. Both live in one enum
because the `#[serde(tag = "type")]` roundtrip handles both directions and no variant confusion is
possible (the handler never sends `GetPosition`; the client never sends `Position`).

### `ipc/handler.rs`

Add `handle_get_position()`:

```rust
fn handle_get_position(engine: &Arc<LoopEngine>) -> Value {
    let tick = engine.current_tick();
    let raw_dur = engine.loop_duration_ticks();
    let loop_duration = if raw_dur == 0 { None } else { Some(raw_dur) };
    json!({"type": "position", "tick": tick, "loop_duration": loop_duration})
}
```

Update `dispatch()`: before the `raw.get("command").is_none()` check, add:

```rust
if raw.get("type").is_some() {
    let msg: Result<IpcMessage, _> = serde_json::from_str(line);
    return match msg {
        Ok(IpcMessage::GetPosition) => handle_get_position(engine),
        _ => error_response("unknown_type", "unrecognised type value"),
    };
}
```

---

## Data Model

| Type                      | Fields / Changes                              | Notes                                                  |
|---------------------------|-----------------------------------------------|--------------------------------------------------------|
| `LoopEngine`              | + `loop_duration_ticks: Arc<AtomicU64>`       | Public read handle; loads with `Relaxed`               |
| `PlayerLoop`              | + `loop_duration_ticks: Arc<AtomicU64>`       | Written in `advance_loop()` after every loop boundary  |
| `run_player_loop`         | + param `loop_duration_ticks: Arc<AtomicU64>` | Forwarded to `PlayerLoop::new()`                       |
| `IpcMessage::GetPosition` | (no fields)                                   | Deserialises from `{"type":"get_position"}`            |
| `IpcMessage::Position`    | `tick: u64`, `loop_duration: Option<u64>`     | Serialises as `{"type":"position","tick":N,...}`       |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID   | Task                                                                                                                                                                             | Type | PRD ref        | Depends on |
|------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|------|----------------|------------|
| T-1  | Unit test: newly created `LoopEngine` returns `loop_duration_ticks() == 0`                                                                                                      | test | F-9            | —          |
| T-2  | Add `loop_duration_ticks: Arc<AtomicU64>` to `LoopEngine`, `PlayerLoop`, and `run_player_loop`; implement `LoopEngine::loop_duration_ticks()`; update `dropping_loop_engine_exits_thread` | impl | F-9            | T-1        |
| T-3  | Integration test: `loop_duration_ticks()` equals the project's `loop_duration` after the engine has played at least one full loop with a project loaded                         | test | F-4, F-9, AC-1 | T-2        |
| T-4  | In `advance_loop()`, unconditionally write `store.active().map(loop_duration).unwrap_or(0)` to `loop_duration_ticks` after `commit_pending()`                                   | impl | F-4, F-9       | T-3        |
| T-5  | Integration test: `loop_duration_ticks()` returns 0 while no project is loaded (engine starts, waits, no project committed)                                                     | test | F-5, AC-3      | T-4        |
| T-6  | Unit test: `IpcMessage::GetPosition` deserialises from `{"type":"get_position"}`                                                                                                | test | F-1, F-7       | —          |
| T-7  | Add `IpcMessage` enum to `ipc/types.rs` with `GetPosition` and `Position { tick: u64, loop_duration: Option<u64> }` variants using `#[serde(tag = "type")]`                    | impl | F-1, F-2, F-7  | T-6        |
| T-8  | Unit test: `IpcMessage::Position { tick: 42, loop_duration: Some(4800) }` serialises to `{"type":"position","tick":42,"loop_duration":4800}`                                    | test | F-2, F-7       | T-7        |
| T-9  | Unit test: `IpcMessage::Position { tick: 0, loop_duration: None }` serialises to `{"type":"position","tick":0,"loop_duration":null}`                                            | test | F-2, F-5, F-7  | T-7        |
| T-10 | Integration test: `connection_handler` receiving `{"type":"get_position"}` when no project is loaded returns `{"type":"position","tick":0,"loop_duration":null}` (AC-3)         | test | F-3, F-5, F-8  | T-7, T-4   |
| T-11 | Add `handle_get_position()` to `handler.rs`; update `dispatch()` to detect `"type"` field and route before the `"command"` check                                               | impl | F-3, F-6, F-8  | T-10       |
| T-12 | Integration test: `get_position` while engine is playing with a project loaded returns `tick ≥ 0` and `loop_duration` matching the project's `header.loop_duration` (AC-1, AC-5) | test | F-3, F-4, AC-1 | T-11       |
| T-13 | Integration test: two sequential `get_position` responses while playing have monotonically non-decreasing `tick` values (AC-4)                                                  | test | F-3, AC-4      | T-11       |
| T-14 | Integration test: while engine is paused via `clock_pause()`, two sequential `get_position` responses have equal `tick` values and non-null `loop_duration` (AC-6)             | test | F-3, F-4, AC-6 | T-11       |

---

## Open Questions

None.

---

## Open Decisions

None.

---

## Revision Log

### Cycle 1 — Confidence: 92%
- Reconciled: nothing (spec created fresh from PRD; codebase inspected to confirm `Command`/`IpcMessage` coexistence design and `advance_loop()` write point)
- Added: nothing — confidence ≥ 90%
