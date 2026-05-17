# EP-4 · Runtime Interface — PRD

## Overview

A protocol allows external clients to send commands to and query status from the engine at runtime, without restarting the daemon. The protocol runs over the Unix domain socket established by EP-1 and remains available for the full lifetime of the daemon. Commands and responses are newline-terminated JSON objects exchanged in a one-command-per-connection model. Each request carries a `"command"` field that identifies the command type; responses carry a `"status"` field of `"ok"` or `"error"`. Supported commands cover project management, BPM and time signature control, loop start/stop, mode switching, and status queries.

**Confidence Level:** 92% — All roadmap requirements are covered and all open questions are reconciled. The PRD is complete. The fine-grained command string naming convention and status response field names are intentionally deferred to the technical specification.

---

## User Journeys

### UJ-1 · Creating a project via the runtime interface

A performer connects a client to the engine's socket and sends a create-project command as a newline-terminated JSON object. The engine validates and accepts the project, stores it as the active project, and returns `{"status": "ok"}`. The connection is then closed.

### UJ-2 · Modifying a running project

While the loop is playing, a performer sends a modify-project command containing the complete new project definition. The engine queues the update, finishes the current bar, applies the replacement at the next bar boundary, and returns `{"status": "ok"}`.

### UJ-3 · Setting BPM via the runtime interface

A performer sends a set-BPM command with a new integer value in range 20–300. The engine updates the BPM, returns `{"status": "ok"}`, and the running loop adjusts its tempo immediately (per EP-3 F-11).

### UJ-4 · Setting time signature via the runtime interface

A performer sends a set-time-signature command. The engine validates and updates the time signature and returns a success or structured error response.

### UJ-5 · Setting operating mode via the runtime interface

A performer sends a set-mode command. The engine transitions to the specified mode (clock, sync, or standalone) and returns `{"status": "ok"}`.

### UJ-6 · Querying current engine status

A performer sends a status query. The engine responds with a JSON object containing at minimum the current operating mode, BPM, time signature, clock state, and whether an active project is present.

### UJ-7 · Sending an invalid command

A performer sends a malformed or semantically invalid command (e.g. a project with an out-of-range BPM). The engine responds with `{"status": "error", "code": "...", "message": "..."}`. The daemon continues running normally.

### UJ-8 · Starting and stopping the loop via the runtime interface

A performer loads a project and then sends a loop-start command. The loop begins playing. Later they send a loop-stop command. The loop halts. Both commands return `{"status": "ok"}`.

---

## Functional Requirements

| ID | Requirement |
|----|-------------|
| F-1 | The engine accepts commands from external clients at runtime without restarting the daemon. |
| F-2 | The protocol runs over the Unix domain socket at `/tmp/propeller.sock` established by EP-1. |
| F-3 | The protocol is available for the full lifetime of the daemon; it becomes available when the socket is created on startup and ceases when the socket is removed on shutdown. |
| F-4 | The engine accepts a create-project command that defines the full project (header and tracks). |
| F-5 | The engine accepts a modify-project command that replaces the entire active project with a complete new project definition; partial updates to individual tracks, bars, or notes are not supported. |
| F-6 | The engine accepts a set-BPM command. |
| F-7 | The engine accepts a set-time-signature command. |
| F-8 | The engine accepts a set-mode command. |
| F-9 | The engine accepts a status query command and responds with at minimum: the current operating mode, current BPM, current time signature, clock state, and whether an active project is present. |
| F-10 | The engine validates every incoming command and rejects invalid commands with a structured error response without affecting the running daemon state. |
| F-11 | The structured error response identifies the type of error and provides a human-readable description. This format is the one referenced by EP-2 F-15. |
| F-12 | Commands and responses are encoded as JSON objects, each terminated by a single newline character (`\n`). No other wire format is supported. |
| F-13 | Each client connection carries exactly one command and receives exactly one response. The engine closes its end of the connection after sending the response. |
| F-14 | The "clock state" field in the status response reports the loop engine's running/stopped state as defined by EP-3 F-9: "started" if the loop is running, "stopped" otherwise. |
| F-15 | The protocol is strictly request-response; the engine never sends unsolicited data to a connected client. Clients must poll via the status query command to observe engine state changes. |
| F-16 | The engine accepts a loop-start command. When received, the loop engine transitions to the running state (per EP-3 F-9 and F-15). |
| F-17 | The engine accepts a loop-stop command. When received, the loop engine transitions to the stopped state (per EP-3 F-10 and F-14). |
| F-18 | Every request JSON object must include a `"command"` string field whose value identifies the command type. Requests missing this field are rejected with an error response. |
| F-19 | A success response is a JSON object with at minimum `{"status": "ok"}`. Status query responses include additional fields describing the queried state. |
| F-20 | An error response is a JSON object with `{"status": "error", "code": "<machine-readable identifier>", "message": "<human-readable description>"}`. |

---

## Non-Functional Requirements

| ID | Requirement |
|----|-------------|
| NF-1 | Command processing must not introduce scheduling delays that would violate the timing guarantees of the loop engine (EP-3 NF-1). |
| NF-2 | The wire format and connection model must be defined precisely enough that a client can be implemented independently of the engine. |

---

## Acceptance Criteria

| ID | Given | When | Then |
|----|-------|------|------|
| AC-1 | The engine is running | a valid create-project command is sent | the engine accepts the project, stores it as the active project, and returns `{"status": "ok"}` |
| AC-2 | The engine has an active project | a valid modify-project command is sent | the engine returns `{"status": "ok"}` and the replacement project takes effect at the next bar boundary |
| AC-3 | The engine is running | a set-BPM command with a valid value is sent | the engine updates the BPM and returns `{"status": "ok"}` |
| AC-4 | The engine is running | a set-time-signature command with a valid value is sent | the engine updates the time signature and returns `{"status": "ok"}` |
| AC-5 | The engine is running | a set-mode command is sent | the engine transitions to the specified mode and returns `{"status": "ok"}` |
| AC-6 | The engine is running | a status query is sent | the engine returns a JSON object with the current mode, BPM, time signature, clock state, and project presence |
| AC-7 | The engine is running | a malformed or semantically invalid command is sent | the engine returns a structured error response and continues running normally |
| AC-8 | The engine is running | a project with an out-of-range BPM is submitted | the engine returns a structured error identifying the invalid field |
| AC-9 | The engine is running | a valid command is sent as a JSON object followed by a newline | the engine responds with a JSON object followed by a newline |
| AC-10 | The engine is running | a client connects, sends one command, and reads the response | the engine closes the connection after sending the response; a new connection is required for the next command |
| AC-11 | The loop is running | a status query is sent | the clock_state field in the response is "started"; when the loop is stopped, it is "stopped" |
| AC-12 | An active project exists | a modify-project command with a complete new project definition is sent | the engine replaces the entire previous project (change takes effect at the next bar boundary) |
| AC-13 | The engine is running | a client connects but sends no data | the engine sends no data until it receives a complete newline-terminated JSON command |
| AC-14 | The loop is stopped and an active project is present | a loop-start command is sent | the engine transitions to a running state and returns `{"status": "ok"}` |
| AC-15 | The loop is running | a loop-stop command is sent | the engine halts playback and returns `{"status": "ok"}` |
| AC-16 | A valid command is processed | the engine sends its response | the JSON response object contains `"status": "ok"` |
| AC-17 | An invalid command is processed | the engine sends its response | the JSON response object contains `"status": "error"`, a `"code"` field, and a `"message"` field |
| AC-18 | A request JSON object is received with no `"command"` field | the engine processes it | the engine returns an error response |

---

## Open Questions

No open questions. All questions have been reconciled.

---

## Refinement Log

### Cycle 1 — Confidence: 45%
- Reconciled: nothing (PRD created from roadmap)
- Added: Q-1 (wire format), Q-2 (connection model), Q-3 (clock state definition), Q-4 (project modification granularity), Q-5 (push notifications)

### Cycle 2 — Confidence: 72%
- Reconciled: Q-1 → F-12 (JSON newline-delimited), AC-9; Q-2 → F-13 (one command per connection), AC-10; Q-3 → F-14 (clock state = loop running/stopped), AC-11; Q-4 → F-5 refined (full project replace, no partial updates), AC-12; Q-5 → F-15 (strictly request-response), AC-13
- Added: Q-6 (loop start/stop commands), Q-7 (command envelope and response shape)

### Cycle 3 — Confidence: 92%
- Reconciled: Q-6 → F-16/F-17 (loop-start and loop-stop commands), UJ-8, AC-14/AC-15; Q-7 → F-18 ("command" field envelope), F-19 (success shape), F-20 (error shape), AC-16/AC-17/AC-18
- Added: none — confidence 92%, PRD is complete
