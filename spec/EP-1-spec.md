# EP-1 · Daemon Process — Technical Specification

## Overview

This epic establishes the foundational daemon infrastructure for the propeller-engine. The engine starts on demand via a CLI `start` command, daemonises itself using a double-fork, opens a Unix domain socket at a configurable path (defaulting to `/tmp/propeller.sock`) as its liveness indicator and IPC endpoint, and shuts down cleanly when a `stop` command is received or SIGTERM arrives. All subsequent epics build on top of this socket-based foundation.

**Confidence Level:** 92% — All questions answered and decisions resolved; every PRD item has a task, TDD ordering is maintained, and concrete crates are named throughout; residual uncertainty is only in integration-test infrastructure details (invoking and cleaning up the daemon binary from `cargo test`).

---

## Architecture Overview

The system has two runtime roles: the **CLI process** (short-lived) and the **daemon process** (long-lived). The CLI is written in Rust using `clap` (derive API) for argument parsing. The CLI's `start` subcommand performs a POSIX double-fork using the `daemonize` crate (which calls `fork()`, `setsid()`, and a second `fork()` internally, redirects stdio, and returns in the grandchild), so the grandchild process is fully detached from the calling shell, which returns immediately. The CLI's `stop` subcommand connects to the Unix socket and sends a stop message, then waits for the connection to close as confirmation of shutdown.

The socket path is resolved at startup from the `PROPELLER_SOCK` environment variable, falling back to `/tmp/propeller.sock`. This allows integration tests to use unique socket paths without port conflicts by setting the env var per test.

The daemon runs under `#[tokio::main]` and enters a `tokio`-driven async event loop. After binding the socket, the main loop uses `tokio::select!` to multiplex incoming IPC connections from the Unix socket and SIGTERM signals. The presence of a connectable socket is the sole liveness indicator (F-6, F-8). Before binding the socket, the daemon runs a startup guard: if a socket file already exists, it attempts a connection; a refused connection means a stale file (removed and startup proceeds); a successful connection means a live instance (startup is rejected). On shutdown — triggered by a stop IPC message or SIGTERM — the daemon closes all resources and unlinks the socket file.

IPC messages are serialised as newline-delimited JSON using `serde_json`, with a `type` discriminant field (e.g. `{"type":"stop"}\n`). Future message variants map to a Rust enum tagged with `#[serde(tag = "type")]`.

Log output uses the `tracing` crate. A `tracing-subscriber` registry is initialised at startup with two layers: an `fmt` layer writing to stderr, and a `tracing_appender` non-blocking file appender writing to the platform log file. Before the subscriber is initialised (i.e. during the earliest startup phase), errors fall back to `eprintln!`.

---

## Components

### CLI

Parses `start` and `stop` subcommands using `clap` (derive API) and forwards to the appropriate handler. Resolves the socket path from `PROPELLER_SOCK` env var at startup.

- `start`: runs the startup guard, performs double-fork via the `daemonize` crate, parent exits immediately (AC-1).
- `stop`: connects to the resolved socket path, sends `{"type":"stop"}\n`, blocks until the connection closes (AC-2).

### Startup Guard

Runs inside the CLI before forking.

- If the socket file does not exist: proceed.
- If it exists and connection is refused: unlink the file and proceed (F-10, AC-10).
- If it exists and connection succeeds: print error and exit non-zero (F-5, AC-4).

### Daemon Process

The daemonised grandchild, running under `#[tokio::main]`. Responsibilities:

- Bind and listen on the resolved socket path (F-8).
- Accept IPC connections and handle the `stop` message via `tokio::select!` (F-3).
- Await SIGTERM via `tokio::signal` in the same `select!` loop (NF-4).
- On shutdown: close the socket, unlink the file, release all resources, exit (F-4).

### Socket Server

Binds the resolved socket path using `tokio::net::UnixListener`, accepts connections asynchronously, spawns a task per connection, reads newline-delimited JSON frames with `tokio::io::AsyncBufReadExt`, and deserialises via `serde_json` into `IpcMessage`. The socket file is unlinked on clean shutdown (F-4, AC-2).

### Signal Handler

Uses `tokio::signal::unix::signal(SignalKind::terminate())` to receive SIGTERM as a future, integrated into the main `tokio::select!` loop alongside the socket accept path. Triggers the same graceful shutdown path as the IPC stop message (NF-4).

### Logger

Uses `tracing` macros (`info!`, `warn!`, `error!`) for structured logging. A `tracing-subscriber` registry with two layers is initialised once after daemonisation:

- **stderr layer** — `fmt::layer().with_writer(std::io::stderr)`, active pre-daemonisation and for startup errors.
- **File appender layer** — `tracing_appender::non_blocking` writing to the platform log file:
  - Linux: `$HOME/.local/share/propeller/propeller.log` (XDG base dir)
  - macOS: `~/Library/Logs/propeller/propeller.log` (Apple HIG convention)

---

## Data Model

| Type | Fields | Notes |
|------|--------|-------|
| `EngineState` | `Running` \| `Stopped` | Internal daemon state; drives event loop and shutdown |
| `SocketPath` | `path: PathBuf` | Resolved from `PROPELLER_SOCK` env var at startup; defaults to `/tmp/propeller.sock` |
| `LogPath` | `path: PathBuf` | Linux: `$HOME/.local/share/propeller/propeller.log`; macOS: `~/Library/Logs/propeller/propeller.log`; resolved at startup |
| `IpcMessage` | `Stop` | Wire format: `{"type":"stop"}\n`; Rust enum with `#[serde(tag = "type")]` via `serde_json`; extended in later epics |
| `StartupOutcome` | `Started` \| `AlreadyRunning` \| `StaleCleared` | Result of the startup guard |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID | Task | Type | PRD ref | Depends on |
|----|------|------|---------|------------|
| T-1 | Write test: logger writes diagnostic message to stderr | test | F-9, NF-3 | — |
| T-2 | Write test: logger writes diagnostic message to platform log file (AC-7) | test | F-9, AC-7 | — |
| T-3 | Impl: logger — `tracing-subscriber` registry with stderr `fmt` layer and `tracing_appender` file layer | impl | F-9 | T-1, T-2 |
| T-4 | Write test: socket at resolved path is connectable after daemon starts (AC-5) | test | F-8, AC-5 | — |
| T-5 | Write test: socket absent or connection refused when daemon not running (AC-6) | test | F-8, AC-6 | — |
| T-6 | Impl: daemon binds and listens on Unix domain socket via `tokio::net::UnixListener` | impl | F-8 | T-4, T-5 |
| T-7 | Write test: `start` CLI command daemonises and calling shell returns immediately (AC-1) | test | F-1, F-7, AC-1 | — |
| T-8 | Write test: time from `start` invocation to socket-ready is under 1 second (AC-8) | test | NF-1, AC-8 | — |
| T-9 | Impl: CLI `start` command with double-fork daemonisation via `daemonize` crate | impl | F-1, F-7 | T-7, T-8 |
| T-10 | Write test: second `start` while running is rejected and first instance is unaffected (AC-4) | test | F-5, AC-4 | — |
| T-11 | Write test: stale socket (connection refused) is removed and restart succeeds (AC-10) | test | F-10, AC-10 | — |
| T-12 | Impl: startup guard — connect-or-clear liveness check before bind | impl | F-5, F-10 | T-10, T-11 |
| T-13 | Write test: daemon remains running indefinitely without further interaction (AC-3) | test | F-2, AC-3 | — |
| T-14 | Write test: idle CPU below 1% and memory below 50 MB after stable period (AC-9) | test | NF-2, AC-9 | — |
| T-15 | Impl: daemon main event loop — `tokio::select!` over socket accept and SIGTERM future | impl | F-2 | T-13, T-14 |
| T-16 | Write test: `stop` CLI command causes daemon to shut down and remove socket (AC-2) | test | F-3, F-4, AC-2 | — |
| T-17 | Impl: CLI `stop` command; daemon IPC stop handler; socket unlink on exit | impl | F-3, F-4 | T-16 |
| T-18 | Write test: SIGTERM on daemon triggers graceful shutdown and socket removal | test | NF-4 | — |
| T-19 | Impl: `tokio::signal::unix` SIGTERM handler integrated into main `select!` loop | impl | NF-4 | T-18 |
| T-20 | Write test: setting `PROPELLER_SOCK` env var routes daemon and CLI to a custom socket path | test | F-8 | — |
| T-21 | Impl: resolve socket path from `PROPELLER_SOCK` env var at startup, defaulting to `/tmp/propeller.sock` | impl | F-8 | T-20 |

---

## Open Questions

No open questions.

---

## Open Decisions

All decisions resolved and reconciled into the specification.

- **D-1 (Rust)** — reconciled across cycles 2–4: `daemonize`, `tokio`, `clap`, `tracing` crates selected.
- **D-2 (JSON/newline-delimited)** — reconciled in cycle 3: `serde_json`, `{"type":"stop"}\n` wire format, `#[serde(tag = "type")]`.
- **D-3 (Linux + macOS)** — reconciled in cycle 3: macOS log path `~/Library/Logs/propeller/propeller.log`.

---

## Revision Log

### Cycle 1 — Confidence: 55%
- Reconciled: none (initial generation)
- Added: D-1 (language), D-2 (IPC message format), D-3 (platform scope)

### Cycle 2 — Confidence: 65%
- Reconciled: none (D-1/D-2/D-3 are checked but require reconciliation via /create-spec; no Q-N answers present)
- Added: Q-1 (async runtime — tokio vs sync), Q-2 (double-fork crate), Q-3 (JSON IPC message schema), Q-4 (macOS log path), Q-5 (CLI framework)

### Cycle 3 — Confidence: 78%
- Reconciled: Q-2 → architecture + CLI updated (`daemonize` crate for double-fork); Q-3 → architecture + Socket Server + `IpcMessage` data model updated (`{"type":"stop"}\n`, `serde_json`, `#[serde(tag = "type")]`); Q-4 → Logger + `LogPath` data model updated (macOS: `~/Library/Logs/propeller/propeller.log`); Q-5 → architecture + CLI updated (`clap` derive API)
- Added: none — Q-1 (async runtime) is the sole remaining open question; answer it to unblock Socket Server and Signal Handler finalisation

### Cycle 4 — Confidence: 83%
- Reconciled: Q-1 → architecture updated (tokio async runtime, `#[tokio::main]`, `tokio::select!`); Socket Server updated (`tokio::net::UnixListener`, `AsyncBufReadExt`); Signal Handler updated (`tokio::signal::unix`); T-6/T-15/T-19 descriptions updated to name tokio APIs
- Added: Q-6 (integration test isolation — socket path conflicts), Q-7 (logging crate — tracing vs log vs plain)

### Cycle 5 — Confidence: 92%
- Reconciled: Q-6 → architecture + `SocketPath` data model updated (`PROPELLER_SOCK` env var, defaults to `/tmp/propeller.sock`); T-20/T-21 added for env-var override; Q-7 → Logger component updated (`tracing` + `tracing-subscriber` two-layer registry, `tracing_appender`); T-3 updated; D-1/D-2/D-3 formally reconciled and Open Decisions section closed
- Added: none — specification is complete at 92%
