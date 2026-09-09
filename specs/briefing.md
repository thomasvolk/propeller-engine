
The sync mode is still buggy and has to be improved.

- After pause and resume the engine loop runs very slow.
- Changing the clocks bpm does not have an effect immediately. The loop has to repeat first

Please create a integration test suite using propeller-clock to test all necessary scenarios.

- start / stop
- pause / resume
- change bpm

Also change the behavior of the loop to react immediately on clocks bpm changes.
