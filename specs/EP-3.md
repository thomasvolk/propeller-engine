# EP-3 · Sync Mode Integration Test Suite — PRD

## Overview
An integration test suite exercises sync mode end-to-end using propeller-clock, covering
start/stop, pause/resume, and BPM-change scenarios. When this epic is done, these scenarios
are verified automatically rather than manually, including the corrected pause/resume speed
(EP-1) and immediate BPM reaction (EP-2) behaviors.

**Confidence Level:** 91% — all five original open questions are answered and reconciled into
concrete, testable requirements: the observability mechanism, suite location/topology, tolerance
definitions (now inherited from EP-1's and EP-2's own settled PRDs), CI integration, and the
test-first approach relative to EP-1/EP-2's completion. The only remaining softness is the exact
mechanics of computing a numeric "speed" from the JSON-socket interface's available fields
(`get-position`'s `tick`/`loop_count` sampled against propeller-clock pulses) — an
implementation-level detail left to the technical spec, consistent with how EP-1 and EP-2 each
left one such detail open at similar confidence.

---

## User Journeys

### UJ-1 · Developer verifies sync mode start/stop via an automated test
A developer runs the integration test suite. A test drives propeller-clock to send a
start and then a stop, against a running propeller-engine instance in sync mode. The test
asserts the engine loop begins playback on start and halts on stop, without a human manually
operating propeller-clock and listening/observing the loop.

### UJ-2 · Developer verifies corrected pause/resume speed via an automated test
A developer runs the integration test suite after EP-1's fix has landed. A test drives
propeller-clock to start playback, pause it, and resume it, then measures the engine loop's
playback speed immediately after resume against its speed immediately before the pause. The
test fails if the loop is slower after resume than before the pause, catching a regression of
the EP-1 defect automatically.

### UJ-3 · Developer verifies immediate BPM reaction via an automated test
A developer runs the integration test suite after EP-2's fix has landed. A test drives
propeller-clock to start playback at one BPM, then changes the BPM mid-repeat. The test
asserts the engine loop's tempo updates to the new BPM without waiting for the current repeat
to finish, catching a regression of the EP-2 defect automatically.

### UJ-4 · Developer runs the suite as part of normal development
A developer working on sync-mode-adjacent code runs the integration test suite (locally
and/or in CI) to gain confidence that their change hasn't broken start/stop, pause/resume, or
BPM-change behavior, without needing to manually reproduce these scenarios by hand.

---

## Functional Requirements

| ID  | Requirement |
|-----|-------------|
| F-1 | The test suite drives propeller-engine's sync mode exclusively through propeller-clock as the clock source — it does not synthesize MIDI clock/transport messages by any other mechanism. |
| F-2 | An automated test starts and stops sync-mode playback via propeller-clock and verifies the engine loop's playback state (playing/stopped) transitions accordingly. |
| F-3 | An automated test pauses and resumes sync-mode playback via propeller-clock and verifies the engine loop's playback speed after resume exactly matches its speed before the pause (zero tolerance for measurable drift, per EP-1 NF-2), confirmed starting at the first tick/pulse emitted after resume (per EP-1 NF-3). |
| F-4 | An automated test changes BPM via propeller-clock during an in-progress repeat and verifies the engine loop's tempo reflects the new BPM by the very next clock pulse processed after the change (no settling window, per EP-2 NF-2), without waiting for the current repeat to complete. |
| F-5 | Each test scenario (start/stop, pause/resume, BPM-change) is independently runnable and independently reports pass/fail. |
| F-6 | Tests observe the engine loop's playback state, speed, and tempo via propeller-engine's existing JSON-socket interface (`docs/json-socket-interface.md`) — using `status` (`mode`, `clock_state`/`sync_clock_state`, `bpm`) and `get-position` (`tick`, `loop_duration`, `loop_count`) — rather than parsing log/trace output or adding new test-only instrumentation. |
| F-7 | The suite lives in a new integration-test crate/target that spawns both a propeller-engine daemon and a propeller-clock instance as subprocesses per test, following the existing pattern in `crates/propeller/tests/integration.rs`, keeping each test hermetic and independent of any shared running instance. |
| F-8 | The pause/resume (F-3) and BPM-change (F-4) tests are written test-first, before EP-1 and EP-2 land: they are expected to fail (red) against current buggy behavior, and are confirmed to pass (green) once EP-1's and EP-2's fixes are merged. EP-3 implementation is not blocked on EP-1/EP-2 completion. |

---

## Non-Functional Requirements

| ID   | Requirement |
|------|-------------|
| NF-1 | Each test in the suite completes within a bounded, reasonable wall-clock time suitable for routine local/CI runs (no indefinite waits on real-time clock ticks). |
| NF-2 | Tests are deterministic — repeated runs against unchanged code produce the same pass/fail result, without flaking due to real-time timing races. |
| NF-3 | The suite runs automatically in continuous integration on every PR/push, alongside propeller-engine's existing test suite, rather than being a local-only/manual suite. |
| NF-4 | Speed and tempo assertions in this suite use the same tolerance definitions settled by EP-1 and EP-2 in their own PRDs, not an independently defined tolerance: EP-1's zero-tolerance/no-settling-window definition (EP-1 NF-2, NF-3) governs the pause/resume test; EP-2's next-clock-pulse/no-settling-window definition (EP-2 NF-2) governs the BPM-change test. |

---

## Acceptance Criteria

| ID   | Given | When | Then |
|------|-------|------|------|
| AC-1 | A propeller-engine instance in sync mode | propeller-clock sends start and then stop | the engine loop's playback state transitions to playing on start and to stopped on stop |
| AC-2 | A running sync-mode loop | propeller-clock pauses and then resumes it | the loop's playback speed immediately after resume exactly matches its speed immediately before the pause (zero tolerance for drift), confirmed at the first tick/pulse emitted after resume |
| AC-3 | A running sync-mode loop mid-repeat | propeller-clock changes the BPM | the very next clock pulse processed after the change already reflects the new BPM, with no settling window |

---

## Open Questions

None — all open questions from the previous cycle have been answered and reconciled. Confidence
is above the 90% threshold.

---

## Refinement Log

### Cycle 1 — Confidence: 55%
- Created PRD from specs/roadmap.md EP-3 section.
- Reconciled: none (new PRD).
- Added: Q1 (test harness observability mechanism), Q2 (suite location and process topology), Q3 (speed/tempo measurement tolerance, tied to EP-1's own open tolerance question), Q4 (CI integration), Q5 (dependency handling relative to EP-1/EP-2 completion).

### Cycle 2 — Confidence: 91%
- Reconciled: Q1 → F-6 (observe via existing JSON-socket interface: `status` + `get-position`), Q2 → F-7 (new integration-test crate spawning propeller-engine + propeller-clock as subprocesses, per the `crates/propeller/tests/integration.rs` pattern), Q3 → NF-4 and tightened F-3/F-4/AC-2/AC-3 (reuse EP-1's zero-tolerance/no-settling-window definition and EP-2's next-pulse/no-settling-window definition, now that both are settled), Q4 → NF-3 (runs in CI on every PR/push), Q5 → F-8 (pause/resume and BPM-change tests written test-first, red until EP-1/EP-2 land, not blocking EP-3 start).
- Added: none — confidence at 91%, above the 90% threshold. Remaining softness (exactly how to compute a numeric "speed" from `get-position`'s `tick`/`loop_count` sampled against propeller-clock pulses) is an implementation detail left to the technical spec, consistent with EP-1 and EP-2's own PRDs at similar confidence.
