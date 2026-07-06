# EP-2 · Play Back Pitch Bend Events in Real Time — Technical Specification

## Overview
This epic extends the real-time playback engine so that a track's pitch-bend events (accepted
and validated by EP-1 as `PitchBend { tick, value }` on `Track.pitch_bends`) are sent as MIDI
pitch bend messages at the correct tick, interleaved with note-on/note-off/clock-pulse dispatch
with no added latency, replayed on every loop pass, and reset to center on stop or pause.

**Confidence Level:** 95% — Every F-x/AC-x maps to a concrete, TDD-ordered task, the design
slots cleanly into the existing `LoopEvent`/`MidiOutput`/tick-dispatch machinery in
`src/loop_engine/player.rs` and `src/loop_engine/midi.rs`, and the one architectural trade-off
(how to track channels with pitch-bend events for the stop/pause reset) is now resolved to a
concrete `HashSet<u8>` field. No open questions or decisions remain.

---

## Architecture Overview
No new components are introduced. The existing single-threaded player loop
(`src/loop_engine/player.rs`) and its `MidiOutput` abstraction (`src/loop_engine/midi.rs`,
implemented for real hardware by `src/midi_port.rs`) are extended in place, following the same
pattern already established for notes and clock pulses:

1. **MIDI output trait** — `MidiOutput` gains a `pitch_bend(channel, value)` method, alongside
   `note_on`/`note_off`/`program_change`/the clock methods. `MidiPortOutput` encodes it as the
   standard 3-byte MIDI Pitch Bend Change message (`0xE0 | (channel-1)`, LSB, MSB); the test
   doubles (`MockMidiOutput`, `CapturingMidiOutput`) record it as a new `MidiEvent::PitchBend`
   variant, exactly as they already do for `NoteOn`/`NoteOff`.
2. **Event model** — `LoopEvent` (private to `player.rs`) gains a `PitchBend { channel, value }`
   variant. Its `priority()` is extended so that, at a shared tick, dispatch order is
   `NoteOff (0) → PitchBend (1) → NoteOn (2) → ClockPulse (3)`, implementing F-5. This is a pure
   renumbering of the existing tie-break scheme already used to sort `(tick, LoopEvent)` pairs.
3. **Event construction** — `build_loop_events` gains a second per-track loop (alongside the
   existing note loop) that pushes `(pb.tick as u64, LoopEvent::PitchBend { channel: track.channel,
   value: pb.value as u16 })` for every entry in `track.pitch_bends`, then relies on the existing
   `events.sort_unstable_by_key(|(tick, ev)| (*tick, ev.priority()))` call to interleave it with
   notes and clock pulses (F-1, F-2, F-3). Because `build_loop_events` already reruns from
   scratch on every loop pass, replay across loop boundaries (F-4) requires no new mechanism.
   The same pass also clears and repopulates a new `pitch_bend_channels: HashSet<u8>` field on
   `PlayerLoop` with the channel of every track that has a non-empty `pitch_bends`, mirroring
   how `last_instruments` is already maintained during this same method (resolved by D-1).
4. **Event dispatch** — `emit_event` gains a match arm for `LoopEvent::PitchBend` that calls
   `self.output.pitch_bend(channel, value)`, sent through the same `play_events` per-tick
   scheduling loop used for every other event (NF-1, NF-2) — no new timing source or thread.
5. **Reset on stop/pause** — `do_stop`, `do_clock_stop`, `do_sync_stop`, and `do_pause` each gain
   a call that sends `pitch_bend(channel, 8192)` for every channel in `self.pitch_bend_channels`
   (F-6, F-7). `do_sync_restart` is deliberately left unchanged: it already replays pitch-bend
   events from the top of the next loop, so adding a reset there would risk a redundant,
   audible blip (per EP-2 PRD Q2/Q3 rationale).

---

## Components

### `MidiOutput` trait — `src/loop_engine/midi.rs`
- New method: `fn pitch_bend(&mut self, channel: u8, value: u16) -> Result<(), MidiSendError>;`.
  `value` is the raw 14-bit pitch-bend value (`0..=16383`), matching the already-validated
  `PitchBend.value` domain field one-to-one — no renormalizing to a signed or centered range.
- `MockMidiOutput` and `CapturingMidiOutput` (test-only) each get a `pitch_bend` implementation
  that pushes a new `MidiEvent::PitchBend { channel, value }` onto their event log, identical in
  shape to their existing `note_on`/`note_off` implementations.

### `MidiPortOutput` — `src/midi_port.rs`
- New `pitch_bend_bytes(channel: u8, value: u16) -> [u8; 3]` following the existing
  `note_on_bytes`/`program_change_bytes` style: `[0xE0 | (channel - 1), (value & 0x7F) as u8,
  ((value >> 7) & 0x7F) as u8]` — standard MIDI 1.0 Pitch Bend Change encoding (status byte,
  LSB, MSB, in that order).
- `MidiOutput::pitch_bend` on `MidiPortOutput` sends `pitch_bend_bytes(channel, value)` through
  the existing `midir` connection, mirroring `note_on`/`note_off`.

### `LoopEvent` and `PlayerLoop` — `src/loop_engine/player.rs`
- `LoopEvent` gains `PitchBend { channel: u8, value: u16 }`.
- `LoopEvent::priority()` is renumbered: `NoteOff => 0`, `PitchBend => 1`, `NoteOn => 2`,
  `ClockPulse => 3`.
- `build_loop_events` pushes one `LoopEvent::PitchBend` per `track.pitch_bends` entry, at the
  event's own tick, for every track — no note-off-style overflow/carry-over handling is needed,
  since a pitch-bend event is a single one-shot message, not a paired on/off duration.
- `emit_event` sends the pitch bend via `MidiOutput::pitch_bend` for the new variant. Unlike
  `NoteOn`/`NoteOff`, no `ActiveNote`-style bookkeeping is needed, since there is nothing to
  clean up per-event (the channel-level reset in F-6/F-7 is handled separately, at stop/pause,
  not per pitch-bend event).
- `PlayerLoop` gains a `pitch_bend_channels: HashSet<u8>` field, cleared and repopulated inside
  `build_loop_events` alongside the existing `last_instruments` update, containing the channel
  of every track with a non-empty `pitch_bends` for the current project.
- `do_stop`, `do_clock_stop`, `do_sync_stop`, and `do_pause` each send a center-reset
  (`pitch_bend(channel, 8192)`) for every channel in `self.pitch_bend_channels`.

---

## Data Model

| Type | Fields | Notes |
|------|--------|-------|
| `MidiOutput::pitch_bend` (new trait method) | `channel: u8`, `value: u16` | `value` is the raw validated `0..=16383` 14-bit pitch-bend value; no conversion beyond narrowing the domain `PitchBend.value: u32` (already range-checked by EP-1) down to `u16` |
| `MidiEvent::PitchBend` (new test-only variant, `midi.rs`) | `channel: u8`, `value: u16` | Recorded by `MockMidiOutput`/`CapturingMidiOutput`, mirrors `MidiEvent::NoteOn`/`NoteOff` |
| `LoopEvent::PitchBend` (new variant, `player.rs`, private) | `channel: u8`, `value: u16` | Priority `1`, between `NoteOff` (`0`) and `NoteOn` (`2`); `ClockPulse` moves to `3` |
| `pitch_bend_bytes` (new fn, `midi_port.rs`) | `channel: u8, value: u16 -> [u8; 3]` | `[0xE0 \| (channel-1), value & 0x7F, (value >> 7) & 0x7F]` |
| `PlayerLoop.pitch_bend_channels` (new field, `player.rs`) | `HashSet<u8>` | Cleared and repopulated in `build_loop_events` with the channel of every track that has a non-empty `pitch_bends`; read by `do_stop`/`do_clock_stop`/`do_sync_stop`/`do_pause` to send the center-reset |

---

## Implementation Tasks

Tasks are ordered TDD-first: every test task must appear before the impl task it covers.

| ID   | Task | Type | PRD ref | Depends on |
|------|------|------|---------|------------|
| T-1  | Unit test: `MockMidiOutput`/`CapturingMidiOutput` record a `MidiEvent::PitchBend { channel, value }` when `pitch_bend(channel, value)` is called | test | F-1 | — |
| T-2  | Add `MidiOutput::pitch_bend`, `MidiEvent::PitchBend`, and mock/capturing implementations | impl | F-1 | T-1 |
| T-3  | Unit test: `pitch_bend_bytes` encodes values 0, 8192, and 16383 into the correct 3-byte `0xEn`/LSB/MSB MIDI message on channels 1 and 16 | test | F-1, AC-1 | — |
| T-4  | Add `pitch_bend_bytes` and `MidiPortOutput::pitch_bend` | impl | F-1, AC-1 | T-2, T-3 |
| T-5  | Unit test: `LoopEvent::priority()` orders `NoteOff < PitchBend < NoteOn < ClockPulse` | test | F-5, AC-5 | — |
| T-6  | Add `LoopEvent::PitchBend` variant and renumber `priority()` | impl | F-5 | T-5 |
| T-7  | Unit test: `build_loop_events` emits one `LoopEvent::PitchBend` per `track.pitch_bends` entry at its own tick and the track's channel; a pitch-bend and a note sharing a tick both appear, sorted note-off/pitch-bend/note-on | test | F-1, F-3, F-5, AC-1, AC-2, AC-5 | T-6 |
| T-8  | Extend `build_loop_events` to push `PitchBend` events from `track.pitch_bends` | impl | F-1, F-3 | T-7 |
| T-9  | Unit test: `emit_event` calls `output.pitch_bend` with the event's channel and value for a `LoopEvent::PitchBend` | test | F-1, AC-1 | T-8 |
| T-10 | Extend `emit_event` with the `PitchBend` match arm | impl | F-1 | T-9 |
| T-11 | Test: `play_events` sends a pitch-bend, a note-off, and a note-on all scheduled at the same tick, and the deadline for the next distinct tick is unaffected by sending the extra message | test | F-2, NF-1, AC-2, AC-4 | T-10 |
| T-12 | Confirm the existing single-pass `play_events` dispatch requires no changes for `PitchBend` (adjust only if T-11 fails) | impl | F-2, NF-1 | T-11 |
| T-13 | Test: calling `build_loop_events` for two successive loop passes emits the same `PitchBend` events at the same ticks each pass | test | F-4, AC-3 | T-8 |
| T-14 | Confirm `build_loop_events`'s existing per-pass rebuild requires no changes for replay (adjust only if T-13 fails) | impl | F-4 | T-13 |
| T-15 | Test: `do_stop`, `do_clock_stop`, and `do_sync_stop` each send `pitch_bend(channel, 8192)` for every channel with pitch-bend events; `do_sync_restart` sends none | test | F-6, AC-6 | T-10 |
| T-16 | Add the center-reset call to `do_stop`, `do_clock_stop`, and `do_sync_stop`, reading `self.pitch_bend_channels` | impl | F-6 | T-15, T-20 |
| T-17 | Test: `do_pause` sends `pitch_bend(channel, 8192)` for every channel with pitch-bend events | test | F-7, AC-7 | T-16 |
| T-18 | Add the center-reset call to `do_pause`, reading `self.pitch_bend_channels` | impl | F-7 | T-17, T-20 |
| T-19 | Unit test: `build_loop_events` clears and repopulates `pitch_bend_channels` with exactly the channels of tracks that have a non-empty `pitch_bends`, fresh on each pass | test | F-6, F-7 | T-8 |
| T-20 | Add `PlayerLoop.pitch_bend_channels: HashSet<u8>` field, cleared and repopulated in `build_loop_events` | impl | F-6, F-7 | T-19 |

---

## Open Questions

None. The specification is complete.

---

## Open Decisions

None. D-1 resolved to option B (see Revision Log Cycle 2); no further architectural trade-offs
remain open.

---

## Revision Log

### Cycle 1 — Confidence: 80%
- Created initial technical specification from the EP-2 PRD (confidence 92%), cross-checked
  against the existing `LoopEvent`/`MidiOutput`/`play_events` machinery in
  `src/loop_engine/player.rs`, `src/loop_engine/midi.rs`, and `src/midi_port.rs`.
- Added: D-1 (mechanism for tracking channels with pitch-bend events, for the stop/pause reset).

### Cycle 2 — Confidence: 95%
- Reconciled: D-1 → option B selected; added `PlayerLoop.pitch_bend_channels: HashSet<u8>` to
  Data Model, updated Architecture Overview/Components, added T-19/T-20 (populate the field in
  `build_loop_events`) and wired T-16/T-18 to depend on T-20.
- No open questions or decisions remain. Specification is complete.
