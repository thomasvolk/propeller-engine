# EP-1 · Accept Pitch Bend Data in Track Definitions — PRD

## Overview
When a client submits a project whose track includes a `pitch-bends` list of `[tick, value]`
pairs, the system accepts it as part of that track's data, alongside its notes. Each pitch
bend event is checked before the project becomes active: an out-of-range value or an
out-of-range tick causes the whole project to be rejected with a clear reason, the same way
an invalid note does today. Tracks without a `pitch-bends` field continue to work exactly as
before.

**Confidence Level:** 95% — Every requirement in the roadmap is covered by a specific,
testable functional requirement or acceptance criterion, all user-facing paths (accept,
reject on value, reject on tick, backward-compatible omission) are represented, and every
ambiguity raised during refinement has been resolved. No open questions remain.

---

## User Journeys

### UJ-1 · Client submits a track with valid pitch-bend events
A client submits a project where one or more tracks include a `pitch-bends` array of
`[tick, value]` pairs, all within range. The project is accepted and the pitch-bend events
are stored alongside that track's notes, ready for playback (playback itself is EP-2).

### UJ-2 · Client submits a track with an out-of-range pitch-bend value
A client submits a project where a pitch-bend event's value falls outside 0–16383. The whole
project is rejected, and the rejection reason names the specific track and event that failed,
consistent with how an invalid note is reported today.

### UJ-3 · Client submits a track with an out-of-range pitch-bend tick
A client submits a project where a pitch-bend event's tick is at or beyond the project's loop
duration. The whole project is rejected, naming the specific track and event, consistent with
how an out-of-range note start tick is reported today.

### UJ-4 · Client submits a track without a `pitch-bends` field
A client submits a track with only `notes` and no `pitch-bends` field (today's existing
shape). The project is accepted and behaves exactly as it does today — no pitch-bend data is
stored or assumed for that track.

---

## Functional Requirements
| ID | Requirement |
|-----|-------------|
| F-1 | A track definition accepts an optional `pitch-bends` list of `[tick, value]` pairs, alongside its existing `notes` list. |
| F-2 | Each pitch-bend event's `value` must be within the inclusive range 0–16383; a value outside this range causes the whole project to be rejected. |
| F-3 | Each pitch-bend event's `tick` must be less than the project's `loop_duration`; a tick at or beyond `loop_duration` causes the whole project to be rejected, mirroring the existing note start-tick check. |
| F-4 | Accepted pitch-bend events are stored on the track alongside its notes, as part of the project's active state. |
| F-5 | A track that omits the `pitch-bends` field is accepted and behaves exactly as it does today; no pitch-bend events are stored or assumed for that track. |
| F-6 | A rejection caused by an invalid pitch-bend event identifies the specific track and event index that failed, consistent with how existing note validation errors identify their track and note index. |
| F-7 | Pitch-bend events within a track may be submitted in any order and may repeat the same tick; only value and tick range violations cause rejection — there is no ordering or uniqueness requirement. |
| F-8 | Within each track, notes are validated before pitch-bends, following the existing per-track validation loop order; if a track has both an invalid note and an invalid pitch-bend event, the note failure is reported. |
| F-9 | The `pitch-bends` JSON field is optional per track, unlike `notes` which is required; when `pitch-bends` is omitted, it is treated as an empty list. |
| F-10 | There is no limit on the number of pitch-bend events a track may carry; only the per-event value (F-2) and tick (F-3) range checks apply. |

---

## Non-Functional Requirements
| ID | Requirement |
|-----|-------------|
| NF-1 | Pitch-bend validation runs as part of the same project-acceptance pass as note validation — a project is never partially accepted (e.g. tracks stored before validation completes). |
| NF-2 | Pitch-bend validation failures are reported through the same structured error mechanism used for note/track validation failures today (no new, differently-shaped error surface for clients to handle). |
| NF-3 | Existing projects and tracks that do not use `pitch-bends` see no behavioral change — this is a strictly additive change. |

---

## Acceptance Criteria
| ID | Given | When | Then |
|------|-------|------|------|
| AC-1 | A track with a `pitch-bends` array of `[tick, value]` pairs | the project is submitted | the system accepts it and stores each event alongside the track's notes |
| AC-2 | A pitch-bend event with value 8192 | the project is submitted | the system accepts it as valid ("no bend") |
| AC-3 | A pitch-bend event with value 0 | the project is submitted | the system accepts it as the lowest valid value |
| AC-4 | A pitch-bend event with value 16383 | the project is submitted | the system accepts it as the highest valid value |
| AC-5 | A pitch-bend event with a value outside 0–16383 | the project is submitted | the system rejects the project and reports which track and event caused the failure |
| AC-6 | A pitch-bend event whose tick is at or beyond the project's loop duration | the project is submitted | the system rejects the project, consistent with how an out-of-range note start tick is rejected today |
| AC-7 | A track with no `pitch-bends` field | the project is submitted | the system accepts the project unchanged |

---

## Open Questions

None. The PRD is complete.

---

## Refinement Log

### Cycle 1 — Confidence: 65%
- Created initial PRD from the EP-1 roadmap entry and the pitch-bend briefing's wire example.
- Added: Q1 (event ordering/duplicate ticks), Q2 (validation order vs. notes), Q3 (wire
  optionality of `pitch-bends`), Q4 (per-track event count limit).

### Cycle 2 — Confidence: 85%
- Reconciled: Q1 → F-7 (any order, repeated ticks allowed), Q2 → F-8 (notes validated before
  pitch-bends per track), Q3 → F-9 (`pitch-bends` optional, defaults to empty list when absent)
- Remaining: Q4 (per-track event count cap) still open; no new questions added.

### Cycle 3 — Confidence: 95%
- Reconciled: Q4 → F-10 (no cap on pitch-bend events per track)
- No open questions remain. PRD is complete.
