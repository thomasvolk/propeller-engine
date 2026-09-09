# EP-2 · Immediate BPM Reaction — Technical Specification

## Overview
This epic fixes the sync-mode BPM-tracking path in `PlayerLoop`
(`crates/propeller/src/loop_engine/player.rs`) so that a BPM change originating from the
external sync clock reaches the loop's scheduled event timing by the very next clock pulse,
instead of sitting inert until the current repeat finishes. Root-cause investigation is itself
in scope (PRD F-5) and precedes the corrective fix, but the mechanism is already well evidenced
from reading the real code (see below) — this is a stronger starting position than EP-1's
equivalent investigation.

**Confidence Level:** 92% — the deferred-application mechanism is directly evidenced by the
code and matches the reported symptom precisely, the placement of the immediate-apply fix is
now settled (D-1: Option C), and the scope boundary with paused/stopped handling is now settled
(Q-1: Option A). The one residual note (not a specification gap) is that this resolution, taken
together with EP-1's own Q-1 resolution, leaves BPM updates staged while `Paused`/`Stopped`
unaddressed by *either* epic — see the note at the end of Architecture Overview.

---

## Architecture Overview

Playback is driven by `PlayerLoop`, a plain (non-async) thread running a state machine over
`EngineState` (`Stopped`, `Waiting`, `Running`, `Paused`). Timing is anchor-based, not
tick-driven by a running clock: a `Scheduler` (`crates/propeller/src/loop_engine/scheduler.rs`)
holds the current tempo (`bpm`, `micros_per_tick`), and an `anchor: Instant` marks when tick 0
of the current loop pass occurred. Every event's fire time is computed on demand as
`scheduler.deadline_for_tick(anchor, tick)` — never stored — so retiming only ever requires
moving `anchor` or changing the scheduler's rate.

Within a running loop pass, `play_events` (player.rs:419-454) iterates the pass's sorted event
list. For each event it computes `let deadline = self.scheduler.deadline_for_tick(self.anchor,
tick)` (player.rs:424) fresh, using whatever `self.scheduler` currently holds, then waits for
that deadline via `sleep_until_with_poll` (player.rs:85-123), which polls the command channel
roughly every 1ms while waiting.

External sync tempo tracking arrives as `LoopCommand::SyncBpmUpdate(f64)` and is staged into
`pending_sync_bpm: Option<f64>` at several points, but is **never applied to the scheduler at
any of those points** — it is only read and consumed once, inside `advance_loop()`
(player.rs:591-650, specifically 607-626), which runs only after `play_events` returns
`LoopOutcome::Complete`, i.e. only when the current loop pass has fully finished:

- `sleep_until_with_poll` (player.rs:106): `Ok(LoopCommand::SyncBpmUpdate(bpm)) =>
  *pending_sync_bpm = Some(bpm)` — reached while waiting between events, mid-repeat.
- `handle_mid_loop_command` (player.rs:364-367): `self.pending_sync_bpm = Some(bpm); None` —
  reached when a command arrives immediately after an event fires, mid-repeat.
- `handle_paused` (player.rs:820-822) and `handle_stopped` (player.rs:711-713): same staging,
  reached while the loop isn't actively iterating events at all.

This confirms the F-5 candidate root cause already identified in EP-1's technical spec: a BPM
update landing anywhere during a loop pass sits in `pending_sync_bpm` until the pass wraps and
`advance_loop()` runs — which is exactly "the loop has to repeat first," the originally
reported symptom. This is a stronger match for EP-2's bug than the same mechanism was for EP-1's
(EP-1's symptom — post-resume slowdown — only plausibly traces to this mechanism; EP-2's
symptom — BPM change requires a full repeat to take effect — is a direct, mechanical
restatement of "`pending_sync_bpm` is applied only once per loop pass"). It is still presented
here as the primary, well-evidenced lead rather than a confirmed fact, per F-5's requirement
that root-cause investigation remain in scope — task T-2 below exists to confirm it with a
failing-test repro before any fix lands.

When `advance_loop()` does apply a staged update, it uses `Scheduler::update_bpm_precise`
(scheduler.rs:51-54), not `update_bpm` (scheduler.rs:41-43), and deliberately does not touch
`anchor` — both documented in scheduler.rs's comments as fixes for real prior drift bugs:
`update_bpm_precise` takes the tracked tempo unrounded (rounding was found to leave a small but
systematic rate bias), and not touching `anchor` avoids rebasing the phase reference to
`Instant::now()` on every update, which in sync mode fires on essentially every pass and would
otherwise accumulate into audible drift. Any fix that moves *when* `pending_sync_bpm` is applied
must preserve both properties: keep using `update_bpm_precise`, and never rebase `anchor` as
part of applying a sync BPM update (that rebase behaviour is intentionally reserved for the
separate local-BPM branch in `advance_loop`, player.rs:627-636, which is out of this epic's
scope per F-4).

Because `deadline_for_tick` is computed fresh from `self.scheduler` on every call, and
`update_bpm_precise` doesn't touch `anchor`, moving the point at which `pending_sync_bpm` is
consumed earlier — into `play_events`, as soon as it's staged, rather than waiting for
`advance_loop()` — is structurally straightforward for any event whose deadline hasn't been
computed yet: the very next iteration of `play_events`'s loop will pick up the new rate
automatically. The one genuine wrinkle is the event currently being waited for when the update
arrives: its `deadline` was already computed (and captured in a local variable) before
`sleep_until_with_poll` was called, so simply letting `pending_sync_bpm` be applied only *after*
that wait returns leaves that one in-flight tick firing at the old rate.

**Resolved (D-1, Option C):** `sleep_until_with_poll` gains a new early-return `SleepResult`
variant, `BpmChanged`, returned as soon as it observes a `SyncBpmUpdate` on the channel, without
otherwise changing its signature (it keeps taking `scheduler: &Scheduler`, read-only).
`play_events` — which already owns `anchor`, `tick`, and `self.scheduler` — handles this
variant by applying `Scheduler::update_bpm_precise`, recomputing `deadline` for the same tick,
and calling `sleep_until_with_poll` again for the remainder of the wait. This covers the
mid-wait edge case exactly, satisfying AC-1/NF-2's literal "no settling window" wording even for
the tick currently being waited on, without enlarging `sleep_until_with_poll`'s signature.

**Resolved (Q-1, Option A):** this epic's fix touches only the actively-running-loop path
(`play_events` / `sleep_until_with_poll`). A `SyncBpmUpdate` staged while `Paused` or `Stopped`
(`handle_paused` player.rs:820-822, `handle_stopped` player.rs:711-713) continues to be picked
up only when the loop next reaches `advance_loop()` or otherwise resumes — unchanged by this
epic. **Note:** EP-1's own technical spec resolved its equivalent question (its Q-1) the same
way — scoping its fix to the resume-time anchor/tempo carry-over only, explicitly not touching
`pending_sync_bpm`'s general application timing. Taken together, both epics' resolutions leave
a BPM update staged while Paused/Stopped applying no earlier than it does today; if that turns
out to matter in practice, it is unaddressed backlog work belonging to neither epic as currently
scoped, not a gap in this specification.

---

## Components

### `PlayerLoop::play_events` (`crates/propeller/src/loop_engine/player.rs:419-454`)

Owns the per-tick wait/emit loop within a single pass. This epic's fix lands here: instead of
letting `pending_sync_bpm` merely accumulate across the pass, `play_events` applies it to
`self.scheduler` via `update_bpm_precise` as soon as it observes the value staged — either
between events (the existing polling paths) or mid-wait via the new `SleepResult::BpmChanged`
early return (D-1, Option C) — recomputing `deadline` for the current tick before resuming the
wait. `loop_elapsed_ticks` and `current_tick` are untouched by this — per F-6/AC-4, only the
scheduler's rate changes, never loop position.

### `sleep_until_with_poll` (`crates/propeller/src/loop_engine/player.rs:85-123`)

Free function; polls the command channel roughly every 1ms while waiting for a fixed `deadline`.
Keeps its existing signature, including the read-only `scheduler: &Scheduler` parameter (used
only for `scheduler.sleep_until` on the final short spin). Gains one new early-return path: on
observing `Ok(LoopCommand::SyncBpmUpdate(bpm))`, instead of only staging it into
`pending_sync_bpm` and continuing to poll, it returns a new `SleepResult::BpmChanged` variant
immediately (D-1, Option C), handing control back to `play_events` to apply the update and
recompute the deadline before the wait resumes.

### `Scheduler` (`crates/propeller/src/loop_engine/scheduler.rs`)

Owns `bpm` / `micros_per_tick` and deadline computation (`deadline_for_tick`, `update_bpm`,
`update_bpm_precise`). No changes anticipated to `Scheduler` itself — `update_bpm_precise`
already has the exact semantics this epic's fix needs (unrounded rate, anchor untouched); the
fix is entirely about *when* `PlayerLoop` calls it.

### `PlayerLoop::advance_loop` (`crates/propeller/src/loop_engine/player.rs:591-650`)

Retains its existing `if let Some(sync_bpm) = self.pending_sync_bpm.take() { ... }` branch
(player.rs:607-626) as a fallback: once the fix lands, `pending_sync_bpm` should normally
already be `None` by the time `advance_loop()` runs (because `play_events` consumed it earlier),
but the branch stays correct and harmless for the case where a BPM update was staged while
`Paused` or `Stopped` (i.e. outside `play_events` entirely) and is only now being picked up as
the loop (re)starts running — that path is unchanged by this epic (Q-1, Option A).

---

## Data Model

No new persistent types. `PauseContext`, `Scheduler`, and `PlayerLoop`'s existing fields
(`pending_sync_bpm: Option<f64>`, `scheduler: Scheduler`, `anchor: Instant`) are sufficient.

| Type | Fields | Notes |
|------|--------|-------|
| `SleepResult` (player.rs:55-64) | new variant `BpmChanged`, returned by `sleep_until_with_poll` as soon as it observes a `SyncBpmUpdate` on the channel | per D-1 (Option C); handled by `play_events`, which applies `update_bpm_precise`, recomputes `deadline` for the current tick, and calls `sleep_until_with_poll` again for the remainder of the wait |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID   | Task | Type | PRD ref | Depends on |
|------|------|------|---------|------------|
| T-1  | Write a test that stages a `SyncBpmUpdate` mid-pass (before all of the current loop pass's events have fired) and asserts the scheduler's tracked rate / the deadline computed for the next not-yet-fired event reflects the new BPM immediately — i.e. assert this directly after `play_events` observes the command, not after the pass wraps via `advance_loop()`. This is the repro for F-5's investigation. | test | F-1, F-2, F-5, AC-1, AC-2, NF-1, NF-2 | — |
| T-2  | Investigate the root cause using T-1 as a repro: trace every place `pending_sync_bpm` is set (`sleep_until_with_poll`, `handle_mid_loop_command`, `handle_paused`, `handle_stopped`) against the single place it's consumed (`advance_loop`, player.rs:607-626); confirm this deferred-application is what makes T-1 fail, and record findings, per F-5. | impl | F-5 | T-1 |
| T-3  | Implement the fix identified by T-2: add `SleepResult::BpmChanged` to `sleep_until_with_poll` (returned on observing `SyncBpmUpdate`), and have `play_events` apply `pending_sync_bpm` to `self.scheduler` via `Scheduler::update_bpm_precise` and recompute `deadline` as soon as it is staged, rather than deferring to `advance_loop()`, so T-1 passes. | impl | F-1, F-2, F-5, AC-1, AC-2, NF-1, NF-2 | T-2 |
| T-4  | Write a test asserting that a mid-repeat `SyncBpmUpdate` leaves `loop_elapsed_ticks` / `current_tick` (loop position) unchanged immediately after being applied — only `self.scheduler`'s rate changes. | test | F-6, AC-4 | T-1 |
| T-5  | Confirm/adjust the T-3 fix so applying `pending_sync_bpm` never mutates `anchor`, `current_tick`, or `loop_elapsed_ticks` (only `Scheduler::update_bpm_precise`'s rate fields change), so T-4 passes. | impl | F-6, AC-4 | T-3, T-4 |
| T-6  | Write a test that stages a `SyncBpmUpdate` strictly *during* `sleep_until_with_poll`'s wait for an already-computed deadline (not right after an event fires) and asserts `sleep_until_with_poll` returns `SleepResult::BpmChanged` and the in-flight tick's own deadline is recomputed to reflect the new tempo immediately, satisfying AC-1/NF-2's "no settling window" wording even in this edge case. | test | NF-2, AC-1 | T-1 |
| T-7  | Implement the `SleepResult::BpmChanged` early return in `sleep_until_with_poll` and the corresponding handling in `play_events` (apply `update_bpm_precise`, recompute `deadline` for the same tick, re-enter the wait) so T-6 passes. | impl | NF-2, AC-1 | T-3, T-6 |
| T-8  | Write a test that applies multiple successive `SyncBpmUpdate`s (both within one pass and across pass boundaries) and asserts each is reflected by its own next pulse with no cumulative lag or drift relative to the clock's tracked tempo. | test | F-3, AC-3 | T-1 |
| T-9  | Extend/confirm the fix so repeated updates each apply immediately with `pending_sync_bpm.take()` semantics preserved (last staged value wins, applied via the unrounded `update_bpm_precise`) at the new, earlier application point, so T-8 passes. | impl | F-3, AC-3 | T-3, T-8 |
| T-10 | Write a regression test confirming that local (non-sync) project BPM changes — `advance_loop`'s `else` branch (player.rs:627-636), which uses `update_bpm` and rebases `anchor` to `Instant::now()` — are unaffected: still applied only once per pass, still rebasing `anchor` as before. | test | F-4 | T-3 |
| T-11 | Confirm/adjust the fix so it touches only the `pending_sync_bpm` / external-clock path introduced/moved by T-3, leaving the local-BPM `advance_loop` branch untouched, so T-10 passes. | impl | F-4 | T-3, T-10 |

---

## Open Questions

None — Q-1 was answered and reconciled. Confidence is above the 90% threshold.

---

## Open Decisions

None — D-1 was answered and reconciled. Confidence is above the 90% threshold.

---

## Revision Log

### Cycle 1 — Confidence: 80%
- Created spec from specs/EP-2.md (PRD), grounded directly in the current implementation of
  `pending_sync_bpm` staging/consumption in player.rs and `update_bpm_precise` in scheduler.rs.
- Reconciled: none (new spec).
- Added: Q-1 (scope overlap with EP-1's paused-state `pending_sync_bpm` handling), D-1 (where
  within `play_events`/`sleep_until_with_poll` to apply the immediate BPM update, with a
  concrete mid-wait edge-case trade-off between three placement options).

### Cycle 2 — Confidence: 92%
- Reconciled: Q-1 → Architecture Overview + `advance_loop` component note confirm the fix is
  scoped to the Running/`play_events` path only, with an explicit note that this — combined with
  EP-1's matching Q-1 resolution — leaves paused/stopped-staged BPM updates unaddressed by either
  epic; D-1 → Architecture Overview, Components (`play_events`, `sleep_until_with_poll`), Data
  Model (`SleepResult::BpmChanged`), and tasks T-3/T-6/T-7 updated to state the early-return
  approach as settled rather than pending.
- Added: none — confidence at 92%, above the 90% threshold. Specification is complete pending
  T-2's investigation findings, which are inherent epic scope (F-5) rather than a specification
  gap.
