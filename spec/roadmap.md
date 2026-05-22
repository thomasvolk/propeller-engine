# Propeller Engine — Roadmap

## Target

A live-coding music environment engine that runs as a long-lived daemon, manages a MIDI note loop driven by a structured project, supports three operating modes (clock output, clock sync, standalone), and exposes a runtime protocol for controlling all aspects at runtime.

---

## Dependency Graph

```
EP-1: Daemon Process
  ├── EP-2: Project Model
  │     └── EP-3: Loop Engine
  │           ├── EP-5: Clock Output Mode
  │           ├── EP-6: Clock Sync Mode
  │           └── EP-8: Real MIDI Output
  └── EP-4: Runtime Interface
              └── EP-7: Mode Management
                    (also depends on EP-5 and EP-6)
```

## Phases

| Phase | Epics | Can run in parallel |
|-------|-------|---------------------|
| 1 | EP-1 | — |
| 2 | EP-2, EP-4 | yes |
| 3 | EP-3 | — |
| 4 | EP-5, EP-6, EP-8 | yes |
| 5 | EP-7 | — |

---

## Epics

### EP-1 · Daemon Process

The engine runs as a long-lived background process.

**Requirements**
- The engine can be started and stays running until explicitly stopped.
- The engine can be stopped cleanly.

**Dependencies:** none

---

### EP-2 · Project Model

A project is the central data structure that defines what MIDI signals the engine will send.

**Requirements**
- A project consists of a header and a list of tracks.
- The header defines the tempo (BPM) and the time signature.
- A track has a name, a MIDI channel, a MIDI instrument, and a list of bars.
- A bar contains a list of notes.
- Every bar in a project has the same length, determined by the time signature.
- A note has a pitch (MIDI standard) and a velocity (MIDI).
- A note has a duration; the duration can be smaller than the note value of the time signature but cannot exceed the length of a bar.
- A note can be a rest, occupying duration without producing a sound.
- The time signature is expressed as two numerals: the upper numeral indicates how many note values constitute a bar; the lower numeral indicates the note value being counted and must be a power of 2 (2, 4, 8, or 16).
- A project can be created and modified at runtime.
- Project updates take effect only on a bar boundary — the current bar always plays to completion first.
- The engine can hold one active project at a time.

**Dependencies:** EP-1

---

### EP-3 · Loop Engine

The engine plays back the active project in a continuous loop.

**Requirements**
- The engine continuously repeats the project from start to finish while running.
- Notes are played on their designated MIDI channel and instrument at the position defined in the project.
- Tempo and time signature are read from the project header.
- Changing the BPM in the project header takes effect on the running loop.
- The loop must run precisely in time; no delays allowed and timing drift is not acceptable.
- There must be no timing gap when the project repeats from the end back to the start.
- There must be no timing gap when the active project is updated (see EP-2 F-13, NF-3).
- Timing precision is the highest-priority requirement; all other concerns are secondary to it.

**Dependencies:** EP-2

---

### EP-4 · Runtime Interface

A protocol allows external clients to send commands and query status while the engine is running.

**Requirements**
- The engine accepts commands at runtime without restarting.
- Supported commands include:
  - Create or modify the active project (header, tracks, bars, notes)
  - Set BPM
  - Set time signature
  - Set operating mode
  - Query current status (mode, BPM, time signature, clock state, project presence)
- The protocol is available as long as the daemon is running.

**Dependencies:** EP-1

---

### EP-5 · Clock Output Mode

The engine can act as a MIDI clock source that other devices follow.

**Requirements**
- The engine can send a MIDI clock signal to connected devices.
- The outgoing clock can be started.
- The outgoing clock can be paused (resumes from current position).
- The outgoing clock can be stopped (resets position).
- The clock signal reflects the current BPM of the loop.
- The clock cannot be started unless an active project is defined.
- If a graceful shutdown is initiated the engine must try to send a stop clock signal.

**Dependencies:** EP-3

---

### EP-6 · Clock Sync Mode

The engine can follow an external MIDI clock source and synchronize its loop to it.

**Requirements**
- The engine can receive an incoming MIDI clock signal.
- When receiving an external clock the loop runs in sync with it.
- In this mode the internal BPM setting has no effect; tempo is driven by the external clock.
- Playback only begins when both an active project is defined and a clock signal is received.
- If the external clock stops or is lost, the engine does not crash; behavior in this case is defined by the status observable via EP-4.

**Dependencies:** EP-3

---

### EP-8 · Real MIDI Output

The engine sends MIDI events to a real hardware or virtual MIDI port instead of the internal mock.

**Requirements**
- The engine opens a named MIDI output port on startup.
- The target port is configurable via an environment variable; a sensible default is used when none is set.
- The runtime interface exposes a command to list all available MIDI output ports by index and name.
- Note-on, note-off, and program-change messages generated by the loop engine are delivered to the selected port.
- If the requested port does not exist at startup the engine logs an error and exits with a non-zero code.

**Dependencies:** EP-3

---

### EP-7 · Mode Management

The engine supports three mutually exclusive operating modes and allows switching between them at runtime.

**Modes**

| Mode | Description |
|------|-------------|
| `clock` | Engine sends a MIDI clock signal (EP-5). Requires an active project before the clock can start. |
| `sync` | Engine follows an external MIDI clock signal (EP-6). Playback requires an active project and an incoming clock signal. BPM control is disabled. |
| `standalone` | Engine runs its loop without sending or receiving a clock. |

**Requirements**
- The engine starts in a defined default mode.
- The active mode can be read via the runtime interface.
- The active mode can be changed via the runtime interface while the engine is running.
- Switching to `sync` mode disables BPM control.
- Switching away from `sync` mode re-enables BPM control.
- The loop continues playing through a mode switch without interruption where possible.

**Dependencies:** EP-4, EP-5, EP-6
