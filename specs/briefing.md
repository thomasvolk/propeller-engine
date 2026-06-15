# Goal

This is about simplifying the protocol and the play loop.

- The separation into bars is no necessary anymore
- The time_signature is not necessary anymore
- The length of the loop will be set in ticks
- A midi event will get a start tick value instead of an implicit calculated the position
- Notes of one channel can overlap

# The new protocol

## Header

The header only includes bpm and the loop duration

    "header": { "bpm": 120, "loop_duration": 1920 },

The microseconds per tick are continue to be calculated like this:
micros_per_tick = 60_000_000 / (bpm × 480)

## Tracks

The track list remains unchanged, but the notes are now a list of unsorted events with an explicit start tick value. This allows for overlapping notes on the same channel and more flexible timing.

```json
  "tracks": [
    {
      "name": "piano",
      "channel": 1,
      "instrument": 0,
      "notes": [
        [0, 480, 60, 80]
      ]
    }
  ]
```

### Notes

The notes within a track are organized in a unsorted list.
Every note has four values:

| start tick | duration | pitch | velocity | 
| ---------- | -------- | ----- | -------- |
| 0          | 480      | 60    | 80       |
| 480        | 600      | 62    | 80       |
| 960        | 480      | 64    | 80       |
| 1440       | 480      | 65    | 80       |

This are the first four notes of the C major scale. The first note starts at tick 0 and has a duration of 480 ticks The second note starts at tick 480 and has a duration of 600 ticks. The third note starts at tick 960 and has a duration of 480 ticks, it overlaps 120 ticks with the second note. The fourth note starts at tick 1440 and has a duration of 480 ticks.

### Example

The myproject.json will look like this:

```json
{
  "header": { "bpm": 120, "loop_duration": 1920 },
  "tracks": [
    {
      "name": "piano",
      "channel": 1,
      "instrument": 0,
      "notes": [
        [0, 480, 60, 80]
      ]
    }
  ]
}

```

## Overlapping notes

With the new protocol, it is possible to have overlapping notes on the same channel. This allows for more complex arrangements and chords. For example, if we want to play a C major chord (C, E, G) at the same time, we can do it like this:

```json
  "tracks": [
    {
      "name": "piano",
      "channel": 1,
      "instrument": 0,
      "notes": [
        [0, 480, 60, 80], // C4
        [0, 480, 64, 80], // E4
        [0, 480, 67, 80]  // G4
      ]
    }
  ]
```

## Note duration is longer than loop duration

A note duration can be twice as long as the loop duration. In this case, the note will be played for the first loop iteration and then again for the second loop iteration. For example, if we have a note that starts at tick 0 and has a duration of 3840 ticks, it will be played for the first loop iteration (0-1920) and then again for the second loop iteration (1920-3840). 

```json
  "tracks": [
    {
      "name": "piano",
      "channel": 1,
      "instrument": 0,
      "notes": [
        [0, 3840, 60, 80] // C4
      ]
    }
  ]
```

