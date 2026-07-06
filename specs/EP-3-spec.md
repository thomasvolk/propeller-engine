# EP-3 · Position CLI — Technical Specification

## Overview

This epic adds a `loop position` subcommand to the CLI that queries the current tick position
from the daemon via the `get_position` IPC message introduced in EP-2. Without `--poll` the
command sends one request, prints one `{tick}/{loop_duration}` line to stdout, and exits.
With `--poll` it drives a `tokio::time::interval` loop — opening a fresh IPC connection each
iteration, printing one line, then sleeping until the next tick — until SIGINT causes a clean
exit with code 0. Depends on EP-1 (atomic tick counter) and EP-2 (get_position IPC protocol).

**Confidence Level:** 90% — all F-x and AC-x map to tasks; the minor residual is that the
exact wire format used by `query_position` (specifically the `"type"` vs `"command"` discriminant)
depends on EP-2's implementation decisions, which are constrained by EP-2 F-1/F-2 but whose
server-side dispatch detail is not yet in code.

---

## Architecture Overview

EP-3 is a pure CLI addition; no daemon code changes are required. The CLI side gains:

1. A new `Position` variant in `LoopCommand` (derive-API clap), with `--poll: bool` and
   `--interval-ms: u64` (default 50).
2. A `query_position` async function in `client.rs` that opens a `tokio::net::UnixStream`,
   sends `{"type":"get_position"}\n` (EP-2 F-1), reads one newline-delimited JSON response,
   and returns `(tick: u64, loop_duration: Option<u64>)` on success or `ClientError` on failure.
3. A `format_position_output` pure function: `"{tick}/{loop_duration}"` when a project is
   loaded, `"{tick}/-"` otherwise.
4. A `cmd_loop_position` function that builds a tokio runtime and blocks on either the
   single-shot path or the poll loop depending on the `--poll` flag.

**Single-shot path:** call `query_position`, print formatted output, exit 0. On connect failure
print to stderr, exit 1.

**Poll path:** `tokio::select!` races `interval.tick()` against `SIGINT` (via
`tokio::signal::unix::signal(SignalKind::interrupt())`). On each interval tick: open a new IPC
connection, call `query_position`, print output. On connection failure in the loop: print to
stderr, exit 1. On SIGINT: exit 0.

---

## Components

### `LoopCommand` (`src/main.rs`)

Add variant:

```rust
/// Query current tick position
Position {
    /// Poll continuously until interrupted
    #[arg(long)]
    poll: bool,
    /// Poll interval in milliseconds (only meaningful with --poll)
    #[arg(long, default_value_t = 50)]
    interval_ms: u64,
},
```

Add dispatch arm in the `Commands::Loop` match block:

```rust
LoopCommand::Position { poll, interval_ms } => cmd_loop_position(poll, interval_ms),
```

### `cmd_loop_position` (`src/main.rs`)

Creates a `tokio::runtime::Runtime` and blocks on one of two async branches:

- `!poll`: call `query_position(&sock_path)`, on success print `format_position_output(...)` to
  stdout and return; on error print diagnostic to stderr and exit 1.
- `poll`: enter a `tokio::select!` loop on `interval.tick()` and SIGINT; on interval tick call
  `query_position` and print; on SIGINT return (exit 0); on connect error print to stderr and
  exit 1.

### `query_position` (`src/client.rs`)

```rust
pub(crate) async fn query_position(sock_path: &Path) -> Result<(u64, Option<u64>), ClientError>
```

- Connects with `tokio::net::UnixStream::connect`; maps `Err` to `ClientError::Connect`.
- Writes `{"type":"get_position"}\n` using `AsyncWriteExt`.
- Reads one line with `AsyncBufReadExt::read_line`.
- Deserialises into `PositionResponse { tick: u64, loop_duration: Option<u64> }` tagged with
  `#[serde(rename = "position")]` (matching EP-2 F-2 wire shape).
- Returns `(response.tick, response.loop_duration)`.

### `format_position_output` (`src/main.rs` or `src/client.rs`)

```rust
fn format_position_output(tick: u64, loop_duration: Option<u64>) -> String
```

Returns `"{tick}/{loop_duration}"` when `loop_duration` is `Some`, `"{tick}/-"` when `None`.

---

## Data Model

| Type                | Fields / Changes                                               | Notes                                               |
|---------------------|----------------------------------------------------------------|-----------------------------------------------------|
| `LoopCommand`       | + `Position { poll: bool, interval_ms: u64 }`                 | Default `interval_ms = 50`; parsed by clap          |
| `PositionResponse`  | `tick: u64`, `loop_duration: Option<u64>`                     | Serde-deserialised from EP-2 `position` response    |
| `query_position`    | `async fn(sock_path: &Path) -> Result<(u64, Option<u64>), ClientError>` | New function in `client.rs`            |
| `format_position_output` | `fn(tick: u64, loop_duration: Option<u64>) -> String`    | Pure formatting helper                              |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID   | Task                                                                                                                                                                                           | Type | PRD ref        | Depends on |
|------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|------|----------------|------------|
| T-1  | Unit test: `LoopCommand::Position` parses from `["loop", "position"]` with defaults `poll=false`, `interval_ms=50`; `--poll` sets `poll=true`; `--interval-ms 100` sets `interval_ms=100`    | test | F-1, F-5, F-6  | —          |
| T-2  | Add `Position { poll: bool, interval_ms: u64 }` variant to `LoopCommand`; wire dispatch arm `cmd_loop_position(poll, interval_ms)` in the `Commands::Loop` match                              | impl | F-1, F-5, F-6  | T-1        |
| T-3  | Unit test: `format_position_output(1234, Some(4800))` returns `"1234/4800"`; `format_position_output(0, None)` returns `"0/-"`                                                                 | test | F-3, F-4       | —          |
| T-4  | Implement `format_position_output(tick: u64, loop_duration: Option<u64>) -> String`                                                                                                           | impl | F-3, F-4       | T-3        |
| T-5  | Unit test: `query_position` against a mock Unix socket returning `{"type":"position","tick":42,"loop_duration":480}\n` → returns `Ok((42, Some(480)))`; against `{"type":"position","tick":0,"loop_duration":null}\n` → returns `Ok((0, None))`; against a non-connectable path → returns `Err(ClientError::Connect(_))` | test | F-2, F-9 | T-2, T-4 |
| T-6  | Implement `async fn query_position(sock_path: &Path) -> Result<(u64, Option<u64>), ClientError>` in `client.rs`: connect, write `{"type":"get_position"}\n`, read one line, deserialise `PositionResponse` | impl | F-2, F-9 | T-5 |
| T-7  | Integration test: single-shot mode with a mock daemon returning project-loaded position → prints exactly one line to stdout matching `\d+/\d+`, exit code 0 (AC-1)                            | test | F-2, F-3, AC-1 | T-6        |
| T-8  | Implement single-shot path in `cmd_loop_position`: tokio runtime, call `query_position`, print `format_position_output(...)` to stdout, exit 0                                                | impl | F-2, F-3, AC-1 | T-7        |
| T-9  | Integration test: single-shot with mock daemon returning no-project position → prints exactly one line matching `\d+/-`, exit code 0 (AC-2)                                                   | test | F-4, AC-2      | T-8        |
| T-10 | Integration test: daemon not running, single-shot → human-readable error on stderr, nothing on stdout, exit code 1 (AC-5 / F-9)                                                               | test | F-9, AC-5      | T-8        |
| T-11 | Implement error path in single-shot: on `ClientError::Connect`, print `"propeller: cannot reach daemon at {path}: {err}"` to stderr, exit 1                                                   | impl | F-9            | T-10       |
| T-12 | Unit test: `--interval-ms 100` without `--poll` → single-shot path is taken; output is one line; validates F-11 no-op                                                                        | test | F-11           | T-11       |
| T-13 | Integration test: `--poll` with mock daemon → produces ≥2 output lines, each matching `\d+/\d+`; validates F-5 / AC-3                                                                        | test | F-5, F-7, AC-3 | T-11       |
| T-14 | Implement poll loop in `cmd_loop_position`: tokio runtime, `tokio::time::interval(Duration::from_millis(interval_ms))`, new `query_position` call per tick, print output each iteration       | impl | F-5, F-6, F-7, F-10 | T-13  |
| T-15 | Integration test: `--poll --interval-ms 100` run for 500 ms → approximately 5 lines produced (±1 tolerance) (AC-4)                                                                           | test | F-6, AC-4      | T-14       |
| T-16 | Integration test: `--poll` mode, process receives SIGINT → exits with code 0; validates F-8 / AC-3                                                                                            | test | F-8, AC-3      | T-14       |
| T-17 | Wire SIGINT into poll loop via `tokio::select!` on `tokio::signal::unix::signal(SignalKind::interrupt())`; exit 0 on signal receipt                                                           | impl | F-8            | T-16       |
| T-18 | Integration test: mock daemon becomes unreachable mid-poll → human-readable error on stderr, exit code 1 (AC-6 / F-12)                                                                       | test | F-12, AC-6     | T-17       |
| T-19 | Handle connection failure in poll loop: on `ClientError::Connect`, print diagnostic to stderr, exit 1                                                                                         | impl | F-12           | T-18       |

---

## Open Questions

### Q-1 · Wire format discriminant for `get_position` IPC request

EP-2 F-1 specifies the request as `{"type":"get_position"}`, while the existing `Command`
enum uses `#[serde(tag = "command")]`. It is not yet decided whether the daemon's connection
handler will route both `"type"` and `"command"` keyed messages through a single dispatch,
or whether a separate parsing branch will handle the `"type"`-keyed messages.

**Options**
- A. Daemon routes `{"type":"get_position"}` through a separate IPC message type alongside the existing `Command` enum; `query_position` sends `{"type":"get_position"}` exactly per EP-2 F-1 — *(recommended — matches EP-2 PRD verbatim and architecture guidelines)*
- B. EP-2 adds `GetPosition` to the existing `Command` enum with `#[serde(rename = "get_position")]`, making the wire format `{"command":"get_position"}`; `query_position` sends that format instead
- C. EP-2 migrates all existing commands to use `"type"` and deprecates `"command"`

**Answer:**

---

## Open Decisions

None.

---

## Revision Log

### Cycle 1 — Confidence: 90%

- Reconciled: nothing (spec created fresh from PRD; no prior answers)
- Added: Q-1 (IPC wire-format discriminant for `get_position` — depends on EP-2 implementation choice)
