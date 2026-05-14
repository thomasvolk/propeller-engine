# EP-1 · Daemon Process — Technical Specification

## Overview

This epic establishes the foundational daemon infrastructure for the propeller-engine. The engine starts on demand via a CLI `start` command, daemonises itself using a double-fork, opens a Unix domain socket at `/tmp/propeller.sock` as its liveness indicator and IPC endpoint, and shuts down cleanly when a `stop` command is received or SIGTERM arrives. All subsequent epics build on top of this socket-based foundation.

**Confidence Level:** 55% — Language, IPC message format, and platform scope are all unresolved; no implementation detail can be finalised until D-1 (language) is chosen.

---

## Architecture Overview

The system has two runtime roles: the **CLI process** (short-lived) and the **daemon process** (long-lived). The CLI's `start` subcommand performs a POSIX double-fork so the grandchild process is fully detached from the calling shell, which returns immediately. The CLI's `stop` subcommand connects to the Unix socket and sends a stop message, then waits for the connection to close as confirmation of shutdown.

The daemon opens `/tmp/propeller.sock` as its first act after forking, then enters a blocking event loop. The presence of a connectable socket is the sole liveness indicator (F-6, F-8). Before binding the socket, the daemon runs a startup guard: if a socket file already exists, it attempts a connection; a refused connection means a stale file (removed and startup proceeds); a successful connection means a live instance (startup is rejected). On shutdown — triggered by a stop IPC message or SIGTERM — the daemon closes all resources and unlinks the socket file.

Errors at any stage are written to stderr (visible before daemonisation) and to the platform log file (visible throughout the daemon's lifetime).

---

## Components

### CLI

Parses `start` and `stop` subcommands and forwards to the appropriate handler.

- `start`: runs the startup guard, performs double-fork, parent exits immediately (AC-1).
- `stop`: connects to `/tmp/propeller.sock`, sends a stop IPC message, blocks until the connection closes (AC-2).

### Startup Guard

Runs inside the CLI before forking (or inside the first fork, depending on double-fork structure).

- If `/tmp/propeller.sock` does not exist: proceed.
- If it exists and connection is refused: unlink the file and proceed (F-10, AC-10).
- If it exists and connection succeeds: print error and exit non-zero (F-5, AC-4).

### Daemon Process

The daemonised grandchild. Responsibilities:

- Bind and listen on `/tmp/propeller.sock` (F-8).
- Accept IPC connections and handle the `stop` message (F-3).
- Install a SIGTERM handler for graceful shutdown (NF-4).
- On shutdown: close the socket, unlink the file, release all resources, exit (F-4).

### Socket Server

Binds `/tmp/propeller.sock`, accepts connections, deserialises IPC messages, and dispatches to handlers. The socket file is unlinked on clean shutdown (F-4, AC-2).

### Signal Handler

Catches SIGTERM and triggers the same graceful shutdown path as the IPC stop message (NF-4).

### Logger

Writes diagnostic output in two channels:

- **stderr** — used pre-daemonisation and for startup errors visible to the operator.
- **Platform log file** — used by the daemon after detaching from the terminal; path follows XDG on Linux (`$HOME/.local/share/propeller/propeller.log`) or the OS-appropriate equivalent on other platforms (F-9, AC-7).

---

## Data Model

| Type | Fields | Notes |
|------|--------|-------|
| `EngineState` | `Running` \| `Stopped` | Internal daemon state; drives event loop and shutdown |
| `SocketPath` | `path: String` | Constant `/tmp/propeller.sock`; single source of truth |
| `LogPath` | `path: PathBuf` | Resolved at startup from env; platform-conditional |
| `IpcMessage` | `Stop` | Sole message type for EP-1; extended in later epics |
| `StartupOutcome` | `Started` \| `AlreadyRunning` \| `StaleCleared` | Result of the startup guard |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID | Task | Type | PRD ref | Depends on |
|----|------|------|---------|------------|
| T-1 | Write test: logger writes diagnostic message to stderr | test | F-9, NF-3 | — |
| T-2 | Write test: logger writes diagnostic message to platform log file (AC-7) | test | F-9, AC-7 | — |
| T-3 | Impl: logger component writing to stderr and platform log file | impl | F-9 | T-1, T-2 |
| T-4 | Write test: socket at `/tmp/propeller.sock` is connectable after daemon starts (AC-5) | test | F-8, AC-5 | — |
| T-5 | Write test: socket absent or connection refused when daemon not running (AC-6) | test | F-8, AC-6 | — |
| T-6 | Impl: daemon binds and listens on Unix domain socket at `/tmp/propeller.sock` | impl | F-8 | T-4, T-5 |
| T-7 | Write test: `start` CLI command daemonises and calling shell returns immediately (AC-1) | test | F-1, F-7, AC-1 | — |
| T-8 | Write test: time from `start` invocation to socket-ready is under 1 second (AC-8) | test | NF-1, AC-8 | — |
| T-9 | Impl: CLI `start` command with double-fork daemonisation | impl | F-1, F-7 | T-7, T-8 |
| T-10 | Write test: second `start` while running is rejected and first instance is unaffected (AC-4) | test | F-5, AC-4 | — |
| T-11 | Write test: stale socket (connection refused) is removed and restart succeeds (AC-10) | test | F-10, AC-10 | — |
| T-12 | Impl: startup guard — connect-or-clear liveness check before bind | impl | F-5, F-10 | T-10, T-11 |
| T-13 | Write test: daemon remains running indefinitely without further interaction (AC-3) | test | F-2, AC-3 | — |
| T-14 | Write test: idle CPU below 1% and memory below 50 MB after stable period (AC-9) | test | NF-2, AC-9 | — |
| T-15 | Impl: daemon main event loop (blocks on I/O, remains idle, stays resident) | impl | F-2 | T-13, T-14 |
| T-16 | Write test: `stop` CLI command causes daemon to shut down and remove socket (AC-2) | test | F-3, F-4, AC-2 | — |
| T-17 | Impl: CLI `stop` command; daemon IPC stop handler; socket unlink on exit | impl | F-3, F-4 | T-16 |
| T-18 | Write test: SIGTERM on daemon triggers graceful shutdown and socket removal | test | NF-4 | — |
| T-19 | Impl: SIGTERM signal handler in daemon that runs the shutdown path | impl | NF-4 | T-18 |

---

## Open Decisions

High-impact architecture and technology choices. Check your preferred option for each decision, then re-run `/create-spec` to reconcile.

### D-1 · Programming language

The language determines the runtime model, available libraries for Unix process management and MIDI (later epics), packaging strategy, and test tooling. This is the most critical unresolved choice — nothing concrete can be implemented until it is settled.

- [ ] A. **Rust** — zero-cost abstractions, strong Unix/async ecosystem (`tokio`, `nix`), single-binary distribution, ideal for low-latency MIDI work in later epics; steeper learning curve and longer compile times. *(recommended — best fit for a low-latency MIDI daemon requiring a single deployable binary)*
- [ ] B. **Go** — simple concurrency model, good `net` stdlib for Unix sockets, fast compile, easy cross-compilation; GC pauses may matter for real-time MIDI in later epics.
- [ ] C. **Python** — fastest to prototype; no native MIDI daemon tooling, requires interpreter installation, GIL limits true parallelism.

---

### D-2 · IPC message format over the Unix socket

The format defines how the CLI `stop` command communicates with the daemon, and how future epics will send MIDI commands. A poor choice here is expensive to change later.

- [ ] A. **Line-delimited UTF-8 text** (e.g. `STOP\n`) — trivially debuggable with `nc`; brittle for binary payloads in later epics.
- [ ] B. **JSON per message, newline-delimited** — human-readable, self-describing, easy to extend; slightly more overhead.
- [ ] C. **Length-prefixed binary frames with a schema (e.g. MessagePack or protobuf)** — compact, strongly typed, good for MIDI payloads; more tooling required. *(recommended — future epics carry real-time MIDI data where binary efficiency matters)*

---

### D-3 · Platform scope

Double-fork daemonisation and the log path (`$HOME/.local/share/…`) are POSIX concepts. Deciding scope now prevents divergent implementations.

- [ ] A. **Linux only** — single XDG log path, simpler CI, covers the primary live-coding use case. *(recommended — simplest scope for EP-1; macOS can be added later)*
- [ ] B. **Linux + macOS** — broader reach; requires conditional log path resolution and macOS-specific MIDI library choices in later epics.

---

## Revision Log

### Cycle 1 — Confidence: 55%
- Reconciled: none (initial generation)
- Added: D-1 (language), D-2 (IPC message format), D-3 (platform scope)
