# EP-4 · Runtime Interface — Technical Specification

## Overview

This epic implements the runtime command protocol that lets external clients control the propeller-engine without restarting the daemon. The IPC layer runs over the Unix domain socket established by EP-1, extends its minimal `IpcMessage` type into a full `Command` enum tagged by a `"command"` JSON field, and dispatches each incoming command to the appropriate subsystem: the `ProjectStore` (EP-2) for project management, the `LoopEngine` (EP-3) for playback control, and a new `EngineSettings` struct for mode and BPM state. Each connection carries exactly one newline-terminated JSON command and receives exactly one newline-terminated JSON response before the connection is closed.

**Confidence Level:** 92% — All Q-N questions and D-N decisions fully reconciled. Specification is complete.

---

## Architecture Overview

EP-4 is an async layer inserted between the existing socket server (EP-1) and the domain / loop-engine subsystems (EP-2, EP-3). The `tokio::net::UnixListener` accept loop from EP-1 is extended so each accepted connection is handed to a new `connection_handler` function rather than EP-1's stop-only dispatcher.

**Wire protocol** (F-12, F-13, F-15, F-18): Each connection carries exactly one command — an ASCII JSON object terminated by `\n`. The handler reads bytes until the first `\n`, deserialises to `Command`, dispatches to the relevant subsystem, serialises a `Response`, writes it followed by `\n`, then drops the connection. No unsolicited data is ever sent. The `"command"` field is the serde tag discriminant. EP-1's previous `{"type":"stop"}` format is superseded; the daemon shutdown command becomes `{"command":"stop"}`.

**CLI startup mode** (F-21): `propeller start` accepts `--clock` (bool flag). In `main.rs`, the `Start` subcommand carries a `clock: bool` field. Before calling `daemon::run()`, the CLI computes `initial_mode = if clock { EngineMode::Clock } else { EngineMode::Standalone }` and passes it as a parameter. Inside `daemon::run()`, `settings.mode` is set to `initial_mode` immediately after `EngineSettings::new()`; the `--sync-port` wiring block then overwrites it with `EngineMode::Sync` if applicable, preserving the existing precedence rule.

**Blocking start** (F-22): `propeller start` must return only after the socket is connectable. The `daemonize` crate is not usable here because its `start()` makes the original process exit before the socket is bound. Instead, `cmd_start` uses a **self-spawn** approach: it spawns a child process running the hidden `daemon-run` subcommand via `std::process::Command`, then polls `std::os::unix::net::UnixStream::connect(&sock_path)` in a loop with 50 ms sleep intervals until connection succeeds or a 10 s timeout is reached. The child is detached from the terminal by passing `process_group(0)` (which calls `setpgid(0,0)` in the child, creating a new process group) and redirecting all stdio to `/dev/null`. This replaces the `daemonize` crate dependency entirely.

**Blocking stop** (F-23): After `cmd_stop` receives the `{"status":"ok"}` response from the stop command, it polls `!sock_path.exists()` with 50 ms sleep intervals until the socket file is gone or a 10 s timeout is reached. The daemon removes the socket file as the last step of `daemon::run()`, so its absence guarantees clean shutdown.

**Shared state** — four `Arc`-wrapped values are created at daemon startup and cloned into each connection task:

| Value | Type | Purpose |
|-------|------|---------|
| `store` | `Arc<std::sync::RwLock<ProjectStore>>` | Project management (EP-2) |
| `engine` | `Arc<LoopEngine>` | Loop start / stop / state (EP-3) |
| `settings` | `Arc<std::sync::Mutex<EngineSettings>>` | Mode and standalone BPM (D-2, D-4) |
| `shutdown_tx` | `Arc<tokio::sync::Mutex<Option<oneshot::Sender<()>>>>` | Daemon shutdown trigger (EP-1) |

`LoopEngine` exposes `start(&self)` / `stop(&self)` / `state()` via internal `Arc` — it is safe to wrap in `Arc<LoopEngine>` and share across tasks. `std::sync::Mutex` is used for `EngineSettings` (not `tokio::sync::Mutex`) because no guard is ever held across an `.await` point — all accesses are brief reads or writes.

**Command dispatch:**

| Command | Subsystem call |
|---------|---------------|
| `CreateProject` | wire → domain; `store.write().set_pending(project)` |
| `ModifyProject` | wire → domain; `store.write().set_pending(project)` |
| `SetBpm { bpm }` | validate; update `settings.bpm`; if project active also `set_pending(clone_with_bpm)`; BPM change takes effect at next bar boundary (Q-1: A) |
| `SetMode { mode }` | parse `EngineMode`; update `settings.mode` |
| `LoopStart` | `engine.start()` |
| `LoopStop` | `engine.stop()` |
| `Status` | read store, engine, settings → build status payload |
| `Stop` | write `{"status":"ok"}` + flush, then send on `shutdown_tx` → EP-1 shutdown path (Q-2: A) |

**NF-1 (no scheduling delays):** All `ProjectStore` and `EngineSettings` operations are short and synchronous. No lock guard is held across `.await`. The loop thread's timing is unaffected.

**Module layout:**

- `src/ipc/mod.rs` — re-exports; creates and wires shared state at daemon startup
- `src/ipc/server.rs` — `run_ipc_server()`: accept loop, spawns per-connection tasks
- `src/ipc/handler.rs` — `connection_handler()`: reads command, dispatches, writes response
- `src/ipc/types.rs` — `Command`, wire types, `Response`, `EngineSettings`, `EngineMode`, error codes

---

## Components

### IpcServer (`src/ipc/server.rs`)

Replaces EP-1's stop-only socket handler. Accepts connections on the existing `tokio::net::UnixListener`, clones the four shared-state Arcs, and spawns a `tokio::task` per connection running `connection_handler`.

### CommandHandler (`src/ipc/handler.rs`)

Stateless async function. Given a `UnixStream` and the four shared-state Arcs, it reads one newline-terminated JSON command using `tokio::io::BufReader` + `read_line()`, dispatches to the correct subsystem, and writes a response.

- If `read_line()` returns 0 bytes (client disconnected before sending data), the connection is silently dropped — no response is sent (AC-13).
- If JSON deserialization fails, a structured error response is returned with code `"parse_error"` or `"missing_command"` / `"unknown_command"` as appropriate (F-10, AC-7, AC-18).
- For the `Stop` command specifically: writes `{"status":"ok"}\n`, calls `AsyncWriteExt::flush()` to guarantee the response reaches the client's receive buffer, and only then sends on `shutdown_tx`. This ordering ensures the response is not lost if the daemon exits immediately after signalling shutdown (Q-4: A).

**Wire-to-domain conversion** (D-3): `WireHeader` and companions in `src/ipc/types.rs` are the serde-deserialised representations. `connection_handler` converts them to domain types before calling `set_pending()`. Conversion errors (e.g. a `bpm` field with a fractional part) produce a structured error response before domain validation runs.

### Types (`src/ipc/types.rs`)

**Command enum** (`#[derive(Deserialize)]`, `#[serde(tag = "command", rename_all = "kebab-case")]`):

```
CreateProject { header: WireHeader, tracks: Vec<WireTrack> }
ModifyProject { header: WireHeader, tracks: Vec<WireTrack> }
SetBpm        { bpm: f64 }          // f64 to detect non-integer values at the boundary
SetMode       { mode: String }
LoopStart
LoopStop
Status
Stop
```

**Response** — constructed inline with the `serde_json::json!` macro in each handler; no dedicated serde Response type. Ok responses are `json!({"status": "ok"})` or `json!({"status": "ok", ...status_fields})` for the Status command; error responses are `json!({"status": "error", "code": "...", "message": "..."})` (D-1: A).

**EngineSettings** (held in `Arc<Mutex<EngineSettings>>`):
- `mode: EngineMode` — default `Standalone`
- `bpm: u32` — default 120; represents the "current BPM" when no project is active (D-2)

**EngineMode**: `Standalone | Clock | Sync` (JSON: `"standalone"`, `"clock"`, `"sync"`) — canonical definition lives here (Q-3: A); EP-7 adds behavioural wiring (enabling/disabling BPM control, connecting EP-5/EP-6) without changing the enum.

**Error codes** (values of the `"code"` field in error responses):

| Code | Trigger |
|------|---------|
| `"parse_error"` | JSON is malformed |
| `"missing_command"` | JSON parsed but no `"command"` field |
| `"unknown_command"` | `"command"` value not recognised |
| `"validation_error"` | EP-2 domain `validate()` returned an error |
| `"bpm_non_integer"` | BPM value has a non-zero fractional part |
| `"bpm_out_of_range"` | BPM integer not in 20–300 |
| `"invalid_mode"` | `"mode"` string not recognised |

**Status response payload** (embedded alongside `"status":"ok"` for the `Status` command):
- `mode: String` — current `EngineMode` as JSON string
- `bpm: u32` — from active project header if present, else from `EngineSettings.bpm`
- `time_signature: Option<{ numerator: u32, denominator: u32 }>` — from active project; `null` if no project
- `clock_state: String` — `"started"` or `"stopped"` from `LoopEngine::state()`
- `project_present: bool` — `store.read().active().is_some()`

---

## Data Model

| Type | Fields | Notes |
|------|--------|-------|
| `Command` | `CreateProject`, `ModifyProject`, `SetBpm { bpm: f64 }`, `SetMode { mode: String }`, `LoopStart`, `LoopStop`, `Status`, `Stop` | Serde tag `"command"`, kebab-case; supersedes EP-1 `IpcMessage` |
| `WireHeader` | `bpm: f64`, `time_signature: WireTimeSignature` | `f64` for BPM to detect fractional values; converted to `u32` after integer check |
| `WireTimeSignature` | `numerator: u32`, `denominator: u32` | Mirrors `TimeSignature`; validated by EP-2 `validate()` |
| `WireTrack` | `name: String`, `channel: u8`, `instrument: u8`, `bars: Vec<WireBar>` | Mirrors `Track` |
| `WireBar` | `notes: Vec<WireNote>` | Mirrors `Bar` |
| `WireNote` | `rest: Option<bool>`, `pitch: Option<u8>`, `velocity: Option<u8>`, `duration_ticks: u32` | `rest: true` → `NoteEvent::Rest`; otherwise `NoteEvent::Note { pitch, velocity }` |
| `EngineSettings` | `mode: EngineMode`, `bpm: u32` | `Arc<std::sync::Mutex<EngineSettings>>`; default Standalone, 120 |
| `EngineMode` | `Standalone \| Clock \| Sync` | JSON: `"standalone"`, `"clock"`, `"sync"`; canonical definition in EP-4; EP-7 adds behavioral consequences (Q-3: A) |
| `Response` | `serde_json::Value` | Constructed inline with `serde_json::json!` macro per handler; no dedicated serde Response type; ok shape: `{"status":"ok"}` or `{"status":"ok",...status fields}`; error shape: `{"status":"error","code":"...","message":"..."}` (D-1: A) |
| `StatusPayload` | `mode`, `bpm`, `time_signature`, `clock_state`, `project_present` | Embedded in status query ok response; `time_signature` is `null` when no project |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID | Task | Type | PRD ref | Depends on |
|----|------|------|---------|------------|
| T-1 | Write test: deserialize `{"command":"loop-start"}` → `Command::LoopStart`; `{"command":"set-bpm","bpm":120}` → `Command::SetBpm { bpm: 120.0 }` | test | F-12, F-18 | — |
| T-2 | Impl: `Command` enum with `#[serde(tag = "command", rename_all = "kebab-case")]`; `WireHeader`, `WireTimeSignature`, `WireTrack`, `WireBar`, `WireNote` with `#[derive(Deserialize)]` in `src/ipc/types.rs` | impl | F-12, F-18 | T-1 |
| T-3 | Write test: deserialize JSON object with no `"command"` field → serde error | test | F-18, AC-18 | — |
| T-4 | Write test: deserialize JSON object with `"command":"unknownxyz"` → serde error | test | F-10, F-18 | — |
| T-5 | Write test: ok response serialises to `{"status":"ok"}`; error response serialises to `{"status":"error","code":"...","message":"..."}` | test | F-19, F-20, AC-16, AC-17 | — |
| T-6 | Impl: response helper functions in `src/ipc/types.rs` using `serde_json::json!` macro: `ok_response() -> Value` and `error_response(code: &str, message: &str) -> Value`; no dedicated serde Response type (D-1: A) | impl | F-19, F-20 | T-5 |
| T-7 | Write test: `connection_handler` with a mock stream — writes `{"command":"loop-start"}\n` → reads back `{"status":"ok"}\n`; stream is closed after the response | test | F-12, F-13, AC-9, AC-10 | — |
| T-8 | Write test: client writes nothing and closes the stream → `connection_handler` writes no response and returns | test | F-15, AC-13 | — |
| T-9 | Write test: client sends malformed JSON → response contains `"status":"error"` and `"code":"parse_error"` | test | F-10, AC-7 | — |
| T-10 | Write test: client sends valid JSON with no `"command"` field → error response with `"code":"missing_command"` | test | F-10, F-18, AC-18 | — |
| T-11 | Impl: `connection_handler()` in `src/ipc/handler.rs`: `BufReader` + `read_line()`, deserialise `Command`, dispatch, write response + `\n`, drop stream; handle parse errors from T-9/T-10 | impl | F-1, F-12, F-13, F-15 | T-2, T-6, T-7, T-8, T-9, T-10 |
| T-12 | Impl: `run_ipc_server()` in `src/ipc/server.rs`: accept loop over `tokio::net::UnixListener`; spawn `tokio::task` per connection running `connection_handler` with cloned Arcs | impl | F-1, F-3 | T-11 |
| T-13 | Write test: valid create-project command (4/4, BPM 120, one track) → `{"status":"ok"}`; `store.active()` is `Some` | test | F-4, AC-1 | — |
| T-14 | Write test: create-project with `bpm: 301` → error response with `"code":"validation_error"` and non-empty `"message"` | test | F-4, F-10, F-11, AC-7, AC-8 | — |
| T-15 | Write test: create-project with `bpm: 120.5` → error response with `"code":"bpm_non_integer"` | test | F-4, F-10, F-11 | — |
| T-16 | Impl: `CreateProject` handler — convert wire types to domain types (check `bpm.fract() == 0.0`); call `store.write().set_pending()`; map `ValidationError` variants to `(code, message)` pairs; return ok or error response | impl | F-4, F-10, F-11 | T-11, T-13, T-14, T-15 |
| T-17 | Write test: valid modify-project command → `{"status":"ok"}`; `store` pending is updated | test | F-5, AC-2, AC-12 | — |
| T-18 | Impl: `ModifyProject` handler — identical conversion and dispatch logic as `CreateProject` | impl | F-5 | T-16, T-17 |
| T-19 | Write test: `set-bpm` with value 150 → `{"status":"ok"}`; `EngineSettings.bpm` is 150 | test | F-6, AC-3 | — |
| T-20 | Write test: `set-bpm` with value 19 → error `"bpm_out_of_range"`; `set-bpm` with value 120.5 → error `"bpm_non_integer"` | test | F-6, F-10 | — |
| T-21 | Impl: `SetBpm` handler — check `bpm.fract() == 0.0`; validate range 20–300; update `settings.bpm`; if `store.active()` is `Some`, call `set_pending()` with cloned project at new BPM | impl | F-6 | T-11, T-19, T-20 |
| T-22 | Write test: `set-mode` with `"clock"` → `{"status":"ok"}`; `EngineSettings.mode` is `Clock` | test | F-8, AC-5 | — |
| T-23 | Write test: `set-mode` with unrecognised string → error `"invalid_mode"` | test | F-8, F-10 | — |
| T-24 | Impl: `EngineSettings` struct and `EngineMode` enum in `src/ipc/types.rs`; `SetMode` handler — parse mode string, update `settings.mode` | impl | F-8 | T-11, T-22, T-23 |
| T-25 | Write test: `loop-start` → `{"status":"ok"}`; `engine.state()` is `Running` or `Waiting` | test | F-16, AC-14 | — |
| T-26 | Impl: `LoopStart` handler — call `engine.start()`, return ok | impl | F-16 | T-11, T-25 |
| T-27 | Write test: `loop-stop` → `{"status":"ok"}`; `engine.state()` is `Stopped` | test | F-17, AC-15 | — |
| T-28 | Impl: `LoopStop` handler — call `engine.stop()`, return ok | impl | F-17 | T-11, T-27 |
| T-29 | Write test: `status` with active project and loop stopped → response contains `"mode"`, `"bpm"`, `"time_signature"`, `"clock_state":"stopped"`, `"project_present":true` | test | F-9, F-14, AC-6, AC-11 | — |
| T-30 | Write test: `status` with loop running → `"clock_state":"started"` | test | F-14, AC-11 | — |
| T-31 | Write test: `status` with no active project → `"project_present":false`, `"time_signature":null`, `"bpm"` equals `EngineSettings.bpm` | test | F-9 | — |
| T-32 | Impl: `Status` handler — read `store.read().active()`, `engine.state()`, `settings`; build status payload; return ok response | impl | F-9, F-14 | T-11, T-29, T-30, T-31 |
| T-33 | Write test: integration — over a live Unix socket: send `create-project` → send `loop-start` → send `status` (assert running) → send `loop-stop` → send `status` (assert stopped); assert all responses are well-formed JSON ending in `\n` | test | F-1, F-3, NF-1, NF-2 | — |
| T-35 | Write test: `{"command":"stop"}` → response is `{"status":"ok"}\n`; `shutdown_tx` receives a signal after the response is written | test | F-19, AC-16 | — |
| T-36 | Impl: `Stop` handler — write `{"status":"ok"}\n`, call `AsyncWriteExt::flush()`, then take and send on `shutdown_tx`; flush must complete before shutdown is signalled | impl | F-1 | T-11, T-35 |
| T-34 | Impl: wire shared state at daemon startup — create `ProjectStore`, `LoopEngine`, `EngineSettings`, `shutdown_tx`; pass Arcs to `run_ipc_server()`; integrate with EP-1's `tokio::select!` event loop | impl | F-1, F-3 | T-12, T-32, T-33, T-36 |
| T-37 | Write test: `propeller start --clock` integration — daemon starts with mode `clock`; status query returns `"mode": "clock"` | test | F-21, AC-19 | T-34 |
| T-38 | Impl: add `--clock` flag to `Commands::Start` in `main.rs`; compute `initial_mode` in `cmd_start()`; extend `daemon::run()` signature with `initial_mode: EngineMode`; set `settings.mode = initial_mode` before the sync-port wiring block | impl | F-21 | T-37 |
| T-39 | Write test: after `propeller start` returns exit 0, `UnixStream::connect(&sock_path)` succeeds without any additional sleep | test | F-22, AC-21 | T-38 |
| T-40 | Impl: replace `daemonize` crate usage in `cmd_start` with self-spawn via `std::process::Command` using hidden `daemon-run` subcommand; add `process_group(0)` and `Stdio::null()` on all three stdio streams; poll `UnixStream::connect` with 50 ms interval up to 10 s timeout | impl | F-22 | T-38 |
| T-41 | Impl: extract `cmd_daemon_run(sync_port, clock)` from `cmd_start` (the code that previously ran after `daemonize.start()`); add hidden `Commands::DaemonRun { sync_port, clock }` subcommand wired to `cmd_daemon_run`; remove `daemonize` from `Cargo.toml` | impl | F-22 | T-40 |
| T-42 | Write test: after `propeller stop` returns exit 0, the socket file no longer exists at the configured path | test | F-23, AC-23 | T-41 |
| T-43 | Impl: in `cmd_stop`, after the stop-command response is received, poll `!sock_path.exists()` with 50 ms interval up to 10 s timeout; exit non-zero with error message on timeout | impl | F-23 | T-42 |

---

## Open Questions

No open questions. All questions have been reconciled.

## Open Decisions

All decisions resolved and reconciled.

---

## Revision Log

### Cycle 1 — Confidence: 55%
- Reconciled: none (initial generation from PRD)
- Added: D-1 (response serialisation strategy), D-2 (BPM standalone state), D-3 (wire type strategy), D-4 (EngineSettings storage)

### Cycle 2 — Confidence: 58%
- Reconciled: none (no answered questions; D-1–D-4 unresolved)
- Added: Q-1 (BPM change timing vs EP-3 F-11), Q-2 (Stop command response before shutdown), Q-3 (EngineMode scoping vs EP-7)

### Cycle 3 — Confidence: 78%
- Reconciled: Q-1 → command dispatch updated (set-bpm uses set_pending() only; bar-boundary timing accepted; no second sync path); Q-2 → Stop handler now writes ok response before shutdown_tx; T-35/T-36 added; T-34 dependency updated; Q-3 → EngineMode canonical in EP-4 (all three variants); data model and Types component updated; D-1–D-4 all checked (formal reconciliation pending /create-spec EP-4)
- Added: Q-4 (Stop handler flush strategy before daemon shutdown)

### Cycle 4 — Confidence: 88%
- Reconciled: Q-4 → CommandHandler Stop sequence updated (explicit AsyncWriteExt::flush() before shutdown_tx send); T-36 description updated; Open Questions cleared
- Added: none — no genuine ambiguities remain; run `/create-spec EP-4` to formally reconcile D-1–D-4 and reach 90%+

### Cycle 6 — Blocking start / stop (F-22, F-23)
- Added: "Blocking start" and "Blocking stop" sections to Architecture Overview
- Added: T-39–T-43 covering self-spawn approach for `cmd_start`, `cmd_daemon_run` extraction, `daemonize` removal, and `cmd_stop` socket-removal polling
- Removed: `daemonize` crate dependency (replaced by `std::process::Command` self-spawn)

### Cycle 5 — Confidence: 92%
- Reconciled: D-1 → Response data model row updated (serde_json::Value via json! macro; no dedicated serde type); Types component Response description updated; T-6 rewritten (ok_response/error_response helpers with json! macro); D-2 → confirmed (EngineSettings.bpm already fully reflected); D-3 → confirmed (WireHeader/WireTrack/WireBar/WireNote already in data model and Types component); D-4 → confirmed (Arc<std::sync::Mutex<EngineSettings>> already in shared state table and data model); all D-N blocks removed from Open Decisions
- Added: none — specification complete
