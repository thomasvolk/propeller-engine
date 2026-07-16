# EP-1 · Project State Query over the Socket Interface — Technical Specification

## Overview

Add a `"project"` command to the daemon's existing socket protocol that reports the
current (active) and pending (staged-but-uncommitted) project, following the exact
pattern already used by the existing `"status"` command: a new unit variant on the
`Command` enum, a new handler function, and one new match arm in `dispatch`. No new
components, connections, or shared state are introduced — the feature extends the
existing `Command` enum, `ProjectStore`, and `dispatch` match block in place.

**Confidence Level:** 97% — PRD coverage, TDD ordering, architecture, and data model
are all fully specified against the real codebase, and both previously-open
implementation decisions are now resolved and reconciled below. No unresolved
decisions or questions remain; the only residual is a minor implementation detail
(exactly which field T-15's playback-continuity assertion polls), which is not a
specification gap.

---

## Architecture Overview

The request path is identical to every other socket command already handled by
`connection_handler`/`dispatch` in `src/ipc/handler.rs`: a client writes one JSON
line to the Unix socket, `dispatch` deserializes it into a `Command` variant (tagged
by the `"command"` field, kebab-case), and routes it to a handler function that
returns a `serde_json::Value` written back as the response line.

This epic adds exactly one new variant, `Command::Project`, matching the literal
`{"command":"project"}` (F-6) via the enum's existing
`#[serde(tag = "command", rename_all = "kebab-case")]` — no custom rename needed
since the name is already a single lowercase word. `dispatch` gains one new match
arm calling a new handler, `handle_get_project` (named to avoid colliding with the
existing `handle_project`, which handles `Command::ModifyProject`).

`handle_get_project` takes only `&Arc<RwLock<ProjectStore>>` — deliberately omitting
the `&Arc<LoopEngine>` and `&Arc<Mutex<EngineSettings>>` parameters that
`handle_status` takes. This is the key architectural decision that satisfies F-7/AC-6
(mode-independence) and NF-3/AC-7 (non-blocking, no shared execution context with
playback) *structurally*: the handler has no way to branch on `EngineMode` because
it never receives it, and it has no way to touch anything the playback loop touches
because `ProjectStore` is the only shared state it's given. It acquires a single
`store.read().unwrap()` guard, reads `active()` and the new `pending()` accessor from
it, and builds the response — mirroring `handle_status`'s pattern of conditionally
inserting keys (`loop_duration` there; `"current"`/`"pending"` here) rather than
emitting `null` or empty placeholders (F-2, F-4).

Converting a domain `Project` to its JSON representation requires new code: `Project`
and its nested types currently have no `Serialize` derive at all (the wire format
only derives `Deserialize`, on the separate `WireHeader`/`WireTrack` structs used
for *input*). A new function, `project_to_json`, fills this gap as the structural
inverse of the existing `build_domain_project`/`wire_track_to_domain` pair, and
produces exactly the shape `WireTrack` deserializes from — array-tuple notes and
kebab-cased `"pitch-bends"` — so F-1's requirement ("the same shape a client would
send to load a project") is satisfied by construction: a client can take the
`"current"` object out of a `project` response and resubmit it verbatim as
`create-project`'s `tracks` field.

No new component is spawned, no new lock is introduced, and no existing handler
changes behavior — this is a pure extension of the existing `Command`/`dispatch`
surface plus one new read-only accessor on `ProjectStore`.

---

## Components

### `Command::Project` (`src/ipc/types.rs`)

New unit variant added to the existing `Command` enum. Deserializes from
`{"command":"project"}` via the enum's existing `rename_all = "kebab-case"` tag —
no per-variant `#[serde(rename = ...)]` needed. (F-6)

### `ProjectStore::pending()` (`src/domain/store.rs`)

New read-only accessor, mirroring the existing `active()`:

```rust
pub fn pending(&self) -> Option<&Project> {
    self.pending.as_ref()
}
```

Unlike `commit_pending()` (which consumes the pending project via `.take()`), this
accessor observes it without disturbing store state — required for NF-1 (a query
must not mutate current/pending as a side effect).

### `project_to_json` (`src/ipc/handler.rs`)

New free function, placed next to `build_domain_project` as its structural inverse.
Converts `&Project` into a `serde_json::Value`:

- `header` → `{"bpm": <u32>, "loop_duration": <u32>}`
- each track → `{"name": <String>, "channel": <u8>, "instrument": <u8>, "notes": [[start_tick, duration, pitch, velocity], ...], "pitch-bends": [[tick, value], ...]}`

This exactly mirrors `WireHeader`/`WireTrack`'s `Deserialize` shape (including the
kebab-cased `"pitch-bends"` key and the four/two-element array-tuple encoding for
notes/pitch bends), giving F-1/F-3's "complete data" round-trip symmetry with
`create-project`/`modify-project` for free.

### `handle_get_project` (`src/ipc/handler.rs`)

New handler, added alongside `handle_status`:

```rust
fn handle_get_project(store: &Arc<RwLock<ProjectStore>>) -> Value
```

Acquires one `store.read().unwrap()` guard, builds `{"status": "ok"}`, then
conditionally sets `resp["current"]` and/or `resp["pending"]` via `project_to_json`
only when the corresponding accessor returns `Some` — omitting the key entirely
otherwise (F-2, F-4). Takes no `engine`/`settings` parameter, so its output cannot
vary by `EngineMode` (F-7/AC-6) and it cannot block on or share state with the
playback path (NF-3/AC-7). This structural guarantee is additionally exercised by a
dedicated integration test (T-15) that queries `"project"` repeatedly while playback
is running, giving a regression signal beyond code review alone if that guarantee is
ever broken.

### `dispatch` (`src/ipc/handler.rs`)

One new match arm added to the existing block:

```rust
Command::Project => handle_get_project(store),
```

---

## Data Model

| Type                     | Fields                                                                                                    | Notes                                                                                                             |
|--------------------------|------------------------------------------------------------------------------------------------------------|---------------------------------------------------------------------------------------------------------------------|
| `Command::Project`       | (unit variant, no fields)                                                                                  | New; deserializes `{"command":"project"}` (F-6)                                                                     |
| `Project`/`Header`/`Track`/`Note`/`PitchBend` | unchanged — `src/domain/project.rs`                                                                        | Existing domain types, reused as-is; no new derives added to the domain layer                                      |
| `ProjectStore::pending()`| `fn pending(&self) -> Option<&Project>`                                                                    | New accessor mirroring `active()`; does not consume the staged project (unlike `commit_pending`)                   |
| `project_to_json`        | `fn project_to_json(project: &Project) -> Value`                                                           | New; produces the same shape `WireHeader`/`WireTrack` deserialize from (F-1)                                        |
| Response envelope        | `{"status": "ok", "current"?: <project>, "pending"?: <project>}`                                           | `"current"`/`"pending"` present only when `ProjectStore` returns `Some`; omitted entirely otherwise (F-2, F-4)      |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID    | Task                                                                                                                              | Type | PRD ref              | Depends on |
|-------|-------------------------------------------------------------------------------------------------------------------------------------|------|-----------------------|------------|
| T-1   | Unit test: `ProjectStore::pending()` returns `None` on a fresh store and `Some(&Project)` after `set_pending` (before commit)       | test | (supports F-3, F-4)  | —          |
| T-2   | Implement `ProjectStore::pending()` accessor in `src/domain/store.rs`                                                               | impl | (supports F-3, F-4)  | T-1        |
| T-3   | Unit test: `project_to_json` maps a `Project` (header, one track, one note, one pitch bend) to the exact `WireHeader`/`WireTrack`-mirrored JSON shape, including array-tuple notes and `"pitch-bends"` key | test | F-1, F-3 | —          |
| T-4   | Implement `project_to_json` in `src/ipc/handler.rs`                                                                                 | impl | F-1, F-3              | T-3        |
| T-5   | Unit test: `{"command":"project"}` deserializes to `Command::Project`                                                               | test | F-6                   | —          |
| T-6   | Add `Command::Project` unit variant to `src/ipc/types.rs`                                                                           | impl | F-6                   | T-5        |
| T-7   | Handler test (via `connection_handler`/`UnixStream::pair`, model: `status_no_project`): no project ever active or pending → response has `"status":"ok"` and omits both `"current"` and `"pending"` keys | test | F-2, F-4, AC-2, AC-4 | T-2, T-4, T-6 |
| T-8   | Handler test: only an active project exists (after `create-project`) → response includes `"current"` with correct data, omits `"pending"` | test | F-1, F-4, AC-1, AC-4 | T-2, T-4, T-6 |
| T-9   | Handler test: an active project exists and a second project is staged via `modify-project` without committing → response includes both `"current"` (original) and `"pending"` (staged), each with correct distinct data | test | F-3, F-5, AC-3, AC-5 | T-2, T-4, T-6 |
| T-10  | Implement `handle_get_project(store: &Arc<RwLock<ProjectStore>>) -> Value` in `src/ipc/handler.rs`, satisfying T-7/T-8/T-9           | impl | F-1, F-2, F-3, F-4, F-5 | T-7, T-8, T-9 |
| T-11  | Wire `Command::Project => handle_get_project(store)` into the `dispatch` match block                                                | impl | F-6                   | T-6, T-10  |
| T-12  | Handler test: identical `ProjectStore` state (one active, one pending) queried once with `EngineSettings.mode = Standalone` and once with `Sync` → byte-identical response both times | test | F-7, AC-6, AC-7, NF-3 | T-11       |
| T-13  | Handler test: invoking the `"project"` query (once, and repeated) never changes `ProjectStore::active()`/`pending()` — no mutation as a side effect | test | NF-1                  | T-11       |
| T-14  | Integration test in `tests/integration.rs` (model: `runtime_protocol_create_start_status_stop`) using `DaemonGuard`: `create-project` + `modify-project` + `{"command":"project"}` → assert `resp["current"]` and `resp["pending"]` both correct through the real socket/daemon subprocess | test | F-1, F-3, F-5 (regression) | T-11       |
| T-15  | Integration test in `tests/integration.rs` using `DaemonGuard`: `create-project`, then `loop-start`, then issue repeated `{"command":"project"}` queries while playback runs — assert each response returns promptly (no hang) and a subsequent `status` query still reports `clock_state:"started"`, confirming playback was unaffected | test | NF-3, AC-7 (regression) | T-11 |

---

## Open Questions

None.

---

## Open Decisions

None.

---

## Revision Log

### Cycle 1 — Confidence: 82%
- Created spec from `specs/EP-1.md` (no prior spec existed); architecture, components, and data model derived from the actual codebase (`src/ipc/types.rs`, `src/ipc/handler.rs`, `src/domain/store.rs`, `src/domain/project.rs`, `tests/integration.rs`), reusing the existing `Command`/`dispatch`/`handle_status` patterns rather than introducing new ones.
- Added: D-1 (domain-to-JSON conversion approach), D-2 (whether NF-3/AC-7 needs a dedicated concurrency test).

### Cycle 2 — Confidence: 97%
- Reconciled: D-1 → B confirmed (manual `project_to_json` builder in the ipc layer; no content change needed, spec already reflected this), D-2 → A confirmed (added T-15, a playback-concurrency integration test, and referenced it from the NF-3/AC-7 paragraph in Architecture Overview).
- No new questions added; specification is complete.
