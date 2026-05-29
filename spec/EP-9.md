# EP-9 · CLI Convenience Commands — PRD

## Overview

Interacting with the running propeller-engine daemon currently requires manually constructing JSON and piping it through `nc -U /tmp/propeller.sock`. This epic introduces a `propeller` CLI binary that wraps the most common runtime operations — loading a project and controlling loop playback — behind ergonomic subcommands. The CLI communicates with the daemon over the existing Unix-domain socket protocol defined in EP-4. The epic also provides `propeller midi ports`, a standalone utility (no daemon required) that lists all available MIDI output ports so the performer can identify the correct port name before starting the daemon.

**Confidence Level:** 92% — All requirements are fully specified, all user paths are represented, and all ACs are testable. No open questions remain.

---

## User Journeys

### UJ-1 · Loading a project from a file

A performer has written a project as a JSON file containing `header` and `tracks` fields. They run `propeller project create myproject.json`. The CLI wraps the content in a `create-project` command, sends it to the daemon via the socket, and exits silently. The performer can immediately start the loop without any manual socket interaction.

### UJ-2 · Loading a project from stdin

A performer generates a project dynamically with a script and pipes it directly: `generate-project.sh | propeller project create`. The CLI reads stdin until EOF, wraps the content in a `create-project` command, sends it to the daemon, and exits silently.

### UJ-3 · Starting the loop

After loading a project, a performer runs `propeller loop start`. The CLI sends a `loop-start` command to the daemon and exits silently. The loop begins playing.

### UJ-4 · Stopping the loop

A performer runs `propeller loop stop`. The CLI sends a `loop-stop` command to the daemon and exits silently. The loop halts.

### UJ-5 · Daemon not running

A performer runs a CLI command when the daemon is not running. The CLI fails to connect to the socket, writes a human-readable error to stderr, and exits with a non-zero code.

### UJ-6 · Updating a running project

While the loop is playing, a performer edits their project JSON and runs `propeller project modify myproject.json`. The CLI sends a `modify-project` command; the daemon queues the update and applies it at the next bar boundary without interrupting playback. The CLI exits silently.

### UJ-7 · Discovering available MIDI ports

Before starting the daemon, a performer wants to know the exact name of their MIDI device. They run `propeller midi ports`. All available MIDI output ports are listed, one per line. The performer copies the exact name and uses it with `PROPELLER_MIDI_PORT` when starting the daemon. No daemon connection is required.

---

## Functional Requirements

| ID | Requirement |
|----|-------------|
| F-1 | A `propeller` CLI binary is available on the path. |
| F-2 | The CLI subcommand `project create [<filename>]` sends a `create-project` command to the daemon. |
| F-3 | When `<filename>` is provided, the CLI reads the project from that file. This applies to both `project create` and `project modify`. |
| F-4 | When `<filename>` is omitted, the CLI reads the project from stdin until EOF. This applies to both `project create` and `project modify`. |
| F-5 | The CLI subcommand `loop start` sends a `loop-start` command to the daemon. |
| F-6 | The CLI subcommand `loop stop` sends a `loop-stop` command to the daemon. |
| F-7 | The CLI connects to the daemon via the Unix-domain socket resolved by the same logic as the daemon: `PROPELLER_SOCK` env var if set, `/tmp/propeller.sock` otherwise. |
| F-8 | If the CLI cannot connect to the socket, it writes a human-readable error to stderr and exits with a non-zero code. |
| F-9 | The input for `project create` and `project modify` is a JSON object containing `header` and `tracks` fields (no `"command"` field). The CLI constructs the protocol command envelope and sends the fully-formed JSON to the daemon. |
| F-10 | The `propeller` CLI is a standalone binary separate from `propeller-engine`, built within the same Cargo workspace. |
| F-11 | On a successful response from the daemon, the CLI produces no output on stdout or stderr and exits with code 0. |
| F-12 | The CLI subcommand `project modify [<filename>]` sends a `modify-project` command to the daemon. The update takes effect at the next bar boundary as defined by EP-4. |
| F-13 | The CLI subcommand `midi ports` lists all available MIDI output ports, printing each port name on its own line, and exits with code 0. No daemon connection is required. |

---

## Non-Functional Requirements

| ID | Requirement |
|----|-------------|
| NF-1 | The CLI must not introduce unnecessary latency; it connects, sends the command, reads the response, and exits. No background threads or persistent connections. |

---

## Acceptance Criteria

| ID | Given | When | Then |
|----|-------|------|------|
| AC-1 | The daemon is running and a valid project file exists | `propeller project create myproject.json` is run | the daemon receives a `create-project` command and the CLI exits with code 0 |
| AC-2 | The daemon is running and a valid project JSON is piped | `cat myproject.json \| propeller project create` is run | the daemon receives a `create-project` command and the CLI exits with code 0 |
| AC-3 | The daemon is running and a project is loaded | `propeller loop start` is run | the daemon receives a `loop-start` command and the CLI exits with code 0 |
| AC-4 | The daemon is running and the loop is playing | `propeller loop stop` is run | the daemon receives a `loop-stop` command and the CLI exits with code 0 |
| AC-5 | The daemon is not running | any `propeller` subcommand is run | the CLI prints a human-readable error to stderr and exits with a non-zero code |
| AC-6 | The daemon returns an error response | any `propeller` subcommand is run | the CLI reports the error to stderr and exits with a non-zero code |
| AC-7 | `PROPELLER_SOCK` is set to a custom path | any `propeller` subcommand is run | the CLI connects to that path instead of `/tmp/propeller.sock` |
| AC-8 | A project file contains `{"header": {...}, "tracks": [...]}` with no `"command"` field | `propeller project create <filename>` or `propeller project modify <filename>` is run | the daemon receives a correctly-wrapped `create-project` or `modify-project` command respectively |
| AC-9 | The daemon returns a success response | any `propeller` subcommand is run | the CLI produces no output on stdout or stderr and exits with code 0 |
| AC-10 | The daemon is running and a valid project file exists | `propeller project modify myproject.json` is run | the daemon receives a `modify-project` command and the CLI exits with code 0 |
| AC-11 | The daemon is running and a valid project JSON is piped | `cat myproject.json \| propeller project modify` is run | the daemon receives a `modify-project` command and the CLI exits with code 0 |
| AC-12 | The daemon is not running | `propeller midi ports` is run | all available MIDI output ports are listed on stdout, one per line, and the CLI exits with code 0 |

---

## Open Questions

No open questions. All questions have been reconciled.

---

## Refinement Log

### Cycle 1 — Confidence: 55%
- Reconciled: nothing (PRD created from user description and codebase analysis)
- Added: Q-1 (project file input format), Q-2 (binary packaging), Q-3 (CLI output behaviour)

### Cycle 2 — Confidence: 82%
- Reconciled: Q-1 → F-9 (input is project JSON without command wrapper), AC-8; Q-2 → F-10 (standalone binary in same workspace); Q-3 → F-11 (silent on success, exit 0), AC-9
- Added: Q-4 (which protocol command `project create` maps to)

### Cycle 3 — Confidence: 92%
- Reconciled: Q-4 → F-2 (`project create` sends `create-project`), F-12 (`project modify` sends `modify-project`), UJ-6 (updating a running project), AC-10, AC-11; F-3/F-4/F-9 updated to cover both subcommands
- Added: none — confidence 92%, PRD is complete

### Cycle 4 — Confidence: 92%
- Added: UJ-7 (discovering MIDI ports), F-13 (`midi ports` subcommand), AC-12 (ports listed without daemon) — moved from EP-6
