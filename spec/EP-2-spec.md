# EP-2 · Project Model — Technical Specification

## Overview

This epic defines the core domain model for the propeller-engine: the `Project` type and all its constituents (header, tracks, bars, notes), a synchronous validator, and a `ProjectStore` that holds the active project in memory and stages pending updates for atomic swap at bar boundaries. No IPC wiring or persistence is introduced here — those belong to EP-4 and are explicitly out of scope per NF-5.

**Confidence Level:** 93% — All questions and decisions are resolved; every PRD item has a task and TDD ordering is maintained; residual uncertainty is only in integration-test infrastructure details (referencing the domain module from `cargo test` within the full crate structure).

---

## Architecture Overview

EP-2 is a pure domain layer addition to the daemon process. It introduces no new IPC messages, no file I/O, and no async work. All types are defined in a `domain` module that is independent of any serialisation format (NF-2).

**Module layout** (D-1): The domain module lives at `src/domain/` with four files:

- `src/domain/mod.rs` — re-exports all public types
- `src/domain/project.rs` — `Project`, `Header`, `TimeSignature`, `Track`, `Bar`, `Note`, `NoteEvent`, `PPQN`
- `src/domain/validation.rs` — `validate()`, `ValidationError`
- `src/domain/store.rs` — `ProjectStore`

The central invariant is the two-slot store: `ProjectStore` holds an `active` project (currently playing) and a `pending` project (queued for next bar boundary). The runtime interface (EP-4) calls `set_pending()` to queue a new or updated project. The loop engine (EP-3) calls `commit_pending()` exactly once at each bar boundary to atomically promote `pending` into `active`. This two-slot design means the active project is never torn mid-bar, satisfying NF-3.

**Concurrency wrapper** (D-2): The daemon creates one `Arc<std::sync::RwLock<ProjectStore>>` at startup and clones the `Arc` to share with EP-3 (loop thread) and EP-4 (async IPC handler). `std::sync::RwLock` is chosen over `tokio::sync::RwLock` because all `ProjectStore` operations are synchronous and short — no guard is ever held across an `.await` point. Multiple readers (the loop reading `active` on every tick) never block each other; a writer (EP-4 calling `set_pending()`) blocks briefly.

Validation is a pure synchronous function `validate(project: &Project) -> Result<(), ValidationError>`. It is called inside `set_pending()` before anything is stored; a rejected project leaves the active project untouched (NF-1). Because validation is a pure `&Project` read, it cannot partially modify state.

All tick arithmetic uses integer types only (NF-4). The PPQN constant is 480. Bar length in ticks for a time signature N/D is `N × (480 × 4 / D)`. Note durations are `u32` ticks.

---

## Components

### Domain Types (`src/domain/project.rs`)

Owns the `Project`, `Header`, `TimeSignature`, `Track`, `Bar`, `Note`, and `NoteEvent` types, plus the `PPQN` constant. These are first-class Rust types in the domain layer — no `serde` derives here. Provides `TimeSignature::bar_ticks()` and `Project::cycle_length()` as computed helpers.

- `TimeSignature::bar_ticks() -> u32` — returns `numerator × (480 × 4 / denominator)`.
- `Project::cycle_length() -> usize` — returns the maximum bar count across all tracks; returns 0 for a zero-track project (F-22).
- `Track::bar_at(cycle_pos: usize) -> &Bar` — returns `&self.bars[cycle_pos % self.bars.len()]` for polymetric looping (F-22). Precondition: `self.bars` is non-empty; the validator guarantees this for all accepted projects (Q-1: A).

### Validator (`src/domain/validation.rs`)

A free function `validate(project: &Project) -> Result<(), ValidationError>` that runs all domain constraints synchronously and atomically (NF-1). It never mutates state. Returns `Ok(())` if all constraints pass; otherwise returns the first `ValidationError` found.

Constraints checked (in order):
1. `header.bpm` in 20–300 (F-21)
2. `header.time_signature.numerator` ≥ 1 (F-9)
3. `header.time_signature.denominator` ∈ {2, 4, 8, 16} (F-10)
4. For each track: `channel` in 1–16, `instrument` in 0–127 (F-3, F-6)
5. For each track: `bars` is non-empty — `EmptyTrackBars { track }` (Q-1: A)
6. For each note in each bar: `duration_ticks` > 0 and ≤ `bar_ticks` (F-7)

Zero-track projects pass validation (F-19). Under-filled bars (note sum < bar length) pass validation (F-17).

### ProjectStore (`src/domain/store.rs`)

Holds `active: Option<Project>` and `pending: Option<Project>`. Provides three operations:

- `set_pending(project: Project) -> Result<(), ValidationError>` — validates the project; on success stores it as `pending` (replacing any previous pending); on failure leaves both slots unchanged (NF-1).
- `commit_pending() -> bool` — moves `pending` into `active` and clears `pending`; returns `true` if a swap occurred, `false` if there was no pending project (F-13, NF-3).
- `active() -> Option<&Project>` — returns a shared reference to the active project.

At the daemon level, `ProjectStore` is wrapped as `Arc<std::sync::RwLock<ProjectStore>>` (D-2). Guards acquired from this lock must not be held across `.await` points. All operations on `ProjectStore` are synchronous and complete in bounded time, satisfying this constraint.

The store is memory-only; it is not serialised or written to disk (NF-5).

---

## Data Model

| Type | Fields | Notes |
|------|--------|-------|
| `PPQN` | `u32` constant = `480` | Pulses per quarter note; smallest schedulable time unit (F-16, F-20) |
| `Project` | `header: Header`, `tracks: Vec<Track>` | Root domain type; zero-length `tracks` is valid (F-1, F-19) |
| `Header` | `bpm: u32`, `time_signature: TimeSignature` | `bpm` is integer 20–300; `u32` enforces non-negative and non-fractional by construction (F-2, F-21, AC-14) |
| `TimeSignature` | `numerator: u32`, `denominator: u32` | `numerator` ≥ 1; `denominator` ∈ {2, 4, 8, 16}; `bar_ticks()` method returns `numerator × (480 × 4 / denominator)` (F-5, F-9, F-10, F-20) |
| `Track` | `name: String`, `channel: u8`, `instrument: u8`, `bars: Vec<Bar>` | `channel`: 1–16; `instrument`: 0–127; `bars` must be non-empty for any accepted project (Q-1: A); no mute or solo field (F-3, F-18) |
| `Bar` | `notes: Vec<Note>` | May be empty or partial; sum of note durations may be less than `bar_ticks` — remainder is implicit silence (F-4, F-17) |
| `Note` | `event: NoteEvent`, `duration_ticks: u32` | `duration_ticks`: 1..=`bar_ticks`; enforced by validator (F-7) |
| `NoteEvent` | `Note { pitch: u8, velocity: u8 }` \| `Rest` | `pitch` and `velocity`: 0–127; `Rest` produces no MIDI note-on (F-6, F-8) |
| `ValidationError` | enum | Variants: `BpmOutOfRange { actual: u32 }`, `InvalidTimeSignatureNumerator`, `InvalidTimeSignatureDenominator { actual: u32 }`, `InvalidMidiChannel { track: usize, actual: u8 }`, `InvalidMidiInstrument { track: usize, actual: u8 }`, `EmptyTrackBars { track: usize }`, `NoteDurationZero { track: usize, bar: usize, note: usize }`, `NoteDurationExceedsBar { track: usize, bar: usize, note: usize, duration: u32, bar_ticks: u32 }` (F-15, Q-1) |
| `ProjectStore` | `active: Option<Project>`, `pending: Option<Project>` | Two-slot store; wrapped as `Arc<std::sync::RwLock<ProjectStore>>` at daemon level (D-2) (F-13, F-14, NF-3, NF-5) |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID | Task | Type | PRD ref | Depends on |
|----|------|------|---------|------------|
| T-1 | Write test: construct Project, Header, TimeSignature, Track, Bar, Note (pitch+velocity), and Note (rest) with valid fields; assert all fields stored correctly; assert PPQN = 480 | test | F-1, F-2, F-3, F-4, F-6, F-8, F-16 | — |
| T-2 | Impl: define Project, Header, TimeSignature, Track, Bar, Note, NoteEvent types in `src/domain/project.rs`; declare `pub const PPQN: u32 = 480`; re-export from `src/domain/mod.rs` | impl | F-1, F-2, F-3, F-4, F-6, F-8, F-16 | T-1 |
| T-3 | Write test: `TimeSignature::bar_ticks()` returns 1920 for 4/4, 1440 for 3/4, 1440 for 6/8, 480 for 1/4, 240 for 1/8 | test | F-5, F-20, AC-11 | — |
| T-4 | Impl: `TimeSignature::bar_ticks() -> u32` using formula `numerator * (PPQN * 4 / denominator)` | impl | F-5, F-20 | T-2, T-3 |
| T-5 | Write test: `validate()` returns `Ok` for a well-formed 4/4 project with BPM 120, one track with one bar, and one quarter-note | test | F-15, AC-1 | — |
| T-6 | Write test: `validate()` returns `Err(BpmOutOfRange)` for BPM 19 and BPM 301 | test | F-21, AC-12 | — |
| T-7 | Write test: `validate()` returns `Err(InvalidTimeSignatureDenominator)` for denominator 3 and denominator 5 | test | F-10, AC-4 | — |
| T-8 | Write test: `validate()` returns `Err(NoteDurationZero)` for a note with `duration_ticks = 0` | test | F-7 | — |
| T-9 | Write test: `validate()` returns `Err(NoteDurationExceedsBar)` for a note whose `duration_ticks` exceeds the 4/4 bar length of 1920 | test | F-7, AC-3, AC-11 | — |
| T-10 | Write test: `validate()` returns `Ok` for a note with `duration_ticks` exactly equal to `bar_ticks` (1920 for 4/4) | test | F-7, AC-11 | — |
| T-11 | Write test: `validate()` returns `Err(InvalidMidiChannel)` for track with channel 0 and channel 17 | test | F-3, AC-7 | — |
| T-12 | Write test: `validate()` returns `Err(InvalidMidiInstrument)` for track with instrument 128 | test | F-6, AC-7 | — |
| T-13 | Write test: `validate()` returns `Ok` for a project with an empty `tracks` vec | test | F-19, AC-10 | — |
| T-14 | Write test: `validate()` returns `Ok` when note tick-sum in a bar is less than `bar_ticks` | test | F-17, AC-9 | — |
| T-15 | Write test: `validate()` returns `Err(InvalidTimeSignatureNumerator)` for numerator 0 | test | F-9 | — |
| T-29 | Write test: `validate()` returns `Err(EmptyTrackBars)` for a track with zero bars | test | F-3, Q-1 | — |
| T-16 | Impl: `validate(project: &Project) -> Result<(), ValidationError>` in `src/domain/validation.rs`; define `ValidationError` enum including `EmptyTrackBars`; validate BPM, time signature, channel, instrument, empty-bars, and note durations in that order | impl | F-7, F-9, F-10, F-15, F-17, F-19, F-21, NF-1, Q-1 | T-2, T-4, T-5, T-6, T-7, T-8, T-9, T-10, T-11, T-12, T-13, T-14, T-15, T-29 |
| T-17 | Write test: new `ProjectStore::new()` has `active() = None` | test | F-11, NF-5 | — |
| T-18 | Impl: `ProjectStore` struct in `src/domain/store.rs` with `active: Option<Project>` and `pending: Option<Project>`; `ProjectStore::new()` constructor; `active() -> Option<&Project>` accessor | impl | F-11, F-14, NF-5 | T-17 |
| T-19 | Write test: `set_pending()` with a valid project returns `Ok`; `active()` still returns `None` | test | F-11, AC-1 | — |
| T-20 | Write test: `set_pending()` with an invalid project (BPM 0) returns `Err`; `active()` is unchanged | test | F-15, NF-1 | — |
| T-21 | Impl: `ProjectStore::set_pending(project: Project) -> Result<(), ValidationError>` — calls `validate()` before storing; on failure leaves both slots unchanged | impl | F-11, F-12, F-15, NF-1 | T-16, T-18, T-19, T-20 |
| T-22 | Write test: `commit_pending()` on a store with a pending project moves it to `active`, clears `pending`, returns `true` | test | F-13, AC-2, NF-3 | — |
| T-23 | Write test: `commit_pending()` on a store with no pending is a no-op and returns `false` | test | F-13 | — |
| T-24 | Impl: `ProjectStore::commit_pending() -> bool` — replaces `active` with `pending` and clears `pending`; returns `true` if a swap occurred | impl | F-13, NF-3 | T-18, T-22, T-23 |
| T-25 | Write test: calling `set_pending()` twice retains only the second (most recent) project as pending | test | F-14, AC-5 | — |
| T-26 | Write test: `Project::cycle_length()` returns 4 for a project with tracks of 1, 2, and 4 bars; returns 0 for a zero-track project | test | F-22, AC-13 | — |
| T-27 | Write test: `Track::bar_at(cycle_pos)` returns bars[0], bars[1], bars[0], bars[1] for a 2-bar track at positions 0–3 | test | F-22, AC-13 | — |
| T-28 | Impl: `Project::cycle_length() -> usize` (max bar count across tracks, 0 if no tracks); `Track::bar_at(cycle_pos: usize) -> &Bar` (`&self.bars[cycle_pos % self.bars.len()]`) | impl | F-22 | T-2, T-26, T-27 |

---

## Open Questions

No open questions.

---

## Open Decisions

All decisions resolved and reconciled into the specification.

- **D-1 (src/domain/ layout)** — reconciled in cycle 2: four files (`mod.rs`, `project.rs`, `validation.rs`, `store.rs`).
- **D-2 (Arc<std::sync::RwLock<ProjectStore>>)** — reconciled in cycle 2: `std::sync::RwLock` wrapper; guards must not be held across `.await` points.

---

## Revision Log

### Cycle 1 — Confidence: 72%
- Reconciled: none (initial generation from PRD)
- Added: D-1 (domain module layout), D-2 (ProjectStore concurrency strategy)

### Cycle 2 — Confidence: 88%
- Reconciled: D-1 → Architecture and Components updated (src/domain/ with project.rs, validation.rs, store.rs, mod.rs; T-2 updated to name files); D-2 → Architecture and ProjectStore component updated (Arc<std::sync::RwLock<ProjectStore>>, guard constraint documented); data model ProjectStore row updated
- Added: Q-1 (track with zero bars — modulo-zero panic risk in bar_at())

### Cycle 3 — Confidence: 93%
- Reconciled: Q-1 → Validator constraint 5 added (EmptyTrackBars); ValidationError enum updated (EmptyTrackBars variant); Track data model row updated (bars must be non-empty); Track::bar_at() precondition firmed up; T-29 added (test for EmptyTrackBars); T-16 depends-on updated to include T-29
- Added: none — confidence 93%, specification is complete
