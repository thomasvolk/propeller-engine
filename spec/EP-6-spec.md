# EP-6 · Clock Sync Mode — Technical Specification

## Overview

This epic adds the ability for the engine to follow an external MIDI clock. A new `MidiClockReceiver` module opens a configured MIDI input port at daemon startup, parses incoming MIDI Timing Clock pulses (0xF8) and transport messages (0xFA/0xFB/0xFC), derives BPM from inter-pulse intervals, and drives the `LoopEngine` via four new `LoopCommand` variants. A `PulseTracker` struct handles BPM computation and clock-loss detection in pure Rust with no hardware dependency, enabling thorough unit testing. Clock loss is detected using a `recv_timeout` pattern: the receiver thread blocks with a BPM-proportional timeout and treats an expired wait as a lost clock signal. Sync mode is activated by the `--sync` flag at daemon startup; the input port name is read from the `PROPELLER_SYNC_PORT` environment variable. In sync mode the IPC `set-bpm` command is rejected with a structured error response, and the status query exposes a `sync_clock_state` field.

**Confidence Level:** 90% — All open questions and decisions are resolved and reconciled. The spec covers every PRD requirement, maintains TDD ordering throughout, and defines all component interactions without ambiguity. T-41/T-42 close the set-mode sync guard gap identified in Cycle 2.

---

## Architecture Overview

EP-6 introduces a new `src/midi_clock/` module that sits alongside the existing `src/loop_engine/` and `src/ipc/` modules. It has no async code and runs on a dedicated OS thread, consistent with the EP-3 threading model. Port enumeration (`propeller midi ports`) is implemented as part of EP-9.

**Component interaction:**

```
MIDI hardware → midir callback (midir-internal thread)
  │  mpsc::Sender<ClockMessage>
  ▼
MidiClockReceiver thread  (dedicated OS thread)
  ├── PulseTracker        — BPM derivation + clock-loss timeout
  ├── SyncTransportState  — tracks Start/Continue/Stop receipt
  ├── Arc<Mutex<SyncClockState>>  — observable by IPC status handler
  └── LoopEngine methods  — sync_start(), sync_continue(), sync_stop(), sync_bpm_update(bpm)
        │  mpsc::Sender<LoopCommand>
        ▼
      LoopEngine player thread  (existing EP-3 thread, extended)
```

**MidiClockReceiver state machine:**

The receiver thread blocks on `channel.recv_timeout(pulse_tracker.timeout_duration())`. Expiry without a message means the clock is lost. On receipt of a message the appropriate action fires immediately.

| Message | Guard | Action |
|---------|-------|--------|
| `Pulse` | — | `pulse_tracker.update(now)`; derive BPM; if BPM changed send `sync_bpm_update`; if was Lost → state = Tracking |
| `Start` | — | reset `pulse_tracker`; `received_transport = true`; state = Tracking; send `sync_start()` to engine |
| `Continue` | clock active (recent pulses) | `received_transport = true`; send `sync_continue()` to engine |
| `Continue` | clock not active | ignore |
| `Stop` | — | `received_transport = false`; send `sync_stop()` to engine; state = Waiting |
| timeout | !already_lost | `received_transport = false`; state = Lost; send `sync_stop()` to engine |
| timeout | already_lost | no-op (stay Lost) |

Clock resume after loss: the next `Pulse` message transitions state to Tracking and re-enables BPM derivation, but does NOT send `sync_start()` — a new MIDI Start or Continue from the external device is required to resume playback (F-13, AC-12, AC-15).

**LoopCommand extension:**

Four new variants are added to the existing `LoopCommand` enum in `src/loop_engine/mod.rs`:

| Variant | Semantics |
|---------|-----------|
| `SyncStart` | Reset `bar_index` to 0; if project present → Running; else → Waiting |
| `SyncContinue` | If project present → Running from current `bar_index`; else → Waiting |
| `SyncStop` | Flush active notes; transition to Stopped |
| `SyncBpmUpdate(u32)` | Apply new BPM to Scheduler at next bar boundary (D-2) |

The `LoopEngine` facade gains four corresponding public methods: `sync_start()`, `sync_continue()`, `sync_stop()`, `sync_bpm_update(bpm: u32)`.

**IPC guard:**

The `SetBpm` handler in `src/ipc/handler.rs` gains an early-return guard: if `settings.mode == EngineMode::Sync`, return `error_response("sync_mode_active", "…")` before updating any state (F-14, AC-5, AC-13).

The `SetMode` handler gains a corresponding guard: if the requested mode is `Sync` and the `sync_clock_state: Option<Arc<Mutex<SyncClockState>>>` passed through `dispatch` is `None`, return `error_response("sync_requires_port", "sync mode requires --sync at startup")` and leave the mode unchanged. This preserves the invariant that `settings.mode == Sync` always implies an active `MidiClockReceiver`.

**Status response extension:**

When mode is `"sync"`, the `Status` handler appends a `"sync_clock_state"` field to its response payload: one of `"waiting"`, `"tracking"`, or `"lost"`. When mode is not `"sync"` the field is omitted (F-5, AC-2).

**Module layout:**

- `src/midi_clock/mod.rs` — `MidiClockReceiver` struct; `ClockMessage` enum; `SyncClockState` enum
- `src/midi_clock/tracker.rs` — `PulseTracker`: BPM derivation, timeout computation, clock-loss predicate
- `src/main.rs` — extended with `--sync` boolean flag; port name read from `PROPELLER_SYNC_PORT` env var at startup

---

## Components

### MidiClockReceiver (`src/midi_clock/mod.rs`)

Created at daemon startup when `--sync` is passed and `PROPELLER_SYNC_PORT` is set. If `--sync` is given but `PROPELLER_SYNC_PORT` is absent, `src/main.rs` exits with an error before creating a receiver. Before spawning the OS thread, `src/main.rs` sets `settings.mode = EngineMode::Sync` on the shared `Arc<Mutex<EngineSettings>>`; this ensures IPC guards are active from the first connection. Holds:
- `mpsc::Receiver<ClockMessage>` — messages from the MIDI input bridge
- `PulseTracker` — BPM derivation and timeout
- `Arc<LoopEngine>` — to send sync commands
- `Arc<Mutex<SyncClockState>>` — written by this thread; read by IPC status handler

Spawns a dedicated OS thread running the state machine described above. Exposes `sync_clock_state() -> SyncClockState` for the IPC status handler.

The MIDI input bridge (hardware glue) is abstracted behind `Box<dyn MidiClockSource>` so tests can inject a `MockMidiClockSource` that sends directly to the `mpsc::Sender<ClockMessage>`.

```rust
pub trait MidiClockSource: Send + 'static {
    fn open(port_name: &str, sender: mpsc::Sender<ClockMessage>) -> Result<Self, String>
    where Self: Sized;
}
```

### PulseTracker (`src/midi_clock/tracker.rs`)

Pure struct — no hardware or thread dependencies. Stores a rolling window of the last 24 pulse timestamps (one quarter note at standard 24-PPQN MIDI clock). Exposes:

- `PulseTracker::new() -> PulseTracker`
- `update(&mut self, now: Instant)` — record a new pulse timestamp
- `bpm(&self) -> Option<u32>` — average BPM from the window; `None` if fewer than 2 pulses seen
- `timeout_duration(&self) -> Option<Duration>` — `3.5 × last_interval`; `None` if no interval yet
- `is_clock_active(&self, now: Instant) -> bool` — true if last pulse was within `timeout_duration()`
- `reset(&mut self)` — clears all state

### Extended LoopEngine (`src/loop_engine/mod.rs`)

New public methods (each send the corresponding `LoopCommand` variant via the existing channel):

- `sync_start(&self)`
- `sync_continue(&self)`
- `sync_stop(&self)`
- `sync_bpm_update(&self, bpm: u32)`

### Extended Player Loop (`src/loop_engine/player.rs`)

Handles the four new `LoopCommand` variants:

- `SyncStart`: same as `Start` but always resets `bar_index = 0`; transitions: if project → Running, else → Waiting; if already Running, resets bar and restarts from bar 0
- `SyncContinue`: if project → Running from current `bar_index`; else → Waiting
- `SyncStop`: same behaviour as `Stop` (flush active notes, transition to Stopped)
- `SyncBpmUpdate(bpm)`: enqueued; applied at the next bar boundary using `scheduler.update_bpm()` and anchor reset (identical to the existing BPM-change-at-bar-boundary logic)

The `Waiting` state already polls for a project and transitions to `Running` when one appears. `SyncStart` and `SyncContinue` both enter `Waiting` when no project is present, so the existing `Waiting` → `Running` promotion covers UJ-3 (project arrives after Start) without additional code.

---

## Data Model

| Type | Fields | Notes |
|------|--------|-------|
| `ClockMessage` | `Pulse`, `Start`, `Continue`, `Stop` | Sent from MIDI bridge callback to `MidiClockReceiver` thread via `mpsc` |
| `SyncClockState` | `Waiting`, `Tracking`, `Lost` | `Arc<Mutex<SyncClockState>>`; written by `MidiClockReceiver`; read by IPC `Status` handler |
| `PulseTracker` | `history: VecDeque<Instant>` (max 25 entries) | Pure struct; no hardware dependency; window of 24 intervals = one beat at 24-PPQN |
| `LoopCommand` (extended) | + `SyncStart`, `SyncContinue`, `SyncStop`, `SyncBpmUpdate(u32)` | New variants; existing `Start` / `Stop` remain for standalone-mode IPC |
| `MidiClockReceiver` | `state: Arc<Mutex<SyncClockState>>`, sender to `LoopEngine`, `PulseTracker`, `received_transport: bool` | Owns MIDI input bridge; drives LoopEngine |

---

## Test Strategy

### Overview of challenges

Three orthogonal problems make EP-6 non-trivial to test:

1. **Real MIDI hardware** — `MidiClockReceiver` normally opens a physical port via `midir`. Mitigated by the `Box<dyn MidiClockSource>` abstraction: tests inject a `MockMidiClockSource` that writes `ClockMessage` directly to the `mpsc::Sender<ClockMessage>`, bypassing all hardware.
2. **Timeout-based clock-loss detection** — `recv_timeout` uses real wall-clock time, so tests that exercise clock loss must either sleep or shrink the timeout. Mitigated by priming `PulseTracker` with high-frequency fake pulses to produce a very short `timeout_duration()`.
3. **Cross-thread synchronisation** — `MidiClockReceiver` runs on its own OS thread; tests must wait for its effects to propagate to `LoopEngine` before asserting. Mitigated by polling `sync_clock_state()` (mirroring the `wait_for_socket` pattern in existing integration tests) rather than using fixed sleeps.

### Layer 1 — `PulseTracker` unit tests (T-1/T-3/T-5/T-7)

`PulseTracker` is a pure struct with no hardware or thread dependencies. Both `update(now: Instant)` and `is_clock_active(now: Instant)` accept an `Instant` parameter, so tests control "time" by computing offsets from a fixed baseline:

```rust
let base = Instant::now();
tracker.update(base);
tracker.update(base + Duration::from_millis(20_833)); // 120 BPM pulse
// ...
assert_eq!(tracker.bpm(), Some(120));
assert!(!tracker.is_clock_active(base + Duration::from_millis(250_000))); // silence
```

No `thread::sleep`. All assertions are deterministic and run in microseconds.

### Layer 2 — Player loop sync commands (T-12 to T-21)

Use the existing pattern: spin `run_player_loop` in a thread with a real `mpsc` channel, `MockMidiOutput`, and a shared `Arc<Mutex<EngineState>>`. Send the new `SyncStart` / `SyncContinue` / `SyncStop` / `SyncBpmUpdate(bpm)` variants and assert on `EngineState` and `MockMidiOutput::events`. No timing sensitivity.

### Layer 3 — IPC handler guards (T-23, T-37-T-39, T-41, T-42)

Call handler functions directly in-process with crafted `EngineSettings` (e.g. `mode = EngineMode::Sync`) and an `Option<Arc<Mutex<SyncClockState>>>`. Assert on the returned JSON. Fast, no I/O, no threads.

### Layer 4 — `MidiClockReceiver` state machine (T-25 to T-36)

Use a real `LoopEngine` (with `MockMidiOutput`) and a `MockMidiClockSource` that exposes the `mpsc::Sender<ClockMessage>` directly to the test.

For **timeout tests** (T-33/T-34), prime the `PulseTracker` with very high-frequency pulses before the silence period so `timeout_duration()` is tiny:

- 30,000 BPM → 2 ms pulse interval → `3.5 × 2 ms` = 7 ms timeout
- Feed 25 pulses at 2 ms spacing (≈ 50 ms real time), then withhold pulses for 15 ms
- Receiver thread detects clock loss after ≈ 7 ms of silence

Thread effects are observed by polling `sync_clock_state()` in a tight loop with a generous deadline (e.g. 3× the expected timeout), rather than a fixed `thread::sleep`.

### Layer 5 — Integration tests

**Testable without hardware:**

- `--sync` present but `PROPELLER_SYNC_PORT` not set → daemon exits with error before binding the socket (covers the startup guard in T-40)

**Requires virtual or physical MIDI loopback (marked `#[ignore]` in CI):**

- Full sync playback flow (AC-1, AC-9, AC-10): start daemon with `--sync`, provide a virtual MIDI loopback port (macOS `IAC Driver`, Linux `ttymidi`), send `0xFA` + `0xF8` pulses, assert loop plays

### Wall-clock cost summary

| Group | Approach | Wall-clock cost |
|---|---|---|
| `PulseTracker` (T-1/T-3/T-5/T-7) | Injected `Instant` offsets, no sleep | ~0 ms |
| Player sync commands (T-12..T-21) | Existing thread + channel + mock pattern | ~0 ms |
| IPC guards (T-23, T-37-T-42) | Direct handler calls | ~0 ms |
| Receiver normal messages (T-25..T-32) | Mock source + polling | ~50 ms total |
| Clock loss timeout (T-33/T-34) | High-BPM priming (7 ms timeout) | ~100 ms |
| Integration startup guard | Real binary, no MIDI | ~200 ms |
| Integration sync flow | Virtual MIDI loopback | `#[ignore]` |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID | Task | Type | PRD ref | Depends on |
|----|------|------|---------|------------|
| T-1 | Write test: `PulseTracker::bpm()` returns `None` with fewer than 2 pulses; returns 120 (±1) after 25 evenly-spaced pulses at 120 BPM (interval 20.833 ms) | test | F-2, NF-3 | — |
| T-2 | Impl: `PulseTracker` in `src/midi_clock/tracker.rs` with `new()`, `update(Instant)`, `bpm() -> Option<u32>` using a rolling window of 24 intervals | impl | F-2 | T-1 |
| T-3 | Write test: `PulseTracker::timeout_duration()` at 120 BPM returns ~72.9 ms (3.5 × 20.833 ms); returns `None` when no interval recorded | test | F-10, NF-3 | — |
| T-4 | Impl: `PulseTracker::timeout_duration() -> Option<Duration>` computed as `3.5 × last_interval` | impl | F-10, NF-3 | T-2, T-3 |
| T-5 | Write test: `is_clock_active(now)` returns true just after a pulse, false after 4 intervals of silence at 120 BPM | test | F-10, AC-2, AC-10 | — |
| T-6 | Impl: `PulseTracker::is_clock_active(now: Instant) -> bool` using `timeout_duration()` | impl | F-10 | T-4, T-5 |
| T-7 | Write test: `PulseTracker::reset()` clears all state — `bpm()` → None, `timeout_duration()` → None, `is_clock_active()` → false | test | F-12, AC-14 | — |
| T-8 | Impl: `PulseTracker::reset(&mut self)` clears `history` | impl | F-12 | T-2, T-7 |
| T-12 | Write test: player loop receives `SyncStart` with active project in store → `state()` = Running; `bar_index` resets to 0 | test | F-6, AC-6 | — |
| T-13 | Impl: `SyncStart` variant in `LoopCommand`; player loop handling: reset `bar_index = 0`; if project → Running, else → Waiting | impl | F-6, F-4 | T-12 |
| T-14 | Write test: player loop receives `SyncStart` while already Running → bar_index resets, playback restarts from bar 0 | test | F-6, AC-6 | — |
| T-15 | Impl: handle `SyncStart` in Running state: flush notes, reset `bar_index = 0`, re-enter Running | impl | F-6 | T-13, T-14 |
| T-16 | Write test: player loop receives `SyncContinue` with active project → `state()` = Running; `bar_index` is unchanged | test | F-7, AC-7 | — |
| T-17 | Impl: `SyncContinue` variant; player loop handling: if project → Running (keep `bar_index`), else → Waiting | impl | F-7, F-4 | T-16 |
| T-18 | Write test: player loop receives `SyncStop` → `state()` = Stopped; `MockMidiOutput` receives note-off for every active note | test | F-8, F-11, AC-8 | — |
| T-19 | Impl: `SyncStop` variant; player loop handling: identical to `Stop` (flush active notes, transition to Stopped) | impl | F-8, F-11 | T-18 |
| T-20 | Write test: player loop receives `SyncBpmUpdate(100)` while Running; after the current bar ends, `scheduler.bpm()` is 100 | test | F-2, NF-1 | — |
| T-21 | Impl: `SyncBpmUpdate(u32)` variant; store pending BPM update; apply at next bar boundary via `scheduler.update_bpm()` and anchor reset (same path as existing BPM-change logic) | impl | F-2 | T-20 |
| T-22 | Impl: `LoopEngine::sync_start()`, `sync_continue()`, `sync_stop()`, `sync_bpm_update(bpm: u32)` — each sends the corresponding `LoopCommand` variant; no-op if channel disconnected | impl | F-6, F-7, F-8 | T-13, T-17, T-19, T-21 |
| T-23 | Write test: `SetBpm` IPC command returns `{"status":"error","code":"sync_mode_active",…}` when `EngineSettings.mode` is `Sync` | test | F-3, F-13, AC-5, AC-12 | — |
| T-24 | Impl: early-return guard in `SetBpm` handler in `src/ipc/handler.rs`: if `settings.mode == EngineMode::Sync` return error before any state mutation | impl | F-3, F-13 | T-23 |
| T-25 | Write test: `MidiClockReceiver` — feed 25 `Pulse` messages via mock channel → `sync_bpm_update()` called on engine with value near 120 BPM | test | F-1, F-2, AC-1 | — |
| T-26 | Impl: `MidiClockReceiver` base structure in `src/midi_clock/mod.rs`: `ClockMessage` enum; `SyncClockState` enum; spawn OS thread; `recv_timeout` loop; Pulse handling (update tracker, dispatch BpmUpdate) | impl | F-1, F-2 | T-2, T-4, T-25 |
| T-27 | Write test: receiver processes `Start` → `sync_start()` called on engine; `SyncClockState` = Tracking | test | F-6, AC-6, AC-1 | — |
| T-28 | Impl: `Start` message handling in receiver: reset `pulse_tracker`; `received_transport = true`; state = Tracking; call `engine.sync_start()` | impl | F-6 | T-26, T-27 |
| T-29 | Write test: receiver processes `Continue` when `pulse_tracker.is_clock_active()` = true → `sync_continue()` called; `Continue` when clock not active → no engine call | test | F-7, F-4, AC-7, AC-13 | — |
| T-30 | Impl: `Continue` message handling: if `pulse_tracker.is_clock_active(now)` → `received_transport = true`; call `engine.sync_continue()`; else ignore | impl | F-7, F-4, F-14 | T-26, T-29 |
| T-31 | Write test: receiver processes `Stop` (0xFC) → `sync_stop()` called; `received_transport` reset to false | test | F-8, F-11, AC-8 | — |
| T-32 | Impl: `Stop` message handling: `received_transport = false`; call `engine.sync_stop()`; state = Waiting | impl | F-8, F-11 | T-26, T-31 |
| T-33 | Write test: receiver times out (no Pulse for `timeout_duration` × 1.1) → `sync_stop()` called; `SyncClockState` = Lost (AC-2, AC-10); subsequent Pulse → state = Tracking but `sync_start()` NOT called (AC-14) | test | F-5, F-10, F-12, AC-2, AC-10, AC-14 | — |
| T-34 | Impl: timeout handling in receiver: `received_transport = false`; state = Lost; call `engine.sync_stop()`; on next Pulse after loss: state = Tracking but no engine transport call | impl | F-5, F-10, F-12, NF-2 | T-26, T-33 |
| T-35 | Write test: receiver — after clock loss and resume, engine receives `sync_start()` only after a new `Start` message (AC-11); `Continue` also re-enables playback if clock active (AC-11) | test | F-12, AC-11 | — |
| T-36 | Impl: after clock-loss recovery, `Start` or `Continue` (with active clock) must be received before calling `engine.sync_start()`/`sync_continue()` — enforced by `received_transport` being false after clock loss | impl | F-12 | T-34, T-35 |
| T-37 | Write test: `Status` IPC command with mode = Sync and `SyncClockState::Tracking` → response contains `"sync_clock_state":"tracking"`; with `SyncClockState::Lost` → `"sync_clock_state":"lost"` | test | F-5, AC-2 | — |
| T-38 | Impl: extend `Status` handler in `src/ipc/handler.rs` to accept `Option<Arc<Mutex<SyncClockState>>>`; if present and `mode == Sync`, append `"sync_clock_state"` to the response | impl | F-5, AC-2 | T-37 |
| T-39 | Write test: `Status` IPC command with mode = Standalone (no sync state provided) → response does NOT contain `"sync_clock_state"` field | test | F-5, AC-2 | — |
| T-40 | Impl: startup wiring in `src/main.rs` and `src/ipc/mod.rs` — add boolean `--sync` flag (clap); if present, read `PROPELLER_SYNC_PORT` env var (exit with error if absent), set `settings.mode = EngineMode::Sync`, create `MidiClockReceiver` opening that port, and its `Arc<Mutex<SyncClockState>>`; pass both to `run_ipc_server()` | impl | F-1, F-9, AC-9 | T-26, T-38, T-39 |
| T-41 | Write test: `SetMode { mode: "sync" }` IPC command returns `{"status":"error","code":"sync_requires_port",…}` when `sync_clock_state` is `None` (no receiver running) | test | — | — |
| T-42 | Impl: guard in `handle_set_mode` in `src/ipc/handler.rs`: if requested mode is `Sync` and `sync_clock_state` is `None` → return `error_response("sync_requires_port", "sync mode requires --sync at startup")`; function signature updated to accept `sync_clock_state: Option<&Arc<Mutex<SyncClockState>>>` (same plumbing added by T-38) | impl | — | T-38, T-41 |

---

## Open Questions

No open questions. All questions have been reconciled.

---

## Open Decisions

No open decisions. All decisions have been reconciled.

---

## Revision Log

### Cycle 1 — Confidence: 65%
- Reconciled: none (initial generation from PRD)
- Added: D-1 (MIDI input library), D-2 (mid-bar BPM update timing), D-3 (port identification method)

### Cycle 2 — Confidence: 82%
- Reconciled: D-1 → midir (already in architecture), D-2 → bar-boundary BPM (already in T-21), D-3 → port name string (already in T-40); all decisions were pre-reflected in spec text
- Added: Q-1 (sync mode activation: --sync-port vs set-mode sync IPC — gap found by inspecting existing src/ipc/types.rs and src/ipc/handler.rs)

### Cycle 3 — Confidence: 90%
- Reconciled: Q-1 (answer: A) → architecture updated (SetMode guard added; set-mode sync rejected when no receiver); MidiClockReceiver component updated (--sync-port sets mode=Sync at startup); T-40 updated (explicit mode-setting step); T-41 (test: set-mode sync rejected without receiver), T-42 (impl: guard in handle_set_mode) added
- Added: none — confidence 90%, specification is complete

### Cycle 4 — Confidence: 90%
- Reconciled: D-1 (midir), D-2 (bar-boundary BPM), D-3 (port name string) — decision blocks removed; implications already reflected in spec text since Cycle 2
- Added: none — no open decisions remain; specification is complete

### Cycle 5 — Confidence: 90%
- Removed: T-9, T-10, T-11 (port list test, impl, and `list-ports` subcommand); `src/midi_clock/port_list.rs` module; `list_midi_input_ports()` re-export — port enumeration moved to EP-9 (`propeller midi ports`)
- Updated: PRD refs in T-3..T-36 renumbered to match EP-6.md Cycle 4 renumbering (F-11..F-15 → F-10..F-14; AC-11..AC-15 → AC-10..AC-14)

### Cycle 6 — Confidence: 90%
- Updated: overview, module layout, `MidiClockReceiver` component — `--sync-port <name>` replaced by `--sync` (boolean flag) + `PROPELLER_SYNC_PORT` env var; daemon exits with error if `--sync` is present but the env var is unset
- Updated: IPC guard error string `"sync mode requires --sync-port at startup"` → `"sync mode requires --sync at startup"`
- Updated: T-40 — startup wiring now reads `PROPELLER_SYNC_PORT` instead of a CLI argument value
- Updated: T-42 — error string updated to match

### Cycle 7 — Confidence: 90%
- Reconciled: nothing (no open questions or decisions pending)
- Added: Test Strategy section — five testing layers (PulseTracker deterministic via injected `Instant` offsets; player loop commands via existing thread/channel/mock pattern; IPC guards via direct handler calls; MidiClockReceiver state machine via MockMidiClockSource + high-BPM timeout priming; integration startup guard without hardware, full sync flow as `#[ignore]`); wall-clock cost table included
