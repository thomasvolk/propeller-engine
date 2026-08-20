# propeller-engine

A live-coding music environment engine that runs as a long-lived background daemon.

Live-coding performances need a process that is always on, accepts commands in real time,
drives MIDI loops with precise timing, and never misses a beat when the project is updated
mid-performance. propeller-engine is that process.

## Quick example

```sh
# Start the daemon — returns immediately; engine runs in the background
propeller start

# Load the bundled example project and start the loop
propeller project create examples/myproject.json
propeller loop start

# Check engine status
propeller status

# Stop the daemon cleanly
propeller stop
```

A ready-to-use starter project lives in `examples/myproject.json`.

## Installation

Prerequisites: a [Rust toolchain](https://rustup.rs) (stable, edition 2024).

1. Clone the repository and enter the project directory.
2. Build the release binary:

   ```sh
   cargo build --release
   ```

3. Add the binary to your PATH, or run it directly:

   ```sh
   export PATH="$PWD/target/release:$PATH"
   ```

## Usage

### Starting the daemon

```sh
propeller start
```

The process double-forks and detaches from your shell immediately. The daemon is ready
to accept connections as soon as the command returns. Starting a second instance while
one is already running is rejected.

To start in clock mode immediately (MIDI clock output, instead of switching later with `set-mode`):

```sh
propeller start --clock
```

To start in sync mode and follow an external MIDI clock source:

```sh
PROPELLER_SYNC_PORT="IAC Driver Bus 1" propeller start --sync
```

`--sync` requires `PROPELLER_SYNC_PORT` to be set. If the variable is absent or names an unknown port the daemon exits with an error before opening any port.

### Stopping the daemon

```sh
propeller stop
```

Sends a stop command over the IPC socket. The daemon finishes any in-progress work,
releases all resources, and removes the socket file before exiting.

### Checking liveness

```sh
propeller status
```

Prints a human-readable message and exits with code 0 if the daemon is running, or a non-zero code if it is not. Suitable for use in scripts.

### Managing projects

Load a project from a file:

```sh
propeller project create examples/myproject.json
```

Read from stdin (useful when generating projects dynamically):

```sh
generate-project.sh | propeller project create
```

The project file must be a JSON object with `header` and `tracks` fields — no `"command"` field; the CLI adds that automatically. See `examples/myproject.json` for a working example, and the Runtime interface section for the full field reference.

Update a running project (change takes effect at the next bar boundary):

```sh
propeller project modify examples/myproject.json
```

View the current (active) and pending (staged-but-uncommitted) project state:

```sh
propeller project get
```

Prints compact JSON with a `"current"` entry, a `"pending"` entry, or both — each omitted entirely when absent, rather than shown as `null`. If the daemon is unreachable or reports an error, `project get` prints a diagnostic to stderr and exits with a non-zero code. See the Runtime interface section for the full response shape.

### Controlling loop playback

```sh
propeller loop start
propeller loop stop
```

Starts or stops the loop in standalone or clock mode. In sync mode these commands are rejected — the external device controls transport via MIDI Start (0xFA) and Stop (0xFC).

### Querying the current loop position

```sh
propeller loop position
```

Prints the current tick position and exits, in the form `tick/loop_duration` — for example `240/1920`. When no project is loaded, `loop_duration` is printed as `-` (e.g. `0/-`).

To keep watching the position — useful for driving a step-highlight UI — poll continuously:

```sh
propeller loop position --poll
```

This prints one line every 50 ms until interrupted with Ctrl-C. Change the refresh rate with `--interval-ms`:

```sh
propeller loop position --poll --interval-ms 100
```

If the daemon is unreachable, `loop position` prints a diagnostic to stderr and exits with a non-zero code (this also applies mid-poll, if the daemon goes away while `--poll` is running).

### Configuring the socket path

Set `PROPELLER_SOCK` to override the default socket location:

```sh
PROPELLER_SOCK=/run/user/1000/propeller.sock propeller start
```

All subcommands read this variable, so set it consistently.

### Selecting a MIDI output port

By default the daemon opens a virtual MIDI port named `propeller`. To route to a real MIDI device instead, set `PROPELLER_MIDI_PORT` to the port name before starting:

```sh
PROPELLER_MIDI_PORT="IAC Driver Bus 1" propeller start
```

If the named port is not found, `start` prints the available port names and exits with a non-zero code.

### Selecting a MIDI input port for sync mode

Set `PROPELLER_SYNC_PORT` to the name of the port that will deliver incoming MIDI clock pulses when starting with `--sync`:

```sh
PROPELLER_SYNC_PORT="IAC Driver Bus 1" propeller start --sync
```

This port is fixed for the lifetime of the daemon.

### Listing available MIDI ports

List all output ports propeller can see:

```sh
propeller midi ports
```

### Log files

Diagnostic output is written to:

- **macOS:** `~/Library/Logs/propeller/propeller.log`
- **Linux:** `~/.local/share/propeller/propeller.log`

### Known issues and workarounds

Startup latency, stuck-note recovery, pitch-bend reset behavior, and sync-mode limitations
encountered in live use are documented with workarounds in
[docs/known-issues.md](docs/known-issues.md).

### Runtime interface

All commands are sent as a single-line JSON object to the Unix socket (`/tmp/propeller.sock` by default, overridden by `PROPELLER_SOCK`). Each connection carries exactly one request and receives one JSON response.

Use `nc -U` or `socat` to send commands from the shell:

```sh
printf '{"command":"loop-start"}\n' | nc -U /tmp/propeller.sock
```

For the full command and field reference, error codes, and worked examples, see [docs/json-socket-interface.md](docs/json-socket-interface.md).

#### create-project

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

Field notes:

- `bpm` — whole number, 20–300.
- `loop_duration` — total loop length in ticks; 480 ticks equals one quarter note.
- `channel` — MIDI channel, 1–16.
- `instrument` — MIDI program number, 0–127.
- Each note is a four-element array `[start_tick, duration, pitch, velocity]`.
  - `start_tick` — tick offset from loop start; must be less than `loop_duration`.
  - `duration` — note length in ticks; must be greater than 0.
  - `pitch` — MIDI note number, 0–127 (middle C = 60).
  - `velocity` — note-on velocity, 0–127.
- `pitch-bends` (optional) — a per-track array of two-element arrays `[tick, value]`.
  - `tick` — tick offset from loop start; must be less than `loop_duration`.
  - `value` — 14-bit pitch-bend value, 0–16383; 8192 is center (no bend).

  ```json
  "pitch-bends": [
    [0,   8192],
    [120, 10192],
    [240, 8192]
  ]
  ```

  Every channel with at least one `pitch-bends` entry is reset to center (8192) whenever the
  loop or clock stops or pauses. See `examples/pitch_bend.json` for a working example.

#### modify-project

Queues a new project definition; the change takes effect at the next bar boundary so the current bar always plays to completion. Same structure as `create-project`.

#### loop-start and loop-stop

```json
{"command": "loop-start"}
{"command": "loop-stop"}
```

`loop-start` with no active project transitions the engine to a waiting state; playback begins automatically once a project is loaded.

In sync mode both commands are rejected with a `sync_mode_active` error. Transport is controlled entirely by the external device: MIDI Start (0xFA) starts the loop from the beginning, and MIDI Stop (0xFC) pauses it, retaining the current position so a following MIDI Continue (0xFB) resumes exactly where it left off.

#### clock-start, clock-pause, clock-resume, clock-stop

Low-level MIDI clock transport control for use in `clock` mode. These operate on the clock signal independently of the loop:

```json
{"command": "clock-start"}
{"command": "clock-pause"}
{"command": "clock-resume"}
{"command": "clock-stop"}
```

`clock-start` requires an active project and returns a `no_project` error otherwise. `clock-pause` and `clock-resume` are unique to these commands — there is no `loop-pause` CLI equivalent.

Note: in `clock` mode the `loop-start` and `loop-stop` CLI convenience commands route to `clock-start` and `clock-stop` automatically.

#### set-bpm

Changes tempo while the loop is playing. The new BPM is applied at the next bar boundary.

```json
{"command": "set-bpm", "bpm": 140}
```

In sync mode this command is rejected with a `sync_mode_active` error; tempo is controlled entirely by the external clock.

#### set-mode

Switches the operating mode at runtime.

```json
{"command": "set-mode", "mode": "standalone"}
```

Valid modes:

- `standalone` — internal BPM drives the loop. The engine starts in this mode.
- `clock` — the engine emits outgoing MIDI clock pulses. Activated at startup with `--clock` or via this command at runtime.
- `sync` — the loop tempo is driven by incoming MIDI clock pulses from an external device. Requires the daemon to have been started with `--sync`; switching to `sync` via this command without that flag returns a `sync_requires_port` error.

#### status

Returns the current engine state.

```json
{"command": "status"}
```

Example response (standalone or clock mode):

```json
{
  "status": "ok",
  "mode": "standalone",
  "bpm": 120,
  "loop_duration": 1920,
  "clock_state": "stopped",
  "project_present": true
}
```

`clock_state` is `"started"` while the loop is playing, `"stopped"` otherwise. `loop_duration` is absent when no project is loaded.

In sync mode the response includes an additional field:

```json
{
  "status": "ok",
  "mode": "sync",
  "bpm": 120,
  "time_signature": { "numerator": 4, "denominator": 4 },
  "clock_state": "started",
  "project_present": true,
  "sync_clock_state": "tracking"
}
```

`sync_clock_state` values: `waiting` (no clock signal yet), `tracking` (clock pulses are flowing), `lost` (clock was present but has gone silent).

#### project

Returns the current (active) and pending (staged-but-uncommitted) project, in the same complete shape used to load a project — read-only, and identical regardless of operating mode.

```json
{"command": "project"}
```

Example response with both an active and a staged project:

```json
{
  "status": "ok",
  "current": {
    "header": { "bpm": 120, "loop_duration": 1920 },
    "tracks": [{ "name": "piano", "channel": 1, "instrument": 0, "notes": [[0, 480, 60, 80]] }]
  },
  "pending": {
    "header": { "bpm": 140, "loop_duration": 960 },
    "tracks": [{ "name": "bass", "channel": 2, "instrument": 33, "notes": [[0, 240, 40, 90]] }]
  }
}
```

`"current"` and `"pending"` are each omitted entirely when no project is active or staged, rather than appearing as `null`. `propeller project get` wraps this command and strips the `"status"` field before printing.

#### get-position

Returns the current tick position:

```json
{"command": "get-position"}
```

Example response:

```json
{"type": "position", "tick": 1234, "loop_duration": 4800}
```

`tick` is the current playback position within the loop, in ticks. `loop_duration` is `null` when no project is loaded. `tick` freezes while the engine is paused and resets to 0 on loop restart, stop, or an incoming MIDI Start (0xFA) in sync mode.

#### Response format

Every command returns a JSON object. On success:

```json
{"status": "ok"}
```

On error:

```json
{"status": "error", "code": "bpm_out_of_range", "message": "BPM must be between 20 and 300"}
```

## Features

- **CLI convenience commands** — `propeller project create/modify/get` and `propeller loop start/stop` wrap common socket operations; file or stdin input, no manual JSON construction required.
- **Daemon lifecycle** — starts, stays running indefinitely, stops cleanly on command or SIGTERM.
- **Unix socket IPC** — communicates over `/tmp/propeller.sock`; socket path is configurable via `PROPELLER_SOCK`.
- **Single-instance guard** — rejects a second `start` if the daemon is already running.
- **Stale socket recovery** — detects and removes leftover socket files from a previous crash, then starts fresh.
- **Graceful shutdown** — handles both the `stop` command and SIGTERM; unlinks the socket on exit.
- **Status check** — `propeller status` reports whether the daemon is running; exits 0 if running, non-zero if not.
- **Structured logging** — writes to the platform log file using `tracing`.
- **Project model** — a project defines a header (BPM, time signature) and tracks (MIDI channel, instrument, bars of notes). Notes carry pitch, velocity, and duration in ticks; a note can be a rest.
- **Pitch bend** — tracks may carry 14-bit pitch-bend events (0–16383, center 8192) at arbitrary tick offsets; bent channels reset to center whenever playback stops or pauses.
- **Continuous loop playback** — repeats the project endlessly with no timing gap between repetitions.
- **Bar-boundary updates** — pending project changes take effect at the next bar boundary; the current bar always plays to completion.
- **Runtime JSON interface** — load projects, control playback, adjust BPM and mode, and query status over the socket without restarting the engine.
- **Position query** — `propeller loop position` (with an optional `--poll`) or the `get-position` socket command report the current tick position for driving visual feedback such as step highlighting.
- **Project state query** — `propeller project get` or the `project` socket command report the active and staged project as complete JSON, without needing to track separately what was last loaded.
- **Operating modes** — `standalone`, `clock`, and `sync` modes are supported. `standalone` and `clock` are switchable at runtime via `set-mode`; `sync` requires `--sync` at daemon startup.

## Changelog

See [CHANGELOG.md](CHANGELOG.md) for the full release history.

## Contributing

Open issues and submit pull requests at <https://github.com/thomasvolk/propeller-engine>. See
[docs/internals.md](docs/internals.md) for the process model, IPC dispatch, and loop-engine
internals before diving into the code. Run `cargo fmt` and `cargo test` before submitting a pull
request; the codebase must build without compiler warnings.

## Support

Open an issue in the repository issue tracker.

## License

Apache 2.0 — see [LICENSE](LICENSE) for details.
