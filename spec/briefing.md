
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

