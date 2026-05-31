# Known Issues

A reference for limitations, surprising behaviours, and workarounds that users encounter when running propeller-engine in live performance.

## Overview

Most of the items below are not bugs — they are intentional design constraints imposed by the MIDI protocol, daemon lifecycle, or timing-precision requirements. Understanding them in advance prevents confusion during a performance. Each entry describes what happens, why, and what to do about it.

## Sync mode port cannot be changed at runtime

The MIDI input port for clock sync is read from `PROPELLER_SYNC_PORT` once, at daemon startup. There is no runtime command to switch to a different input port.

**Workaround:** stop the daemon, set `PROPELLER_SYNC_PORT` to the new port name, and restart.

```sh
propeller stop
PROPELLER_SYNC_PORT="New Port Name" propeller start --sync
```

## Switching to sync mode requires a daemon restart

Running `set-mode sync` via the socket returns a `sync_requires_port` error unless the daemon was originally started with `--sync`. The runtime interface cannot open a new MIDI input port after the daemon is already running.

```json
{"command": "set-mode", "mode": "sync"}
```

```json
{"status": "error", "code": "sync_requires_port", "message": "..."}
```

**Workaround:** restart the daemon with `--sync` and `PROPELLER_SYNC_PORT` set before switching mode.

```sh
PROPELLER_SYNC_PORT="IAC Driver Bus 1" propeller start --sync
```

## MIDI output port is fixed for the daemon lifetime

`PROPELLER_MIDI_PORT` is read at daemon startup and cannot be changed at runtime. All note, program-change, and clock messages go to the port that was open when the daemon started.

**Workaround:** stop the daemon, set `PROPELLER_MIDI_PORT` to the desired port, and restart.

```sh
propeller stop
PROPELLER_MIDI_PORT="My Synth" propeller start
```

## Stuck notes after an unclean daemon exit

A graceful shutdown (via `propeller stop` or SIGTERM) sends MIDI note-off for every sounding note before exiting. If the daemon process is killed with SIGKILL, crashes, or the machine loses power, those note-off messages are never sent. Connected hardware will sustain the notes indefinitely.

**Workaround:** send an All Notes Off message (CC 123, value 0) on each active MIDI channel from your DAW or a utility such as `sendmidi`:

```sh
sendmidi dev "IAC Driver Bus 1" cc 1 123 0
```

Repeat for each channel your project uses (channels 1–16 if in doubt).

## 20 ms startup latency on loop and clock start

When the loop or MIDI clock starts, the engine applies a fixed 20 ms delay before emitting the first note-on and the first clock pulse. Program Change messages are sent immediately; all note and clock events follow after the window. This is intentional: some hardware synthesisers need settling time after a Program Change before they respond reliably to NoteOn.

The 20 ms delay is not configurable. If your hardware does not need it, the latency is still present.

## No CLI command for clock-pause and clock-resume

`propeller loop start` and `propeller loop stop` route to `clock-start` and `clock-stop` in clock mode. There is no `propeller loop pause` CLI subcommand. `clock-pause` and `clock-resume` must be sent directly over the socket:

```sh
printf '{"command":"clock-pause"}\n' | nc -U /tmp/propeller.sock
printf '{"command":"clock-resume"}\n' | nc -U /tmp/propeller.sock
```

## Sync mode: clock loss detection is slow at low BPM

The engine declares the external clock lost after approximately 3–4 expected pulse intervals of silence. The expected interval is derived from the last observed BPM. At low tempos this timeout is proportionally long:

| BPM | Pulse interval | Clock-loss timeout (approx.) |
| --- | -------------- | ---------------------------- |
| 20  | ~125 ms        | 375–500 ms                   |
| 60  | ~42 ms         | 125–165 ms                   |
| 120 | ~21 ms         | 63–83 ms                     |
| 240 | ~10 ms         | 31–42 ms                     |

During this window the loop continues to play with stale timing. There is no way to shorten the timeout.

## Sync mode: the loop does not restart automatically after clock loss

When the external clock resumes after being declared lost, the engine re-establishes tempo tracking from the new pulses but does **not** restart the loop automatically. Your external device must send MIDI Start (0xFA) or MIDI Continue (0xFB) explicitly to resume playback.

This is by design: auto-restart on clock recovery would cause unintended playback if the clock source glitches or reconnects mid-set.

## Project updates and BPM changes take effect at the next bar boundary

`modify-project` and `set-bpm` queue their change; the engine finishes the current bar completely before applying it. At slow tempos (e.g. 20 BPM, 4/4 time) a bar lasts 12 seconds, so the update may feel delayed.

There is no way to force an immediate mid-bar update. Use `create-project` (not `modify-project`) to replace the active project; it also takes effect at the next bar boundary.

## `loop-start` with no project loads silently

If you issue `loop-start` (or send `{"command":"loop-start"}` over the socket) before loading a project, the engine enters a waiting state without producing any MIDI output or error. The status response shows `clock_state: "started"` and `project_present: false`. Playback begins automatically once you load a project.

Check status if the engine appears silent after starting:

```sh
propeller status
```

```json
{"status": "ok", "mode": "standalone", "bpm": 120, "time_signature": null,
 "clock_state": "started", "project_present": false}
```

If `project_present` is `false`, load a project with `propeller project create <file>`.

## See also

- [JSON Socket Interface](json-socket-interface.md) — full command and error-code reference
- [Runtime interface section in README](../README.md#runtime-interface) — quick reference for all commands
- [Selecting a MIDI output port in README](../README.md#selecting-a-midi-output-port)
- [Selecting a MIDI input port for sync mode in README](../README.md#selecting-a-midi-input-port-for-sync-mode)
