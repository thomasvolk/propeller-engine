# EP-8 · Real MIDI Output — PRD

## Overview

The engine currently routes all MIDI events (note-on, note-off, program-change) to an in-memory mock that records events but never transmits them. This epic replaces the mock with a real MIDI driver that opens a system MIDI output port and delivers events to connected hardware or virtual instruments. The target port is selected by name via an environment variable at startup. A runtime command lets operators discover available ports without leaving the propeller interface.

**Confidence Level:** 90% — All roadmap requirements are covered, port-selection semantics are resolved, and all ACs are testable. One minor open question remains around default port selection behaviour when the environment variable is not set.

---

## User Journeys

### UJ-1 · Playing notes through a synthesiser

A performer has Surge XT (or any MIDI-capable synthesiser) open and visible as a MIDI port on the system. They set `PROPELLER_MIDI_PORT` to the port's name, start the daemon, load a project, and issue `loop-start`. The synthesiser receives note-on and note-off messages and plays the sequence.

### UJ-2 · Discovering available ports before starting

A performer does not know the exact port name of their synthesiser. They start the daemon without setting `PROPELLER_MIDI_PORT` (daemon opens a virtual port as default), send a `list-midi-ports` command, and read back the index-and-name list of all ports currently visible to the OS. They stop the daemon, set the variable to the correct name, and restart.

### UJ-3 · Starting the daemon with a non-existent port

A performer mistypes the port name in `PROPELLER_MIDI_PORT`. The daemon logs a human-readable error identifying the unknown port name, lists the ports that are actually available, and exits with a non-zero code.

### UJ-4 · Program-change reaches the synthesiser

A performer loads a project where a track specifies instrument 42. On loop start the engine sends a MIDI program-change message for that track's channel before the first note, audibly switching the synthesiser to the correct patch.

---

## Functional Requirements

| ID | Requirement |
|----|-------------|
| F-1 | The engine sends MIDI note-on, note-off, and program-change events to a real system MIDI output port. |
| F-2 | The target port is identified by name, configurable via the `PROPELLER_MIDI_PORT` environment variable. |
| F-3 | When `PROPELLER_MIDI_PORT` is not set the engine opens a virtual MIDI output port named `propeller` and uses it as the default output. |
| F-4 | On startup, if `PROPELLER_MIDI_PORT` is set to a name that does not match any available port, the engine logs an error that includes the requested name and the list of available ports, then exits with a non-zero code. |
| F-5 | The runtime interface accepts a `list-midi-ports` command and responds with a JSON array of objects, each containing the port index and name, in system enumeration order. |
| F-6 | The MIDI driver implements the existing `MidiOutput` trait; no changes to the trait interface are required. |
| F-7 | The real MIDI driver replaces `MockMidiOutput` in the production daemon path; `MockMidiOutput` is retained for use in tests only. |
| F-8 | Port matching is case-sensitive and must be an exact string match against the port name as reported by the OS. |

---

## Non-Functional Requirements

| ID | Requirement |
|----|-------------|
| NF-1 | Delivering a MIDI event to the port must not block the loop engine thread long enough to violate the timing guarantees of EP-3 NF-1. |
| NF-2 | The implementation uses the `midir` crate for cross-platform MIDI access. |

---

## Acceptance Criteria

| ID | Given | When | Then |
|----|-------|------|------|
| AC-1 | `PROPELLER_MIDI_PORT` names a valid port and the daemon is started | the loop plays a note | the connected MIDI device receives a note-on followed by a note-off on the correct channel |
| AC-2 | A project track specifies a non-zero instrument | the loop starts | a program-change message is sent on that track's channel before the first note-on |
| AC-3 | `PROPELLER_MIDI_PORT` is not set and the daemon is started | the loop plays | MIDI events are delivered to the virtual port named `propeller` |
| AC-4 | `PROPELLER_MIDI_PORT` is set to a name that does not exist | the daemon is started | the daemon logs the unknown name and the available port list, then exits with a non-zero code |
| AC-5 | The daemon is running | a `list-midi-ports` command is sent | the engine returns `{"status": "ok", "ports": [{"index": 0, "name": "..."}, ...]}` |
| AC-6 | The daemon is running in production | the loop is playing | `MockMidiOutput` is not used; all events go to the real MIDI driver |
| AC-7 | Two ports share a prefix but differ in full name (e.g. `Surge` and `Surge XT`) | `PROPELLER_MIDI_PORT` is set to `Surge XT` | only the `Surge XT` port is opened; `Surge` is not matched |

---

## Open Questions

**Q-1** — Default behaviour when `PROPELLER_MIDI_PORT` is unset: the current spec (F-3) opens a virtual output port named `propeller`. Is this the desired default, or should the engine instead refuse to start and require the variable to be set explicitly? A virtual port is convenient for development but may surprise performers who forget to set the variable. Resolution needed before implementation.

---

## Refinement Log

### Cycle 1 — Confidence: 90%
- Reconciled: nothing (PRD created from roadmap requirement and codebase analysis)
- Added: F-1 through F-8, NF-1/NF-2, AC-1 through AC-7, Q-1 (default port behaviour)
