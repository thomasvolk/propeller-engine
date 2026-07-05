Goal is it to extend the propeller-engine protocol to give the client the possibility to 
retrieve the current position in ticks. With this feature the client can highlight the current position of the project.

- The client can ask for the current tick in a frequency that makes sense for an optical feedback
- The loop must run precisely in time - no delays allowed
- The client can use the socket protocol to get the tick position
