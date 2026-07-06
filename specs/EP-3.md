# EP-3 · Position CLI — PRD

## Overview

Add a `loop position` subcommand to the CLI that queries the current tick position from the daemon.
Without `--poll` the command prints one line and exits, suitable for scripting. With `--poll` it
queries repeatedly at the interval controlled by `--interval-ms` (default 50 ms, ~20 Hz) until the
user interrupts with Ctrl-C. Each output line takes the form `{tick}/{loop_duration}` when a project
is loaded, or `{tick}/-` when no project is active.

**Confidence Level:** 92% — all roadmap requirements, error paths, and output contracts are fully
specified; no open questions remain.

---

## User Journeys

### UJ-1 · Script reads current position once

A shell script or one-shot tool invokes `propeller loop position`, receives a single line such as
`1234/480`, parses the tick and loop duration, computes a fractional position, and exits. The script
relies on a predictable exit code and a single well-formed output line with no extraneous output on
stdout.

### UJ-2 · UI drives a visual progress bar by polling

A desktop UI launches `propeller loop position --poll` as a child process and reads its stdout line
by line at ~20 Hz. It uses each `tick/loop_duration` pair to update a progress indicator. When the
user closes the UI, it sends SIGINT to the child process, which exits cleanly with code 0.

### UJ-3 · Operator sets a custom refresh rate

An operator needs a slower refresh to avoid flooding a log file, and invokes
`propeller loop position --poll --interval-ms 100` to receive position updates at ~10 Hz.

---

## Functional Requirements

| ID   | Requirement                                                                                                                                          |
|------|------------------------------------------------------------------------------------------------------------------------------------------------------|
| F-1  | The CLI exposes a `loop position` subcommand parsed with `clap` (derive API).                                                                        |
| F-2  | Without `--poll`, the command sends one `get_position` IPC request, prints one output line, and exits with code 0.                                   |
| F-3  | The output line format is `{tick}/{loop_duration}` where `tick` is a non-negative decimal integer and `loop_duration` is a positive decimal integer. |
| F-4  | When no project is active the output line format is `{tick}/-`.                                                                                      |
| F-5  | `--poll` is a boolean flag; when set, the command enters a loop that queries the daemon and prints one line per iteration, running until interrupted. |
| F-6  | `--interval-ms <N>` sets the poll interval in milliseconds; the default value is 50. The flag is only meaningful in `--poll` mode.                   |
| F-7  | The poll loop is implemented with `tokio::time::interval`; it does not busy-wait.                                                                    |
| F-8  | In `--poll` mode, receipt of SIGINT causes the loop to terminate and the process to exit with code 0.                                                |
| F-9  | When the daemon socket is not reachable in single-shot mode, the CLI prints a human-readable diagnostic to stderr and exits with code 1.             |
| F-10 | In `--poll` mode, the CLI opens a new IPC connection for each query iteration and closes it before the next interval fires.                          |
| F-11 | `--interval-ms` passed without `--poll` is accepted by the CLI without error and has no effect on the single-shot query.                             |
| F-12 | In `--poll` mode, if a connection attempt fails, the CLI prints a human-readable diagnostic to stderr and exits with code 1.                         |

---

## Non-Functional Requirements

| ID   | Requirement                                                                                 |
|------|---------------------------------------------------------------------------------------------|
| NF-1 | Single-shot mode must print its output line and exit within 500 ms under normal conditions. |
| NF-2 | The poll loop must use `tokio::time::interval` and must not busy-wait between queries.      |

---

## Acceptance Criteria

| ID   | Given                                          | When                                                    | Then                                                                                                                          |
|------|------------------------------------------------|---------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------|
| AC-1 | Daemon running, project loaded                 | `loop position` is invoked without `--poll`             | Exactly one line matching `\d+/\d+` is printed to stdout; process exits with code 0.                                         |
| AC-2 | Daemon running, no project loaded              | `loop position` is invoked without `--poll`             | Exactly one line matching `\d+/-` is printed to stdout; process exits with code 0.                                           |
| AC-3 | Daemon running, project loaded, engine playing | `loop position --poll` is invoked then interrupted      | Each output line matches `\d+/\d+`; ticks are monotonically non-decreasing within a single loop; process exits with code 0. |
| AC-4 | Daemon running, project loaded                 | `loop position --poll --interval-ms 100` runs for 500ms | Approximately 5 output lines are produced (±1 tolerance).                                                                    |
| AC-5 | Daemon not running                             | `loop position` is invoked without `--poll`             | A human-readable error is printed to stderr, nothing is printed to stdout, and the process exits with code 1.                |
| AC-6 | `--poll` mode is running                       | The daemon becomes unreachable mid-poll                 | A human-readable error is printed to stderr and the process exits with code 1.                                               |

---

## Open Questions

No open questions remain. The PRD is complete.

---

## Refinement Log

### Cycle 1 — Confidence: 60%

- Reconciled: nothing (PRD created from roadmap, no prior answers)
- Added: Q1 (daemon unavailability handling), Q2 (poll connection lifecycle), Q3 (`--interval-ms` without `--poll`)

### Cycle 2 — Confidence: 80%

- Reconciled: Q1 → F-9, AC-5 (daemon unreachable: stderr error, exit 1); Q2 → F-10 (new connection per poll iteration); Q3 → F-11 (`--interval-ms` without `--poll` is a no-op)
- Added: Q4 (connection failure mid-poll behaviour)

### Cycle 3 — Confidence: 92%

- Reconciled: Q4 → F-12, AC-6 (mid-poll connection failure: stderr error, exit 1)
- Added: nothing (confidence ≥ 90%; PRD is complete)
