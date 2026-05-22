# EP-8 · Real MIDI Output — Technical Specification

## Overview

This epic replaces the in-memory `MockMidiOutput` in the production daemon path with a real MIDI driver backed by the `midir` crate. A new `src/midi_port.rs` module provides port discovery, exact-name port selection, and a `MidiPortOutput` struct that implements the existing `MidiOutput` trait. The target port is chosen at daemon startup via `PROPELLER_MIDI_PORT`; if the variable is absent the daemon opens a virtual port named `propeller`. A `list-midi-ports` runtime command lets performers enumerate available ports by index and name without leaving the propeller interface.

**Confidence Level:** 92% — All D-N decisions reconciled, all Q-N questions reconciled, TDD ordering complete, all F-x and AC-x covered. Minor gap: error handling when `open_virtual()` itself fails (e.g. no MIDI subsystem at all) is not explicitly specified.

---

## Architecture Overview

`MidiOutput` is already an abstract trait (`src/loop_engine/midi.rs`); only the production wiring needs to change. The new module `src/midi_port.rs` owns all midir interaction and exposes `list_ports()`, `open_port()`, `open_virtual()`, and `MidiPortOutput`. No changes are made to the trait itself (F-6).

**Port selection (D-2 A — `main.rs`)**: In `main.rs`, before calling `daemon::run()`, the binary reads `PROPELLER_MIDI_PORT`. If set, it calls `midi_port::open_port(name)`; on `MidiPortError::NotFound` it prints a human-readable error to stderr (including the requested name and available port names) and calls `std::process::exit(1)`. If the variable is unset, it calls `midi_port::open_virtual()` to open a virtual port named `propeller` (D-1 A). The resulting `Box<dyn MidiOutput>` is passed into `daemon::run()`, which passes it directly to `LoopEngine::new()` (F-7). The daemon module has no knowledge of midir.

**Port name matching** (F-8): `find_port_by_name(ports, name)` is a pure function that performs a case-sensitive exact string comparison. Prefix matches are explicitly rejected (AC-7).

**MIDI byte encoding**: Three private helper functions (`note_on_bytes`, `note_off_bytes`, `program_change_bytes`) build the raw MIDI byte slices. propeller channels are 1-indexed; MIDI bytes use 0-indexed status nibbles, so each function subtracts 1 from the channel number.

**`list-midi-ports` command**: The IPC handler for `ListMidiPorts` calls `list_ports()` as a free function. No shared state is needed. The response embeds a `Vec<MidiPortInfo>` serialised as a JSON array under the `"ports"` key (F-5, AC-5). If `midir::MidiOutput::new()` fails at list time, `list_ports()` returns an empty `Vec` and the response is `{"status":"ok","ports":[]}` (Q-2 A).

**Thread safety**: `midir::MidiOutputConnection` is `Send`, satisfying the `MidiOutput: Send + 'static` bound. The loop engine thread calls `send()` directly on the connection; no additional synchronisation is required (NF-1).

**Testing `MidiPortOutput`** (Q-1 A): Tests for `MidiPortOutput`, `open_port()`, and `open_virtual()` are `#[ignore]`-tagged integration tests that open a virtual MIDI output port and connect a `midir::MidiInput` to the same port to observe received bytes. These tests are skipped in CI unless a MIDI subsystem is present and can be run locally with `cargo test -- --ignored`.

**Module layout additions:**

- `src/midi_port.rs` — `list_ports()`, `find_port_by_name()`, byte helpers, `MidiPortOutput`, `MidiPortError`, `MidiPortInfo`
- `src/loop_engine/midi.rs` — no changes
- `src/ipc/types.rs` — add `ListMidiPorts` variant to `Command`
- `src/ipc/handler.rs` — add `ListMidiPorts` arm to dispatch
- `src/main.rs` — port selection before daemon fork; passes `Box<dyn MidiOutput>` into `daemon::run()`
- `Cargo.toml` — add `midir` dependency with `virtual` feature

---

## Components

### MidiPort (`src/midi_port.rs`)

Owns all midir interaction. Exposes:

- `pub fn list_ports() -> Vec<MidiPortInfo>` — creates a temporary `midir::MidiOutput`, enumerates its ports, and returns them as `MidiPortInfo` values in system enumeration order. The temporary output handle is dropped after listing. If `midir::MidiOutput::new()` fails, returns an empty `Vec` without error (Q-2 A).
- `pub fn find_port_by_name(names: &[String], target: &str) -> Option<usize>` — pure function; returns the index of the first entry equal to `target` under case-sensitive exact comparison. Returns `None` if no entry matches (F-8, AC-7).
- `pub fn open_port(name: &str) -> Result<MidiPortOutput, MidiPortError>` — enumerates ports, calls `find_port_by_name`, opens the matching port via `midir::MidiOutput::connect()`, wraps the result in `MidiPortOutput`. Returns `MidiPortError::NotFound` with the requested name and all available names when no match is found.
- `pub fn open_virtual() -> Result<MidiPortOutput, MidiPortError>` — calls `midir::MidiOutput::create_virtual("propeller")` and wraps the result. Called when `PROPELLER_MIDI_PORT` is not set (D-1 A).

### MidiPortOutput (`src/midi_port.rs`)

Tuple struct `MidiPortOutput(midir::MidiOutputConnection)`. Implements `MidiOutput`:

- `note_on` — calls `note_on_bytes(channel, pitch, velocity)` and forwards the slice to `self.0.send()`
- `note_off` — calls `note_off_bytes(channel, pitch)` and forwards
- `program_change` — calls `program_change_bytes(channel, program)` and forwards

### IPC handler extension (`src/ipc/handler.rs`)

Adds a `Command::ListMidiPorts` arm. The handler calls `midi_port::list_ports()`, wraps the result in a `json!({"status": "ok", "ports": ports})` response. When `list_ports()` returns an empty `Vec` due to midir init failure, the response is `{"status":"ok","ports":[]}` (F-5, AC-5, Q-2 A).

### Startup wiring (`src/main.rs`)

Port selection is performed in `main.rs` before `daemon::run()` is called. Reads `PROPELLER_MIDI_PORT`; if set, calls `open_port(name)` and on `MidiPortError::NotFound` prints a descriptive error to stderr and exits. If unset, calls `open_virtual()`. Passes the resulting `Box<dyn MidiOutput>` into `daemon::run()` (D-2 A).

---

## Data Model

| Type | Fields | Notes |
|------|--------|-------|
| `MidiPortInfo` | `index: usize`, `name: String` | `#[derive(Serialize, Clone)]`; serialised as `{"index": N, "name": "..."}` (AC-5) |
| `MidiPortOutput` | `(midir::MidiOutputConnection)` | Implements `MidiOutput`; `Send + 'static` |
| `MidiPortError` | `NotFound { requested: String, available: Vec<String> }`, `ConnectionFailed(midir::ConnectError<midir::MidiOutput>)`, `InitFailed(midir::InitError)` | `NotFound` carries the information printed on startup failure (F-4, AC-4) |
| `Command::ListMidiPorts` | — | New unit variant; serde tag `"list-midi-ports"` (F-5, AC-5) |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID | Task | Type | PRD ref | Depends on |
|----|------|------|---------|------------|
| T-1 | Add `midir` to `Cargo.toml` with `virtual` feature enabled | impl | NF-2 | — |
| T-2 | Test: `find_port_by_name(&["Surge XT", "Surge"], "Surge XT")` → `Some(0)`; `find_port_by_name(&["Surge", "Surge XT"], "Surge XT")` → `Some(1)` | test | F-8, AC-7 | — |
| T-3 | Test: `find_port_by_name(&["Surge XT"], "Surge")` → `None` (prefix alone does not match) | test | F-8, AC-7 | — |
| T-4 | Test: `find_port_by_name(&[], "anything")` → `None` | test | F-8 | — |
| T-5 | Impl: `find_port_by_name(names: &[String], target: &str) -> Option<usize>` — case-sensitive exact match | impl | F-8 | T-2, T-3, T-4 |
| T-6 | Test: `note_on_bytes(1, 60, 80)` → `[0x90, 60, 80]`; `note_on_bytes(16, 60, 80)` → `[0x9F, 60, 80]` | test | F-1, AC-1 | — |
| T-7 | Test: `note_off_bytes(1, 60)` → `[0x80, 60, 0]`; `note_off_bytes(16, 60)` → `[0x8F, 60, 0]` | test | F-1, AC-1 | — |
| T-8 | Test: `program_change_bytes(1, 42)` → `[0xC0, 42]`; `program_change_bytes(2, 0)` → `[0xC1, 0]` | test | F-1, AC-2 | — |
| T-9 | Impl: `note_on_bytes(ch: u8, pitch: u8, vel: u8) -> [u8; 3]`, `note_off_bytes(ch: u8, pitch: u8) -> [u8; 3]`, `program_change_bytes(ch: u8, prog: u8) -> [u8; 2]` in `src/midi_port.rs`; status nibble uses `ch - 1` | impl | F-1 | T-6, T-7, T-8 |
| T-10 | Test: `MidiPortInfo { index: 0, name: "Surge XT".into() }` serialises to `{"index":0,"name":"Surge XT"}` | test | F-5, AC-5 | — |
| T-11 | Impl: `MidiPortInfo` struct with `#[derive(Serialize, Clone)]` in `src/midi_port.rs` | impl | F-5 | T-10 |
| T-12 | Test: `{"command":"list-midi-ports"}` deserialises to `Command::ListMidiPorts` | test | F-5, AC-5 | — |
| T-13 | Impl: add `ListMidiPorts` unit variant to `Command` enum in `src/ipc/types.rs` | impl | F-5 | T-12 |
| T-14 | Impl: `list_ports() -> Vec<MidiPortInfo>` in `src/midi_port.rs` — creates `midir::MidiOutput::new("propeller-list")`; on init failure returns empty `Vec` (Q-2 A); on success enumerates ports and collects into `Vec<MidiPortInfo>` with index and name | impl | F-5 | T-11 |
| T-15 | Test: integration — send `{"command":"list-midi-ports"}` over a live Unix socket → response is `{"status":"ok","ports":[...]}` where `ports` is a JSON array | test | F-5, AC-5 | — |
| T-16 | Impl: `ListMidiPorts` handler arm in `src/ipc/handler.rs` — calls `midi_port::list_ports()`, returns `json!({"status": "ok", "ports": ports})` | impl | F-5 | T-13, T-14, T-15 |
| T-17 | Test: `#[ignore]` integration — open virtual MIDI output "propeller-test"; connect `midir::MidiInput` to "propeller-test"; construct `MidiPortOutput` from the connection; call `note_on(1, 60, 80)`, `note_off(1, 60)`, `program_change(1, 42)`; assert received byte sequences `[0x90, 60, 80]`, `[0x80, 60, 0]`, `[0xC0, 42]` (Q-1 A) | test | F-1, AC-1, AC-2 | — |
| T-18 | Impl: `MidiPortOutput(midir::MidiOutputConnection)` struct and `MidiOutput` impl — `note_on`/`note_off`/`program_change` call the byte helpers then `self.0.send()` | impl | F-1, F-6 | T-1, T-9, T-17 |
| T-19 | Test: `open_port("__propeller_nonexistent__")` returns `Err(MidiPortError::NotFound { requested, available })` where `requested == "__propeller_nonexistent__"` | test | F-4, F-8, AC-4 | — |
| T-20 | Impl: `MidiPortError` enum; `open_port(name: &str) -> Result<MidiPortOutput, MidiPortError>` — enumerate ports, call `find_port_by_name`, open with `midir::MidiOutput::connect()`; return `NotFound` with `available` names when no match | impl | F-1, F-4, F-8 | T-5, T-18, T-19 |
| T-21 | Test: `#[ignore]` integration — `open_virtual()` returns `Ok(MidiPortOutput)`; the virtual port named `"propeller"` is visible in the list returned by a subsequent `list_ports()` call (Q-1 A) | test | F-3, AC-3 | — |
| T-22 | Impl: `open_virtual() -> Result<MidiPortOutput, MidiPortError>` — calls `midir::MidiOutput::create_virtual("propeller")`, wraps the result | impl | F-3 | T-18, T-21 |
| T-23 | Test: integration — start binary with `PROPELLER_MIDI_PORT=nonexistent_xyz` → process exits with non-zero code and stderr contains the unknown port name | test | F-4, AC-4 | — |
| T-24 | Impl: port selection in `src/main.rs` before daemon fork — read `PROPELLER_MIDI_PORT`; if set call `open_port()`; on `MidiPortError::NotFound` print error with requested name and `available` list to stderr then `std::process::exit(1)`; if unset call `open_virtual()` and forward result as `Box<dyn MidiOutput>` to `daemon::run()` (D-1 A, D-2 A) | impl | F-2, F-3, F-4 | T-20, T-22, T-23 |
| T-25 | Impl: update `daemon::run()` signature to accept `midi_output: Box<dyn MidiOutput>` and forward to `LoopEngine::new()` instead of constructing `MockMidiOutput` internally | impl | F-7, AC-6 | T-24 |

---

## Open Decisions

All decisions resolved and reconciled.

---

## Revision Log

### Cycle 1 — Confidence: 70%
- Reconciled: nothing (initial generation from PRD)
- Added: D-1 (default port when env var absent, from PRD Q-1), D-2 (startup location for port selection); tasks T-1 through T-22 covering all F-x and AC-x

### Cycle 2 — Confidence: 70%
- Reconciled: nothing (no answered Q-N questions; D-1 and D-2 are checked but await /create-spec reconciliation)
- Added: Q-1 (test strategy for MidiPortOutput — T-17/T-18/T-19 lack test tasks), Q-2 (list_ports() error handling on midir init failure)

### Cycle 3 — Confidence: 80%
- Reconciled: Q-1 A → added T-17 (#[ignore] MidiPortOutput loopback test), T-19 (open_port NotFound test), T-21 (#[ignore] open_virtual test); renumbered old T-17–T-22 to T-18–T-25; architecture updated (testing strategy paragraph added); Q-2 A → list_ports() returns empty Vec on init failure; T-14 and MidiPort component updated
- Added: nothing — no remaining Q-N questions; run /create-spec EP-8 to reconcile D-1 and D-2

### Cycle 4 — Confidence: 92%
- Reconciled: D-1 A → virtual port `propeller` is the definitive default; `open_virtual()` description updated, T-24 updated, overview updated; D-2 A → port selection in `main.rs` before daemon fork; architecture overview updated, module layout fixed (`src/main.rs` not "or src/daemon.rs"), new Startup wiring component added, T-24 updated; both D-1 and D-2 removed from Open Decisions
- Added: nothing — specification complete; minor gap noted (open_virtual() failure path not explicitly specified)
