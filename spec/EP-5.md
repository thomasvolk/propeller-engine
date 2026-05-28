# EP-5 · Clock Output Mode — PRD

## Overview

The engine can act as a MIDI clock source that other devices follow. In clock output mode it sends standard MIDI timing clock pulses (0xF8, 24 per quarter note) at the rate determined by the active project's BPM. Starting the clock sends MIDI Start (0xFA); resuming from pause sends MIDI Continue (0xFB); stopping sends MIDI Stop (0xFC). The clock and loop engine are fully coupled: starting the clock starts the loop, and stopping the clock stops the loop. Clock-pause introduces a paused sub-state in which note output halts and the tick position is retained, allowing a seamless MIDI Continue resume. If the active project is removed while the clock is running, the clock continues sending timing pulses and the loop idles silently. The clock cannot be started unless an active project is loaded. On graceful daemon shutdown the engine attempts to send a MIDI Stop message so connected devices do not hang.

**Confidence Level:** 95% — All roadmap requirements are covered and all open questions are reconciled. F-19/AC-17 added to capture the startup latency requirement. The PRD is complete.

---

## User Journeys

### UJ-1 · Starting the clock in clock output mode

A performer has loaded a project and is in clock output mode. They send a clock-start command. The engine sends MIDI Start (0xFA), begins MIDI Timing Clock pulses (0xF8) at the current BPM, and starts the loop engine simultaneously. Connected devices receive the clock and begin playing in sync.

### UJ-2 · Pausing the clock

The performer sends a clock-pause command. The engine sends MIDI note-off for all active notes, stops sending MIDI clock pulses, and halts loop playback while retaining both clock and loop positions. Connected devices hold their current position. When the clock is resumed, MIDI Continue (0xFB) is sent and both clock pulses and loop playback resume from the retained positions.

### UJ-3 · Stopping the clock

The performer sends a clock-stop command. The engine sends MIDI Stop (0xFC), halts the loop, and resets both clock and loop positions to the beginning. Connected devices stop and reset.

### UJ-4 · Adjusting BPM while the clock is running

While the clock is running the performer changes the BPM in the project header. The clock pulse rate adjusts to reflect the new tempo. Connected devices follow the new tempo without the clock stopping.

### UJ-5 · Attempting to start the clock without a project

The performer tries to start the clock before loading any project. The engine rejects the command with an error.

### UJ-6 · Engine graceful shutdown with clock running

The engine receives a shutdown signal while the clock is active. Before exiting the engine sends MIDI Stop (0xFC) so connected devices do not hang on a running clock.

### UJ-7 · Clock and loop start and stop in lockstep

A performer is in clock output mode with an active project loaded. They send clock-start; both MIDI clock output and loop playback begin together. They later send clock-stop; both halt simultaneously and positions reset. Connected devices stay in sync with the loop throughout.

### UJ-8 · Project removed and replaced while clock is running

The clock is running and the performer removes the active project. The MIDI clock continues sending timing pulses without interruption; the loop idles silently with no note output. The performer loads a new project. The loop begins playing the new project from its first tick while the clock continues uninterrupted. Connected devices never lose the clock signal.

---

## Functional Requirements

| ID | Requirement |
|----|-------------|
| F-1 | In clock output mode, the engine sends MIDI timing clock messages to connected devices. |
| F-2 | The MIDI clock pulse rate is derived from the current BPM of the active project. |
| F-3 | When the BPM changes, the clock pulse rate adjusts to reflect the new tempo without stopping the clock. |
| F-4 | The clock can be started via a command. |
| F-5 | The clock can be paused via a command; when paused, clock pulse transmission stops and the current position is retained. |
| F-6 | A paused clock can be resumed; resumption continues pulse transmission from the retained position. |
| F-7 | The clock can be stopped via a command; stopping resets the clock position to the beginning. |
| F-8 | The clock cannot be started unless an active project is defined; a start command issued with no active project is rejected with an error. |
| F-9 | On graceful daemon shutdown, the engine attempts to send a MIDI Stop message before the daemon exits. |
| F-10 | The clock-start command causes the engine to send a MIDI Start message (0xFA) immediately before the first MIDI Timing Clock pulse (0xF8). |
| F-11 | The clock-resume command causes the engine to send a MIDI Continue message (0xFB) immediately before the first resumed MIDI Timing Clock pulse (0xF8). |
| F-12 | The clock-stop command (and graceful shutdown per F-9) causes the engine to send a MIDI Stop message (0xFC). |
| F-13 | The MIDI clock rate is 24 pulses per quarter note (MIDI standard). At the project's PPQN of 480 (EP-2 F-20), one MIDI Timing Clock pulse corresponds to 20 internal ticks. |
| F-14 | In clock output mode, the clock and loop engine states are fully coupled: clock-start also starts the loop engine; clock-pause also halts loop playback (retaining the loop's current position); clock-stop also stops the loop engine and resets both clock and loop positions to the beginning. |
| F-15 | The MIDI output port used for clock signals and note output is specified at daemon startup and remains fixed for the lifetime of the daemon. |
| F-16 | On clock-pause, the engine sends MIDI note-off for every currently-sounding note (to prevent stuck notes), then halts all further note output and retains the loop's current tick position. |
| F-17 | On clock-resume, the loop continues from the retained tick position, resuming note output at that tick. |
| F-18 | If the active project is removed while the clock is running, the MIDI clock continues sending timing pulses; the loop enters the idle state (no note output, per EP-3 F-13) until a new project is loaded. No MIDI Stop message is sent. |
| F-19 | On loop start and clock start, the engine applies a fixed startup latency window (20 ms) before emitting the first note event and the first clock pulse. Initial MIDI setup messages (Program Change, MIDI Start 0xFA) are sent immediately before this window; all bar events including NoteOn and ClockPulse fire only after the window has elapsed. This ensures connected MIDI devices have time to process setup messages and avoid dropping the first note. |

---

## Non-Functional Requirements

| ID | Requirement |
|----|-------------|
| NF-1 | MIDI clock pulse timing must meet the same < 5 ms jitter target as the loop engine (EP-3 NF-3) to provide useful sync to connected devices. |
| NF-2 | The MIDI Stop message on graceful shutdown must be sent before the Unix socket is closed (EP-1 F-8). |

---

## Acceptance Criteria

| ID | Given | When | Then |
|----|-------|------|------|
| AC-1 | An active project is loaded | a clock-start command is sent | the engine begins sending MIDI clock pulses at the rate corresponding to the current BPM |
| AC-2 | The clock is running | a clock-pause command is sent | clock pulse transmission stops and the clock position is retained |
| AC-3 | The clock is paused | a clock-resume command is sent | clock pulse transmission resumes from the retained position |
| AC-4 | The clock is running or paused | a clock-stop command is sent | the engine sends a MIDI Stop message and resets the clock position to the beginning |
| AC-5 | No active project is loaded | a clock-start command is sent | the engine rejects the command with an error |
| AC-6 | The clock is running | BPM is changed in the project header | the clock pulse rate adjusts to the new BPM without stopping the clock |
| AC-7 | The clock is running | the daemon receives a graceful shutdown signal | the engine sends a MIDI Stop message before exiting |
| AC-8 | An active project is loaded | a clock-start command is sent | a MIDI Start byte (0xFA) is sent before the first MIDI Timing Clock pulse (0xF8) |
| AC-9 | The clock is paused | a clock-resume command is sent | a MIDI Continue byte (0xFB) is sent before the first resumed MIDI Timing Clock pulse (0xF8) |
| AC-10 | The clock is running or paused | a clock-stop command is sent | a MIDI Stop byte (0xFC) is sent |
| AC-11 | An active project is loaded | a clock-start command is sent | the loop engine also begins playback simultaneously |
| AC-12 | The clock is running | a clock-stop command is sent | the loop engine also halts and both clock and loop positions are reset |
| AC-13 | A MIDI output port is configured at startup | the clock is started | clock pulses are sent on the configured port |
| AC-14 | The clock is running and notes are sounding | a clock-pause command is sent | MIDI note-off events are sent for all active notes, note output ceases, and the loop tick position is retained |
| AC-15 | The clock is paused | a clock-resume command is sent | loop note output resumes from the retained tick position |
| AC-16 | The clock is running | the active project is removed | the clock continues sending MIDI timing pulses and the loop produces no note output |
| AC-17 | An active project is loaded | a loop-start or clock-start command is received | the first NoteOn (and first ClockPulse in clock mode) is emitted at least 20 ms after the Program Change and/or MIDI Start (0xFA) sent during startup |

---

## Open Questions

No open questions. All questions have been reconciled.

---

## Refinement Log

### Cycle 1 — Confidence: 52%
- Reconciled: nothing (PRD created from roadmap)
- Added: Q-1 (MIDI transport message types), Q-2 (clock-loop relationship), Q-3 (MIDI output port)

### Cycle 2 — Confidence: 78%
- Reconciled: Q-1 → F-10/F-11/F-12 (MIDI message types for start/resume/stop), F-13 (24 PPQN / 20 internal ticks per pulse), AC-8/AC-9/AC-10; Q-2 → F-14 (fully coupled clock-loop states), UJ-7, AC-11/AC-12; Q-3 → F-15 (startup-configured MIDI port), AC-13
- Added: Q-4 (loop pause semantics given EP-3's started/stopped model), Q-5 (project removed while clock running)

### Cycle 3 — Confidence: 92%
- Reconciled: Q-4 → F-16 (note-off on pause + tick position retained), F-17 (resume from retained tick), AC-14/AC-15; Q-5 → F-18 (clock continues on project removal, loop idles), UJ-8, AC-16
- Added: none — confidence 92%, PRD is complete

### Cycle 4 — Confidence: 95%
- Reconciled: none
- Added: F-19 (startup latency window of 20 ms before first note/clock-pulse event), AC-17 — motivated by real MIDI device dropping the first NoteOn when it arrives back-to-back with the Program Change or MIDI Start message sent at startup
