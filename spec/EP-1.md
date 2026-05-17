# EP-1 · Daemon Process — PRD

## Overview

The engine runs as a long-lived background process. It starts on demand, remains running without further interaction, and shuts down cleanly when explicitly stopped. All subsequent epics depend on this foundation.

**Confidence Level:** 95% — PRD is complete. NF-4 is intentionally qualitative (SIGKILL cannot be intercepted; "as gracefully as possible" is the correct bound for a PRD).

---

## User Journeys

### UJ-1 · Starting the engine before a session

A performer is preparing for a live-coding session. They start the engine. The engine launches, moves to the background, and remains active without blocking further work. The performer can now proceed to load a project and send commands.

### UJ-2 · Stopping the engine after a session

After a performance the performer stops the engine. The engine finishes any in-progress activity, releases all resources, and exits. The performer can verify that nothing is left running.

### UJ-3 · Attempting to start a second instance

The performer accidentally tries to start the engine while it is already running. The second start attempt is rejected with a clear indication that an instance is already active. The running instance is unaffected.

### UJ-6 · Checking whether the engine is running

A performer wants to confirm the engine is active before starting a session. They run `propeller status`. The command reports the daemon's state in human-readable output and exits with a code that allows scripted checks.

### UJ-4 · Engine encounters an unexpected error

During a session the engine encounters an internal error. The failure is observable — the performer can detect that something went wrong. The engine does not disappear silently.

### UJ-5 · Restarting after a crash

The engine crashes mid-session, leaving a stale socket file at `/tmp/propeller.sock`. The performer runs the start command again. The engine detects the unresponsive socket, removes it, and starts fresh. The performer resumes their session without manual cleanup.

---

## Functional Requirements

| ID | Requirement |
|----|-------------|
| F-1 | The engine can be started. |
| F-2 | Once started, the engine runs continuously in the background without requiring further interaction. |
| F-3 | The engine can be stopped by an explicit stop command. |
| F-4 | On stop, the engine releases all held resources before exiting. |
| F-5 | Only one instance of the engine can run at a time. A second start attempt is rejected while an instance is already running. |
| F-6 | The running or stopped state of the engine is detectable from outside. |
| F-7 | The engine is started and stopped via CLI commands. On start, it daemonises itself (double-fork), detaching from the calling shell immediately. |
| F-8 | The engine opens a Unix domain socket at `/tmp/propeller.sock` on startup and removes it on clean shutdown. The presence of a connectable socket at that path serves as the liveness indicator. |
| F-9 | On an internal error, the engine writes diagnostic information to stderr and to a user-specific log file (e.g. `$HOME/.local/share/propeller/propeller.log` or the platform-appropriate equivalent). |
| F-10 | On startup, if `/tmp/propeller.sock` already exists, the engine attempts to connect to it. If the connection is refused, the stale file is removed and startup proceeds. If the connection succeeds, the start attempt is rejected (F-5). |
| F-11 | A `status` CLI subcommand reports whether the daemon is currently running. It produces human-readable output and exits with code 0 if the daemon is running and a non-zero code if it is not. |

---

## Non-Functional Requirements

| ID | Requirement |
|----|-------------|
| NF-1 | The engine must complete startup and be ready to accept connections within 1 second. |
| NF-2 | While idle (no active project, no clock running), the engine must consume less than 1% CPU and less than 50 MB of memory. |
| NF-3 | An unexpected internal error must be observable; the engine must not fail silently. |
| NF-4 | The engine must handle a forced termination signal (e.g. from the operating system) as gracefully as possible. |

---

## Acceptance Criteria

| ID | Given | When | Then |
|----|-------|------|------|
| AC-1 | The engine is not running | I run the start command | The engine daemonises and the calling shell returns immediately |
| AC-2 | The engine is running | I send a stop command | The engine shuts down and all resources are released |
| AC-3 | The engine has been started | No further interaction occurs | The engine remains running indefinitely |
| AC-4 | The engine is already running | I attempt to start a second instance | The attempt is rejected; the running instance continues unaffected |
| AC-5 | The engine is running | I attempt to connect to `/tmp/propeller.sock` | The connection succeeds |
| AC-6 | The engine is not running | I attempt to connect to `/tmp/propeller.sock` | The connection is refused or the socket file does not exist |
| AC-7 | The engine is running | An unexpected internal error occurs | Diagnostic output appears on stderr and in the user-specific log file |
| AC-8 | The engine is not running | I start the engine and time from invocation to socket ready | Elapsed time is under 1 second |
| AC-9 | The engine is running with no active project | CPU and memory are sampled after a stable period | CPU is below 1% and memory is below 50 MB |
| AC-10 | The engine has crashed, leaving a stale `/tmp/propeller.sock` | I run the start command | The engine removes the stale file, starts successfully, and the socket is connectable |
| AC-11 | The engine is running | I run `propeller status` | The output indicates the daemon is running and the command exits with code 0 |
| AC-12 | The engine is not running | I run `propeller status` | The output indicates the daemon is not running and the command exits with a non-zero code |

---

## Open Questions

No open questions.

---

## Refinement Log

### Cycle 1 — Confidence: 60%
- Reconciled: none
- Added: Q-1 (start/stop invocation), Q-2 (state detection), Q-3 (performance targets), Q-4 (error observability)

### Cycle 2 — Confidence: 80%
- Reconciled: Q-1 → F-7 (CLI double-fork daemonisation) + AC-1 sharpened, Q-2 → F-8 (socket liveness) + AC-5/AC-6 sharpened, Q-3 → NF-1/NF-2 concrete targets + AC-8/AC-9, Q-4 → F-9 (stderr + log file) + AC-7 sharpened
- Added: Q-5 (socket address), Q-6 (log file path)

### Cycle 3 — Confidence: 87%
- Reconciled: Q-5 → F-8 updated (Unix socket at `/tmp/propeller.sock`) + AC-5/AC-6 made concrete, Q-6 → F-9 updated (user-specific platform log path) + AC-7 made concrete
- Added: Q-7 (stale socket handling after crash)

### Cycle 4 — Confidence: 92%
- Reconciled: Q-7 → F-10 (stale socket recovery) + UJ-5 (crash restart journey) + AC-10
- Added: none — PRD is complete

### Cycle 5 — Confidence: 95%
- Reconciled: none
- Added: F-11 (status subcommand with exit code), UJ-6 (checking daemon state), AC-11/AC-12
