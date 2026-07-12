# Changelog

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
