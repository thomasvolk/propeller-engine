# EP-9 · CLI Convenience Commands — Technical Specification

## Overview

This epic adds `project create`, `project modify`, `loop start`, and `loop stop` subcommands to the existing `propeller` binary. The new commands communicate with the running daemon over the existing Unix-domain socket protocol (EP-4), using the same socket-path resolution logic already in `src/socket_path.rs`. A shared `send_command` helper is extracted into a new `src/client.rs` module, implemented with synchronous `std::os::unix::net::UnixStream` — no tokio runtime is needed in the client module.

**Confidence Level:** 93% — All PRD requirements map to concrete tasks, the architecture is fully specified, and no open decisions remain. Minor remaining uncertainty is whether T-8 (`read_project_input` from stdin) can be exercised as a unit test without process-level stdin redirection; in practice this is handled by the integration tests T-11 and T-14.

---

## Architecture Overview

The `propeller` binary (`src/main.rs`) already handles `start`, `stop`, and `status` via a `clap`-derived `Commands` enum. EP-9 extends this enum with three new top-level subcommands, each with nested sub-subcommands:

- `Commands::Project(ProjectCommand)` — `Create { filename: Option<PathBuf> }` and `Modify { filename: Option<PathBuf> }`
- `Commands::Loop(LoopCommand)` — `Start` and `Stop`
- `Commands::Midi(MidiCommand)` — `Ports`

The `Midi(MidiCommand::Ports)` handler calls `midi_port::list_ports()` directly (already implemented in `src/midi_port.rs`) and requires no daemon connection.

A new `src/client.rs` module provides two functions used by all new command handlers:

- `send_command(sock_path: &Path, cmd: Value) -> Result<Value, ClientError>` — opens a synchronous `UnixStream`, writes `cmd` serialised as a JSON line (`\n`-terminated), reads the response line, and returns the parsed response. Returns `ClientError::Connect` if the socket is unreachable, or `ClientError::Daemon { message }` if the response carries `"status": "error"`. This is a plain synchronous function; no tokio runtime is involved.
- `read_project_input(filename: Option<PathBuf>) -> Result<Value, ClientError>` — reads from the named file or from stdin until EOF, parses as JSON, and returns the parsed `Value`. It does not add the `"command"` field; callers insert it before calling `send_command`.

Command handlers in `main.rs` call these two functions and map errors to `eprintln!` + `process::exit(1)`. Because `send_command` is synchronous, handlers need no `Runtime::block_on` wrapper.

`socket_path::resolve()` is reused unchanged from `src/socket_path.rs`.

---

## Components

### `src/client.rs` (new)

Provides `send_command` and `read_project_input`. Contains no business logic — only I/O and JSON handling. Uses `std::os::unix::net::UnixStream` and `std::io::{BufRead, Write}`. Unit tests use `std::os::unix::net::UnixListener` with a background thread acting as the mock server, and `#[test]` (not `#[tokio::test]`).

`ClientError` is a non-public enum with three variants:

```
ClientError::Connect(io::Error)         — socket unreachable
ClientError::Daemon { message: String } — daemon returned "status": "error"
ClientError::Input(String)              — file/stdin read or JSON parse failure
```

### `src/main.rs` (extended)

Adds `Project(ProjectCommand)`, `Loop(LoopCommand)`, and `Midi(MidiCommand)` to the `Commands` enum. Five new handler functions: `cmd_project_create`, `cmd_project_modify`, `cmd_loop_start`, `cmd_loop_stop`, `cmd_midi_ports`. The four daemon-communicating handlers follow the same pattern: resolve socket path → call client functions → `eprintln!` + `exit(1)` on error → silent exit 0 on success. `cmd_midi_ports` calls `midi_port::list_ports()`, prints each port name, and exits 0 — no socket interaction required.

### `src/socket_path.rs` (unchanged)

Reused as-is. `socket_path::resolve()` already reads `PROPELLER_SOCK` and defaults to `/tmp/propeller.sock`.

---

## Data Model

| Type | Fields | Notes |
|------|--------|-------|
| `ProjectCommand` | `Create { filename: Option<PathBuf> }`, `Modify { filename: Option<PathBuf> }` | Clap subcommand enum |
| `LoopCommand` | `Start`, `Stop` | Clap subcommand enum |
| `MidiCommand` | `Ports` | Clap subcommand enum; no daemon connection |
| `ClientError` | `Connect(io::Error)`, `Daemon { message: String }`, `Input(String)` | Non-public; mapped to stderr + exit in handlers |
| project input | `serde_json::Value` with `header` and `tracks` fields | Parsed from file or stdin; `"command"` field inserted by handler before dispatch |
| protocol command | `serde_json::Value` with `"command"` and payload fields | e.g. `{"command":"create-project","header":…,"tracks":…}` |
| protocol response | `serde_json::Value` with `"status"` field | `"ok"` → silent exit 0; `"error"` → stderr + exit 1 |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID | Task | Type | PRD ref | Depends on |
|----|------|------|---------|------------|
| T-1 | Test: `send_command` — spawn a thread-based `UnixListener` that echoes `{"status":"ok"}\n`; verify `send_command` writes the correct JSON line and returns `Ok(Value)` | test | F-7, F-11, AC-9 | — |
| T-2 | Impl: `send_command(sock_path: &Path, cmd: Value) -> Result<Value, ClientError>` in `src/client.rs` using `std::os::unix::net::UnixStream` | impl | F-7, F-8, F-11 | T-1 |
| T-3 | Test: `send_command` returns `ClientError::Connect` when no socket exists at the given path | test | F-8, AC-5 | T-1 |
| T-4 | Impl: connection error branch in `send_command` | impl | F-8 | T-3, T-2 |
| T-5 | Test: `send_command` returns `ClientError::Daemon { message }` when mock server replies with `{"status":"error","message":"…"}` | test | AC-6 | T-1 |
| T-6 | Impl: daemon error response branch in `send_command` | impl | F-8 | T-5, T-2 |
| T-7 | Test: `read_project_input(Some(path))` reads a temp file containing `{"header":…,"tracks":[]}` and returns the parsed `Value` | test | F-3, F-9 | — |
| T-8 | Test: `read_project_input(None)` with stdin redirected to a temp file returns the parsed `Value` (integration-level; covered by T-11/T-14 if unit test is impractical) | test | F-4 | — |
| T-9 | Impl: `read_project_input(filename: Option<PathBuf>) -> Result<Value, ClientError>` — file branch uses `std::fs::read_to_string`, stdin branch uses `std::io::stdin().lock()` | impl | F-3, F-4, F-9 | T-7, T-8 |
| T-10 | Test (integration): `propeller project create <file>` with a running daemon — daemon receives `{"command":"create-project","header":…,"tracks":…}`, process exits 0, no stdout/stderr | test | F-2, F-9, AC-1, AC-8, AC-9 | — |
| T-11 | Test (integration): `propeller project create` with project JSON on stdin — daemon receives `create-project` command, process exits 0 | test | F-4, AC-2 | — |
| T-12 | Impl: `Project(ProjectCommand::Create)` arm in `main.rs` — calls `read_project_input`, inserts `"command":"create-project"`, calls `send_command` | impl | F-2 | T-10, T-11, T-9, T-2 |
| T-13 | Test (integration): `propeller project modify <file>` — daemon receives `{"command":"modify-project","header":…,"tracks":…}`, process exits 0 | test | F-12, AC-10, AC-8 | — |
| T-14 | Test (integration): `propeller project modify` with stdin | test | F-4, AC-11 | — |
| T-15 | Impl: `Project(ProjectCommand::Modify)` arm — inserts `"command":"modify-project"` instead | impl | F-12 | T-13, T-14, T-9, T-2 |
| T-16 | Test (integration): `propeller loop start` — daemon receives `{"command":"loop-start"}`, process exits 0 | test | F-5, AC-3 | — |
| T-17 | Impl: `Loop(LoopCommand::Start)` arm — calls `send_command` with `json!({"command":"loop-start"})` | impl | F-5 | T-16, T-2 |
| T-18 | Test (integration): `propeller loop stop` — daemon receives `{"command":"loop-stop"}`, process exits 0 | test | F-6, AC-4 | — |
| T-19 | Impl: `Loop(LoopCommand::Stop)` arm — calls `send_command` with `json!({"command":"loop-stop"})` | impl | F-6 | T-18, T-2 |
| T-20 | Test (integration): any client command with `PROPELLER_SOCK` set to a custom path — CLI connects to that path, not `/tmp/propeller.sock` | test | F-7, AC-7 | — |
| T-21 | Impl: all four handlers call `socket_path::resolve()` to obtain the socket path | impl | F-7 | T-20, T-2 |
| T-22 | Test (integration): client command when daemon not running — stderr contains a human-readable message, exit code is non-zero | test | F-8, AC-5 | — |
| T-23 | Impl: `ClientError::Connect` mapped to `eprintln!("propeller: …")` + `exit(1)` in all handlers | impl | F-8 | T-22, T-4 |
| T-24 | Test (integration): daemon returns `{"status":"error","message":"…"}` — CLI writes message to stderr, exits non-zero | test | AC-6 | — |
| T-25 | Impl: `ClientError::Daemon` mapped to `eprintln!("propeller: …")` + `exit(1)` in all handlers | impl | AC-6 | T-24, T-6 |
| T-26 | Test: `propeller midi ports` prints each port name on its own line and exits 0; no socket required (use `assert!(output.status.success())` with no daemon running) | test | F-13, AC-12 | — |
| T-27 | Impl: `Midi(MidiCommand::Ports)` arm in `main.rs` — calls `midi_port::list_ports()`, prints `port.name` for each entry, exits 0 | impl | F-13, AC-12 | T-26 |

---

## Open Decisions

No open decisions. The specification is complete.

---

## Revision Log

### Cycle 1 — Confidence: 85%
- Reconciled: nothing (spec created from PRD and codebase analysis)
- Added: D-1 (sync vs async socket I/O in `send_command`)

### Cycle 2 — Confidence: 93%
- Reconciled: D-1 → B (synchronous `std::os::unix::net::UnixStream`); architecture overview, `src/client.rs` component description, and T-1/T-2/T-8/T-9 task descriptions updated to reflect sync I/O and thread-based test helpers
- Added: none — no open decisions remain, specification is complete

### Cycle 3 — Confidence: 93%
- Added: `Commands::Midi(MidiCommand)` with `Ports` variant; `cmd_midi_ports` handler; T-26 (test) and T-27 (impl) for `propeller midi ports` — moved from EP-6
