# EP-NP-2 · Wire Protocol & IPC Types — Technical Specification

## Overview

Update `src/ipc/types.rs` to match the new tick-based wire protocol. `WireHeader`
gets `bpm: u32` and `loop_duration: u32`, replacing `bpm: f64` and
`time_signature: WireTimeSignature`. `WireTrack` gets `notes: Vec<[u32; 4]>`,
replacing `bars: Vec<WireBar>`. The three old wire types `WireTimeSignature`,
`WireBar`, and `WireNote` are deleted entirely. Unit tests covering all acceptance
criteria are added in `src/ipc/types.rs`. This epic depends on EP-NP-1 (domain
model) and is a prerequisite for EP-NP-4 (handler & validation).

**Confidence Level:** 93% — all structural changes, test scope, and handler
compilation strategy are fully specified; no open decisions or questions remain.

---

## Architecture Overview

All primary changes are confined to `src/ipc/types.rs`. The `Command` enum is
unchanged in structure: `CreateProject` and `ModifyProject` continue to carry
`header: WireHeader` and `tracks: Vec<WireTrack>`.

Serde's default behaviour (unknown fields are silently ignored) handles wire
format mixing gracefully: a payload that includes both old fields (`time_signature`,
`bars`) and the required new fields (`loop_duration`, `notes`) deserialises
successfully using only the new fields. No `#[serde(deny_unknown_fields)]` is
added.

Because `bpm` changes from `f64` to `u32`, serde rejects fractional values (e.g.
`120.5`) at the deserialization boundary for `create-project` and
`modify-project`. The handler returns `unknown_command` for such payloads (F-9);
the `bpm_non_integer` error path in `build_domain_project` becomes unreachable and
is removed in EP-NP-4.

`[u32; 4]` arrays are natively handled by serde_json: a JSON four-element integer
array deserialises to `[u32; 4]` without custom attributes.

**Handler compilation dependency**: `handler.rs` currently imports `WireBar`,
`WireNote`, and `WireTimeSignature` from types.rs and uses them in
`wire_bar_to_domain`, `wire_note_to_domain`, and `build_domain_project`. Deleting
these types breaks compilation of handler.rs. EP-NP-2 includes the minimal
handler.rs changes needed to restore compilation: dead imports are removed,
`wire_bar_to_domain` and `wire_note_to_domain` are deleted, and
`wire_track_to_domain` and `build_domain_project` are stubbed against the new
types. Handler tests that construct payloads in the old wire format are updated.
Full handler validation logic lands in EP-NP-4.

---

## Components

### `src/ipc/types.rs` (primary)

Owns all IPC deserialisation types. Specific changes:

- `WireHeader`: change `bpm: f64 → u32`; remove `time_signature: WireTimeSignature`;
  add `loop_duration: u32`
- `WireTrack`: remove `bars: Vec<WireBar>`; add `notes: Vec<[u32; 4]>`
- Delete struct definitions for `WireTimeSignature`, `WireBar`, `WireNote`
- Add unit tests for AC-1 through AC-4, AC-7, AC-8 in the existing `#[cfg(test)]`
  module

### `src/ipc/handler.rs` (minimal)

Remove dead imports (`WireBar`, `WireNote`, `WireTimeSignature`). Remove
`wire_bar_to_domain` and `wire_note_to_domain`. Stub `wire_track_to_domain` and
`build_domain_project` to compile against the new `WireHeader` and `WireTrack`
shapes. Update handler tests that construct payloads in the old wire format. Full
handler validation logic is EP-NP-4's scope.

---

## Data Model

| Type                | Fields                                                                    | Notes                                                     |
|---------------------|---------------------------------------------------------------------------|-----------------------------------------------------------|
| `WireHeader`        | `bpm: u32`, `loop_duration: u32`                                         | Replaces `bpm: f64` + `time_signature: WireTimeSignature` |
| `WireTrack`         | `name: String`, `channel: u8`, `instrument: u8`, `notes: Vec<[u32; 4]>` | Replaces `bars: Vec<WireBar>`                             |
| `WireTimeSignature` | — (deleted)                                                               | No replacement                                            |
| `WireBar`           | — (deleted)                                                               | No replacement                                            |
| `WireNote`          | — (deleted)                                                               | No replacement                                            |

`[u32; 4]` tuple layout: `[start_tick, duration, pitch, velocity]`. All four
elements are `u32` on the wire; EP-NP-4 casts `pitch` and `velocity` to `u8` when
constructing domain `Note` values.

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID   | Task                                                                                                                                     | Type | PRD ref               | Depends on     |
|------|------------------------------------------------------------------------------------------------------------------------------------------|------|-----------------------|----------------|
| T-1  | Write failing test: `create-project` with `loop_duration` and flat `notes` tuples deserialises to correct field values                  | test | AC-1, F-1, F-2, F-3  | —              |
| T-2  | Write failing test: `modify-project` with `loop_duration` and flat `notes` tuples deserialises to correct field values                  | test | AC-2, F-7             | —              |
| T-3  | Write failing test: `create-project` with `time_signature` but no `loop_duration` → serde error, no `Command` produced                 | test | AC-3, F-8             | —              |
| T-4  | Write failing test: `create-project` with `bars` but no `notes` → serde error, no `Command` produced                                   | test | AC-4, F-8             | —              |
| T-5  | Write failing test: `create-project` with `"bpm": 120.5` → serde error, no `Command` produced                                          | test | AC-7, F-9             | —              |
| T-6  | Write failing test: `create-project` with both `loop_duration` and legacy `time_signature` → success; `time_signature` silently ignored | test | AC-8, F-8             | —              |
| T-7  | Update `WireHeader`: change `bpm: f64 → u32`, remove `time_signature` field, add `loop_duration: u32`                                  | impl | F-1                   | T-1, T-2, T-5 |
| T-8  | Update `WireTrack`: remove `bars: Vec<WireBar>`, add `notes: Vec<[u32; 4]>`                                                            | impl | F-2, F-3              | T-7            |
| T-9  | Delete `WireTimeSignature`, `WireBar`, `WireNote` struct definitions from types.rs                                                      | impl | F-4, F-5, F-6         | T-8            |
| T-10 | Verify types.rs unit tests pass and cargo reports zero warnings for this module                                                         | impl | NF-1, NF-2, AC-5, AC-6 | T-9          |
| T-11 | Remove dead wire type imports from handler.rs; delete `wire_bar_to_domain` and `wire_note_to_domain`; stub `wire_track_to_domain` and `build_domain_project` to compile; update handler tests to new wire format | impl | NF-3 | T-9 |

---

## Open Questions

No open questions remain.

---

## Open Decisions

No open decisions remain.

---

## Revision Log

### Cycle 1 — Confidence: 82%
- Reconciled: none (spec created from PRD; no prior answered questions or decisions)
- Added: D-1 (handler.rs compilation scope — resolving this will raise confidence to ~92%)

### Cycle 2 — Confidence: 93%
- Reconciled: D-1 (A selected) → Architecture Overview updated (handler compilation strategy made explicit); Components section updated (handler.rs section no longer marked contingent); T-11 made unconditional; D-1 removed from Open Decisions
- Added: nothing (confidence ≥ 90%)
