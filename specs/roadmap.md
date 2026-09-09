# Roadmap: Sync Mode Reliability and Test Coverage

Sync mode currently misbehaves after a pause/resume cycle and does not react immediately to
clock BPM changes. This roadmap fixes both timing defects and backs sync mode with an
integration test suite driven by propeller-clock, covering start/stop, pause/resume, and BPM
changes.

---

## Architecture and technology constraints

- The integration test suite must use propeller-clock to test all necessary scenarios — applies to EP-3

---

## Dependency graph

| Epic | Depends on | Can start in parallel with |
| ---- | ---------- | --------------------------- |
| EP-1 | —          | EP-2                        |
| EP-2 | —          | EP-1                        |
| EP-3 | EP-1, EP-2 | —                            |

---

## EP-1 — Correct-Speed Resume

When sync mode playback is paused and then resumed, the loop must resume at its correct
speed. Today, resuming after a pause leaves the loop running noticeably slower than before
the pause. When this epic is done, an operator pausing and resuming sync mode observes the
loop continuing at the same speed it had before the pause, with no lingering slowdown.

**Acceptance criteria**

- Given a running sync-mode loop, when it is paused and then resumed, the loop's playback
  speed immediately after resume matches its speed immediately before the pause.
- No perceptible or measurable slowdown of the loop persists after a pause/resume cycle.

---

## EP-2 — Immediate BPM Reaction

When the sync clock's BPM changes, the loop must reflect the new tempo immediately rather
than waiting for the current repeat to finish. When this epic is done, an operator changing
the clock's BPM during sync mode observes the loop's tempo change take effect right away,
mid-repeat if necessary.

**Acceptance criteria**

- Given a running sync-mode loop, when the clock's BPM changes, the loop's tempo updates to
  the new BPM without waiting for the current repeat to complete.
- The loop does not require a full repeat cycle to elapse before the new BPM takes effect.

---

## EP-3 — Sync Mode Integration Test Suite

An integration test suite exercises sync mode end-to-end using propeller-clock, covering
start/stop, pause/resume, and BPM-change scenarios. When this epic is done, these scenarios
are verified automatically rather than manually, including the corrected pause/resume speed
and immediate BPM reaction behavior.

**Acceptance criteria**

- An automated test verifies sync mode start and stop behavior, driven by propeller-clock as
  the clock source.
- An automated test verifies sync mode pause and resume behavior, confirming the loop's speed
  after resume matches its speed before the pause.
- An automated test verifies that a BPM change made via propeller-clock takes effect on the
  loop immediately, without requiring the loop to complete a repeat first.

**Constraints**

- Must use propeller-clock to test all necessary scenarios

---
