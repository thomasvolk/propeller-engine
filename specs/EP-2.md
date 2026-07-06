# EP-2 · Position Query Protocol — PRD

## Overview

Add a `get_position` / `position` request–response pair to the IPC socket protocol
(newline-delimited JSON, `#[serde(tag = "type")]`). When the daemon receives a
`get_position` message, it reads the atomic tick counter from EP-1 and writes back a
`position` response containing the current tick and the loop duration of the active
project. This lets clients compute fractional playback progress without a separate
`status` call. Depends on EP-1.

**Confidence Level:** 93% — all questions resolved; minor residual: the `waiting`
engine-state query behaviour is derivable from F-5 and EP-1 but not stated as an
explicit AC.

---

## User Journeys

### UJ-1 · Client polls position during playback

A UI client connects to the IPC socket while the daemon plays a project. It sends
`{"type":"get_position"}` repeatedly at ~20 Hz and receives responses such as
`{"type":"position","tick":1234,"loop_duration":4800}`. It divides `tick` by
`loop_duration` to compute fractional progress and highlights the current step. When
`tick` snaps back to a value near 0 the client treats it as a loop boundary and resets
its visual indicator.

### UJ-2 · Client queries position before any project is loaded

A client connects before a project has been loaded. It sends `get_position` and receives
`{"type":"position","tick":0,"loop_duration":null}`. The client hides the progress bar
because there is no loop length to divide by.

### UJ-3 · Client verifies position resets after loop restart

A client sends `get_position` continuously through a loop boundary. It observes the tick
count approach `loop_duration`, then sees it return to 0 (or a very small value) in the
next response, confirming that the loop restarted correctly.

---

## Functional Requirements

| ID   | Requirement                                                                                                                        |
|------|------------------------------------------------------------------------------------------------------------------------------------|
| F-1  | The IPC protocol gains a `GetPosition` request variant serialised as `{"type":"get_position"}`.                                   |
| F-2  | The IPC protocol gains a `Position` response variant serialised as `{"type":"position","tick":<u64>,"loop_duration":<u64|null>}`. |
| F-3  | The `tick` field reflects the current value of the `current_tick` atomic counter defined in EP-1.                                 |
| F-4  | The `loop_duration` field contains the total tick count of the active project's loop (`store.active.loop_duration`).              |
| F-5  | `loop_duration` is `null` when no project is loaded (`store.active` is null).                                                     |
| F-6  | The handler reads both `tick` and `loop_duration` without acquiring the engine mutex or any other lock.                           |
| F-7  | Both variants are added to the existing `IpcMessage` enum using `#[serde(tag = "type")]`.                                         |
| F-8  | The handler is wired into the daemon's IPC dispatch loop alongside existing message handlers.                                     |
| F-9  | The daemon maintains a shared `Arc<AtomicU64>` mirror of the active project's `loop_duration`, updated atomically whenever `store.active` changes; a stored value of `0` encodes no project loaded and serialises to `null` in the response. |
| F-10 | The `position` response is a best-effort snapshot: `tick` and `loop_duration` are read in two independent atomic operations, and a project change between the two reads may produce a response where `tick` exceeds `loop_duration`; clients must tolerate this as a transient artefact. |

---

## Non-Functional Requirements

| ID   | Requirement                                                                                                                        |
|------|------------------------------------------------------------------------------------------------------------------------------------|
| NF-1 | The `get_position` handler must not acquire the engine mutex; both the `tick` and `loop_duration` read paths must be lock-free, reading from their respective `Arc<AtomicU64>` values. |
| NF-2 | The handler must not block the async executor; all socket writes use async primitives.                                             |
| NF-3 | No consistency guarantee is made between the `tick` and `loop_duration` fields in a single response; the two reads are independent and may reflect different project generations during a project change. |

---

## Acceptance Criteria

| ID   | Given                                                      | When                                                       | Then                                                                                    |
|------|------------------------------------------------------------|------------------------------------------------------------|-----------------------------------------------------------------------------------------|
| AC-1 | Daemon is running with a project loaded and playing        | Client sends `get_position` after ticks have advanced      | Response `tick` is > 0 and `loop_duration` equals the project's total tick count       |
| AC-2 | Daemon is playing and approaches the loop boundary         | Client polls `get_position` across a loop restart          | `tick` drops back to 0 (or a very small value) in the response after the restart       |
| AC-3 | No project is loaded (`store.active` is null)              | Client sends `get_position`                                | Response is `{"type":"position","tick":0,"loop_duration":null}`                         |
| AC-4 | Daemon is playing                                          | Client sends two `get_position` requests in sequence       | The second response `tick` is ≥ the first (monotonically non-decreasing within a loop) |
| AC-5 | Daemon is playing with a project loaded                    | Client sends `get_position` over a real Unix socket        | Response deserialises correctly and `tick` is a non-negative integer                    |
| AC-6 | Engine is paused with a project loaded                     | Client sends `get_position` repeatedly while paused        | `tick` does not advance across successive responses; `loop_duration` is non-null        |

---

## Open Questions

None — all questions resolved.

---

## Refinement Log

### Cycle 1 — Confidence: 65%
- Reconciled: nothing (PRD created from roadmap; allium spec confirms `loop_duration` unit is ticks)
- Added: Q1 (lock-free access mechanism for `loop_duration`), Q2 (paused-state response semantics)

### Cycle 2 — Confidence: 82%
- Reconciled: Q1 → F-9 (Arc<AtomicU64> mirror for loop_duration), NF-1 updated (covers both tick and loop_duration reads); Q2 → AC-6 (paused state: tick frozen, loop_duration non-null)
- Added: Q3 (snapshot consistency between the two independent atomic reads)

### Cycle 3 — Confidence: 93%
- Reconciled: Q3 → F-10 (best-effort snapshot semantics documented), NF-3 (no cross-field consistency guarantee)
- Added: none — PRD is complete
