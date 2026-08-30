# Changelog

## [0.8.2] - 2026-08-30

### Changes

- Fixed MIDI output being silently dropped by Ableton Live 12 (and other CoreMIDI hosts that
  rely on packet timestamps to schedule events): outgoing MIDI messages were sent with a zero
  timestamp, so Live would show input activity (Track In blinking, MIDI Map learning) but never
  actually schedule the notes onto a track. Enabled midir's `coremidi_send_timestamped` feature
  so every sent message now carries a real host timestamp.
- Added a regression test guarding against this feature being disabled again in the future.

---

## [0.8.1] - 2026-08-27

### Changes

- Added `midi_port_name` to the `status` response, reporting the name of the configured MIDI
  output port; the field is omitted when the daemon falls back to its default virtual port.
- Added `sync_port_name` to the `status` response, reporting the name of the MIDI input port
  used for clock sync; present whenever the engine is in sync mode with an active clock
  receiver, mirroring the existing `sync_clock_state` field's scope.
- Updated the README and JSON socket interface documentation with the new status fields, and
  fixed a stray `time_signature` field in a README example that didn't correspond to any real
  response data.

---

## [0.8.0] - 2026-08-20

### Changes

- **Breaking:** Removed the redundant `"type": "position"` discriminant from the `get-position`
  response; the response is now just `{"tick": ..., "loop_duration": ...}`. The CLI client no
  longer validates a response `type` tag before parsing `tick`/`loop_duration`.
- Added a loop counter to the engine, exposed as a new `loop_count` field on the `get-position`
  response. It starts at 0 and increments by one each time a loop completes; it is unaffected by
  pause/resume, and resets to 0 on `stop`/`clock-stop` or an incoming MIDI Start (0xFA) in sync
  mode, mirroring `tick`'s own reset points.

---

## [0.7.0] - 2026-08-20

### Changes

- **Breaking:** Unified the IPC wire format so every message, including the tick-position
  query, uses a `"command"` discriminant instead of mixing `"command"` and `"type"` tags;
  the `get_position` message is now sent as `get-position`.
- Removed the `IpcMessage` enum; `get-position` is now a variant of the single `Command` enum
  alongside every other request, and the daemon dispatch logic collapses to one parse path.
- Removed the now-unreachable `unknown_type` error code — a request still using the old
  `"type"` tag is now reported as `missing_command`.
- Updated the architecture guideline to mandate `"command"` (not `"type"`) as the required
  discriminant for all IPC messages.
- Updated the README and JSON socket interface documentation to describe the `get-position`
  command and remove references to the retired `"type"` tag.

---

## [0.6.0] - 2026-07-16

### Changes

- Added a `"project"` socket command that reports the current (active) and pending
  (staged-but-uncommitted) project as complete data, read-only and identical regardless of
  operating mode.
- Added a `propeller project get` CLI command that queries the daemon for project state and
  prints it as compact JSON, following the same connection and error-reporting conventions as
  the other CLI commands.
- Extended `ProjectStore` with a read-only `pending()` accessor mirroring the existing `active()`.
- Added Allium specs (EP-1, EP-2) describing the project-state query feature and its roadmap.
- Documented the new command in the README's runtime interface and JSON socket interface
  reference, including a worked example and field reference table.

---

## [0.5.0] - 2026-07-12

### Changes

- Fixed MIDI sync playback so a MIDI Stop (0xFC) now pauses and retains the current song
  position instead of hard-stopping, per the MIDI 1.0 spec.
- Fixed a following MIDI Continue (0xFB) to resume playback from the exact tick where it was
  paused, rather than restarting the loop from the beginning.
- MIDI Start (0xFA) continues to reset the song position to 0, distinguishing it from Stop/
  Continue behavior.
- Updated the internal loop-engine state machine so `SyncStop` transitions to `Paused` (reusing
  the existing pause/resume mechanism) instead of `Stopped`.
- Documented the known-issues, JSON socket interface, and internals docs to describe the
  corrected sync-mode pause/resume semantics and cross-link related documentation pages.
- Updated the README with contributing guidelines (`cargo fmt`/`cargo test`, no compiler
  warnings) and a link to the known issues document.

---

## [0.4.0] - 2026-07-06

### Changes

- Added pitch bend support: tracks can now declare a `pitch-bends` event list alongside notes,
  scheduled and sent as MIDI Pitch Bend messages during playback.
- Any channel with pitch bend events is automatically reset to center (8192) whenever the loop
  or clock stops or pauses, preventing a connected synth from being left permanently detuned.
- Added an example project (`examples/pitch_bend.json`) demonstrating the new feature.
- Extended the IPC wire format with a `pitch-bends` field on tracks, defaulting to empty when
  absent for backward compatibility with existing projects.
- Added Allium briefing, roadmap, and specs (EP-1, EP-2) describing the pitch bend feature.
- Documented the center-reset behavior as a known issue and updated the README, internals, and
  JSON socket interface docs to cover pitch bend.

---

## [0.3.0] - 2026-07-06

### Changes

- Added a `loop position` CLI command and matching daemon support to query the current tick
  position and loop duration of a running project, with a `--poll` mode for continuous
  monitoring at a configurable interval.
- Extended the IPC protocol with a `get_position`/`position` message pair, returning `null`
  loop duration when no project is active.
- Added Allium epics and specs (EP-1, EP-2, EP-3) describing the tick-position feature and its
  roadmap.
- Fixed spec paths referenced by the `implement-epic`, `refine-epic`, `refine-spec`, and
  `verify-epic` skill commands.
- Added a `roadmap` skill command and roadmap documentation for the tick-position feature.
- Updated the README and JSON socket interface documentation to cover the new position query.

---

## [0.2.0] - 2026-06-16

### Changes

- Redesigned the IPC protocol (epic 1-5): new command and response structure replaces the
  previous wire format, affecting all CLI-to-daemon communication.
- Updated all documentation and JSON socket interface examples to reflect the new protocol.
- Added chord support with a bundled chord example project.
- Removed and renamed internal identifiers that referred to "bar", aligning terminology
  across the codebase.
- Added release automation via a `/release` skill that bumps the version and writes the
  changelog in one step.
- Added Allium specification and project roadmap to support ongoing architecture planning.
