# Goal

The MIDI protocol has a support for pitch bend. The goal is to extend the propeller-engine to support pitch bend messages.

## MIDI Pitch Bend message

A Pitch Bend message on the same channel to move the pitch up or down by any amount within the synth’s pitch-bend range.
Pitch bend is a 14-bit value (0–16383), with 8192 = “no bend”.
If your synth’s bend range is 2 semitones total (±1 semitone), then:

* 0 = −1 semitone
* 8192 = 0
* 16383 = +1 semitone

## Requirements

Pitch Bend messages will be handled similar to note events. 
Therefore a new parameter pitch-bend will added to the track object:
```
"tracks": [
    {
      "name": "piano",
      "channel": 1,
      "instrument": 0,
      "notes": [
        [0, 480, 60, 80]
      ],
      "pitch-bends": [
        [120, 12288],
        [240, 16384],
        [480, 8192]
      ]
    }
  ]
```

A pitch bend event hat two values. The first value is the start tick, and the second one the value.

