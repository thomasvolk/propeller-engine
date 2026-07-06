# Roadmap: Current Position in Ticks

Goal: extend the propeller-engine protocol so clients can retrieve the current
playback position in ticks and use it for optical feedback (e.g. highlighting
the current step in a UI).

---

## EP-1 — Tick Position State

Extend the `Engine` entity with a `current_tick` field that is incremented
on every internal clock step while the engine is running. The field resets
to zero on each loop start and on `ExternalClockStart` (0xFA). Pausing
freezes the counter; stopping and restarting resets it.

The counter must be updated inside the existing timing loop without adding
any delay or sleep to the hot path. Concurrent reads from IPC handlers are
safe because the value is stored in an atomic integer — no lock is taken on
the audio/MIDI path.

**Deliverables**

- Spec: add `current_tick: Integer` to the `Engine` entity and rules that
  define when it resets, increments, and freezes.
- Implementation: atomic `u64` on the engine struct, incremented each tick.
- Tests: verify counter advances during playback, resets on restart, and
  freezes on pause.

---

## EP-2 — Position Query Protocol

Add a `get_position` / `position` request–response pair to the IPC socket
protocol (newline-delimited JSON, `#[serde(tag = "type")]`).

```
→ {"type":"get_position"}
← {"type":"position","tick":1234}
```

The handler reads the atomic counter and writes the response immediately
without touching the engine lock. The response also includes `loop_duration`
(from the active project, or `null` when no project is loaded) so the client
can compute a fractional progress without a separate `status` call.

**Deliverables**

- Spec: `GetPosition` command and `PositionReported` rule on the
  `OperatorInterface` surface.
- Implementation: new message variants in the `IpcMessage` enum; handler in
  the daemon's IPC dispatch loop.
- Tests: integration test over a real Unix socket that verifies the response
  value advances over time and equals zero immediately after a loop restart.

---

## EP-3 — Position CLI

Add a `loop position` subcommand to the CLI that queries the current tick from
the daemon. Without `--poll` the command prints one line and exits immediately,
suitable for scripting. With `--poll` it queries repeatedly at the interval set
by `--interval-ms` (default 50 ms, i.e. ~20 Hz) and runs until interrupted
(Ctrl-C), giving callers control over the visual refresh rate without
hardcoding a frequency in the daemon.

```
propeller loop position
propeller loop position --poll
propeller loop position --poll --interval-ms 100
```

Output per line: `tick/<loop_duration>` when a project is loaded, or `tick/-`
when no project is active.

**Deliverables**

- Spec: document the `loop position` surface operation and its output contract.
- Implementation: `loop position` subcommand in `clap`; boolean `--poll` flag;
  async poll loop using `tokio::time::interval` (only entered when `--poll` is
  set); clean exit on SIGINT.
- Tests: single-shot test verifying one line of output without `--poll`;
  integration test that starts the daemon, loads a project, runs with `--poll`
  for a fixed number of iterations, and asserts that the reported ticks are
  monotonically non-decreasing within a loop.
