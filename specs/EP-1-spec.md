# EP-1 · Correct-Speed Resume — Technical Specification

## Overview
This epic fixes the sync-mode pause/resume path in `PlayerLoop`
(`crates/propeller/src/loop_engine/player.rs`) so that resumed playback runs at the same
tempo it had before the pause, from the very first tick, with no drift across repeated
pause/resume cycles. The root cause of the current slowdown is not yet confirmed; investigating
it is itself in scope (PRD F-5) and precedes the corrective fix.

**Confidence Level:** 90% — scope boundary with EP-2 and the test verification strategy are now
settled; the only remaining softness is that the precise fix inside the resume path awaits T-2's
investigation, which is inherent to F-5 rather than a gap in this specification.

---

## Architecture Overview

Playback is driven by `PlayerLoop`, a plain (non-async) thread running a state machine over
`EngineState` (`Stopped`, `Waiting`, `Running`, `Paused`), dispatched each iteration by
`run()` to `handle_stopped` / `handle_waiting` / `handle_running` / `handle_paused`.

Timing is anchored, not tick-driven by a running clock: a `Scheduler` holds the current tempo
(`bpm`, `micros_per_tick`), and an `anchor: Instant` marks when tick 0 of the current loop pass
occurred. Every event's fire time is computed on demand as
`scheduler.deadline_for_tick(anchor, tick)`, so retiming only ever requires moving `anchor` or
changing the scheduler's rate — individual event deadlines are never stored.

Pausing (`do_pause`, player.rs:271-279, reached from `ClockPause`/`SyncStop` — see
`handle_sleep_result` and `handle_mid_loop_command`) flushes active notes, resets pitch bend,
and freezes the current loop pass's unplayed events into a `PauseContext { remaining_events,
loop_duration }` (player.rs:20-24). `current_tick` is deliberately left untouched so the frozen
position survives the pause.

Resuming into `Running` (`handle_running`, player.rs:756-769) takes the `PauseContext` and
recomputes `anchor` from the tick of the first remaining event, so that event's deadline is
effectively "now":

```
let tick_of_next = ctx.remaining_events.first().map(|(t, _)| *t).unwrap_or(0);
self.anchor = Instant::now()
    - Duration::from_micros(tick_of_next * self.scheduler.micros_per_tick());
```

External sync tempo tracking arrives as `LoopCommand::SyncBpmUpdate(f64)` and is staged into
`pending_sync_bpm` — including while `Paused` (`handle_paused`, player.rs:820-822) — but is
only ever applied to the scheduler once per loop pass, inside `advance_loop()`
(player.rs:591-650), via `Scheduler::update_bpm_precise`. This staged-application design is
also the documented cause of the separate EP-2 symptom ("BPM change doesn't take effect until
the loop repeats").

**Scope boundary (resolved by Q-1):** this epic's fix must not change how or when
`pending_sync_bpm` is generally applied — that mechanism, and any change to its cadence, is
EP-2's scope. This epic's fix is limited to the resume-time anchor/tempo carry-over below. If
T-2's investigation finds that the general `pending_sync_bpm` cadence is itself a contributor to
this epic's slowdown (not just a candidate, see the second suspected area below), that finding
is out of scope to fix here and must be handed off as follow-on work against EP-2 rather than
implemented as part of this epic.

**Suspected areas (unconfirmed — T-2 investigates before any fix lands):**
- The resume-time anchor recompute above uses `Scheduler::micros_per_tick()`, the **truncated**
  (whole-microsecond) tick duration, while `deadline_for_tick` — used everywhere else — uses
  the **untruncated** tick duration specifically to avoid drift (scheduler.rs:23-34). This is
  squarely within this epic's scope: it lives entirely in the resume-time recompute, not in
  `pending_sync_bpm`'s general application cadence.
- A `SyncBpmUpdate` received while `Paused` is staged but not applied until the next
  `advance_loop()` boundary, so a loop resumed mid-pass continues at whatever tempo the
  scheduler held before the pause, not any tempo learned during the pause. Per the scope
  boundary above, if this turns out to be a contributor, fixing it is not this epic's job.

**Test verification strategy (resolved by D-1):** tests assert on `PlayerLoop`'s internal
computed state directly — `anchor`, the scheduler's rate, and the deadline computed for the next
tick after resume — via direct calls into the resume path, not real-clock timing. This matches
this codebase's existing test conventions and is the only way to meaningfully prove NF-2's
zero-tolerance requirement.

---

## Components

### `PlayerLoop` resume path — `handle_paused`, `handle_running`, `do_pause`, `do_sync_continue`
(`crates/propeller/src/loop_engine/player.rs`)

Owns the `Paused` → `Running` transition: recomputing `anchor`, restoring `loop_duration`, and
replaying `remaining_events` through `play_events`. This epic's fix lands here — scoped to the
resume-time anchor/tempo carry-over only, per Q-1's resolution; it does not change when/how
`pending_sync_bpm` is generally applied.

### `Scheduler` (`crates/propeller/src/loop_engine/scheduler.rs`)

Owns `bpm` / `micros_per_tick` and deadline computation (`deadline_for_tick`,
`update_bpm`, `update_bpm_precise`). May gain a corrected resume-anchor helper or an adjusted
`micros_per_tick` usage, depending on T-2's findings — not yet decided, but any change here
stays confined to the resume-time computation, not the general BPM-application cadence.

---

## Data Model

No new types are anticipated. The fix is expected to be a correction to existing logic in
`PlayerLoop` (`PauseContext`, the resume-time `anchor` recompute) and/or `Scheduler`, not a new
data structure. If T-2's investigation reveals a need for additional state (e.g. capturing the
tempo at the moment of pause separately from the live-tracked tempo), that will be added as a
field on `PauseContext` in a later revision cycle once confirmed.

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID   | Task                                                                                                                                                             | Type | PRD ref                         | Depends on |
|------|--------------------------------------------------------------------------------------------------------------------------------------------------------------|------|----------------------------------|------------|
| T-1  | Write a test asserting exact resume tempo: drive `PlayerLoop`'s pause/resume path directly and assert the anchor/deadline computed for the first tick after resume reflects the same tempo as before the pause, with zero tolerance | test | F-1, F-2, AC-1, NF-1, NF-2, NF-3 | —          |
| T-2  | Investigate the root cause of the post-resume slowdown using T-1 as a repro: trace `do_pause`, `do_sync_continue`, and the `handle_running` anchor recompute; determine whether `pending_sync_bpm`'s general application cadence is also a contributor (if so, note it as EP-2 follow-on work per the scope boundary, not something to fix here); record findings | impl | F-5                              | T-1        |
| T-3  | Implement the fix identified by T-2 within the resume-time anchor/tempo carry-over so T-1 passes — must not change when/how `pending_sync_bpm` is generally applied | impl | F-1, F-2, AC-1, NF-1, NF-2, NF-3 | T-2        |
| T-4  | Write a test asserting speed stays correct across the repeats that follow a resumed loop, not just the instant of resume                                       | test | F-2, AC-2                        | T-1        |
| T-5  | Extend the T-3 fix if T-4 exposes drift beyond the immediate resume instant                                                                                     | impl | AC-2                             | T-3, T-4   |
| T-6  | Write a test that pauses/resumes a sync-mode loop multiple times in one session and asserts no cumulative drift across cycles                                  | test | F-3, AC-3                        | T-1        |
| T-7  | Extend the fix, if needed, so repeated pause/resume cycles remain drift-free                                                                                    | impl | F-3, AC-3                        | T-3, T-6   |
| T-8  | Write a regression test confirming standalone (non-sync) `ClockPause`/`ClockResume` behaviour is unchanged by the fix                                          | test | F-4                               | T-3        |
| T-9  | Confirm/adjust the fix so it applies only to the sync-mode pause/resume path, leaving standalone clock-pause/resume untouched                                  | impl | F-4                               | T-3, T-8   |

---

## Open Questions

None — Q-1 was answered and reconciled. Confidence is above the 90% threshold.

---

## Open Decisions

None — D-1 was answered and reconciled. Confidence is above the 90% threshold.

---

## Revision Log

### Cycle 1 — Confidence: 68%
- Created spec from specs/EP-1.md (PRD).
- Reconciled: none (new spec).
- Added: Q-1 (scope overlap with EP-2's BPM-immediacy fix), D-1 (test verification strategy for zero-tolerance resume speed).

### Cycle 2 — Confidence: 90%
- Reconciled: Q-1 → Architecture Overview + Components narrowed to a resume-only scope boundary with EP-2, and T-2/T-3 task descriptions updated accordingly; D-1 → Architecture Overview confirms tests assert on computed state directly (T-1 already matched this, now stated as settled rather than proposed).
- Added: none — confidence at 90%, above the 90% threshold. Specification is complete pending T-2's investigation findings, which are inherent epic scope (F-5) rather than a specification gap.
