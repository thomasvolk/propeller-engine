# EP-NP-2 · Wire Protocol & IPC Types — PRD

## Overview

Update the JSON deserialization layer in `src/ipc/types.rs` to match the new
tick-based wire protocol. The bar-based types `WireTimeSignature`, `WireBar`,
and `WireNote` are removed. `WireHeader` replaces the `time_signature` object
field with a scalar `loop_duration: u32` and changes `bpm` from `f64` to `u32`.
`WireTrack` replaces `bars: Vec<WireBar>` with `notes: Vec<[u32; 4]>`, where
each tuple encodes `[start_tick, duration, pitch, velocity]`. This epic depends
on EP-NP-1 (domain model) and is itself a prerequisite for EP-NP-4 (handler &
validation).

**Confidence Level:** 93% — all structural changes and rejection semantics are
fully specified; the only residual is a downstream EP-NP-4 concern (the
`bpm_non_integer` handler path becomes unreachable when `bpm` is `u32`).

---

## User Journeys

### UJ-1 · CLI sends `create-project` with the new wire format

A client serialises a project as JSON with `header.loop_duration` and a flat
`notes` array of four-element integer arrays per track. The daemon's
deserialization layer accepts the payload and populates `WireHeader` and
`Vec<WireTrack>` correctly so that the handler can proceed to build the domain
model.

### UJ-2 · CLI sends `modify-project` with the new wire format

Same as UJ-1 but via the `modify-project` command. The new wire types are
identical between the two commands; both must deserialise the same structure.

### UJ-3 · Legacy client sends `create-project` with the old format

A client using the old protocol sends `header.time_signature` and
`tracks[].bars`. Because `loop_duration` and `notes` are now required fields,
serde fails to deserialise the payload and the handler returns an error
response.

---

## Functional Requirements

| ID   | Requirement                                                                                                                                                                                                         |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| F-1  | `WireHeader` contains `bpm: u32` and `loop_duration: u32`. The `time_signature` field is removed.                                                                                                                  |
| F-2  | `WireTrack` contains `name: String`, `channel: u8`, `instrument: u8`, and `notes: Vec<[u32; 4]>`. The `bars` field is removed.                                                                                    |
| F-3  | The `[u32; 4]` tuple layout is `[start_tick, duration, pitch, velocity]`, matching the briefing exactly.                                                                                                           |
| F-4  | `WireTimeSignature` is deleted from `src/ipc/types.rs` with no replacement.                                                                                                                                        |
| F-5  | `WireBar` is deleted from `src/ipc/types.rs` with no replacement.                                                                                                                                                  |
| F-6  | `WireNote` is deleted from `src/ipc/types.rs` with no replacement.                                                                                                                                                 |
| F-7  | The `Command::CreateProject` and `Command::ModifyProject` variants continue to carry `header: WireHeader` and `tracks: Vec<WireTrack>`.                                                                             |
| F-8  | A payload where `loop_duration` is absent fails deserialization because `loop_duration` is a required field. A payload where `notes` is absent fails because `notes` is required. A payload that contains both old and new fields succeeds if all required new fields are present; `serde_json` silently ignores unknown extra fields (`time_signature`, `bars`). No `#[serde(deny_unknown_fields)]` is added. |
| F-9  | Because `bpm` is now `u32`, serde rejects a fractional value such as `120.5` at the deserialization boundary; the IPC handler returns `unknown_command` (not `bpm_non_integer`) for such payloads. EP-NP-4 must account for the `bpm_non_integer` handler path becoming unreachable. |

---

## Non-Functional Requirements

| ID   | Requirement                                                                                                                                    |
| ---- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| NF-1 | The change must not introduce any compiler warnings (`cargo build` and `cargo test` produce zero warnings).                                    |
| NF-2 | All pre-existing unit tests in `src/ipc/types.rs` are updated or replaced. No test is silently deleted without a replacement.                  |
| NF-3 | `cargo test` passes after this epic lands, independently of EP-NP-3, EP-NP-4, and EP-NP-5.                                                    |
| NF-4 | Unit tests covering AC-1 through AC-4 are written in `src/ipc/types.rs` as part of this epic; they must be present at merge time. EP-NP-4 handler tests may build on top of but must not replace this coverage. |

---

## Acceptance Criteria

| ID   | Given                                                                                               | When                       | Then                                                                                    |
| ---- | --------------------------------------------------------------------------------------------------- | -------------------------- | --------------------------------------------------------------------------------------- |
| AC-1 | A `create-project` JSON with `header.loop_duration` and a flat `notes` array per track             | deserialized               | `WireHeader.loop_duration` holds the correct value; `WireTrack.notes` holds the tuples |
| AC-2 | A `modify-project` JSON with `header.loop_duration` and a flat `notes` array per track             | deserialized               | Same as AC-1                                                                            |
| AC-3 | A `create-project` JSON that omits `loop_duration` but includes `time_signature`                   | deserialized               | Serde returns an error; no `Command` value is produced                                  |
| AC-4 | A `create-project` JSON that includes `bars` instead of `notes`                                    | deserialized               | Serde returns an error; no `Command` value is produced                                  |
| AC-5 | The codebase is compiled                                                                            | `cargo build` runs         | `WireTimeSignature`, `WireBar`, `WireNote` are absent and the build succeeds            |
| AC-6 | `cargo test` is run after this epic lands                                                           | no other epics are applied | All tests pass; zero compiler warnings                                                  |
| AC-7 | A `create-project` JSON with `"bpm": 120.5`                                                        | deserialized               | Serde returns an error at the `Command` boundary; no `Command` value is produced        |
| AC-8 | A `create-project` JSON with both `loop_duration` and a legacy `time_signature` field              | deserialized               | Deserialization succeeds; `time_signature` is silently ignored; `loop_duration` is read correctly |

---

## Open Questions

No open questions remain. The PRD is complete.

---

## Refinement Log

### Cycle 1 — Confidence: 62%

- Reconciled: none (PRD created from roadmap; no prior answered questions)
- Added: Q1 (bpm wire type), Q2 (old-format rejection semantics), Q3 (test scope)

### Cycle 2 — Confidence: 72%

- Reconciled: Q3 → NF-4 (unit tests for AC-1–AC-4 must live in types.rs, within this epic)
- Added: none (Q1 and Q2 remain the binding gaps)

### Cycle 3 — Confidence: 93%

- Reconciled: Q1 (B) → F-1 updated (bpm: u32), F-9 added (fractional bpm → unknown_command), AC-7 added; Q2 (A) → F-8 updated (serde default, no deny_unknown_fields), AC-8 added; Q3 body removed (was already reconciled to NF-4 in Cycle 2, body not deleted then)
- Added: none (confidence ≥ 90%)
