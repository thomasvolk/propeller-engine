# EP-3 · Loop Engine — Technical Specification

## Overview

This epic implements the loop engine: the component that reads the active project from the `ProjectStore` (EP-2), converts tick positions to wall-clock time using BPM and PPQN, and emits MIDI events continuously in a seamless loop. The engine runs on a dedicated OS thread decoupled from the tokio async runtime, receives start/stop commands via a synchronous channel, detects BPM and instrument changes at every bar boundary, and prevents stuck notes by tracking all sounding notes and flushing them on stop.

**Confidence Level:** 90% — All PRD items and acceptance criteria have tasks, TDD ordering is maintained, all five open decisions and all five open questions are now reconciled; residual uncertainty is limited to integration-test infrastructure details that surface only during implementation.

---

## Architecture Overview

The loop engine is a self-contained module (`src/loop_engine/`) added to the daemon process. It contains no async code and runs on a dedicated OS thread. The tokio IPC handler (EP-4) communicates with it via a `std::sync::mpsc` channel.

**Component interaction:**

```
Tokio IPC handler (EP-4)
  │  std::sync::mpsc::Sender<LoopCommand>
  ▼
LoopEngine facade  (lives on tokio side; holds Sender and Arc<Mutex<EngineState>>)
  │
  ▼
Loop thread  (dedicated OS thread, owns Receiver<LoopCommand>)
  ├── Arc<RwLock<ProjectStore>>  — reads active(), calls commit_pending()
  ├── Scheduler                 — converts tick positions to Instant deadlines
  ├── Box<dyn MidiOutput>       — emits note-on, note-off, program-change
  └── Vec<ActiveNote>           — sounding notes for stuck-note prevention
```

**State machine:**

The loop thread maintains one of three states:

- `Stopped` — default; thread blocks on `recv()`; no MIDI output.
- `Waiting` — start issued but no active project; thread polls for a project with a 10 ms sleep (NF-5); no MIDI output.
- `Running` — active project present; thread advances through bar events in real time.

Transitions:

| From | Trigger | To |
|------|---------|-----|
| Stopped | Start + active project present | Running |
| Stopped | Start + no active project | Waiting |
| Waiting | `commit_pending()` promotes a project | Running |
| Waiting | Stop | Stopped |
| Running | Stop | Stopped (flush active notes first) |

A `Start` command received while already in `Running` or `Waiting` is silently ignored (no-op); it does not restart playback or reset the bar index.

**Tick scheduling:**

PPQN = 480 (from EP-2). Microseconds per tick: `60_000_000 / (bpm * PPQN)`. The `Scheduler` anchors each event deadline to a reference `Instant` (set at loop start or BPM-change time) and computes each deadline as `anchor + tick_offset * micros_per_tick`. Anchoring prevents cumulative drift (NF-2). Actual sleeping uses a hybrid strategy: `thread::sleep` until approximately 500 μs before the deadline, then spin on `Instant::now()` to hit the exact moment (D-5).

**Bar event list:**

At the start of each bar, the loop thread builds a sorted list of `(absolute_tick, BarEvent)` entries from all tracks. `BarEvent` is either `NoteOn { channel, pitch, velocity }` or `NoteOff { channel, pitch }`. Note-offs are inserted at `start_tick + duration_ticks`. The list is sorted by `(tick, priority)` where `NoteOff` carries priority 0 and `NoteOn` carries priority 1 — this ensures `NoteOff` always precedes `NoteOn` at the same tick, preventing voice-stealing on retrigger (MIDI convention). The thread then walks this list in order, sleeping to each deadline, emitting the event, and checking for a `Stop` or `Disconnected` command after each event via `try_recv()`.

**Bar boundary processing:**

At the end of each bar, before advancing, the loop thread:
1. Calls `store.write().commit_pending()` to atomically swap any pending project update.
2. Reads the new active project's `header.bpm`; if changed, calls `scheduler.update_bpm()` and resets the tick anchor to now (F-4, F-11).
3. Compares each track's `instrument` in the new project against the last-seen values; emits a `ProgramChange` for any track whose instrument changed (F-12).
4. Advances the bar index using `project.cycle_length()` and `track.bar_at()` from EP-2.

**Shutdown:**

Dropping `LoopEngine` drops its internal `Sender<LoopCommand>`. The player loop exits on the next `recv()` or `try_recv()` call that returns an `Err` due to the disconnected channel: in `Stopped` state `recv()` returns `Err(RecvError)` immediately; in `Waiting` or `Running` state `try_recv()` returns `Err(TryRecvError::Disconnected)` on the next poll or event check. No explicit `join()` call is required — the process exits promptly after the daemon's clean shutdown sequence.

---

## Components

### LoopEngine (`src/loop_engine/mod.rs`)

Public facade created once at daemon startup. Holds a `Sender<LoopCommand>` and an `Arc<Mutex<EngineState>>` (for observability and testing). Exposes:

- `LoopEngine::new(store: Arc<RwLock<ProjectStore>>, output: Box<dyn MidiOutput>) -> LoopEngine`
- `LoopEngine::start(&self)` — sends `LoopCommand::Start`; no-op if already `Running` or `Waiting`
- `LoopEngine::stop(&self)` — sends `LoopCommand::Stop`
- `LoopEngine::state(&self) -> EngineState` — reads shared state
- `Drop` — dropping `LoopEngine` drops the `Sender`, causing the loop thread to exit on its next channel operation

### Scheduler (`src/loop_engine/scheduler.rs`)

Converts BPM + PPQN into tick durations and `Instant` deadlines. Anchored to a reference `Instant` to prevent drift.

- `Scheduler::new(bpm: u32) -> Scheduler`
- `Scheduler::micros_per_tick() -> u64` — `60_000_000 / (bpm * 480)`
- `Scheduler::deadline_for_tick(anchor: Instant, tick: u64) -> Instant` — `anchor + tick * micros_per_tick()`
- `Scheduler::update_bpm(&mut self, bpm: u32)` — updates the rate; caller is responsible for resetting the anchor
- `Scheduler::sleep_until(deadline: Instant)` — hybrid sleep + spin; must not overshoot by more than 5 ms

### MidiOutput (`src/loop_engine/midi.rs`)

Trait abstracting MIDI device access to enable testing without hardware.

```rust
pub trait MidiOutput: Send + 'static {
    fn note_on(&mut self, channel: u8, pitch: u8, velocity: u8);
    fn note_off(&mut self, channel: u8, pitch: u8);
    fn program_change(&mut self, channel: u8, program: u8);
}
```

`MockMidiOutput` records all emitted events in a `Vec<MidiEvent>` for test assertions.

### Loop Thread (`src/loop_engine/player.rs`)

Runs on the dedicated OS thread. Owns `Receiver<LoopCommand>`, `Arc<RwLock<ProjectStore>>`, `Scheduler`, `Box<dyn MidiOutput>`, `Vec<ActiveNote>`, and the `Arc<Mutex<EngineState>>` write handle.

Core loop:
1. `Stopped` — block on `recv()`; on `Start` check store and transition; on `Err(RecvError)` (Sender dropped) exit the thread.
2. `Waiting` — sleep 10 ms; call `commit_pending()`; if project now active, transition to `Running`; on `try_recv()` returning `Err(Disconnected)` exit the thread.
3. `Running` — emit program changes on entry; process bar event list sorted by `(tick, priority)` (NoteOff priority 0, NoteOn priority 1); at bar end call `commit_pending()`, detect BPM and instrument changes; wrap bar index; check for `Stop` or disconnect between events via `try_recv()`; on `Err(Disconnected)` exit the thread.

---

## Data Model

| Type | Fields | Notes |
|------|--------|-------|
| `EngineState` | `Stopped`, `Running`, `Waiting` | Shared via `Arc<Mutex<EngineState>>` between facade and loop thread |
| `LoopCommand` | `Start`, `Stop` | Sent from `LoopEngine::start()`/`stop()` to loop thread |
| `Scheduler` | `bpm: u32`, `micros_per_tick: u64` | PPQN fixed at 480; `micros_per_tick` recalculated on `update_bpm()` |
| `ActiveNote` | `channel: u8`, `pitch: u8` | Tracks every currently-sounding note for stuck-note prevention |
| `MidiEvent` | `NoteOn { channel, pitch, velocity }`, `NoteOff { channel, pitch }`, `ProgramChange { channel, program }` | Used in `MockMidiOutput` for assertions |
| `BarEvent` | `NoteOn { channel: u8, pitch: u8, velocity: u8 }`, `NoteOff { channel: u8, pitch: u8 }` | Internal per-bar event; built into a sorted `Vec<(tick, BarEvent)>` at bar start; converted to `MidiOutput` calls as deadlines arrive |
| `MidiOutput` | trait | `note_on`, `note_off`, `program_change` methods; `Send + 'static` |
| `MockMidiOutput` | `events: Vec<MidiEvent>` | Test double; records all emitted events in order |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID | Task | Type | PRD ref | Depends on |
|----|------|------|---------|------------|
| T-1 | Write test: `Scheduler::micros_per_tick()` returns the correct integer value for BPM 125 (exact: 1000) and BPM 120 (truncated: 1041) | test | F-3, NF-4 | — |
| T-2 | Impl: `Scheduler` in `src/loop_engine/scheduler.rs` with `new(bpm: u32) -> Scheduler` and `micros_per_tick() -> u64` using `60_000_000u64 / (bpm as u64 * 480)` | impl | F-3, NF-4 | T-1 |
| T-3 | Write test: `Scheduler::deadline_for_tick(start, 0)` returns `start`; `deadline_for_tick(start, 480)` returns `start + Duration::from_micros(480 * 1000)` at BPM 125 | test | F-3, NF-2, NF-4 | — |
| T-4 | Impl: `Scheduler::deadline_for_tick(anchor: Instant, tick: u64) -> Instant` | impl | F-3, NF-2 | T-2, T-3 |
| T-5 | Write test: after `update_bpm(60)`, `micros_per_tick()` reflects the new rate; `deadline_for_tick` uses the updated rate | test | F-4, F-11, AC-3, AC-10 | — |
| T-6 | Impl: `Scheduler::update_bpm(&mut self, bpm: u32)` — recalculates `micros_per_tick` | impl | F-4, F-11 | T-2, T-5 |
| T-7 | Write test: `Scheduler::sleep_until(deadline)` wakes no more than 5 ms after the deadline on an idle system | test | NF-1, NF-3 | — |
| T-8 | Impl: `Scheduler::sleep_until(deadline: Instant)` using hybrid strategy: `thread::sleep(remaining.saturating_sub(500μs))` then spin on `Instant::now() >= deadline` | impl | NF-1, NF-3, NF-4 | T-2, T-7 |
| T-9 | Write test: `MockMidiOutput::events` records `NoteOn`, `NoteOff`, and `ProgramChange` events in insertion order | test | F-7, F-12 | — |
| T-10 | Impl: `MidiOutput` trait and `MidiEvent` enum in `src/loop_engine/midi.rs`; `MockMidiOutput` with `events: Vec<MidiEvent>` | impl | F-7, F-12 | T-9 |
| T-11 | Write test: `LoopEngine::new()` returns an engine whose `state()` is `EngineState::Stopped` | test | F-9 | — |
| T-12 | Impl: `LoopEngine` in `src/loop_engine/mod.rs`; `EngineState` enum; `mpsc` channel; `new()`, `start()`, `stop()`, `state()`; spawn dedicated OS thread running the player loop | impl | F-9, F-10 | T-2, T-8, T-10, T-11 |
| T-13 | Write test: `start()` with no active project in store → `state()` returns `Waiting`; `MockMidiOutput` receives no events | test | F-13, F-15, AC-12 | — |
| T-14 | Write test: `start()` with an active project in store → `state()` transitions to `Running` | test | F-9, AC-7 | — |
| T-15 | Impl: `LoopCommand::Start` handling in player loop: if state is already `Running` or `Waiting`, return immediately (no-op); otherwise read `store.read().active()`; if `Some` → `Running`, else → `Waiting` | impl | F-9, F-13, F-15 | T-12, T-13, T-14 |
| T-16 | Write test: `stop()` while engine is in `Running` state → `state()` returns `Stopped` | test | F-10, AC-8 | — |
| T-17 | Impl: `LoopCommand::Stop` handling in player loop: emit note-off for all `ActiveNote` entries, clear the list, transition to `Stopped` | impl | F-10, F-14 | T-12, T-16 |
| T-18 | Write test: running engine with a single non-rest note bar → `MockMidiOutput` receives one `NoteOn` followed by one `NoteOff` | test | F-7, AC-1, AC-6 | — |
| T-19 | Write test: running engine with a rest note bar → `MockMidiOutput` receives no events | test | F-8, AC-5 | — |
| T-20 | Impl: bar event list builder in player loop — iterates all tracks, inserts `NoteOn` at note start tick and `NoteOff` at `start_tick + duration_ticks` for non-rest notes, skips rests; sorts by `(tick, priority)` where `NoteOff = 0, NoteOn = 1`; walks sorted list sleeping to each deadline | impl | F-7, F-8 | T-12, T-18, T-19 |
| T-21 | Write test: running engine with two tracks each having one note → `MockMidiOutput` receives note-on events for both tracks | test | F-2, AC-1 | — |
| T-22 | Impl: bar event list builder processes all tracks; events from the same tick across different tracks are both present in the sorted list | impl | F-2 | T-20, T-21 |
| T-23 | Write test: running engine sends a `ProgramChange` for each track before any `NoteOn` at the start of the first loop iteration | test | F-12, AC-11 | — |
| T-24 | Impl: on first entry into `Running` state, emit `ProgramChange` for every track before processing bar events; record last-seen instrument per track | impl | F-12 | T-12, T-20, T-23 |
| T-25 | Write test: after the final bar of the project plays, the engine wraps to bar 0 and continues emitting events without stopping | test | F-1, F-5, AC-2 | — |
| T-26 | Impl: at end of each bar advance bar index using `project.cycle_length()` and `track.bar_at()` from EP-2; loop continues seamlessly | impl | F-1, F-5 | T-20, T-25 |
| T-27 | Write test: pending project set via `store.set_pending()` takes effect in the bar immediately after the current bar completes; prior bar plays unchanged | test | F-6, AC-4 | — |
| T-28 | Impl: at each bar boundary, call `store.write().commit_pending()` before advancing the bar index | impl | F-6 | T-26, T-27 |
| T-29 | Write test: updated project with a changed BPM — tick deadlines in the subsequent bar use the new rate; no stop occurs | test | F-4, F-11, AC-3, AC-10 | — |
| T-30 | Impl: after `commit_pending()`, read new active BPM; if changed, call `scheduler.update_bpm()` and reset the tick anchor to the current instant | impl | F-4, F-11 | T-6, T-28, T-29 |
| T-31 | Write test: updated project with a track whose instrument changed → `ProgramChange` re-sent for that track at the bar boundary; unchanged tracks receive no new `ProgramChange` | test | F-12, AC-11 | — |
| T-32 | Impl: after `commit_pending()`, compare each track's new instrument against the last-seen values; emit `ProgramChange` and update the record for changed tracks only | impl | F-12 | T-24, T-28, T-31 |
| T-33 | Write test: engine in `Waiting` state after `store.set_pending(project)` is called, the engine transitions to `Running` automatically and begins emitting events | test | F-15, AC-14 | — |
| T-34 | Impl: in `Waiting` loop, sleep 10 ms then call `store.write().commit_pending()`; if it returns `true`, transition to `Running` and proceed with initial program changes; on `try_recv()` returning `Err(Disconnected)`, exit the thread | impl | F-15 | T-15, T-24, T-33 |
| T-35 | Write test: `stop()` while a note is sounding (note-on emitted, note-off not yet due) → `MockMidiOutput` receives a `NoteOff` before any further events cease | test | F-14, AC-13 | — |
| T-36 | Impl: update `ActiveNote` list on every note-on (push) and note-off (remove); on `Stop` command, emit `note_off` for every remaining `ActiveNote` then clear | impl | F-14 | T-17, T-35 |
| T-37 | Write test: bar event list where a `NoteOff` and a `NoteOn` share the same tick position emits `NoteOff` before `NoteOn` in `MockMidiOutput` | test | F-7, AC-6 | — |
| T-38 | Impl: update bar event list builder to sort by `(tick, priority)` tuple where `NoteOff` carries priority 0 and `NoteOn` carries priority 1 | impl | F-7 | T-20, T-37 |
| T-39 | Write test: run the engine for 4 bars with a high BPM project and a `MockMidiOutput` that captures event arrival timestamps via `Instant::now()`; assert every captured event arrives within ±5 ms of its ideal scheduled tick deadline | test | AC-9, NF-1, NF-3 | T-12, T-18, T-20 |
| T-40 | Write test: `start()` called while engine is in `Running` state does not change the state and does not restart bar playback (state remains `Running`) | test | F-9 | — |
| T-41 | Impl: at the start of `LoopCommand::Start` handling, check current state; if already `Running` or `Waiting`, return immediately without modifying state | impl | F-9 | T-15, T-40 |
| T-42 | Write test: dropping the `LoopEngine` handle causes the loop thread to exit within a short timeout (assert `JoinHandle::join()` completes) | test | — | — |
| T-43 | Impl: ensure the player loop exits cleanly on channel disconnect in all three states: `Stopped` (`recv()` returns `Err`) breaks the outer loop; `Waiting` and `Running` (`try_recv()` returns `Err(Disconnected)`) break their respective inner loops | impl | — | T-12, T-42 |

---

## Open Questions

No open questions. All questions have been reconciled.

---

## Open Decisions

High-impact architecture and technology choices. Check your preferred option for each decision, then re-run `/create-spec EP-3` to reconcile.

### D-1 · Module layout

Where does the loop engine code live? This affects how imports are structured in other modules.

- [ ] A. Single file `src/loop_engine.rs` — simple to start; may grow large
- [x] B. Directory `src/loop_engine/` with `mod.rs`, `scheduler.rs`, `midi.rs`, `player.rs` — matches EP-2 pattern *(recommended — consistent with domain module layout; separates concerns cleanly)*

Answer: B.

### D-2 · MidiOutput abstraction

How should MIDI device access be structured for testability and portability?

- [x] A. `MidiOutput` trait in `src/loop_engine/midi.rs`; concrete hardware impl injected at daemon startup *(recommended — enables unit tests without hardware; defers crate selection to runtime wiring in EP-4)*
- [ ] B. Direct use of `midir` crate throughout the loop engine — fewer abstractions but tightly coupled to hardware
- [ ] C. `wmidi` for message types + `midir` for I/O — two crates with explicit typing; higher ceremony

Answer: A.

### D-3 · Thread model for the loop engine

Which thread primitive should the loop engine use to ensure timing precision without blocking the tokio executor?

- [x] A. `std::thread::spawn` — dedicated OS thread; scheduler sleeps without affecting tokio *(recommended — matches NF-1 and NF-4; no async executor overhead)*
- [ ] B. `tokio::task::spawn_blocking` — reuses tokio's blocking thread pool; slightly less explicit
- [ ] C. Tokio async task with `tokio::time::sleep` — simplest but async timer resolution is typically 1 ms, making the < 5 ms jitter target harder to guarantee

Answer: A.

### D-4 · Command channel between tokio IPC handler and loop thread

How should the async IPC handler send start/stop commands to the synchronous loop thread?

- [x] A. `std::sync::mpsc::channel`; `Sender` stored in `LoopEngine`, `Receiver` owned by loop thread; non-blocking `try_recv()` between events *(recommended — simple, no cross-runtime bridging)*
- [ ] B. `Arc<Mutex<LoopState>>` + `Arc<Condvar>` — allows immediate wakeup from `Stopped`; more complex
- [ ] C. `tokio::sync::mpsc` with `blocking_recv()` in loop thread — mixes async and sync runtimes without benefit

Answer: A.

### D-5 · Timing strategy for < 5 ms jitter

How should `Scheduler::sleep_until` achieve the jitter target (NF-3)?

- [ ] A. `std::thread::sleep` only — simplest; may exceed 5 ms jitter on a loaded system
- [x] B. Hybrid: `thread::sleep(remaining − 500 μs)` then spin on `Instant::now()` — predictable wakeup with bounded CPU overhead *(recommended — achieves < 5 ms reliably on Linux without platform-specific APIs)*
- [ ] C. Platform-specific high-resolution timers (e.g. ALSA `hrtimer` on Linux) — best precision; not portable

Answer: B.

---

## Revision Log

### Cycle 1 — Confidence: 65%
- Reconciled: none (initial generation from PRD)
- Added: D-1 (module layout), D-2 (MidiOutput abstraction), D-3 (thread model), D-4 (command channel), D-5 (timing strategy)

### Cycle 2 — Confidence: 75%
- Reconciled: nothing (all five Open Decisions have user-supplied answers but formal reconciliation is for /create-spec); BarEvent added to Data Model (omission from Cycle 1)
- Added: Q-1 (Waiting-state poll interval), Q-2 (same-tick NoteOff/NoteOn ordering), Q-3 (AC-9 sustained timing test coverage)

### Cycle 3 — Confidence: 75%
- Reconciled: none (Q-1, Q-2, Q-3 unanswered)
- Added: Q-4 (start() idempotency when already Running or Waiting), Q-5 (LoopEngine shutdown and thread join contract)

### Cycle 4 — Confidence: 90%
- Reconciled: Q-1 → Architecture and Loop Thread updated (10 ms poll interval, NF-5); T-34 updated; Q-2 → Architecture bar event list and Loop Thread updated (NoteOff-first sort); T-20 updated; T-37/T-38 added; Q-3 → T-39 added (AC-9 timing integration test); Q-4 → Architecture state machine and LoopEngine component updated (Start no-op); T-15 updated; T-40/T-41 added; Q-5 → Architecture Shutdown section added; LoopEngine and Loop Thread components updated; T-42/T-43 added
- Added: none — confidence 90%, specification is complete

### Cycle 5 — Confidence: 90%
- Reconciled: none (no open questions)
- Added: none — specification is complete at 90%; no further questions needed
