# Architecture Guidelines

---

## Runtime Model

The system has exactly two runtime roles:

- **CLI process** — short-lived, user-facing, performs a single command and exits
- **Daemon process** — long-lived, runs detached from any shell session

The CLI never performs work that belongs to the daemon. The daemon never directly interacts with a human user.

---

## Daemonisation

The `start` subcommand daemonises using a POSIX double-fork via the `daemonize` crate. After the fork the parent exits immediately; the grandchild becomes the daemon. The calling shell must return before the daemon is fully initialised.

Do not implement daemonisation manually. Use the `daemonize` crate.

---

## IPC — Unix Domain Socket

All communication between the CLI and the daemon goes through a single Unix domain socket.

- **Default path:** `/tmp/propeller.sock`
- **Override:** `PROPELLER_SOCK` environment variable, read at startup by both the CLI and the daemon
- The socket file is the sole liveness indicator: a connectable socket means a live daemon
- The socket file is unlinked by the daemon on any clean shutdown

Use the `PROPELLER_SOCK` env var in tests to give each test process a unique socket path and avoid conflicts.

---

## IPC Message Format

Messages are newline-delimited JSON:

```
{"type":"stop"}\n
```

- Every message must have a `"type"` discriminant field
- Deserialise into a Rust enum tagged with `#[serde(tag = "type")]` via `serde_json`
- Extend the enum with new variants for future message types; do not change the wire format

---

## Async Runtime

The daemon runs under `#[tokio::main]`. The main event loop uses `tokio::select!` to multiplex:

- incoming IPC connections from `tokio::net::UnixListener`
- SIGTERM via `tokio::signal::unix::signal(SignalKind::terminate())`

Do not block the async executor. Offload blocking work with `tokio::task::spawn_blocking`.

---

## Startup Guard

Before binding the socket the daemon (via the CLI) runs a liveness check:

| Socket file state | Action |
|---|---|
| Does not exist | Proceed |
| Exists, connection refused | Unlink the file and proceed |
| Exists, connection succeeds | Print error, exit non-zero |

Never skip the startup guard. It prevents two daemon instances from running simultaneously.

---

## Shutdown

Graceful shutdown is triggered by either a `stop` IPC message or SIGTERM. Both paths must:

1. Stop accepting new connections
2. Release all held resources
3. Unlink the socket file
4. Exit with code 0

Both shutdown paths converge into a single shared shutdown routine.

---

## Logging

Use the `tracing` crate for all log output (`info!`, `warn!`, `error!`). Initialise a `tracing-subscriber` registry with two layers once, after daemonisation:

| Layer | Destination |
|---|---|
| `fmt` layer | `stderr` |
| `tracing_appender` non-blocking file layer | Platform log file (see below) |

**Platform log file paths:**

| Platform | Path |
|---|---|
| Linux | `$HOME/.local/share/propeller/propeller.log` |
| macOS | `~/Library/Logs/propeller/propeller.log` |

Before the subscriber is initialised, fall back to `eprintln!` for early startup errors.

---

## CLI

Parse subcommands with `clap` using the derive API. Resolve `PROPELLER_SOCK` at the start of every invocation, before any subcommand logic runs.

---

## Development Process

- **TDD:** every implementation task must be preceded by a corresponding test task
- Write the failing test first, then write the minimum implementation to make it pass
- Integration tests must use unique socket paths via `PROPELLER_SOCK` to run safely in parallel

---

## Code Structure

- Stateful loops must be expressed as methods on a struct, not as free functions with a large set of local variables acting as implicit fields. All mutable state that persists across iterations belongs on the struct.
- Repeated `match` arms that handle the same commands in multiple places within a single module must be consolidated into a shared helper method. Duplication of command-dispatch logic is a maintenance defect.
- State machine transitions must be exhaustive and explicit. Every reachable `(current_state, command)` pair must either be handled or explicitly ignored with a comment explaining why.

---

## Approved Crates

| Purpose | Crate |
|---|---|
| CLI argument parsing | `clap` (derive API) |
| Daemonisation | `daemonize` |
| Async runtime | `tokio` |
| IPC serialisation | `serde_json` |
| Structured logging | `tracing`, `tracing-subscriber`, `tracing-appender` |
