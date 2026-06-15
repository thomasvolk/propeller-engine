# Roadmap — New Protocol

Refactoring from bar-based to tick-based protocol as specified in `specs/briefing.md`.

## Summary of changes

The current protocol organises notes inside bars and derives loop length from
`time_signature × bar_count`. The new protocol replaces that with:

- `loop_duration` in ticks (replaces `time_signature`)
- A flat, unsorted note list per track with explicit `start_tick` values (replaces `bars`)
- Overlapping notes on the same channel are allowed
- Notes whose duration extends past `loop_duration` carry over into the next loop
  iteration (duration ≤ 2 × loop_duration)

## Dependency graph

```
EP-NP-1  (domain model)
    │
    ├── EP-NP-2  (wire protocol)  ──┐
    ├── EP-NP-3  (loop engine)      ├── EP-NP-4  (handler + validation)
    └── EP-NP-5  (docs + spec) ─────┘  (EP-NP-5 can also finish in parallel)
```

Waves 2 and 3 contain independent work streams that touch distinct files and can
be executed by separate engineers simultaneously.

---

## Wave 1 — Foundation

### EP-NP-1: Domain Model Refactoring

**Files:** `src/domain/project.rs`

Remove the bar-based model and replace it with a flat, tick-addressed note list.

#### Removals

| Symbol removed         | Reason                                            |
| ---------------------- | ------------------------------------------------- |
| `TimeSignature`        | Superseded by `loop_duration`                     |
| `Bar`                  | Notes are now a flat list on `Track`              |
| `NoteEvent`            | Rests are gone; every note has pitch and velocity |
| `bar_ticks()`          | Derived from `TimeSignature`; no longer needed    |
| `cycle_length()`       | Concept of cycling over bars is removed           |
| `bar_at()`             | Track no longer has bars                          |

#### New / changed types

| Type / field                   | Change                                                   |
| ------------------------------ | -------------------------------------------------------- |
| `Header.time_signature`        | Replaced by `Header.loop_duration: u32`                  |
| `Track.bars: Vec<Bar>`         | Replaced by `Track.notes: Vec<Note>`                     |
| `Note`                         | New struct: `start_tick: u32, duration: u32, pitch: u8, velocity: u8` |

#### Acceptance criteria

- All existing tests updated or replaced; `cargo test` passes
- `PPQN` constant (`480`) remains; no other scheduling constants change
- `Header.loop_duration` is the sole loop-length source for the rest of the system

---

## Wave 2 — Parallel streams

These three epics are independent of each other. They touch disjoint files and
can land in any order or simultaneously, provided EP-NP-1 is merged first.

### EP-NP-2: Wire Protocol & IPC Types

**Files:** `src/ipc/types.rs`

Update the JSON deserialization layer to match the new wire format.

#### Removals

| Symbol removed    | Reason                                    |
| ----------------- | ----------------------------------------- |
| `WireTimeSignature` | Replaced by `loop_duration` scalar      |
| `WireBar`         | No bar concept in the new protocol        |
| `WireNote`        | Notes are now fixed-length integer tuples |

#### New / changed types

| Type / field                       | Change                                             |
| ---------------------------------- | -------------------------------------------------- |
| `WireHeader.time_signature`        | Replaced by `WireHeader.loop_duration: u32`        |
| `WireTrack.bars: Vec<WireBar>`     | Replaced by `WireTrack.notes: Vec<[u32; 4]>`       |

The note tuple layout is `[start_tick, duration, pitch, velocity]`, matching the
briefing exactly.

#### Acceptance criteria

- `create-project` and `modify-project` deserialise the new wire format
- Old format (with `bars` / `time_signature`) is rejected
- All deserialization unit tests updated; `cargo test` passes

---

### EP-NP-3: Loop Engine Refactoring

**Files:** `src/loop_engine/player.rs`, `src/loop_engine/mod.rs`

Replace bar-by-bar iteration with full-loop event scheduling.

#### Removals / replacements on `PlayerLoop`

| Field / method removed      | Replacement                                      |
| --------------------------- | ------------------------------------------------ |
| `bar_index: usize`          | Removed; the loop has no concept of sub-bars     |
| `last_bar_ticks: u64`       | `loop_duration: u64` read from active project    |
| `build_normal_bar()`        | `build_loop_events()` builds the entire loop     |
| `advance_bar()`             | `advance_loop()` advances anchor by `loop_duration` ticks |
| `init_running_from_project()` | Reads `header.loop_duration` instead of `bar_ticks()` |

#### New behaviour

**Flat event building.** `build_loop_events()` iterates `track.notes` directly,
emitting `NoteOn` at `note.start_tick` and `NoteOff` at
`note.start_tick + note.duration`. Notes with the same start tick on the same
channel are allowed; sorting by `(tick, priority)` is unchanged.

**Cross-loop note carry-over.** If `note.start_tick + note.duration > loop_duration`,
the note-off falls in the next loop iteration. At the start of each loop the
player emits any carried-over note-offs before new loop events (tick offset = 0
relative to loop start). Carry-over is bounded: duration ≤ 2 × loop_duration.

**Clock mode.** Clock pulses are inserted every 20 ticks across the full
`loop_duration`, unchanged in principle.

**Pause / resume.** `PauseContext.remaining_events` now holds tick offsets
relative to the loop start rather than a bar start; semantics are otherwise
unchanged.

**BPM change and `modify-project` apply at loop boundary** (the loop is the new
bar).

#### Acceptance criteria

- Player iterates over loop_duration ticks, not bar ticks
- Two notes starting at the same tick on the same channel play independently
- A note with `start_tick=0, duration=3840` in a 1920-tick loop produces
  NoteOn at tick 0 of loop 1 and NoteOff at tick 0 of loop 2 (carried over)
- Clock pulses still appear every 20 ticks across the full loop
- Pause and resume retain the remaining events within the current loop
- `cargo test` passes

---

### EP-NP-5: Documentation & Spec

**Files:** `docs/json-socket-interface.md`, `specs/propeller.allium`

Update all user-facing and specification documents to reflect the new protocol.
This epic is purely textual and can be merged at any point after EP-NP-1 is
understood.

#### `docs/json-socket-interface.md`

- Replace `header` example: remove `time_signature`, add `loop_duration`
- Update `create-project` and `modify-project` command examples
- Replace bar/note field reference table with new note tuple table
  `[start_tick, duration, pitch, velocity]`
- Remove rest concept (`"rest": true` no longer exists)
- Update `modify-project` semantics: changes take effect at the next **loop**
  boundary (not bar boundary)
- Remove `time_signature` from status response description;
  add `loop_duration` if the status response exposes it
- Add section explaining overlapping notes and cross-loop notes
- Update error code table: remove `NoteDurationExceedsBar`; add any new
  validation errors introduced by EP-NP-4

#### `specs/propeller.allium`

- Align all entities and rules with the new domain model
- Remove `TimeSignature`, `Bar`, `NoteEvent`, `Rest` entities
- Add `loop_duration` to `Header`
- Replace `bars` with `notes` on `Track`
- Add rules for cross-loop note carry-over

#### Acceptance criteria

- No reference to `bars`, `time_signature`, or rests remains in either document
- All code examples use the new wire format and produce valid JSON
- The allium spec passes its own linter / `tend` validation

---

## Wave 3 — Integration

### EP-NP-4: IPC Handler & Validation

**Files:** `src/ipc/handler.rs`, `src/domain/validation.rs`

**Depends on:** EP-NP-1 (domain types) and EP-NP-2 (wire types).
EP-NP-3 and EP-NP-5 may be in-flight simultaneously.

#### `src/domain/validation.rs`

Remove bar/time-signature rules; add tick-based rules.

| Error variant removed                | Replacement                                                    |
| ------------------------------------ | -------------------------------------------------------------- |
| `InvalidTimeSignatureNumerator`      | Removed                                                        |
| `InvalidTimeSignatureDenominator`    | Removed                                                        |
| `EmptyTrackBars`                     | Removed (empty note list is valid)                             |
| `NoteDurationExceedsBar`             | `NoteDurationExceedsLimit { track, note, duration, limit }` — duration > 2 × loop_duration |

| Error variant added                  | Condition                                                      |
| ------------------------------------ | -------------------------------------------------------------- |
| `LoopDurationZero`                   | `header.loop_duration == 0`                                    |
| `NoteStartTickOutOfRange`            | `note.start_tick >= loop_duration`                             |
| `NoteDurationZero`                   | `note.duration == 0` (replaces the bar-scoped version)         |
| `NoteDurationExceedsLimit`           | `note.start_tick + note.duration > 2 * loop_duration`          |

#### `src/ipc/handler.rs`

| Function changed              | Change                                                                     |
| ----------------------------- | -------------------------------------------------------------------------- |
| `build_domain_project()`      | Convert `[u32; 4]` note tuples; no `time_signature` field                 |
| `wire_track_to_domain()`      | Map flat notes; remove call to `wire_bar_to_domain()`                      |
| `handle_set_bpm()`            | Reconstruct project with updated BPM; carry `loop_duration` instead of `time_signature` |
| `handle_status()`             | Remove `time_signature` field; optionally expose `loop_duration`           |
| `validation_error_response()` | Add arms for new error variants; remove old bar/time-signature arms         |

Functions `wire_bar_to_domain()` and `wire_note_to_domain()` are deleted.

#### Acceptance criteria

- `create-project` with `loop_duration: 0` → `validation_error`
- `create-project` with note `start_tick >= loop_duration` → `validation_error`
- `create-project` with `note.duration = 0` → `validation_error`
- `create-project` with `start_tick + duration > 2 * loop_duration` → `validation_error`
- `create-project` with valid overlapping notes → `ok`, project stored
- `handle_set_bpm` carries `loop_duration` correctly (no `time_signature` field)
- Status response contains `loop_duration` when a project is loaded; `time_signature`
  field is absent
- All handler tests updated and passing; `cargo test` passes
