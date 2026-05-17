# EP-6 · Clock Sync Mode — PRD

## Overview

The engine can follow an external MIDI clock source and synchronize its loop to it. In clock sync mode the loop tempo is driven entirely by incoming MIDI clock pulses (0xF8); the internal BPM setting has no effect. The engine follows standard MIDI sync semantics: clock pulses alone provide tempo information but do not trigger playback — the external device must send MIDI Start (0xFA, from the beginning) or Continue (0xFB, from the current position) to begin the loop. Playback also requires an active project. MIDI Stop (0xFC) halts the loop. If the external clock is lost the engine stops the loop and waits for a new Start or Continue. A port-enumeration utility lets the performer identify the correct MIDI input port before starting the daemon.

**Confidence Level:** 92% — All roadmap requirements and open questions are reconciled; the PRD is complete. Remaining minor details (exact status field naming, port-scan utility interface) are intentionally deferred to the technical specification.

---

## User Journeys

### UJ-1 · Connecting an external clock and starting playback

A performer loads a project and switches to clock sync mode. Their external MIDI device sends MIDI Start (0xFA) followed by MIDI Timing Clock pulses (0xF8). The engine uses the pulses for tempo and begins loop playback from the first tick, keeping in sync with the external tempo.

### UJ-2 · External clock stops mid-performance

While the loop is running in sync mode the external clock source stops sending pulses. After approximately 3–4 pulse intervals of silence the engine declares the clock lost, sends MIDI note-off for all active notes, and halts the loop. The engine's state is observable via the status query. The loop will not restart until the external device sends MIDI Start or Continue.

### UJ-3 · Waiting for a project in sync mode

The engine is in sync mode and the external device sends MIDI Start and clock pulses, but no project is loaded. The engine tracks tempo but does not play. When the performer loads a project, playback begins immediately (the MIDI Start was already received).

### UJ-4 · Waiting for a clock signal with a project loaded

The engine is in sync mode with an active project loaded, but no external clock or MIDI Start has been received. No playback occurs. When the external device sends MIDI Start followed by clock pulses, the loop begins.

### UJ-5 · Attempting to set BPM in sync mode

While in clock sync mode the performer issues a set-BPM command. The engine rejects it with an error. The loop tempo continues to be determined by the external clock.

### UJ-6 · External device controls loop transport

A performer's external sequencer sends MIDI Start (0xFA). The engine resets the loop to the first tick and begins playback. Later the sequencer sends MIDI Stop (0xFC); the engine halts the loop and sends MIDI note-off for all active notes. The sequencer then sends MIDI Continue (0xFB); the loop resumes from its current position.

### UJ-7 · Port enumeration before daemon startup

Before starting the daemon the performer invokes the MIDI port enumeration utility. All available MIDI input ports are listed. The performer identifies their external clock device's port and passes it as the configured input port when starting the daemon.

---

## Functional Requirements

| ID | Requirement |
|----|-------------|
| F-1 | In clock sync mode, the engine receives and processes an incoming MIDI clock signal on a configured MIDI input port. |
| F-2 | The loop tempo is derived from the inter-pulse interval of the incoming MIDI Timing Clock pulses (0xF8). |
| F-3 | The internal BPM setting has no effect on loop tempo in clock sync mode; all tempo information comes from the external clock. |
| F-4 | Playback begins only when both an active project is defined and the engine has received a MIDI Start (0xFA) or Continue (0xFB) message while a clock signal is active. If either condition is not met the loop does not play. |
| F-5 | If the external clock stops or is lost the engine does not crash. The resulting state is observable via the status query (EP-4 F-9). |
| F-6 | The engine responds to MIDI Start (0xFA) by resetting the loop position to the first tick and beginning playback, provided an active project is defined. |
| F-7 | The engine responds to MIDI Continue (0xFB) by resuming loop playback from the current tick position, provided an active project is defined and a clock signal is active. |
| F-8 | The engine responds to MIDI Stop (0xFC) by halting loop playback and sending MIDI note-off for all currently-sounding notes. |
| F-9 | The MIDI input port used for the external clock signal is specified at daemon startup and remains fixed for the lifetime of the daemon. |
| F-10 | The engine provides a MIDI port enumeration utility (invocable before or independently of the daemon) that lists all available MIDI input ports, so the performer can identify the correct port to configure. |
| F-11 | The engine declares the external clock lost after approximately 3–4 expected pulse intervals of silence, where the expected interval is derived from the last observed BPM. |
| F-12 | When the external clock is declared lost or MIDI Stop (0xFC) is received, the engine halts loop playback immediately, sending MIDI note-off for all currently-sounding notes. |
| F-13 | When the external clock signal resumes after being declared lost, the engine re-establishes tempo tracking from the pulses. Loop playback restarts only when the external device subsequently sends MIDI Start (0xFA) or Continue (0xFB) with an active project present. |
| F-14 | In clock sync mode, a set-BPM command is rejected with an error response; BPM control is disabled while the engine is in sync mode. |
| F-15 | MIDI Timing Clock pulses (0xF8) received in the absence of a prior MIDI Start or Continue message within the current playback session are used for tempo derivation only; they do not trigger loop playback. |

---

## Non-Functional Requirements

| ID | Requirement |
|----|-------------|
| NF-1 | The engine must process incoming MIDI clock pulses with sufficient timeliness to keep the loop within the < 5 ms jitter target (EP-3 NF-3). |
| NF-2 | The engine must not enter an error or crash state when the external clock is interrupted, lost, or never starts. |
| NF-3 | The clock loss timeout must be proportional to the last observed BPM (approximately 3–4 inter-pulse intervals) to avoid false positives at low BPM and missed detections at high BPM. |

---

## Acceptance Criteria

| ID | Given | When | Then |
|----|-------|------|------|
| AC-1 | Sync mode is active, a project is loaded, and MIDI Start (0xFA) or Continue (0xFB) has been received with clock running | the engine processes the signal | the loop begins playing in sync with the incoming tempo |
| AC-2 | The loop is running in sync mode | the external clock stops sending pulses | the engine does not crash; a status query reflects the clock-lost state |
| AC-3 | Clock sync mode is active with an active project | no clock signal is present | no loop playback occurs |
| AC-4 | Clock sync mode is active and a clock signal is present | no active project is defined | no loop playback occurs |
| AC-5 | The loop is running in sync mode | a set-BPM command is issued | the loop tempo is unchanged and continues to be driven by the external clock |
| AC-6 | Sync mode is active with a project loaded and clock running | MIDI Start (0xFA) is received | the loop resets to the first tick and begins playback |
| AC-7 | The loop is halted (by MIDI Stop or clock loss) and a clock signal is active | MIDI Continue (0xFB) is received and a project is present | the loop resumes from the current tick position |
| AC-8 | The loop is running in sync mode | MIDI Stop (0xFC) is received | the loop halts and MIDI note-off events are sent for all active notes |
| AC-9 | A MIDI input port is configured at startup and a clock signal arrives on that port | the engine is running in sync mode | the engine processes the incoming clock |
| AC-10 | The performer invokes the MIDI port enumeration utility | the utility runs | all available MIDI input ports are listed |
| AC-11 | The loop is running in sync mode at 120 BPM (pulse interval ~20.8 ms) | the clock signal goes silent for approximately 80–100 ms | the engine declares the clock lost and the status query reflects this |
| AC-12 | The engine has declared the clock lost (loop halted) and a project is still active | the external clock resumes and the external device sends MIDI Start or Continue | the loop begins playing again |
| AC-13 | The engine is in clock sync mode | a set-BPM command is issued | the engine returns an error response |
| AC-14 | Sync mode is active with a project loaded and clock pulses are flowing | no MIDI Start or Continue has been received in the current session | no loop playback occurs |
| AC-15 | The engine has declared the clock lost and clock pulses resume without a MIDI Start or Continue | the clock is flowing again | the engine tracks tempo but does not restart the loop |

---

## Open Questions

No open questions. All questions have been reconciled.

---

## Refinement Log

### Cycle 1 — Confidence: 48%
- Reconciled: nothing (PRD created from roadmap)
- Added: Q-1 (incoming MIDI transport messages), Q-2 (MIDI input port), Q-3 (clock loss timeout), Q-4 (loop behaviour on clock loss), Q-5 (BPM command in sync mode)

### Cycle 2 — Confidence: 80%
- Reconciled: Q-1 → F-6/F-7/F-8 (MIDI Start/Continue/Stop handling), UJ-6, AC-6/AC-7/AC-8; Q-2 → F-9 (startup-configured port), F-10 (port enumeration utility), UJ-7, AC-9/AC-10; Q-3 → F-11 (3–4 pulse interval timeout), NF-3 (BPM-proportional timeout), AC-11; Q-4 → F-12 (stop + note-off on clock loss), F-13 (auto-resume on clock return), AC-12; Q-5 → F-14 (set-BPM rejected in sync mode), AC-13
- Added: Q-6 (playback trigger: clock pulses alone vs MIDI Start required)

### Cycle 3 — Confidence: 92%
- Reconciled: Q-6 → F-4 updated (Start/Continue required to trigger playback), F-13 updated (clock return alone does not restart — Start/Continue still required), F-15 (0xF8 alone = tempo only), AC-1/AC-12 updated, AC-14/AC-15 added; UJ-1/UJ-3/UJ-4 updated to remove placeholder references
- Added: none — confidence 92%, PRD is complete
