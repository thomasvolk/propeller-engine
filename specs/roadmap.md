# Roadmap: Pitch Bend Support

Extend propeller-engine so a project's tracks can carry pitch bend events alongside notes,
and so those events are played back to the synth at the correct time. The target end-state is
a client can submit a track with a `pitch-bends` list and hear pitch bend applied in time with
the rest of the performance.

---

## Dependency graph

| Epic | Depends on | Can start in parallel with |
| ---- | ---------- | --------------------------- |
| EP-1 | —          | —                            |
| EP-2 | EP-1       | —                            |

---

## EP-1 — Accept Pitch Bend Data in Track Definitions

When a client submits a project whose track includes a `pitch-bends` list of `[tick, value]`
pairs, the system accepts it as part of that track's data, alongside its notes. Each pitch
bend event is checked before the project becomes active: an out-of-range value or an
out-of-range tick causes the whole project to be rejected with a clear reason, the same way
an invalid note does today. Tracks without a `pitch-bends` field continue to work exactly as
before.

**Acceptance criteria**

- Given a track with a `pitch-bends` array of `[tick, value]` pairs, when the project is
  submitted, the system accepts it and stores each event alongside the track's notes.
- Given a pitch bend value of 8192, the system accepts it as valid ("no bend").
- Given a pitch bend value of 0, the system accepts it as the lowest valid value.
- Given a pitch bend value of 16383, the system accepts it as the highest valid value.
- Given a pitch bend value outside the range 0–16383, the system rejects the project and
  reports which track and event caused the failure.
- Given a pitch bend event whose tick is at or beyond the project's loop duration, the system
  rejects the project, consistent with how an out-of-range note start tick is rejected today.
- Given a track with no `pitch-bends` field, the system accepts the project unchanged.

---

## EP-2 — Play Back Pitch Bend Events in Real Time

While a project with pitch bend events is playing, the system sends a MIDI pitch bend message
on the track's channel at the exact tick each event occurs, with the same timing precision as
note-on and note-off events. Pitch bend events on multiple tracks, and pitch bend events that
land on the same tick as notes, all play back together without any of them being dropped,
delayed, or reordered relative to each other.

**Acceptance criteria**

- Given an active project reaches the tick of a pitch bend event, the system sends a pitch
  bend message on that track's MIDI channel carrying the event's value.
- Given a pitch bend event and one or more note events fall on the same tick, the system sends
  all of them at that tick without dropping or delaying any.
- Given a project loops back to its start, pitch bend events are sent again on each pass,
  consistent with how notes are replayed across loop boundaries.
- Given pitch bend events are being sent during playback, doing so introduces no observable
  delay to the timing of subsequent clock ticks or note events.
