# EP-7 · Mode Management — PRD

## Overview

The engine supports three mutually exclusive operating modes — `clock`, `sync`, and `standalone` — and allows switching between them at runtime via the runtime interface. The active mode governs whether the engine sends a MIDI clock, follows an external clock, or runs entirely independently. Mode transitions must not interrupt loop playback where possible.

**Confidence Level:** 93% — All roadmap requirements are covered, all questions resolved, and all ACs are testable; minor editorial notes remain (NF-2 is conservative and AC-8 is a policy statement rather than a strict assertion) but no gaps warrant further questions.

---

## User Journeys

### UJ-1 · Engine starts in default mode

A user launches the engine without any explicit mode argument. The engine comes up in `standalone` mode, ready to accept runtime commands. The user can immediately query the active mode via the runtime interface.

### UJ-2 · Switching to clock mode to drive external devices

A user has loaded a project and wants the engine to act as the MIDI clock master for connected devices. They issue a `set mode clock` command via the runtime interface. The engine transitions to clock mode; the MIDI clock output becomes available and can be started. The loop continues playing without interruption.

### UJ-3 · Switching to sync mode to follow an external clock

A user wants the engine to follow an external MIDI clock source. They issue a `set mode sync` command. The loop pauses immediately. BPM control is disabled. Playback resumes once both an active project is present and an incoming clock signal is received; it then plays in sync with the external clock.

### UJ-4 · Switching away from sync mode

A user decides to stop following the external clock and return to standalone or clock mode. They issue the appropriate `set mode` command. BPM control is re-enabled. The engine transitions and the loop resumes under internal timing.

### UJ-5 · Querying the current mode

A user queries the engine status via the runtime interface and receives the current mode, BPM (if applicable), and clock state as defined by EP-4.

---

## Functional Requirements

| ID | Requirement |
|----|-------------|
| F-1 | The engine starts in a defined default mode on launch. |
| F-2 | The active mode can be queried via the runtime interface. |
| F-3 | The active mode can be changed via the runtime interface while the engine is running. |
| F-4 | Switching to `sync` mode disables BPM control; BPM set commands are rejected with a structured error response while in sync mode. |
| F-5 | Switching away from `sync` mode re-enables BPM control. |
| F-6 | The loop continues playing through a mode switch without interruption where the target mode supports it. |
| F-7 | In `clock` mode the MIDI clock signal cannot be started unless an active project is defined (per EP-5). |
| F-8 | In `sync` mode playback only begins when both an active project is defined and an incoming clock signal is received (per EP-6). |
| F-9 | Modes are mutually exclusive; at most one mode is active at any time. |
| F-10 | The engine's default operating mode on launch is `standalone`; this default is hardcoded and not configurable via arguments or configuration files. |
| F-11 | Transitions between `standalone` and `clock` modes are seamless and do not interrupt the loop. Transitions entering or leaving `sync` mode may interrupt the loop. |
| F-12 | When leaving `clock` mode while the MIDI clock is actively running, the engine automatically sends a MIDI stop signal before completing the transition. |
| F-13 | Mode switching is performed via a `set mode <name>` command following the EP-4 protocol pattern; the response indicates success or an error with a structured payload. |
| F-14 | When transitioning into `sync` mode from a running state, the loop pauses immediately on receiving the command without waiting for the current bar to finish. |
| F-15 | When the `set mode` command receives an unrecognised mode name, the engine returns a structured error response with a descriptive message and remains in the current mode without any state change. |

---

## Non-Functional Requirements

| ID | Requirement |
|----|-------------|
| NF-1 | Mode transitions must not introduce timing gaps or glitches in the MIDI note stream where the roadmap requires uninterrupted playback. |
| NF-2 | Mode transitions must complete within one bar boundary to remain consistent with the project-update model of EP-2. |

---

## Acceptance Criteria

| ID | Given | When | Then |
|----|-------|------|------|
| AC-1 | The engine has been started with no explicit mode argument | The engine is running | The engine is in `standalone` mode |
| AC-2 | The engine is running in any mode | The user queries the runtime interface for current status | The response includes the active mode name |
| AC-3 | The engine is running in `standalone` mode with an active project | The user sends `set mode clock` | The engine transitions to `clock` mode without stopping the loop |
| AC-4 | The engine is running in `clock` mode | The user sends `set mode sync` | The engine transitions to `sync` mode and BPM control commands are disabled |
| AC-5 | The engine is running in `sync` mode | The user sends `set mode standalone` | The engine transitions to `standalone` mode and BPM control is re-enabled |
| AC-6 | The engine is in `clock` mode with the clock running | A graceful shutdown is initiated | The engine sends a MIDI stop clock signal before exiting (per EP-5) |
| AC-7 | The engine is in `sync` mode | The user attempts to set BPM | The engine returns a structured error response and the BPM remains driven by the external clock |
| AC-8 | The engine is in `sync` mode with the loop playing | The user sends a mode-change command | The loop may pause before resuming under internal timing (interruption is acceptable) |
| AC-9 | The engine is in `clock` mode with the MIDI clock actively running | The user sends any mode-change command | The engine sends a MIDI stop signal before completing the transition |
| AC-10 | The engine is running in any mode | The user sends `set mode <valid-name>` | The engine transitions to the specified mode and returns a structured success response |
| AC-11 | The engine is in any mode with the loop playing | The user sends `set mode sync` | The loop pauses immediately, without waiting for the current bar to finish |
| AC-12 | The engine is running in any mode | The user sends `set mode <unrecognised-name>` | The engine returns a structured error response with a descriptive message and the mode remains unchanged |

---

## Open Questions

No open questions.

---

## Refinement Log

### Cycle 1 — Confidence: 45%
- Reconciled: (none — PRD created from roadmap; no prior answered questions)
- Added: Q1 (default mode), Q2 (transition semantics), Q3 (leaving clock mode while running), Q4 (runtime command format)

### Cycle 2 — Confidence: 75%
- Reconciled: Q1 → F-10, AC-1 updated (default is `standalone`); Q2 → F-11, AC-8 (only sync transitions may interrupt); Q3 → F-12, AC-9 (auto MIDI stop when leaving clock mode); Q4 → F-13, AC-10 (`set mode <name>` per EP-4 pattern)
- Added: Q5 (loop pause timing when entering sync), Q6 (invalid mode name handling)

### Cycle 3 — Confidence: 88%
- Reconciled: Q5 → F-14, AC-11 (loop pauses immediately on entering sync); Q6 → F-15, AC-12 (unrecognised mode name returns structured error, no state change)
- Added: Q7 (BPM command behaviour in sync mode — "rejected or ignored" disjunction in F-4/AC-7 must be resolved for testability)

### Cycle 4 — Confidence: 93%
- Reconciled: Q7 → F-4 updated ("rejected with a structured error response"), AC-7 updated ("engine returns a structured error response and BPM remains unchanged")
- Added: (none — confidence at 93%, PRD complete)
