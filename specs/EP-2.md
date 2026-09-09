# EP-2 · Immediate BPM Reaction — PRD

## Overview
When the sync clock's BPM changes, the loop must reflect the new tempo immediately rather
than waiting for the current repeat to finish. When this epic is done, an operator changing
the clock's BPM during sync mode observes the loop's tempo change take effect right away,
mid-repeat if necessary.

**Confidence Level:** 93% — all four original open questions are answered and reconciled into
concrete, testable requirements. What remains unspecified at the PRD level (and is left to the
technical spec) is the exact definition of a "clock pulse" in implementation terms and how
bursts of rapid successive BPM changes should be sequenced — both are implementation-detail
questions rather than product-requirement gaps.

---

## User Journeys

### UJ-1 · Operator changes clock BPM during sync-mode playback
An operator is running propeller-engine in sync mode, slaved to an external MIDI clock. The
loop is actively playing at the external clock's tempo. The operator (or the external device)
changes the clock's BPM while the loop is mid-repeat. The operator expects the loop's tempo
to update immediately, without waiting for the current repeat to finish.

### UJ-2 · Operator changes clock BPM repeatedly across a session
An operator adjusts the clock's BPM multiple times over the course of a session (e.g. tempo
ramps between song sections). Each BPM change must take effect immediately; the loop must not
fall behind or drift from the clock's current tempo as a result of successive changes.

---

## Functional Requirements

| ID  | Requirement |
|-----|-------------|
| F-1 | When the sync clock's BPM changes while the loop is running, the engine loop's tracked tempo updates to the new BPM without waiting for the current repeat to complete. |
| F-2 | A BPM change takes effect mid-repeat, i.e. within the repeat that is in progress at the moment of the change, not only at the start of the next repeat. |
| F-3 | Repeated BPM changes within a session are each applied immediately, without cumulative lag or drift relative to the clock's current tempo. |
| F-4 | This epic's fix covers BPM changes originating from the external sync clock (e.g. MIDI clock tempo messages) only. BPM changes made locally (e.g. an operator editing the project BPM directly while sync mode is active) are out of scope. |
| F-5 | Root-cause investigation is in scope: the implementation must identify and address the underlying mechanism that currently defers BPM application until repeat completion, rather than adding a workaround that only masks the symptom. (Note: EP-1's technical spec identified a candidate mechanism shared with EP-1's pause/resume bug — `pending_sync_bpm` being applied only once per loop pass in `advance_loop()`, `crates/propeller/src/loop_engine/player.rs`. This epic's investigation should confirm whether the same mechanism is responsible here.) |
| F-6 | When a BPM change occurs mid-repeat, the loop's position within the current repeat (e.g. current beat/step) is left unchanged; only the rate at which the loop advances updates to reflect the new tempo. |

---

## Non-Functional Requirements

| ID   | Requirement |
|------|-------------|
| NF-1 | The loop's tempo after a BPM change must not lag the clock's new tempo by a full repeat cycle. |
| NF-2 | "Immediate" is defined as: the next clock pulse processed after a BPM change already reflects the new tempo. No settling window of additional pulses is acceptable. |

---

## Acceptance Criteria

| ID   | Given | When | Then |
|------|-------|------|------|
| AC-1 | A running sync-mode loop | the clock's BPM changes | the very next clock pulse processed after the change already reflects the new tempo, with no settling window |
| AC-2 | A running sync-mode loop mid-repeat | the clock's BPM changes | the loop does not require a full repeat cycle to elapse before the new BPM takes effect; the next clock pulse processed after the change reflects the new tempo |
| AC-3 | A sync-mode loop that has had its BPM changed multiple times in one session | its tempo is measured after each change | tempo matches the clock's current BPM every time, with no cumulative lag across changes |
| AC-4 | A sync-mode loop mid-repeat at a given position | the clock's BPM changes | the loop's position within the repeat is unchanged immediately after the change, and only the rate of advancement reflects the new tempo |

---

## Open Questions

None. All open questions from prior cycles have been answered and reconciled.

---

## Refinement Log

### Cycle 1 — Confidence: 55%
- Created PRD from specs/roadmap.md EP-2 section.
- Reconciled: none (new PRD).
- Added: Q1 (source and scope of BPM changes), Q2 (precision/tolerance for "immediate"), Q3 (root-cause investigation scope), Q4 (effect of mid-repeat BPM change on repeat position).

### Cycle 2 — Confidence: 93%
- Reconciled: Q1 → F-4 (scope limited to external clock BPM changes), Q2 → NF-2 and AC-1/AC-2 tightened to next-pulse precision, Q3 → F-5 (root-cause investigation in scope, noting shared-mechanism risk with EP-1), Q4 → F-6 and AC-4 (mid-repeat change is rate-only, position unchanged).
- Added: none — confidence at 93%, above the 90% threshold.
