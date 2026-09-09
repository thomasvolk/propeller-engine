# EP-3 · Sync Mode Integration Test Suite — Technical Specification

## Overview
This epic adds a new black-box integration-test crate that drives propeller-engine's sync
mode end-to-end through a real `propeller-clock` subprocess and observes the result exclusively
through propeller-engine's existing JSON-socket interface, covering start/stop, pause/resume,
and BPM-change scenarios. The pause/resume and BPM-change tests are written test-first against
today's known-buggy behavior and are expected to go green once EP-1 and EP-2 land their fixes.

**Confidence Level:** 93% — all three previously open items are now settled: the black-box
verification methodology (D-1), the virtual-MIDI-port isolation fix (D-2), and how the
test-first red tests coexist with CI (Q-1). The one residual note (not a specification gap) is
that Q-1's resolution requires a follow-up task in EP-1's and EP-2's own specs to remove the
`#[ignore]` markers once each fix lands — that follow-up has not yet been added to
`specs/EP-1-spec.md` / `specs/EP-2-spec.md` themselves.

---

## Architecture Overview

The suite lives in a new workspace member, `crates/propeller-sync-tests`, containing only a
`tests/` directory (no library code) — analogous in spirit to how `crates/propeller/tests/
integration.rs` already spawns the `propeller` binary as a subprocess and talks to it over its
Unix-socket JSON protocol, extended here to spawn **two** subprocesses per test: the
`propeller` binary (in `--sync` mode) and the `propeller-clock` binary.

To obtain `CARGO_BIN_EXE_propeller` and `CARGO_BIN_EXE_propeller-clock` at test time (the same
mechanism `crates/propeller/tests/integration.rs` uses via `env!("CARGO_BIN_EXE_propeller")`
for its own in-package binary), the new crate's `Cargo.toml` adds `propeller` and
`propeller-clock` as path `[dev-dependencies]`. This is purely so Cargo builds and exposes those
binaries to the test binary; the new crate never links against either crate's library code —
all interaction is through subprocesses and sockets, per F-6/F-7.

**Process topology per test:**
1. Spawn `propeller-clock` (`propeller-clock start`, env `PROPELLER_CLOCK_SOCK=<unique>`), which
   self-daemonizes (see `crates/propeller-clock/src/main.rs::spawn_daemon`) and opens a virtual
   MIDI output port (default name `"propeller-clock"`, from `DEFAULT_VIRTUAL_PORT_NAME` in
   `main.rs`, via `midi::open_virtual_named`, `midir::os::unix::VirtualOutput` — works headless
   on both macOS/CoreMIDI and Linux/ALSA-sequencer, so no CI hardware dependency).
2. Spawn `propeller` (`propeller start --sync`, env `PROPELLER_SOCK=<unique>`,
   `PROPELLER_SYNC_PORT=<the propeller-clock output port's name>`), which listens for that
   virtual port's MIDI clock/transport messages (`crates/propeller/src/main.rs::cmd_start`,
   `cmd_daemon_run`).
3. The test drives transport/tempo exclusively via short-lived `propeller-clock` CLI
   invocations (`start` / `pause` / `resume` / `bpm <n>` / `stop`), per F-1 — never synthesizing
   MIDI messages by any other mechanism.
4. The test observes engine state exclusively via `propeller`'s JSON socket (`status`,
   `get-position`), per F-6 — never parsing logs or adding new instrumentation.
5. Both subprocesses are torn down (via their own `stop` commands) and their socket files
   removed on test completion, via RAII guards mirroring `DaemonGuard` in
   `crates/propeller/tests/integration.rs`.

**Verification approach (resolved, D-1: Option A):** EP-1's own technical spec
(`specs/EP-1-spec.md`, D-1) established that its zero-tolerance assertions are only
meaningfully provable by asserting directly on `PlayerLoop`'s internal computed state (`anchor`,
scheduler rate) in an in-process unit test — real-clock timestamp comparisons were explicitly
rejected there as only proving *bounded* drift, not *zero* drift. EP-3 cannot take that
approach: F-6/F-7 require this suite to observe a separate subprocess purely through its JSON
socket, so there is no access to internal scheduler state. The only signal available is
`get-position`'s `tick`/`loop_count`, sampled across real wall-clock time. This suite's
assertions are therefore necessarily black-box rate comparisons (elapsed ticks over elapsed wall
time, sampled just before/after the event under test) checked against the *tightest defensible
epsilon* achievable given socket round-trip and OS-scheduling jitter — a pragmatic reading of
NF-4's "zero tolerance" bounded by what black-box measurement can actually prove, not a literal
zero. This scoping is made explicit so it isn't silently weaker than what NF-4 appears to
promise.

**Test isolation (resolved, D-2: Option B):** `propeller-clock` gains a small CLI/env-var
option so each `ClockGuard` can request a uniquely-named virtual MIDI port at creation time,
mirroring how `unique_sock_path()` already generates unique socket paths for hermetic parallel
tests. This is a small product change to `crates/propeller-clock` (not just test-harness code)
and is implemented as part of T-2 (see Implementation Tasks and Components below).

**Test-first red tests vs. CI (resolved, Q-1: Option A):** the pause/resume and BPM-change test
functions are marked `#[ignore = "pending EP-1/EP-2"]` once their red-run baseline is recorded,
so `cargo test --workspace` — and therefore CI (T-11) — stays green for unrelated PRs during the
interim window before EP-1 and EP-2 land. The tests remain runnable on demand via
`cargo test -p propeller-sync-tests -- --ignored`. **Cross-spec follow-up (not yet done):**
EP-1's and EP-2's own technical specs each need a task added to remove the corresponding
`#[ignore]` marker once their respective fix lands — this spec cannot add that task to
`specs/EP-1-spec.md` / `specs/EP-2-spec.md` itself, so it is flagged here as outstanding
follow-up work.

Note also (grounded in `git show c052cd7`, "sync mode: status now reports the live tracked
tempo"): in sync mode, `status`'s `bpm` field is the live-tracked tempo *rounded to the nearest
whole number*, derived from an averaging estimate over recent clock pulses — not an exact,
instantaneous scheduler value. This further motivates using `get-position`'s raw `tick` counter
(not `status`'s `bpm`) as the primary signal for the pause/resume and BPM-change assertions.

No GitHub Actions (or other CI) configuration exists anywhere in this repository today — NF-3
requires this epic to introduce one, not merely extend one.

---

## Components

### Test harness (`crates/propeller-sync-tests/tests/support/`)

Shared helpers, ported and extended from `crates/propeller/tests/integration.rs`'s
`unique_sock_path` / `wait_for_socket` / `DaemonGuard` / `send_command` pattern:

- `EngineGuard` — spawns `propeller start --sync` with a unique `PROPELLER_SOCK` and the
  `PROPELLER_SYNC_PORT` pointing at the paired `ClockGuard`'s MIDI port name; `Drop` sends the
  socket `stop` command and removes the socket file.
- `ClockGuard` — spawns `propeller-clock start` with a unique `PROPELLER_CLOCK_SOCK` and a
  uniquely-generated virtual MIDI port name (via `propeller-clock`'s new port-naming option, D-2
  Option B), so concurrently-running tests never collide on the previously-hardcoded
  `"propeller-clock"` port name; `Drop` runs `propeller-clock stop`. Exposes the port name it
  generated so the paired `EngineGuard` can set `PROPELLER_SYNC_PORT` to match.
- `send_engine_command(sock, cmd) -> serde_json::Value` — one-shot JSON-line request/response
  over the engine's Unix socket, mirroring `send_command` in the existing integration suite.
- `run_clock(sock, args)` — invokes the `propeller-clock` CLI as a short-lived subprocess
  (`start` / `pause` / `resume` / `bpm` / `stop`), mirroring `start_daemon`/`stop_daemon`'s use
  of `Command::new(...).status()`.
- `sample_position(sock) -> PositionSample` — issues `get-position` and pairs the response with
  the local `Instant` it was read at.
- `assert_rate_matches(before: [PositionSample; 2], after: [PositionSample; 2])` — computes an
  instantaneous tick rate from each pair and asserts they match within D-1's epsilon; used by
  both the pause/resume and BPM-change tests.

### Start/stop scenario (`crates/propeller-sync-tests/tests/start_stop.rs`)
Drives `propeller-clock start` then `propeller-clock stop`; polls `status`'s `clock_state`
(bounded by NF-1) and asserts the `started` → `stopped` transition (F-2, AC-1).

### Pause/resume scenario (`crates/propeller-sync-tests/tests/pause_resume.rs`)
Test-first per F-8: starts playback, samples the tick rate, pauses, resumes, immediately
samples the tick rate again, and asserts they match per D-1's methodology (F-3, AC-2). Marked
`#[ignore = "pending EP-1/EP-2"]` once its red-run baseline against current behavior is recorded
(Q-1, Option A) — runnable on demand via `cargo test -- --ignored`, not part of the default
`cargo test --workspace` run until EP-1 lands and the marker is removed (cross-spec follow-up).

### BPM-change scenario (`crates/propeller-sync-tests/tests/bpm_change.rs`)
Test-first per F-8: starts playback mid-repeat, samples the tick rate, issues `propeller-clock
bpm <n>`, immediately samples the tick rate again, and asserts it reflects the new tempo by the
next processed pulse per D-1's methodology (F-4, AC-3). Marked `#[ignore = "pending EP-1/EP-2"]`
once its red-run baseline is recorded (Q-1, Option A), for the same reason and until EP-2 lands
and the marker is removed (cross-spec follow-up).

### CI workflow (`.github/workflows/ci.yml`, new)
Runs `cargo test --workspace` (or an equivalent scoped invocation covering the new crate) on
every push and pull request (NF-3). With the pause/resume and BPM-change tests `#[ignore]`d per
Q-1, this run is green on the start/stop scenario and the harness smoke test alone until EP-1/
EP-2 land.

---

## Data Model

| Type              | Fields                                              | Notes                                                                                                   |
|-------------------|------------------------------------------------------|-----------------------------------------------------------------------------------------------------------|
| `EngineGuard`      | `sock_path: PathBuf`, `sync_port_name: String`        | RAII wrapper around a `propeller start --sync` subprocess; mirrors `DaemonGuard` in the existing suite.  |
| `ClockGuard`       | `sock_path: PathBuf`, `port_name: String`             | RAII wrapper around a `propeller-clock start` subprocess; `port_name` is generated uniquely per instance via `propeller-clock`'s new port-naming option (D-2, Option B), mirroring `unique_sock_path()`. |
| `PositionSample`   | `tick: u64`, `loop_count: u64`, `sampled_at: Instant` | One `get-position` response paired with the wall-clock instant it was read at.                          |
| `RateAssertion`    | `epsilon: f64`                                        | Not a stored type — the comparison function used by `assert_rate_matches` (see D-1 for how `epsilon` is derived). |

No new types are needed on the `propeller`/`propeller-clock` side; this epic's data model is
entirely test-harness-local.

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID   | Task                                                                                                                                                                                    | Type | PRD ref                        | Depends on |
|------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|------|----------------------------------|------------|
| T-1  | Write a harness smoke test: spawn two `EngineGuard` + `ClockGuard` pairs concurrently, assert all sockets become connectable, assert each pair's virtual MIDI port names are distinct (D-2 regression coverage), and assert everything is torn down (sockets removed) after the test ends | test | F-6, F-7, NF-1                  | —          |
| T-2  | Scaffold `crates/propeller-sync-tests` (add to workspace `members`; `propeller`/`propeller-clock` as path dev-dependencies); add the port-naming CLI/env-var option to `crates/propeller-clock` (D-2, Option B, mirroring `unique_sock_path()`); implement `EngineGuard`, `ClockGuard` (using the new option to generate a unique port name), `send_engine_command`, `run_clock` so T-1 passes | impl | F-6, F-7                        | T-1        |
| T-3  | Write the start/stop scenario test: drive via `propeller-clock start`/`stop`, poll `status.clock_state`, assert `started` → `stopped`, each scenario as an independent `#[test]` fn    | test | F-1, F-2, F-5, AC-1, NF-1, NF-2  | T-2        |
| T-4  | Finalize the harness's `status`-polling helper (bounded-wait, no real-time races) so T-3 passes reliably                                                                               | impl | F-1, F-2, AC-1                  | T-2, T-3   |
| T-5  | Write the pause/resume speed-match test (test-first per F-8 — expected **red** against current pause/resume behavior until EP-1 lands), using `sample_position`/`assert_rate_matches`  | test | F-3, F-8, AC-2, NF-4             | T-2        |
| T-6  | Implement `PositionSample`, `sample_position`, and `assert_rate_matches` (per D-1's rate-comparison methodology) used by T-5; record and document the current red-run failure as the F-8 baseline | impl | F-3, F-8, NF-4                   | T-2, T-5   |
| T-7  | Write the BPM-change immediacy test (test-first per F-8 — expected **red** against current BPM-application behavior until EP-2 lands), reusing `assert_rate_matches`                   | test | F-4, F-8, AC-3, NF-4             | T-2, T-6   |
| T-8  | Wire the BPM-change test's `propeller-clock bpm <n>` invocation and next-pulse sampling into the harness so T-7 exercises the intended failure mode; record baseline red-run evidence  | impl | F-4, F-8, NF-4                   | T-6, T-7   |
| T-9  | Mark the pause/resume and BPM-change test functions `#[ignore = "pending EP-1/EP-2"]` once T-6/T-8's red-run baseline is recorded (Q-1, Option A), so they no longer fail the default `cargo test` run; confirm they still run and fail via `cargo test -- --ignored` | impl | F-8                               | T-6, T-8   |
| T-10 | Verify locally that `cargo test --workspace` builds and runs the new crate's tests alongside the existing suites (start/stop and harness smoke tests green; pause/resume and BPM-change skipped/ignored per T-9); document this as the exact command the CI job must reproduce | test | NF-3                              | T-4, T-9   |
| T-11 | Add `.github/workflows/ci.yml` running the command verified by T-10 on every `push` and `pull_request`                                                                                 | impl | NF-3                              | T-10       |

---

## Open Questions

None — Q-1 was answered and reconciled. Confidence is above the 90% threshold.

---

## Open Decisions

None — D-1 and D-2 were answered and reconciled. Confidence is above the 90% threshold.

---

## Revision Log

### Cycle 1 — Confidence: 62%
- Created spec from `specs/EP-3.md` (PRD), grounded in `crates/propeller/tests/integration.rs`
  (existing subprocess/socket test pattern), `docs/json-socket-interface.md` (F-6's available
  commands), `crates/propeller-clock` (CLI, daemon self-spawn, virtual MIDI port behavior),
  `specs/EP-1-spec.md` (D-1's exact-internal-state verification precedent, directly informing
  this epic's own D-1), and `specs/EP-1.md`/`specs/EP-2.md` (NF wording for NF-4). `specs/EP-2-
  spec.md` did not yet exist at the time of this cycle; EP-2.md's PRD (F-4/F-5/F-6/NF-2/AC-4)
  was used directly instead, per instructions.
- Reconciled: none (new spec).
- Added: Q-1 (whether F-8's red tests should block CI for unrelated PRs before EP-1/EP-2 land),
  D-1 (verification methodology for black-box zero-tolerance assertions — the central tension
  between F-6/F-7's black-box constraint and NF-4's inherited zero-tolerance requirement), D-2
  (test isolation for propeller-clock's hardcoded virtual MIDI port name, a concrete
  parallel-test-flakiness risk found in the actual code).

### Cycle 2 — Confidence: 93%
- Reconciled: Q-1 → Architecture Overview, Components (pause_resume.rs/bpm_change.rs, CI
  workflow), and tasks updated so the red tests are marked `#[ignore]` (new task T-9) and CI
  (T-11, was T-10) stays green in the interim; a cross-spec follow-up (removing `#[ignore]` in
  EP-1's/EP-2's own specs once each fix lands) is flagged as outstanding, not yet added to those
  files. D-1 → Architecture Overview's verification-approach paragraph marked as settled
  (Option A), no structural change needed since the initial draft already wrote to this option.
  D-2 → Architecture Overview, Components (`ClockGuard`), Data Model, and tasks T-1/T-2 updated:
  `propeller-clock` gains a port-naming option so `ClockGuard` generates a unique virtual MIDI
  port per instance, mirroring `unique_sock_path()`.
- Added: none — confidence at 93%, above the 90% threshold. Specification is complete; the
  cross-spec `#[ignore]`-removal follow-up for EP-1/EP-2 remains as external outstanding work,
  not a gap in this document.
