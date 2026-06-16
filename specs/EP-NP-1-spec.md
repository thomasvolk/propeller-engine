# EP-NP-1 · Domain Model Refactoring — Technical Specification

## Overview

This epic replaces the bar-based domain model in `src/domain/project.rs` with a flat,
tick-addressed note list. `Header` gains `loop_duration: u32` in place of `time_signature`.
`Track` gains `notes: Vec<Note>` in place of `bars: Vec<Bar>`. `Note` becomes a named
four-field struct with absolute `start_tick`. The types `TimeSignature`, `Bar`, and
`NoteEvent`, along with the methods `bar_ticks()`, `cycle_length()`, and `bar_at()`, are
deleted with no replacements. Because NF-3 requires `cargo test` to pass independently of
later epics, all five downstream files that reference the removed types are updated in this
epic. `ipc/types.rs` is not changed (that is EP-NP-2's scope); a compatibility shim in
`handler.rs` bridges the old wire format to the new domain types.

**Confidence Level:** 93% — all open questions and decisions are resolved; architecture,
data model, and task list are complete and unambiguous.

---

## Architecture Overview

The primary change is to `src/domain/project.rs`. The file becomes a pure data-definition
module: one constant, four named structs, no `impl` blocks with instance methods.

Because the domain types are consumed by three other modules (`validation.rs`,
`player.rs`, `handler.rs`) and re-exported through `mod.rs`, all four of those files must
also be updated. The roadmap assigns the authoritative implementations of these modules to
later epics (EP-NP-2, EP-NP-3, EP-NP-4); EP-NP-1 updates them only to the degree needed
to compile and pass tests.

**Wire format shim.** `src/ipc/types.rs` retains the old `WireHeader` with
`time_signature` and `WireBar`/`WireNote` until EP-NP-2. Inside
`handler.rs::build_domain_project()`, `Header.loop_duration` is computed from the wire
time-signature using the formula:

```
loop_duration = ts.numerator * (PPQN * 4 / ts.denominator)
```

This keeps all existing handler tests green without touching `types.rs`.

**Status response change.** `handle_status()` no longer emits a `time_signature` JSON
field. The test `status_with_project_stopped` is updated to assert that `time_signature`
is absent. EP-NP-4 will add `loop_duration` to the status response when it extends the
handler.

**Minimal player update.** `player.rs` is updated only enough to compile and pass
existing tests. `build_normal_bar()` iterates `track.notes` using absolute `start_tick`
and `duration` values; `header.loop_duration` replaces `bar_ticks()`. `bar_index` is
hardcoded to `0` on every loop pass and its field is retained on `PlayerLoop` and
`PauseContext` to avoid dead-code restructuring. EP-NP-3 then removes `bar_index` and
implements carry-over.

---

## Components

### `src/domain/project.rs`

Sole file for the domain data definitions. After this epic it contains: `pub const
PPQN: u32 = 480`, and the structs `Project`, `Header`, `Track`, `Note`. No `impl` blocks.

### `src/domain/mod.rs`

Re-exports the public domain API. Updated to remove `Bar`, `NoteEvent`, `TimeSignature`
from the `pub use` statement; `Note` was already exported and remains.

### `src/domain/validation.rs`

Validates a `Project` against business rules. After this epic the `ValidationError` enum
retains only `BpmOutOfRange`, `InvalidMidiChannel`, `InvalidMidiInstrument`, and
`NoteDurationZero { track: usize, note: usize }` (the `bar` field is removed). The
`validate()` function is rewritten to iterate `track.notes` instead of `track.bars`.
Bar-count, time-signature, and bar-overflow rules are deleted; their addition in the new
tick-based form is EP-NP-4's scope.

### `src/loop_engine/player.rs`

The playback loop. Updated minimally to compile with new domain types:
- `build_normal_bar()` iterates `track.notes`, reading `note.start_tick` and
  `note.duration` directly.
- `bar_ticks()` calls are replaced with `header.loop_duration`.
- `bar_index` is forced to `0` on every iteration; the field is kept on `PlayerLoop` and
  `PauseContext` unchanged. EP-NP-3 owns the full restructuring.

### `src/ipc/handler.rs`

IPC command handler. Updated to construct domain objects using new types:
- Inline test helpers updated to use `Header { bpm, loop_duration }` and
  `Note { start_tick, duration, pitch, velocity }`.
- `wire_bar_to_domain()` and `wire_note_to_domain()` are deleted; note conversion is
  inlined into `wire_track_to_domain()`.
- `build_domain_project()` computes `loop_duration` via the shim formula above.
- `handle_status()` drops the `time_signature` field from its JSON response; the
  corresponding test is updated to assert its absence.

---

## Data Model

| Type      | Fields                                                               | Notes                                                         |
|-----------|----------------------------------------------------------------------|---------------------------------------------------------------|
| `Project` | `header: Header`, `tracks: Vec<Track>`                              | No instance methods; unchanged struct shape                   |
| `Header`  | `bpm: u32`, `loop_duration: u32`                                    | Replaces `time_signature: TimeSignature`                      |
| `Track`   | `name: String`, `channel: u8`, `instrument: u8`, `notes: Vec<Note>`| Replaces `bars: Vec<Bar>`                                     |
| `Note`    | `start_tick: u32`, `duration: u32`, `pitch: u8`, `velocity: u8`    | Replaces `{ event: NoteEvent, duration_ticks: u32 }`          |
| `PPQN`    | `pub const PPQN: u32 = 480`                                         | Unchanged                                                     |

**Deleted types:** `TimeSignature`, `Bar`, `NoteEvent`

**Deleted methods:** `TimeSignature::bar_ticks()`, `Project::cycle_length()`, `Track::bar_at()`

**`ValidationError` after this epic:**

| Variant                                              | Status  |
|------------------------------------------------------|---------|
| `BpmOutOfRange { actual: u32 }`                      | Kept    |
| `InvalidMidiChannel { track: usize, actual: u8 }`   | Kept    |
| `InvalidMidiInstrument { track: usize, actual: u8 }`| Kept    |
| `NoteDurationZero { track: usize, note: usize }`    | Kept    |
| `InvalidTimeSignatureNumerator`                      | Deleted |
| `InvalidTimeSignatureDenominator { actual: u32 }`   | Deleted |
| `EmptyTrackBars { track: usize }`                    | Deleted |
| `NoteDurationExceedsBar { ... }`                     | Deleted (EP-NP-4 adds `NoteDurationExceedsLimit`) |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID   | Task                                                                                                                             | Type | PRD ref              | Depends on    |
|------|----------------------------------------------------------------------------------------------------------------------------------|------|----------------------|---------------|
| T-1  | Write `test_header_fields`: construct `Header { bpm: 120, loop_duration: 1920 }`, assert both fields readable                   | test | F-1, AC-1            | —             |
| T-2  | Write `test_header_zero_loop_duration`: construct `Header { bpm: 120, loop_duration: 0 }`, assert construction succeeds         | test | F-12                 | —             |
| T-3  | Write `test_note_fields`: construct `Note { start_tick, duration, pitch, velocity }`, assert all four fields readable           | test | F-3, AC-2            | —             |
| T-4  | Write `test_track_with_notes`: construct `Track` with `notes: vec![…]`, assert `track.notes` matches; confirm no `bars` field   | test | F-2, AC-3            | T-3           |
| T-5  | Write `test_track_empty_notes`: construct `Track { notes: vec![] }`, assert construction succeeds without panic                 | test | F-11, AC-4           | —             |
| T-6  | Write `test_ppqn_value`: assert `PPQN == 480u32` via `use crate::domain::PPQN`                                                  | test | F-10, AC-7           | —             |
| T-7  | Redefine `Header`, `Note`, `Track` in `project.rs`; delete `TimeSignature`, `Bar`, `NoteEvent`; delete `bar_ticks()`, `cycle_length()`, `bar_at()`; delete old tests `test_construct_project`, `test_bar_ticks`, `test_cycle_length`, `test_bar_at` | impl | F-1–F-13, AC-1–AC-8, NF-2 | T-1–T-6 |
| T-8  | Update `mod.rs`: remove `Bar`, `NoteEvent`, `TimeSignature` from `pub use`; keep `Note`                                         | impl | F-4–F-6, NF-4        | T-7           |
| T-9  | Rewrite validation tests using new domain types: `test_validate_ok`, `test_validate_bpm_out_of_range`, `test_validate_invalid_channel`, `test_validate_invalid_instrument`, `test_validate_note_duration_zero` (using `NoteDurationZero { track, note }` without `bar`), `test_validate_zero_tracks`; delete `test_validate_invalid_denominator`, `test_validate_note_duration_exceeds_bar`, `test_validate_empty_track_bars`, `test_validate_invalid_numerator`, `test_validate_underfilled_bar` | test | NF-2, NF-3 | T-7, T-8 |
| T-10 | Update `validation.rs`: rewrite `ValidationError` enum (remove four variants; change `NoteDurationZero` to `{ track: usize, note: usize }`); rewrite `validate()` to iterate `track.notes` instead of `track.bars`; remove time-signature and bar-overflow checks | impl | NF-2, NF-3           | T-9           |
| T-11 | Update handler tests: replace `Header { bpm, time_signature }` with `Header { bpm, loop_duration }`; replace old `Note`/`Bar`/`NoteEvent` constructions with `Note { start_tick, duration, pitch, velocity }`; update `status_with_project_stopped` to assert `time_signature` is absent | test | NF-2, NF-3 | T-7, T-8 |
| T-12 | Update `handler.rs`: rewrite `build_domain_project()` with shim formula `loop_duration = ts.numerator * (PPQN * 4 / ts.denominator)`; rewrite `handle_set_bpm()`, `handle_status()` (drop `time_signature` field), `wire_track_to_domain()`; delete `wire_bar_to_domain()`, `wire_note_to_domain()` | impl | NF-2, NF-3           | T-11, T-10    |
| T-13 | Update `player.rs` tests to use new domain types: construct `Note` structs instead of `NoteEvent`/`Bar`; assert playback events are built correctly from `start_tick` and `duration` | test | NF-2, NF-3           | T-7           |
| T-14 | Update `player.rs`: rewrite `build_normal_bar()` to iterate `track.notes` with `start_tick`/`duration`; replace `bar_ticks()` with `header.loop_duration`; hardcode `bar_index = 0`; retain `bar_index` field on `PlayerLoop` and `PauseContext` | impl | NF-3                 | T-13          |

**Tests deleted in T-9 (no equivalent new behaviour in this epic):**

| Test deleted                              | Reason                                                    |
|-------------------------------------------|-----------------------------------------------------------|
| `test_validate_invalid_denominator`       | `TimeSignature` is removed; no equivalent validation      |
| `test_validate_invalid_numerator`         | `TimeSignature` is removed; no equivalent validation      |
| `test_validate_note_duration_exceeds_bar` | `NoteDurationExceedsBar` deleted; EP-NP-4 adds equivalent |
| `test_validate_empty_track_bars`          | Empty `notes` is valid per F-11; no equivalent constraint |
| `test_validate_underfilled_bar`           | Bar concept removed; no equivalent constraint             |

**Tests deleted in T-7 (no equivalent new behaviour in this epic):**

| Test deleted         | Reason                                              |
|----------------------|-----------------------------------------------------|
| `test_bar_ticks`     | `bar_ticks()` deleted; no equivalent computation   |
| `test_cycle_length`  | `cycle_length()` deleted; no equivalent method     |
| `test_bar_at`        | `bar_at()` deleted; no equivalent method           |

---

## Open Questions

No open questions remain.

---

## Open Decisions

None.

---

## Revision Log

### Cycle 1 — Confidence: 60%
- Reconciled: none (spec created from PRD and full codebase analysis)
- Added: Q-1 (loop_duration shim vs. wire change), Q-2 (player.rs update scope),
  Q-3 (status time_signature field), Q-4 (NoteDurationZero field set)

### Cycle 2 — Confidence: 85%
- Reconciled: Q-1 → Architecture updated with shim formula `ts.numerator * (PPQN * 4 / ts.denominator)`; T-12 dependencies simplified (Q-1, Q-3 removed)
- Reconciled: Q-2 → player.rs component section updated with minimal-update approach; T-13 added as test task; old T-13 renumbered to T-14; TDD order restored
- Reconciled: Q-3 → `handle_status()` drops `time_signature` from response; T-11 updated to include status test assertion
- Added: none (Q-4 already present)

### Cycle 3 — Confidence: 93%
- Reconciled: Q-4 (A selected) → `NoteDurationZero` confirmed as `{ track: usize, note: usize }` (no `bar` field); Data Model table updated (status note removed); Components/validation.rs updated (field shape made definitive); T-9 and T-10 updated to reference the final variant shape; Q-4 removed from Open Questions
- Added: nothing (confidence ≥ 90%)
