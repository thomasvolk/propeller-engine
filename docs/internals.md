# Propeller Engine — Internal Architecture

This document describes how propeller-engine works from the inside: how processes
start and communicate, how projects flow through the system, and how MIDI events
are timed and emitted.

---

## Process Model

The system is split into two roles:

- **CLI process** (`propeller <subcommand>`) — short-lived, user-facing. Validates
  arguments, sends one IPC command, waits for a response, and exits.
- **Daemon process** (`propeller daemon-run ...`) — long-lived, detached from any
  terminal. Owns the MIDI output, the loop engine, and the IPC socket.

The `start` subcommand forks a detached daemon by spawning itself with the hidden
`daemon-run` subcommand. Stdin/stdout/stderr are redirected to `/dev/null` and the
child is placed in its own process group:

```rust
cmd.arg("daemon-run")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .process_group(0)
    .spawn()?;
```

The CLI then busy-polls the socket, sending a `status` probe every 50 ms until it
receives a valid JSON response (meaning the daemon is ready to accept real commands)
or until a 10-second deadline is exceeded.

---

## Startup Guard

Before binding the socket the CLI runs a liveness check
(`startup_guard::check`):

| Socket file state         | Outcome        | Action                          |
| ------------------------- | -------------- | ------------------------------- |
| Does not exist            | `Started`      | Proceed normally                |
| Exists, connection refused | `StaleCleared` | Remove file, proceed            |
| Exists, connection succeeds | `AlreadyRunning` | Print error, exit non-zero  |

```rust
pub fn check(sock_path: &Path) -> StartupOutcome {
    if !sock_path.exists() {
        return StartupOutcome::Started;
    }
    match UnixStream::connect(sock_path) {
        Ok(_) => StartupOutcome::AlreadyRunning,
        Err(_) => {
            let _ = std::fs::remove_file(sock_path);
            StartupOutcome::StaleCleared
        }
    }
}
```

---

## IPC Layer

### Socket

All CLI–daemon communication uses a single Unix domain socket. The default path is
`/tmp/propeller.sock`; the `PROPELLER_SOCK` environment variable overrides it. The
daemon binds the socket at startup and unlinks it on any clean shutdown.

### Wire Format

Messages are newline-delimited JSON. Every request must contain a `"command"` field:

```json
{"command": "create-project", "header": {"bpm": 120, "loop_duration": 1920}, "tracks": [...]}
```

Every response is a JSON object on a single line followed by `\n`:

```json
{"status": "ok"}
{"status": "error", "code": "validation_error", "message": "BPM 301 is out of range (20–300)"}
```

### Server Loop

`run_ipc_server` (`src/ipc/server.rs`) accepts connections in a `tokio` async loop
and spawns a task for each:

```rust
loop {
    match listener.accept().await {
        Ok((stream, _)) => {
            tokio::spawn(async move {
                connection_handler(stream, store, engine, settings, shutdown_tx).await;
            });
        }
        Err(e) => { error!("accept error: {e}"); break; }
    }
}
```

### Command Dispatch

`connection_handler` (`src/ipc/handler.rs`) reads exactly one line from the stream,
deserialises it into the `Command` enum (tagged by `"command"`) and calls `dispatch`.
The `Command` enum covers all supported operations:

```rust
#[serde(tag = "command", rename_all = "kebab-case")]
pub enum Command {
    CreateProject { header: WireHeader, tracks: Vec<WireTrack> },
    ModifyProject { header: WireHeader, tracks: Vec<WireTrack> },
    SetBpm { bpm: f64 },
    SetMode { mode: String },
    LoopStart, LoopStop,
    ClockStart, ClockPause, ClockResume, ClockStop,
    ListMidiPorts,
    Status,
    Stop,
}
```

The `Stop` command is special: the response is flushed before the shutdown signal
is sent so the CLI receives the `ok` reply before the socket disappears.

---

## Domain Model

### Data Structures

```
Project
  header: Header { bpm: u32, loop_duration: u32 }
  tracks: Vec<Track>

Track
  name:       String
  channel:    u8   (MIDI channel 1–16)
  instrument: u8   (GM program 0–127)
  notes:      Vec<Note>

Note
  start_tick: u32  (offset from loop start)
  duration:   u32  (in ticks)
  pitch:      u8   (MIDI note number)
  velocity:   u8
```

Time is measured in **ticks**. The resolution is 480 PPQN (pulses per quarter note),
matching the `PPQN` constant in `src/domain/project.rs`. At 120 BPM one quarter note
is 500 ms, so one tick is approximately 1.04 ms.

A `loop_duration` of 1920 ticks represents four quarter notes (one 4/4 bar).

### Validation

`set_pending` validates the project before storing it. Checks include:

- BPM in range 20–300
- `loop_duration > 0`
- MIDI channel in 1–16
- Instrument in 0–127
- Note duration > 0 and note start\_tick < loop\_duration
- Note duration ≤ 2 × loop\_duration

Validation errors are returned as typed `ValidationError` variants and translated
into structured JSON error responses.

### ProjectStore

`ProjectStore` (`src/domain/store.rs`) holds two slots:

- `active` — the project currently being played
- `pending` — a validated project waiting to take effect

```rust
pub fn set_pending(&mut self, project: Project) -> Result<(), ValidationError>
pub fn commit_pending(&mut self) -> bool   // swaps pending → active
```

The loop engine calls `commit_pending` at every loop boundary. This lets a live
project update take effect seamlessly between loops without interrupting playback.

---

## Loop Engine

### Overview

`LoopEngine` (`src/loop_engine/mod.rs`) is the public handle to the player thread.
It owns an `mpsc` sender and an `Arc<Mutex<EngineState>>`:

```rust
pub struct LoopEngine {
    sender: mpsc::Sender<LoopCommand>,
    state:  Arc<Mutex<EngineState>>,
}
```

The player thread (`PlayerLoop::run`) is a plain `std::thread` — not async — because
it needs to busy-spin for sub-millisecond timing accuracy.

Commands cross the channel as `LoopCommand` variants:

```rust
pub(crate) enum LoopCommand {
    Start, Stop,
    ClockStart, ClockPause, ClockResume, ClockStop,
    SyncStart, SyncContinue, SyncStop,
    SyncBpmUpdate(u32),
}
```

### Player State Machine

`PlayerLoop::run` dispatches on a four-state enum:

```
Stopped  ──Start/ClockStart/SyncStart──►  Running / Waiting
Waiting  ──project becomes available──►  Running
Running  ──Stop/ClockStop/SyncStop──►  Stopped
Running  ──ClockPause──►  Paused
Paused   ──ClockResume──►  Running
Paused   ──ClockStop/Stop──►  Stopped
```

**Waiting** is entered from `Stopped` when a `Start` (standalone) or `SyncStart`
command arrives but no project is active. The player polls the store every 10 ms
until a project appears, then transitions to `Running`.

### Tick Scheduling

`Scheduler` (`src/loop_engine/scheduler.rs`) converts tick numbers into wall-clock
deadlines:

```rust
// micros_per_tick = 60_000_000 / (bpm * 480)
pub fn deadline_for_tick(&self, anchor: Instant, tick: u64) -> Instant {
    anchor + Duration::from_micros(tick * self.micros_per_tick)
}
```

Sleep is implemented as a hybrid: the scheduler sleeps in 1 ms chunks while polling
the command channel, then switches to a busy-spin for the final 500 μs:

```rust
pub fn sleep_until(&self, deadline: Instant) {
    // ...
    if remaining > Duration::from_micros(500) {
        std::thread::sleep(remaining - Duration::from_micros(500));
    }
    while Instant::now() < deadline {}
}
```

### Event List

At the start of each loop pass `build_loop_events` builds a flat, sorted list of
`(tick, LoopEvent)` pairs from the active project:

```rust
enum LoopEvent {
    NoteOn  { channel, pitch, velocity },
    NoteOff { channel, pitch },
    ClockPulse,                           // only in clock/sync modes
}
```

Events at the same tick are ordered by priority: `NoteOff` (0) before `NoteOn` (1)
before `ClockPulse` (2). This ensures a note that ends and another that begins at
the same tick do not create a stuck-note condition.

MIDI clock pulses are inserted at every 20-tick interval (= 24 pulses per quarter
note at 480 PPQN) when clock mode is active.

### Cross-Loop Note Carry-Over

A note whose `start_tick + duration > loop_duration` would end after the loop
boundary. The player collects these as `overflow` during `build_loop_events`, then
after `advance_loop` converts them to offsets relative to the next loop start and
prepends them to the next pass's event list as `carry_over`.

```
loop N:  NoteOn at tick 0, NoteOff scheduled at tick 1921 (> loop_duration 1920)
         → NoteOff placed in overflow

advance_loop(): carry_over = [(1921 - 1920, NoteOff)] = [(1, NoteOff)]

loop N+1: carry_over is prepended → NoteOff fires at offset tick 1
```

### Loop Advance

`advance_loop` runs at the end of every complete loop pass:

1. Moves the anchor forward by exactly `loop_duration` ticks so drift does not
   accumulate.
2. Calls `store.commit_pending()` to pick up any project update.
3. If BPM changed (from a `set-bpm` command or a sync BPM update), resets the
   scheduler and re-anchors to `Instant::now()`.
4. Converts overflow NoteOff ticks to carry-over offsets.

### Instrument Tracking

The player keeps a `HashMap<channel, instrument>` (`last_instruments`). Before each
loop pass it compares each track's instrument against the cached value and emits a
`Program Change` (0xCx) only when the instrument has changed. This avoids redundant
PC messages every loop.

### Stop / Flush

When any stop or pause command is received while notes are sounding, `flush_notes`
immediately sends `NoteOff` for every entry in `active_notes`. This prevents stuck
notes on the connected device.

---

## Engine Modes

`EngineMode` (`src/ipc/types.rs`) determines how playback is started and stopped:

| Mode         | Who starts/stops playback              | MIDI clock output |
| ------------ | -------------------------------------- | ----------------- |
| `Standalone` | `loop-start` / `loop-stop` IPC command | None              |
| `Clock`      | `clock-start` / `clock-stop` IPC command | 0xFA / 0xF8 / 0xFB / 0xFC |
| `Sync`       | External MIDI Start (0xFA) / Stop (0xFC) | None (slave)    |

In **Clock** mode the daemon is the MIDI clock master. The engine emits `0xFA`
(Start), followed by `0xF8` (Timing Clock) every 20 ticks, and `0xFC` (Stop) on
stop. Pause emits no `0xFC`; resume emits `0xFB` (Continue).

In **Sync** mode the daemon is a clock slave. `loop-start` and `set-bpm` IPC
commands are rejected; playback is driven entirely by the `MidiClockReceiver`.

---

## MIDI Clock Receiver (Sync Mode)

### MidiClockReceiver

When started with `--sync`, the daemon opens the MIDI input port named by
`PROPELLER_SYNC_PORT` and starts `MidiClockReceiver` (`src/midi_clock/mod.rs`).
A callback thread feeds raw MIDI system-real-time bytes into an `mpsc` channel
as `ClockMessage` variants:

```rust
[0xF8] => ClockMessage::Pulse
[0xFA] => ClockMessage::Start
[0xFB] => ClockMessage::Continue
[0xFC] => ClockMessage::Stop
```

### run_receiver

A dedicated thread processes messages and drives the `LoopEngine` via `sync_*`
methods:

| Message            | Action                                                            |
| ------------------ | ----------------------------------------------------------------- |
| `Pulse` (0xF8)     | Update `PulseTracker`; if BPM changed notify engine              |
| `Start` (0xFA)     | Reset tracker; call `engine.sync_start()`                        |
| `Continue` (0xFB)  | Call `engine.sync_continue()` if clock is still active           |
| `Stop` (0xFC)      | Call `engine.sync_stop()`; state → `Waiting`                     |
| Timeout            | State → `Lost`; call `engine.sync_stop()`                        |

### PulseTracker

`PulseTracker` (`src/midi_clock/tracker.rs`) keeps a sliding window of the last 25
pulse timestamps. BPM is derived from the average inter-pulse interval:

```
BPM = 60_000_000 / (avg_interval_μs × 24)
```

The **timeout** is 3.5× the last pulse interval. If no pulse arrives within that
window, the clock is declared lost. On resumption (`Pulse` after `Lost`) the state
returns to `Tracking` without restarting playback — a new `Start` (0xFA) is required.

### SyncClockState

The receiver exposes its state as `SyncClockState` (`Waiting`, `Tracking`, `Lost`)
via an `Arc<Mutex<SyncClockState>>`. The `status` IPC command reads this arc and
includes `"sync_clock_state"` in the response when the daemon is in `Sync` mode.

---

## MIDI Output

### MidiOutput Trait

All MIDI emission goes through the `MidiOutput` trait (`src/loop_engine/midi.rs`):

```rust
pub trait MidiOutput: Send + 'static {
    fn note_on(&mut self, channel: u8, pitch: u8, velocity: u8) -> Result<(), MidiSendError>;
    fn note_off(&mut self, channel: u8, pitch: u8) -> Result<(), MidiSendError>;
    fn program_change(&mut self, channel: u8, program: u8) -> Result<(), MidiSendError>;
    fn clock_tick(&mut self) -> Result<(), MidiSendError>;
    fn clock_start(&mut self) -> Result<(), MidiSendError>;
    fn clock_continue(&mut self) -> Result<(), MidiSendError>;
    fn clock_stop(&mut self) -> Result<(), MidiSendError>;
}
```

### MidiPortOutput

In production `MidiPortOutput` (`src/midi_port.rs`) wraps a `midir::MidiOutputConnection`.
It encodes MIDI messages as raw byte arrays before sending:

```rust
fn note_on_bytes(channel: u8, pitch: u8, velocity: u8) -> [u8; 3] {
    [0x90 | (channel - 1), pitch, velocity]
}
fn clock_tick_bytes() -> [u8; 1] { [0xF8] }
fn clock_start_bytes() -> [u8; 1] { [0xFA] }
```

If `PROPELLER_MIDI_PORT` is unset the daemon opens a virtual MIDI port named
`"propeller"` via `midir`'s `create_virtual` API. Downstream software (DAWs, other
applications) can connect to this virtual port.

---

## Logging

`logger::init` (`src/logger.rs`) is called once after the daemon process is
detached. It installs two `tracing-subscriber` layers:

- `fmt` layer → stderr
- `tracing_appender` non-blocking file layer → platform log path

| Platform | Log path                                           |
| -------- | -------------------------------------------------- |
| macOS    | `~/Library/Logs/propeller/propeller.log`           |
| Linux    | `$HOME/.local/share/propeller/propeller.log`       |

Before `init` is called, early startup errors fall back to `eprintln!`.

---

## Shutdown Sequence

Shutdown is triggered by either a `stop` IPC command or SIGTERM. Both paths
converge in `daemon::run` (`src/daemon.rs`) via `tokio::select!`:

```rust
tokio::select! {
    _ = run_ipc_server(...) => {}
    _ = shutdown_rx => { info!("stop command received"); }
    _ = sigterm.recv() => { info!("SIGTERM received"); }
}

engine_for_shutdown.clock_stop_on_shutdown();
let _ = std::fs::remove_file(&sock_path);
```

`clock_stop_on_shutdown` checks whether the engine is running, sends `ClockStop`,
and busy-polls up to 100 ms until the player thread confirms `Stopped`. This
ensures the MIDI 0xFC byte is sent to connected devices before the process exits.

---

## Thread Map

```
main thread (CLI) ──spawn──► daemon process
                                │
                                ├── tokio runtime (async)
                                │     ├── run_ipc_server (accept loop)
                                │     └── connection_handler × N (per connection)
                                │
                                ├── PlayerLoop thread (std::thread)
                                │     reads mpsc::Receiver<LoopCommand>
                                │     calls MidiOutput directly
                                │
                                └── MidiClockReceiver thread (std::thread, --sync only)
                                      reads mpsc::Receiver<ClockMessage>
                                      calls LoopEngine sync_* methods
```

The `ProjectStore` is wrapped in `Arc<RwLock<_>>` so the tokio connection handlers
(writers on `create-project`) and the player thread (reader at loop boundary, writer
at `advance_loop`) can share it safely without blocking each other longer than
necessary.
