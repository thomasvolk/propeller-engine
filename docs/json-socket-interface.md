# JSON Socket Interface

Send commands to a running propeller-engine daemon and receive structured responses over a Unix domain socket.

## Overview

Every propeller-engine operation — loading a project, starting playback, changing BPM, querying status — is available as a JSON command sent to the daemon's Unix socket. The CLI convenience commands (`propeller loop start`, `propeller project create`, etc.) are thin wrappers around this interface. Scripting directly against the socket gives you full control without the CLI overhead, and lets you drive the engine from any language that can open a Unix socket.

Each connection carries exactly one command and receives exactly one response, then the connection closes. Commands are newline-terminated JSON objects with a `"command"` field that names the operation.

## Prerequisites

- A running daemon: `propeller start`
- A tool that can write to a Unix socket, such as `nc -U` or `socat`
- The socket path (default `/tmp/propeller.sock`; override with `PROPELLER_SOCK`)

## Step-by-step guide

1. Start the daemon if it is not already running:

   ```sh
   propeller start
   ```

2. Send a command using `nc`:

   ```sh
   printf '{"command":"status"}\n' | nc -U /tmp/propeller.sock
   ```

3. Read the response — a single JSON line printed to stdout:

   ```json
   {"status":"ok","mode":"standalone","bpm":120,"clock_state":"stopped","project_present":false}
   ```

4. Load a project before starting playback:

   ```sh
   printf '{"command":"create-project","header":{"bpm":120,"loop_duration":1920},"tracks":[{"name":"piano","channel":1,"instrument":0,"notes":[[0,480,60,80],[480,480,62,80]]}]}\n' \
     | nc -U /tmp/propeller.sock
   ```

5. Start the loop:

   ```sh
   printf '{"command":"loop-start"}\n' | nc -U /tmp/propeller.sock
   ```

6. Stop the loop:

   ```sh
   printf '{"command":"loop-stop"}\n' | nc -U /tmp/propeller.sock
   ```

## Command reference

### create-project

Creates and immediately activates a project. A project must be loaded before the loop can play.

```json
{
  "command": "create-project",
  "header": {
    "bpm": 120,
    "loop_duration": 1920
  },
  "tracks": [
    {
      "name": "piano",
      "channel": 1,
      "instrument": 0,
      "notes": [
        [0, 480, 60, 80],
        [480, 480, 62, 80]
      ]
    }
  ]
}
```

### modify-project

Queues a new project definition. The change takes effect at the next loop boundary; the current loop always plays to completion. Same structure as `create-project`.

### loop-start

```json
{"command": "loop-start"}
```

Starts the loop. If no project is loaded, the engine enters a waiting state and begins playback automatically once a project is loaded.

In sync mode this command is rejected — transport is controlled by the external MIDI device.

### loop-stop

```json
{"command": "loop-stop"}
```

Stops the loop. In sync mode this command is rejected.

### clock-start

```json
{"command": "clock-start"}
```

Starts clock output and playback in clock mode. Requires an active project.

### clock-pause

```json
{"command": "clock-pause"}
```

Pauses the clock mid-loop, retaining the current loop position for a seamless resume.

### clock-resume

```json
{"command": "clock-resume"}
```

Resumes from the paused position and sends MIDI Continue (0xFB) to connected devices.

### clock-stop

```json
{"command": "clock-stop"}
```

Stops the clock, sends MIDI Stop (0xFC), and resets the loop position.

### set-bpm

```json
{"command": "set-bpm", "bpm": 140}
```

Changes tempo while the loop is playing. Takes effect at the next loop boundary. Rejected in sync mode.

### set-mode

```json
{"command": "set-mode", "mode": "standalone"}
```

Switches the operating mode at runtime. Valid values: `standalone`, `clock`, `sync`. Switching to `sync` requires the daemon to have been started with `--sync`; otherwise returns a `sync_requires_port` error.

### status

```json
{"command": "status"}
```

Returns the current engine state. See the Response reference below.

### stop

```json
{"command": "stop"}
```

Shuts down the daemon cleanly. Equivalent to `propeller stop`.

## Field reference

### Header fields

| Field           | Type / Values   | Description                        |
| --------------- | --------------- | ---------------------------------- |
| `bpm`           | integer, 20–300 | Tempo in beats per minute          |
| `loop_duration` | integer, > 0    | Total loop length in ticks         |

### Track fields

| Field        | Type / Values   | Description                                        |
| ------------ | --------------- | -------------------------------------------------- |
| `name`       | string          | Human-readable label; not sent to MIDI             |
| `channel`    | integer, 1–16   | MIDI channel                                       |
| `instrument` | integer, 0–127  | MIDI program number                                |
| `notes`      | array of tuples | Flat list of notes as four-element integer arrays  |

### Note fields

Each note is a four-element integer array `[start_tick, duration, pitch, velocity]`:

| Index | Field        | Type / Values  | Description                            |
| ----- | ------------ | -------------- | -------------------------------------- |
| 0     | `start_tick` | integer, ≥ 0   | Tick offset from the start of the loop |
| 1     | `duration`   | integer, > 0   | Note duration in ticks                 |
| 2     | `pitch`      | integer, 0–127 | MIDI note number (middle C = 60)       |
| 3     | `velocity`   | integer, 0–127 | Note-on velocity                       |

### Status response fields

| Field              | Type / Values                        | Description                                                        |
| ------------------ | ------------------------------------ | ------------------------------------------------------------------ |
| `status`           | `"ok"` or `"error"`                  | Whether the command succeeded                                      |
| `mode`             | `"standalone"`, `"clock"`, `"sync"`  | Current operating mode                                             |
| `bpm`              | integer                              | Active BPM (from project if loaded, otherwise from engine setting) |
| `loop_duration`    | integer or absent                    | Loop length in ticks; absent when no project is loaded             |
| `clock_state`      | `"started"`, `"stopped"`             | Whether the loop is currently playing                              |
| `project_present`  | boolean                              | Whether a project is currently loaded                              |
| `sync_clock_state` | `"waiting"`, `"tracking"`, `"lost"`  | Sync mode only: state of the incoming external clock signal        |

## Overlapping notes

Multiple notes with the same `start_tick` on the same channel are valid. The engine emits a NoteOn for each of them at that tick, forming a chord. There is no limit on the number of notes that can share a start tick.

Example — a C major chord starting at tick 0:

```json
"notes": [
  [0, 480, 60, 80],
  [0, 480, 64, 80],
  [0, 480, 67, 80]
]
```

## Cross-loop notes

A note whose `start_tick + duration > loop_duration` extends beyond the end of the current loop iteration. The engine carries the sounding note into the next iteration and emits the NoteOff at the correct tick in the next loop.

The duration of any single note is bounded at `2 × loop_duration`; a note may not span more than two loop iterations.

Example — a note that starts at tick 1440 and lasts 960 ticks in a loop of 1920 ticks:

```json
{
  "header": { "bpm": 120, "loop_duration": 1920 },
  "tracks": [
    {
      "name": "pad",
      "channel": 1,
      "instrument": 0,
      "notes": [
        [1440, 960, 60, 80]
      ]
    }
  ]
}
```

The NoteOn is emitted at tick 1440 of the current loop. The NoteOff is emitted at tick `(1440 + 960) − 1920 = 480` of the next loop iteration.

## Error codes

| Code                           | Meaning                                                              | How to fix                                                                  |
| ------------------------------ | -------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `parse_error`                  | The JSON is malformed                                                | Check your JSON syntax; ensure the payload is valid UTF-8                   |
| `missing_command`              | The JSON object has no `"command"` field                             | Add `"command": "<name>"` to your payload                                   |
| `unknown_command`              | The `"command"` value is not recognised                              | Check the spelling; refer to the command list above                         |
| `validation_error`             | A field value failed domain validation                               | Check ranges: BPM 20–300, channel 1–16, instrument 0–127, etc.             |
| `bpm_non_integer`              | The `bpm` value has a fractional part                                | Use a whole number, e.g. `120` not `120.5`                                  |
| `bpm_out_of_range`             | BPM integer is outside 20–300                                        | Use a value between 20 and 300 inclusive                                    |
| `loop_duration_zero`           | `loop_duration` is 0 or negative                                     | Use a positive integer, e.g. `1920`                                         |
| `note_start_tick_out_of_range` | A note's `start_tick` is ≥ `loop_duration`                          | Ensure every note starts within the loop: `start_tick < loop_duration`      |
| `note_duration_zero`           | A note's `duration` is 0 or negative                                 | Use a positive integer for duration                                         |
| `note_duration_exceeds_limit`  | A note's `start_tick + duration > 2 × loop_duration`                | Shorten the note or move its start tick earlier                             |
| `invalid_mode`                 | The `"mode"` string is not recognised                                | Use `"standalone"`, `"clock"`, or `"sync"`                                  |
| `no_project`                   | `clock-start` sent with no active project                            | Load a project with `create-project` first                                  |
| `sync_mode_active`             | `loop-start`, `loop-stop`, or `set-bpm` sent while in sync mode     | Use the external MIDI device to control transport and tempo                 |
| `sync_requires_port`           | `set-mode` to `sync` without `--sync` at daemon startup             | Restart the daemon with `PROPELLER_SYNC_PORT=<port> propeller start --sync` |

## Examples

### Query status with socat

Check whether a project is loaded and whether the loop is playing:

```sh
echo '{"command":"status"}' | socat - UNIX-CONNECT:/tmp/propeller.sock
```

### Load and play a two-track project

Send a project with a bass and a melody track, then start the loop:

```sh
printf '{"command":"create-project","header":{"bpm":100,"loop_duration":1920},"tracks":[{"name":"bass","channel":2,"instrument":32,"notes":[[0,960,36,100]]},{"name":"melody","channel":1,"instrument":0,"notes":[[0,480,60,80],[480,480,62,80]]}]}\n' \
  | nc -U /tmp/propeller.sock
printf '{"command":"loop-start"}\n' | nc -U /tmp/propeller.sock
```

### Change BPM while playing

Nudge the tempo up without stopping the loop:

```sh
printf '{"command":"set-bpm","bpm":130}\n' | nc -U /tmp/propeller.sock
```

The new tempo takes effect at the next loop boundary.

### Use a custom socket path

If you started the daemon with a custom `PROPELLER_SOCK`:

```sh
PROPELLER_SOCK=/run/user/1000/propeller.sock propeller start
printf '{"command":"status"}\n' | nc -U /run/user/1000/propeller.sock
```

## See also

- [Runtime interface section in README](../README.md#runtime-interface)
- [Managing projects section in README](../README.md#managing-projects)
