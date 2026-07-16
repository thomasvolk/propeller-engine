# EP-2 · `project get` CLI Command — Technical Specification

## Overview

Adds `propeller project get`, a CLI leaf command that reaches the daemon over the
existing Unix domain socket, requests the `"project"` query defined by EP-1, and
prints the current/pending project state as compact JSON — mirroring how
`project create`/`project modify` already reach the daemon and how every CLI
command already resolves the socket path and reports connection failure.

**Confidence Level:** 100% — architecture, components, data model, and task
breakdown are all grounded in existing sibling code (`cmd_project_create`,
`client::send_command`, `handle_client_error`, `handle_status`'s wrapper
convention), every F-x/AC-x/NF-x has at least one test and impl task in strict
TDD order, and both prior Open Decisions (transport, response-stripping
strategy) are now resolved. No open questions or unchecked decisions remain.

---

## Architecture Overview

EP-2 adds a single new CLI leaf command, `propeller project get`, to the existing
`Project` subcommand group already used by `project create`/`project modify`. It
follows exactly the same shape as its siblings: resolve the daemon socket path
(shared across every CLI command via `src/socket_path.rs`), send a single-line
JSON request over a synchronous `UnixStream` using the existing
`client::send_command` helper, and route any failure — whether the socket refuses
to connect or the daemon replies with `{"status":"error",...}` — through the one
existing `handle_client_error` function, so every `project get` failure prints to
stderr and exits non-zero through a single code path (per
architecture-guidelines.md's command-dispatch-consolidation rule).

The only genuinely new logic is a small, pure formatting step that strips the
daemon's `"status"` wrapper and prints only whichever of `"current"`/`"pending"`
are present in the response, compact and single-line — mirroring the existing
"omit entirely when absent" convention already used by `handle_status`
(`src/ipc/handler.rs:337`) for its own optional fields.

No daemon-side, async runtime, or IPC-protocol changes are needed for EP-2
itself — those are EP-1's responsibility. EP-2 only consumes the `"project"`
query's response shape that EP-1's PRD and spec define. The daemon side is not
yet implemented in this codebase (no `Command::Project` variant exists in
`src/ipc/types.rs` at spec time), so EP-2's own tests use an in-process mock
`UnixListener` standing in for the daemon (the same pattern `client.rs`'s
`spawn_mock` and `tests/integration.rs` already use), keeping EP-2's test suite
independent of EP-1's implementation schedule.

---

## Components

### CLI Command Layer (`src/main.rs`)

- `ProjectCommand::Get` — new unit variant alongside `Create`/`Modify`, no
  arguments.
- `cmd_project_get()` — new function, dispatched from the existing
  `Commands::Project { command }` match arm in `main()`.
- Resolves the socket path via `socket_path::resolve()` (F-6, NF-1), identical to
  every other CLI command.
- Sends `{"command": "project"}` via `client::send_command` — the same
  synchronous helper `cmd_project_create`/`cmd_project_modify` already use — not
  the bespoke async connect-and-print path `cmd_status` uses. F-6 only requires
  reaching the daemon the same way (socket resolution + connect); it explicitly
  does not require inheriting `status`'s bespoke stdout error convention (F-8).
- On `Ok(response)`, formats the response with `client::format_project_get_output`
  and prints the result to stdout via `println!` (F-1, F-2, F-3, F-4, F-9).
- On `Err(e)`, delegates to the existing, unchanged `handle_client_error(e,
  &sock_path)`, which already has distinct arms for `ClientError::Connect`
  (F-5, F-8, AC-5) and `ClientError::Daemon` (F-10, AC-6), both printing to
  stderr and exiting non-zero (NF-2). No new branching is introduced in this
  function.

### Client Helper Layer (`src/client.rs`)

- New pure function: `pub(crate) fn format_project_get_output(response: &Value)
  -> String`.
- Builds a fresh `serde_json::Map` containing only the `"current"` and
  `"pending"` keys, copied over from `response` only when present — never
  `"status"` or any other wrapper key (F-7).
- Serializes with `serde_json::to_string` (never `to_string_pretty`), guaranteeing
  compact, single-line output (F-9).
- No new error handling is introduced here: `send_command` and `ClientError` are
  reused unchanged.

### Daemon Side (out of scope for EP-2)

- The `"project"` query command, its `Command` enum variant, and its handler are
  delivered by EP-1 and are a hard dependency, not built as part of this epic.
- EP-2's mock-daemon tests (see Implementation Tasks) exercise the CLI binary
  against a hand-rolled `UnixListener` test double that speaks the wire shape
  EP-1's PRD defines, so EP-2 can be fully implemented and tested independently
  of EP-1's actual delivery.

---

## Data Model

| Type                                          | Fields                                                                 | Notes |
|------------------------------------------------|-------------------------------------------------------------------------|-------|
| Wire request (`serde_json::Value`)             | `{"command": "project"}`                                                | Sent by `cmd_project_get()` via `client::send_command`; matches EP-1's `"project"` query command name. |
| Wire response (`serde_json::Value`, per EP-1)  | `{"status": "ok", "current"?: Value, "pending"?: Value}`                 | `current`/`pending` are omitted entirely (not null/empty) when absent, matching the existing convention in `handle_status` (`src/ipc/handler.rs:337`). |
| `client::ClientError` (existing, unchanged)    | `Connect(io::Error)`, `Daemon { message: String }`, `Input(String)`      | Already distinguishes daemon-unreachable (`Connect`) from daemon-reported error (`Daemon`); both route through `handle_client_error` for F-8/F-10. |
| `ProjectCommand::Get` (new, `main.rs`)         | unit variant, no fields                                                 | New clap subcommand: `propeller project get`. |
| `format_project_get_output` (new fn, `client.rs`) | `fn(response: &Value) -> String`                                      | Pure function: copies only `"current"`/`"pending"` keys present in `response` into a fresh, compact-serialized JSON object. |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task appears before the impl task it covers.

| ID   | Task | Type | PRD ref | Depends on |
|------|------|------|---------|------------|
| T-1  | Unit tests for `format_project_get_output`: both `current` and `pending` present → both kept, `status` stripped; output has no whitespace (compact). | test | F-1, F-2, F-7, F-9, AC-1, AC-2 | — |
| T-2  | Implement `format_project_get_output` in `src/client.rs`: copy only `current`/`pending` keys when present, serialize compact. | impl | F-1, F-2, F-7, F-9 | T-1 |
| T-3  | Unit test: `format_project_get_output` with no `pending` key in input → output has no `pending` key at all (not null/empty). | test | F-3, AC-3 | T-2 |
| T-4  | Unit test: `format_project_get_output` with neither `current` nor `pending` present → output is `{}`. | test | F-4, AC-4 | T-2 |
| T-5  | (impl folded into T-2) Confirm the allowlist-copy implementation from T-2 satisfies T-3/T-4 without special-casing; adjust if it does not. | impl | F-3, F-4 | T-3, T-4 |
| T-6  | Clap parsing test: `propeller project get` parses to `Commands::Project { command: ProjectCommand::Get }`. | test | F-6, NF-1 | — |
| T-7  | Add `ProjectCommand::Get` variant and wire the match arm in `main()` to call `cmd_project_get()`. | impl | F-6, NF-1 | T-6 |
| T-8  | Integration test (mock `UnixListener` + `PROPELLER_SOCK`, subprocess binary): daemon returns `{"status":"ok","current":{...}}` only → stdout is the compact stripped JSON with just `current`, exit code 0. | test | F-1, F-3, F-9, AC-1, AC-3 | T-7 |
| T-9  | Implement `cmd_project_get()` in `src/main.rs`: resolve socket, send `{"command":"project"}` via `client::send_command`, format with `format_project_get_output`, print via `println!`. | impl | F-1, F-3, F-9, F-6, NF-1 | T-2, T-7, T-8 |
| T-10 | Integration test: mock daemon returns `{"status":"ok","current":{...},"pending":{...}}` → stdout includes both entries. | test | F-2, AC-2 | T-9 |
| T-11 | Integration test: mock daemon returns `{"status":"ok"}` (no active, no pending) → command exits 0, stdout is `{}`, no error printed. | test | F-4, AC-4 | T-9 |
| T-12 | Integration test: no listener bound on the resolved `PROPELLER_SOCK` path → stderr is `propeller: cannot connect to <path>: <err>`, exit code non-zero, nothing printed to stdout. | test | F-5, F-8, NF-2, AC-5 | T-9 |
| T-13 | Confirm `cmd_project_get()` routes `Err(ClientError::Connect(_))` through the existing `handle_client_error` with no bespoke handling; adjust only if T-12 fails. | impl | F-5, F-8, NF-2 | T-12 |
| T-14 | Integration test: mock daemon replies to `{"command":"project"}` with `{"status":"error","message":"..."}` → stderr carries the daemon error message via the same shared handler, exit code non-zero, no project JSON on stdout. | test | F-10, NF-2, AC-6 | T-9 |
| T-15 | Confirm `cmd_project_get()` routes `Err(ClientError::Daemon{..})` through the same `handle_client_error` call as T-13, with no separate branch; adjust only if T-14 fails. | impl | F-10, NF-2, AC-6 | T-14 |

---

## Open Questions

None. All F-x/AC-x/NF-x requirements from the PRD are covered by the task table
above with strict test-before-impl ordering. The two items below are
architecture-level decisions, not open questions — both have recommended,
non-blocking defaults.

---

## Open Decisions

None. D-1 (transport) and D-2 (response-stripping strategy) were both resolved
in favour of their recommended options — see Architecture Overview and
Components, which already describe the synchronous `client::send_command`
transport and the allowlist-copy stripping approach as the chosen design.

---

## Revision Log

### Cycle 1 — Confidence: 90%
- Created technical specification from `specs/EP-2.md` (PRD confidence 96%, no
  open PRD questions). Architecture and task breakdown are grounded directly in
  the existing sibling implementation (`cmd_project_create`/`cmd_project_modify`,
  `client::send_command`, `handle_client_error`, and `handle_status`'s
  status/data wrapper convention).
- Added: D-1 (sync vs. async transport for `cmd_project_get`), D-2 (allowlist vs.
  denylist strategy for stripping the response wrapper).
- No Open Questions added; confidence meets the 90% threshold and both open
  decisions have low-risk recommended defaults that do not block implementation.

### Cycle 2 — Confidence: 100%
- Reconciled: D-1 → confirmed (synchronous `client::send_command` transport,
  already reflected in Architecture Overview/Components), D-2 → confirmed
  (allowlist-copy stripping in `format_project_get_output`, already reflected in
  Components/Data Model). Both decision blocks removed from Open Decisions.
- No new questions or decisions added; no open items remain. Specification is
  complete.
