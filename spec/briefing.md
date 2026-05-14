
The propeller-engine is the engine of a live-coding music environment

- It runs as a long-lived daemon
- It can send a midi clock signal (clock mode)
- If it is in the clock mode the clock can be started, paused or stopped
- It can receive a midi clock signal to run a synchronized loop (sync mode)
- It can run run without sending or receiving a clock
- The speed of the loop can be modified (bpm), except in the sync mode
- It has a protocol to accept midi notes to play in a loop at runtime
- Speed and time signature can also be set at runtime
- The clock mode an status can be set at runtime

# A project

A project has a list of tracks and a header. Beside the clock commands,
the project defines what midi signals will be send.

In the clock mode: Before the clock can be started, there must be a project defined.
In the sync mode: Before the engine only starts playing the loop if there is a project sand the clock singnal is received.

## header

The header defines the speed (bpm) and the time signature

## tracks

A track has a name, a midi-channel, a midi-instrument and a list of bars.

## bar
A bar contains a list of notes. Every bar in a project has the same length depending on the time signature.
The bar is always played completely before the engine will update the project.

## Note

A note has a pitch (MIDI standard), a velocity (MIDI) and a duration depending on the time signature.
A note can also be a rest. The duration can be smaller than the note value defined by the time signature but not bigger then the duration of a bar.

## time signature
Most time signatures consist of two numerals, one stacked above the other:

The lower numeral indicates the note value that the signature is counting. This number is always a power of 2 (unless the time signature is irrational), usually 2, 4 or 8, but less often 16 is also used, usually in Baroque music. 2 corresponds to the half note (minim), 4 to the quarter note (crotchet), 8 to the eighth note (quaver), 16 to the sixteenth note (semiquaver).
The upper numeral indicates how many such note values constitute a bar.
