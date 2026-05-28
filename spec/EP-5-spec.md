# EP-5 · Clock Output Mode — Technical Specification

## Overview

This epic adds MIDI clock output capability to the daemon by extending the existing `LoopEngine` (EP-3) and IPC layer (EP-4). Four new IPC commands (`clock-start`, `clock-pause`, `clock-resume`, `clock-stop`) are wired through new `LoopCommand` variants to the player loop thread. The `MidiOutput` trait gains four transport-message methods. MIDI timing clock pulses (0xF8, every 20 internal ticks) are inserted as `BarEvent::ClockPulse` entries in the per-bar event list. A new `Paused` engine state enables mid-bar position retention for seamless MIDI Continue resumes. The daemon's graceful-shutdown path sends a MIDI Stop before removing the socket.

**Confidence Level:** 95% — All F-x and AC-x are covered by tasks, TDD ordering is maintained, architecture and data model are complete. All decisions reconciled; no open questions or decisions remain. F-19/AC-17 tasks added.

---

## Architecture Overview

EP-5 extends three existing layers without adding new top-level modules.

**Layer 1 — MidiOutput trait** (`src/loop_engine/midi.rs`): four transport-message methods are added (`clock_tick`, `clock_start`, `clock_continue`, `clock_stop`). All implementors — `MockMidiOutput` and the hardware implementation wired at daemon startup — must implement them. `MidiEvent` gains matching variants so `MockMidiOutput` can record them in test assertions.

**Layer 2 — Player loop** (`src/loop_engine/player.rs` and `mod.rs`): four new `LoopCommand` variants (`ClockStart`, `ClockPause`, `ClockResume`, `ClockStop`) are added. `EngineState` gains a `Paused` variant. A new `PauseContext` value stores the remaining unprocessed events for the current bar, enabling mid-bar resume. When the player loop receives `ClockStart`, it calls `output.clock_start()` (0xFA) and then emits both note events and `ClockPulse` events from the bar event list. `ClockPulse` entries are inserted at every tick that is a multiple of 20 within the bar at list-build time. When it receives `ClockPause`, it flushes all active-note off-events, captures the remaining bar events as `PauseContext`, and transitions to `Paused`. When it receives `ClockResume`, it calls `output.clock_continue()` (0xFB) and resumes processing from `PauseContext`. When it receives `ClockStop`, it calls `output.clock_stop()` (0xFC), flushes active notes, resets bar index, and transitions to `Stopped`.

**Layer 3 — IPC dispatch** (`src/ipc/types.rs` and `handler.rs`): four new `Command` variants (`ClockStart`, `ClockPause`, `ClockResume`, `ClockStop`) are added. The F-8 rejection — clock-start without an active project — is enforced at the dispatch layer by checking `store.read().active()` before sending the command to the engine; no response channel back from the player loop is needed.

**Component interaction:**

```
IPC dispatch
  │  F-8 check: store.read().active() for ClockStart
  │  mpsc::Sender<LoopCommand>
  ▼
LoopEngine  (new: clock_start, clock_pause, clock_resume, clock_stop methods)
  │
  ▼
Player loop thread
  ├── EngineState: Stopped | Waiting | Running | Paused
  ├── PauseContext: remaining Vec<(u64, BarEvent)> for current bar
  ├── Bar event list (BarEvent: NoteOn | NoteOff | ClockPulse)
  └── Box<dyn MidiOutput>  — extended with 4 clock transport methods
```

**Extended state machine:**

| From | Trigger | To | Action |
|------|---------|----|--------|
| Stopped | ClockStart + active project | Running | send 0xFA, begin event list with ClockPulse |
| Stopped | ClockStart + no project | — | rejected at IPC layer (never reaches player loop) |
| Running | ClockPause | Paused | flush active-note offs, store PauseContext |
| Running | ClockStop | Stopped | flush active-note offs, send 0xFC, reset bar index |
| Paused | ClockResume | Running | send 0xFB, restore PauseContext events, reset anchor |
| Paused | ClockStop | Stopped | send 0xFC, clear PauseContext, reset bar index |

Existing EP-3 transitions (Start/Stop commands) are unchanged for standalone mode.

**Clock pulse insertion:**

`ClockPulse` events are inserted at tick offsets 0, 20, 40, …, `(bar_ticks − 1)` when building the per-bar sorted event list. They carry sort priority −1 (lower than NoteOff's 0), ensuring clock pulses precede any note events at the same tick, which keeps connected devices in sync per MIDI convention.

**Pause and resume mechanics:**

On `ClockPause`, the player loop halts between event deadlines (caught by `try_recv()`). It stores the slice of the sorted event list that has not yet been emitted as `PauseContext`. On `ClockResume`, it sends 0xFB, resets the timing anchor to `now − tick_of_next_event * micros_per_tick` (so the first resumed event fires immediately at the correct relative position), and proceeds through the remaining events.

**Project removal while clock running (F-18):**

At each bar boundary, `commit_pending()` is called. If the resulting `active()` is `None`, the player stays in `Running` state but builds an event list containing only `ClockPulse` entries (no note events). Clock pulses continue; no MIDI Stop is sent. When a new project is committed, the loop rebuilds a full event list on the next bar boundary.

**Hardware MidiOutput implementation (F-15, AC-13):**

`MidiPortOutput` (`src/midi_port.rs`, introduced by EP-8) implements the `MidiOutput` trait. EP-5 extends the trait with four required clock methods, so EP-5 must also add their implementation on `MidiPortOutput`. Each method sends a single-byte MIDI status message via `self.0.send()`: `clock_tick` → `[0xF8]`, `clock_start` → `[0xFA]`, `clock_continue` → `[0xFB]`, `clock_stop` → `[0xFC]`. This is a direct dependency: `MidiPortOutput` will not compile until these methods are present.

**Daemon shutdown MIDI Stop (F-9, NF-2):**

`daemon.rs` calls `engine.clock_stop_on_shutdown()` after the tokio select exits (on SIGTERM or stop command) but before `fs::remove_file(&sock_path)`. This method checks `engine.state()` and, if `Running` or `Paused`, sends `ClockStop` synchronously before the socket is removed.

**Startup latency window (F-19, AC-17):**

A module-level constant `START_LATENCY_MICROS: u64 = 20_000` (20 ms) is defined in `player.rs`. On `Start`, `ClockStart`, and `Waiting→Running` transitions, `anchor` is set to `Instant::now() + Duration::from_micros(START_LATENCY_MICROS)` instead of `Instant::now()`. Initial MIDI setup messages (Program Change, `clock_start()`) are sent immediately before the event loop; all bar events — NoteOn, NoteOff, ClockPulse — then fire once `anchor` is reached (~20 ms later). The BPM-change anchor reset (`anchor = Instant::now()` at bar boundaries) and the pause-resume anchor recalculation are unaffected.

---

## Components

### `LoopEngine` (extended) — `src/loop_engine/mod.rs`

New public methods added to the existing `LoopEngine` facade:

- `clock_start(&self)` — sends `LoopCommand::ClockStart`
- `clock_pause(&self)` — sends `LoopCommand::ClockPause`
- `clock_resume(&self)` — sends `LoopCommand::ClockResume`
- `clock_stop(&self)` — sends `LoopCommand::ClockStop`
- `clock_stop_on_shutdown(&self)` — calls `clock_stop()` only if `state()` is `Running` or `Paused`; used by daemon shutdown path

### `MidiOutput` trait (extended) — `src/loop_engine/midi.rs`

New required methods:

```rust
fn clock_tick(&mut self);      // 0xF8 MIDI Timing Clock
fn clock_start(&mut self);     // 0xFA MIDI Start
fn clock_continue(&mut self);  // 0xFB MIDI Continue
fn clock_stop(&mut self);      // 0xFC MIDI Stop
```

`MidiEvent` gains four matching variants: `ClockTick`, `ClockStart`, `ClockContinue`, `ClockStop`. `MockMidiOutput` records all four. `MidiPortOutput` (`src/midi_port.rs`) also gains the four methods, each sending a single status byte via `self.0.send()`.

### Player loop (extended) — `src/loop_engine/player.rs`

`BarEvent` gains a `ClockPulse` variant. The bar event list builder inserts `ClockPulse` at every tick `% 20 == 0` within the bar. A local boolean `is_clock_mode: bool` (set `true` on `ClockStart`, `false` on `Start`) gates whether `ClockPulse` entries are included in the event list. The player loop's `try_recv()` check is extended to handle `ClockPause`, `ClockResume`, and `ClockStop` in addition to the existing `Stop`. The `Paused` branch in the state machine blocks on `recv()` waiting for `ClockResume` or `ClockStop`.

### IPC types (extended) — `src/ipc/types.rs`

Four new `Command` variants: `ClockStart`, `ClockPause`, `ClockResume`, `ClockStop`.

### IPC handler (extended) — `src/ipc/handler.rs`

`dispatch()` gains four new match arms. The `ClockStart` arm checks `store.read().active().is_some()` and returns `error_response("no_project", "clock-start requires an active project")` if no project is loaded.

---

## Data Model

| Type | Fields | Notes |
|------|--------|-------|
| `EngineState` | `Stopped`, `Waiting`, `Running`, `Paused` | `Paused` added for clock-pause sub-state |
| `LoopCommand` | `Start`, `Stop`, `ClockStart`, `ClockPause`, `ClockResume`, `ClockStop` | Four new variants for clock control |
| `PauseContext` | `remaining_events: Vec<(u64, BarEvent)>`, `bar_index: usize` | Events not yet emitted in the current bar at pause time plus the bar index for correct multi-bar resume; held in player loop local state |
| `BarEvent` | `NoteOn { channel, pitch, velocity }`, `NoteOff { channel, pitch }`, `ClockPulse` | `ClockPulse` added; sort priority −1 (before NoteOff=0, NoteOn=1) |
| `MidiEvent` | `NoteOn`, `NoteOff`, `ProgramChange`, `ClockTick`, `ClockStart`, `ClockContinue`, `ClockStop` | Four new variants for clock transport messages |
| `MidiOutput` | trait | `clock_tick`, `clock_start`, `clock_continue`, `clock_stop` methods added |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID | Task | Type | PRD ref | Depends on |
|----|------|------|---------|------------|
| T-1 | Write test: `MockMidiOutput` records `ClockTick`, `ClockStart`, `ClockContinue`, `ClockStop` events in insertion order alongside existing events | test | F-1, F-10, F-11, F-12 | — |
| T-2 | Impl: extend `MidiOutput` trait with `clock_tick`, `clock_start`, `clock_continue`, `clock_stop`; add four variants to `MidiEvent`; implement all on `MockMidiOutput` | impl | F-1, F-10, F-11, F-12 | T-1 |
| T-3 | Write test: `EngineState::Paused` exists and can be stored in `Arc<Mutex<EngineState>>`; `LoopCommand` has `ClockStart`, `ClockPause`, `ClockResume`, `ClockStop` variants | test | F-5, F-6 | — |
| T-4 | Impl: add `Paused` to `EngineState`; add `ClockStart`, `ClockPause`, `ClockResume`, `ClockStop` to `LoopCommand`; add `PauseContext` struct | impl | F-5, F-6, F-14 | T-3 |
| T-5 | Write test: `LoopEngine::clock_start()` sends `ClockStart` on the channel (verify via state change); `clock_stop()` sends `ClockStop` | test | F-4, F-7 | — |
| T-6 | Impl: add `clock_start`, `clock_pause`, `clock_resume`, `clock_stop`, `clock_stop_on_shutdown` methods to `LoopEngine` | impl | F-4, F-5, F-6, F-7, F-9 | T-4, T-5 |
| T-7 | Write test: `clock_start()` with active project → `MockMidiOutput` receives `ClockStart` (0xFA) before the first `ClockTick` (0xF8) | test | F-10, AC-8 | — |
| T-8 | Impl: handle `ClockStart` in player loop — if no active project discard (guard at IPC layer); send `output.clock_start()`; transition to `Running`; rebuild bar event list including `ClockPulse` entries | impl | F-1, F-10, F-14, AC-1, AC-8 | T-2, T-4, T-6, T-7 |
| T-9 | Write test: running engine in clock mode emits `ClockTick` events; with a 1/4-bar project at BPM 300 (480 ticks, 24 pulses) the event list contains exactly 24 `ClockPulse` entries | test | F-1, F-13, AC-1 | — |
| T-10 | Impl: bar event list builder inserts `BarEvent::ClockPulse` at every tick `% 20 == 0` within `[0, bar_ticks)`; sort `ClockPulse` with priority −1 | impl | F-1, F-2, F-13 | T-8, T-9 |
| T-11 | Write test: `clock-start` IPC command with no active project in store → response `{"status":"error","code":"no_project",...}` and engine state remains `Stopped` | test | F-8, AC-5 | — |
| T-12 | Impl: add `ClockStart`, `ClockPause`, `ClockResume`, `ClockStop` to `Command` enum in `src/ipc/types.rs`; add handlers in `dispatch()`; `ClockStart` handler checks `store.read().active().is_some()` and returns `no_project` error if `None` | impl | F-4, F-5, F-6, F-7, F-8, AC-5 | T-4, T-6, T-11 |
| T-13 | Write test: `clock_start()` with active project → engine state transitions to `Running` and loop also plays note events simultaneously | test | F-14, AC-11 | — |
| T-14 | Impl: `ClockStart` transitions to `Running` and the existing EP-3 note-event playback path runs concurrently with clock pulses within the same event list | impl | F-14, AC-11 | T-8, T-13 |
| T-15 | Write test: `clock_pause()` while running → engine state transitions to `Paused`; `MockMidiOutput` receives `NoteOff` for every active note and no `ClockStop` | test | F-5, F-16, AC-2, AC-14 | — |
| T-16 | Impl: handle `ClockPause` in player loop — flush active notes (note-off for each `ActiveNote`), store remaining bar events in `PauseContext`, transition to `Paused`; no MIDI Stop sent | impl | F-5, F-14, F-16, AC-2, AC-14 | T-6, T-14, T-15 |
| T-17 | Write test: `clock_resume()` while paused → `MockMidiOutput` receives `ClockContinue` (0xFB) before the first resumed `ClockTick`; engine state transitions back to `Running` | test | F-6, F-11, AC-3, AC-9 | — |
| T-18 | Impl: handle `ClockResume` in player loop — send `output.clock_continue()`; restore remaining events from `PauseContext`; reset timing anchor to `now − tick_of_next_event * micros_per_tick`; transition to `Running` | impl | F-6, F-11, F-17, AC-3, AC-9, AC-15 | T-16, T-17 |
| T-19 | Write test: loop note output resumes from the retained tick position after `clock_resume()` — notes from the retained position onward are emitted, earlier-in-bar notes are not re-emitted | test | F-17, AC-15 | — |
| T-20 | Impl: `PauseContext.remaining_events` contains only events at tick ≥ pause-tick; resume processing starts from this subset (verified by T-19) | impl | F-17, AC-15 | T-18, T-19 |
| T-21 | Write test: `clock_stop()` while running → `MockMidiOutput` receives `NoteOff` for all active notes then `ClockStop` (0xFC); engine state transitions to `Stopped` | test | F-7, F-12, AC-4, AC-10, AC-12 | — |
| T-22 | Impl: handle `ClockStop` in player loop — flush active notes, send `output.clock_stop()`, reset bar index to 0, transition to `Stopped` | impl | F-7, F-12, F-14, AC-4, AC-10, AC-12 | T-6, T-14, T-21 |
| T-23 | Write test: `clock_stop()` while paused → `ClockStop` (0xFC) emitted, state transitions to `Stopped` | test | AC-4 | — |
| T-24 | Impl: handle `ClockStop` in `Paused` branch — clear `PauseContext`, send `output.clock_stop()`, reset bar index, transition to `Stopped` | impl | F-7, F-12 | T-22, T-23 |
| T-25 | Write test: BPM changes while clock is running → clock continues without stopping and subsequent `ClockTick` events use the new BPM-derived tick spacing | test | F-3, AC-6 | — |
| T-26 | Impl: bar-boundary BPM update (EP-3 T-30) already adjusts `micros_per_tick`; verify `ClockPulse` entries in the new bar event list are scheduled at the updated tick duration | impl | F-3, AC-6 | T-10, T-25 |
| T-27 | Write test: project removed (store cleared) while clock is running → `MockMidiOutput` continues receiving `ClockTick` events and receives no `NoteOn` events; no `ClockStop` emitted | test | F-18, AC-16 | — |
| T-28 | Impl: player loop bar-boundary logic in clock mode — if `active()` is `None` after `commit_pending()`, build event list with `ClockPulse`-only entries and continue in `Running` state; note events omitted | impl | F-18, AC-16 | T-14, T-27 |
| T-29 | Write test: daemon graceful shutdown (SIGTERM) while clock is running → `MockMidiOutput` receives `ClockStop` (0xFC); state is `Stopped` | test | F-9, NF-2, AC-7 | — |
| T-30 | Impl: `daemon.rs` shutdown path calls `engine.clock_stop_on_shutdown()` after tokio select exits and before `fs::remove_file(&sock_path)` | impl | F-9, NF-2, AC-7 | T-6, T-22, T-29 |
| T-31 | Write test: `MidiPortOutput` clock byte helpers return `[0xF8]` for `clock_tick`, `[0xFA]` for `clock_start`, `[0xFB]` for `clock_continue`, `[0xFC]` for `clock_stop` (test via private helper functions analogous to EP-8's `note_on_bytes`) | test | F-15, AC-13 | T-2 |
| T-32 | Impl: add `clock_tick`, `clock_start`, `clock_continue`, `clock_stop` on `MidiPortOutput` (`src/midi_port.rs`) — each sends the corresponding single status byte via `self.0.send()`; satisfies F-15/AC-13 | impl | F-15, AC-13 | T-2, T-31 |
| T-33 | Write test: after `engine.start()` or `engine.clock_start()`, the first `NoteOn` event is received no sooner than `START_LATENCY_MICROS` (20 ms) after the call, confirming that the startup latency window is applied before the first note event; uses a timestamped capturing output to compare wall-clock timestamps | test | F-19, AC-17 | — |
| T-34 | Impl: define `START_LATENCY_MICROS: u64 = 20_000` in `player.rs`; set `anchor = Instant::now() + Duration::from_micros(START_LATENCY_MICROS)` on `Start`, `ClockStart`, and `Waiting→Running` transitions; leave BPM-change and pause-resume anchor resets unchanged | impl | F-19, AC-17 | T-33 |

---

## Open Questions

No open questions. All questions have been reconciled.

---

## Open Decisions

No open decisions remain. All decisions have been reconciled.

---

## Revision Log

### Cycle 1 — Confidence: 60%
- Reconciled: none (initial generation from PRD)
- Added: D-1 (MidiOutput extension), D-2 (ClockPulse scheduling), D-3 (PauseContext), D-4 (clock-mode detection), D-5 (daemon shutdown wiring)

### Cycle 2 — Confidence: 82%
- Reconciled: none (no answered Open Questions; Open Decisions were pre-selected at creation time)
- Fixed: PauseContext data model now includes `bar_index: usize` per D-3 option A; player loop component documents `is_clock_mode: bool` local flag per D-4 option A
- Added: Q-1 (hardware MidiOutput clock-method implementation — EP-5 vs EP-8 responsibility)

### Cycle 3 — Confidence: 93%
- Reconciled: Q-1 (option A) → architecture updated (hardware MidiOutput section added); MidiOutput trait component updated to reference `MidiPortOutput`; added T-31 (test clock byte helpers on `MidiPortOutput`) and T-32 (impl clock methods on `MidiPortOutput`, covers F-15/AC-13)
- Added: none — confidence 93%, specification is complete

### Cycle 4 — Confidence: 95%
- Reconciled: D-1 (A) → required methods on MidiOutput confirmed in spec; D-2 (A) → ClockPulse inserted every 20 ticks in shared event list confirmed; D-3 (A) → PauseContext with remaining_events and bar_index confirmed in data model; D-4 (A) → is_clock_mode bool flag confirmed in player loop component; D-5 (A) → explicit clock_stop_on_shutdown call in daemon.rs confirmed in architecture — all five decision blocks removed from Open Decisions
- Added: none — confidence 95%, specification is complete

### Cycle 5 — Confidence: 95%
- Reconciled: none
- Added: F-19/AC-17 from PRD → architecture updated (Startup latency window section); T-33 (test: first NoteOn fires no sooner than 20 ms after start) and T-34 (impl: START_LATENCY_MICROS constant and anchor offset on Start/ClockStart/Waiting→Running transitions)
