# propeller-engine

A live-coding music environment engine that runs as a long-lived background daemon.

Live-coding performances need a process that is always on, accepts commands in real time,
drives MIDI loops with precise timing, and never misses a beat when the project is updated
mid-performance. propeller-engine is that process.

## Quick example

```sh
# Start the daemon — returns immediately; engine runs in the background
propeller start

# Confirm it is running (socket is connectable)
nc -U /tmp/propeller.sock </dev/null && echo "propeller is running"

# Stop the daemon cleanly
propeller stop
```

## Installation

Prerequisites: a [Rust toolchain](https://rustup.rs) (stable, edition 2024).

1. Clone the repository and enter the project directory.
2. Build the release binary:

   ```sh
   cargo build --release
   ```

3. Add the binary to your PATH, or run it directly:

   ```sh
   export PATH="$PWD/target/release:$PATH"
   ```

## Usage

### Starting the daemon

```sh
propeller start
```

The process double-forks and detaches from your shell immediately. The daemon is ready
to accept connections as soon as the command returns. Starting a second instance while
one is already running is rejected.

### Stopping the daemon

```sh
propeller stop
```

Sends a stop command over the IPC socket. The daemon finishes any in-progress work,
releases all resources, and removes the socket file before exiting.

### Checking liveness

The daemon's presence is indicated by a connectable Unix socket at `/tmp/propeller.sock`:

```sh
nc -U /tmp/propeller.sock </dev/null && echo "running" || echo "not running"
```

### Configuring the socket path

Set `PROPELLER_SOCK` to override the default socket location:

```sh
PROPELLER_SOCK=/run/user/1000/propeller.sock propeller start
```

Both `start` and `stop` read this variable, so set it consistently.

### Log files

Diagnostic output is written to:

- **macOS:** `~/Library/Logs/propeller/propeller.log`
- **Linux:** `~/.local/share/propeller/propeller.log`

## Features

- **Daemon lifecycle** — starts, stays running indefinitely, stops cleanly on command or SIGTERM.
- **Unix socket IPC** — connects over `/tmp/propeller.sock`; socket path is configurable via environment variable.
- **Single-instance guard** — rejects a second `start` if the daemon is already running.
- **Stale socket recovery** — detects and removes leftover socket files from a previous crash, then starts fresh.
- **Graceful shutdown** — handles both the `stop` command and SIGTERM; unlinks the socket on exit.
- **Structured logging** — writes to stderr and to the platform log file using `tracing`.

## Contributing

Specifications live in the `spec/` directory. Read `spec/briefing.md` for the project
vision and `spec/roadmap.md` for the planned epic sequence before starting work.
Architecture and coding conventions are in `spec/architecture-guidelines.md` and
`spec/coding-guidelines.md`.

Open issues and submit pull requests in the project repository.

## Support

Open an issue in the repository issue tracker.

## License

No license has been declared yet. Until one is added, default copyright applies and
no use, modification, or distribution is permitted without the author's explicit consent.
Add a `LICENSE` file and a `license` field to `Cargo.toml` to clarify terms for contributors
and users.
