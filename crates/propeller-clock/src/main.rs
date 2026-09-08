// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

mod client;
mod daemon;
mod engine;
mod ipc;
mod midi;
mod scheduler;
mod socket_path;
mod tui;

use clap::{Parser, Subcommand};
use engine::ClockEngine;
use ipc::ClockSettings;
use propeller_common::startup_guard;
use std::sync::Arc;

const DEFAULT_VIRTUAL_PORT_NAME: &str = "propeller-clock";

#[derive(Parser)]
#[command(
    name = "propeller-clock",
    about = "Standalone MIDI clock daemon for sync tests"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the clock (launches the daemon in the background if it isn't already running)
    Start,
    /// Pause the clock, keeping the daemon process alive
    Pause,
    /// Resume the clock after a pause
    Resume,
    /// Stop the clock and terminate the daemon
    Stop,
    /// Show daemon status
    Status,
    /// Set the tempo in beats per minute (20-300)
    Bpm { bpm: u32 },
    /// Set the song position pointer (0-16383 MIDI beats) while stopped
    Seek { position: u16 },
    /// Open an interactive terminal UI (launches the daemon in the background if needed)
    Console,
    #[command(hide = true)]
    DaemonRun,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Start => cmd_start(),
        Commands::Pause => cmd_pause(),
        Commands::Resume => cmd_resume(),
        Commands::Stop => cmd_stop(),
        Commands::Status => cmd_status(),
        Commands::Bpm { bpm } => cmd_bpm(bpm),
        Commands::Seek { position } => cmd_seek(position),
        Commands::Console => cmd_console(),
        Commands::DaemonRun => cmd_daemon_run(),
    }
}

fn handle_client_error(e: client::ClientError) -> ! {
    match e {
        client::ClientError::Connect(err) => {
            eprintln!("propeller-clock: not running: {err}");
            std::process::exit(1);
        }
        client::ClientError::Daemon { message } => {
            eprintln!("propeller-clock: daemon error: {message}");
            std::process::exit(1);
        }
        client::ClientError::Input(msg) => {
            eprintln!("propeller-clock: {msg}");
            std::process::exit(1);
        }
    }
}

// Ensures the daemon process is running (spawning it in the background if needed),
// without affecting the clock's start/stop state.
fn ensure_daemon_running(sock_path: &std::path::Path) {
    // Validate PROPELLER_CLOCK_PORT before spawning so errors are visible in the terminal.
    if let Ok(name) = std::env::var("PROPELLER_CLOCK_PORT") {
        let names = midi::list_port_names();
        if midi::find_port_by_name(&names, &name).is_none() {
            eprintln!(
                "propeller-clock: MIDI port {:?} not found; available ports: [{}]",
                name,
                names.join(", ")
            );
            std::process::exit(1);
        }
    }

    match startup_guard::check(sock_path) {
        startup_guard::StartupOutcome::AlreadyRunning => {}
        startup_guard::StartupOutcome::StaleCleared => {
            eprintln!("propeller-clock: removed stale socket, starting fresh");
            spawn_daemon(sock_path);
        }
        startup_guard::StartupOutcome::Started => {
            spawn_daemon(sock_path);
        }
    }
}

fn cmd_start() {
    let sock_path = socket_path::resolve();
    ensure_daemon_running(&sock_path);

    if let Err(e) = client::send_command(&sock_path, serde_json::json!({"command": "start"})) {
        handle_client_error(e);
    }
}

fn cmd_console() {
    let sock_path = socket_path::resolve();
    ensure_daemon_running(&sock_path);

    if let Err(e) = tui::run(&sock_path) {
        eprintln!("propeller-clock: TUI error: {e}");
        std::process::exit(1);
    }
}

fn spawn_daemon(sock_path: &std::path::Path) {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("propeller-clock: cannot determine executable path: {e}");
            std::process::exit(1);
        }
    };

    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("daemon-run");
    use std::os::unix::process::CommandExt;
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0);

    if let Err(e) = cmd.spawn() {
        eprintln!("propeller-clock: failed to start daemon: {e}");
        std::process::exit(1);
    }

    // Block until the IPC server is accepting and processing commands.
    use std::io::{BufRead, BufReader, Write};
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(sock_path) {
            let probe = r#"{"command":"status"}"#.to_string() + "\n";
            if stream.write_all(probe.as_bytes()).is_ok() {
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                if reader.read_line(&mut line).is_ok() && !line.is_empty() {
                    return;
                }
            }
        }
        if std::time::Instant::now() >= deadline {
            eprintln!("propeller-clock: timed out waiting for daemon to become ready");
            std::process::exit(1);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn cmd_daemon_run() {
    let sock_path = socket_path::resolve();

    let port_name = std::env::var("PROPELLER_CLOCK_PORT").ok();
    let (output, effective_port_name): (Box<dyn midi::ClockOutput>, String) = match port_name {
        Some(ref name) => match midi::open_port(name) {
            Ok(out) => (Box::new(out), name.clone()),
            Err(e) => {
                eprintln!("propeller-clock: failed to open MIDI port: {e}");
                std::process::exit(1);
            }
        },
        None => match midi::open_virtual_named(DEFAULT_VIRTUAL_PORT_NAME) {
            Ok(out) => (Box::new(out), DEFAULT_VIRTUAL_PORT_NAME.to_string()),
            Err(e) => {
                eprintln!("propeller-clock: failed to open virtual MIDI port: {e}");
                std::process::exit(1);
            }
        },
    };

    let spp_enabled = std::env::var("PROPELLER_CLOCK_SPP")
        .map(|v| !matches!(v.to_ascii_lowercase().as_str(), "0" | "false" | "off"))
        .unwrap_or(true);

    let engine = Arc::new(ClockEngine::new(output, 120.0, spp_enabled));
    let settings = Arc::new(ClockSettings {
        port_name: effective_port_name,
        spp_enabled,
    });

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(daemon::run(sock_path, engine, settings));
}

fn cmd_pause() {
    let sock_path = socket_path::resolve();
    if let Err(e) = client::send_command(&sock_path, serde_json::json!({"command": "pause"})) {
        handle_client_error(e);
    }
}

fn cmd_resume() {
    let sock_path = socket_path::resolve();
    if let Err(e) = client::send_command(&sock_path, serde_json::json!({"command": "resume"})) {
        handle_client_error(e);
    }
}

fn cmd_stop() {
    let sock_path = socket_path::resolve();
    if let Err(e) = client::send_command(&sock_path, serde_json::json!({"command": "stop"})) {
        handle_client_error(e);
    }

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if !sock_path.exists() {
            return;
        }
        if std::time::Instant::now() >= deadline {
            eprintln!("propeller-clock: timed out waiting for daemon to stop");
            std::process::exit(1);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn cmd_bpm(bpm: u32) {
    let sock_path = socket_path::resolve();
    if let Err(e) = client::send_command(
        &sock_path,
        serde_json::json!({"command": "bpm", "bpm": bpm}),
    ) {
        handle_client_error(e);
    }
}

fn cmd_seek(position: u16) {
    let sock_path = socket_path::resolve();
    if let Err(e) = client::send_command(
        &sock_path,
        serde_json::json!({"command": "seek", "position": position}),
    ) {
        handle_client_error(e);
    }
}

fn cmd_status() {
    let sock_path = socket_path::resolve();
    match client::send_command(&sock_path, serde_json::json!({"command": "status"})) {
        Ok(v) => {
            println!("propeller-clock is running");
            if let Some(state) = v.get("state") {
                println!("  state: {state}");
            }
            if let Some(bpm) = v.get("bpm") {
                println!("  bpm: {bpm}");
            }
            if let Some(port) = v.get("port") {
                println!("  port: {port}");
            }
            if let Some(tick) = v.get("tick") {
                println!("  tick: {tick}");
            }
            if let Some(position) = v.get("position") {
                println!("  position: {position}");
            }
        }
        Err(client::ClientError::Connect(_)) => {
            println!("propeller-clock is not running");
            std::process::exit(1);
        }
        Err(e) => handle_client_error(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_parses() {
        let cli = Cli::try_parse_from(["propeller-clock", "start"]).unwrap();
        assert!(matches!(cli.command, Commands::Start));
    }

    #[test]
    fn bpm_parses_value() {
        let cli = Cli::try_parse_from(["propeller-clock", "bpm", "140"]).unwrap();
        match cli.command {
            Commands::Bpm { bpm } => assert_eq!(bpm, 140),
            _ => panic!("expected Bpm"),
        }
    }

    #[test]
    fn bpm_requires_value() {
        let result = Cli::try_parse_from(["propeller-clock", "bpm"]);
        assert!(result.is_err());
    }

    #[test]
    fn seek_parses_value() {
        let cli = Cli::try_parse_from(["propeller-clock", "seek", "240"]).unwrap();
        match cli.command {
            Commands::Seek { position } => assert_eq!(position, 240),
            _ => panic!("expected Seek"),
        }
    }

    #[test]
    fn seek_requires_value() {
        let result = Cli::try_parse_from(["propeller-clock", "seek"]);
        assert!(result.is_err());
    }

    #[test]
    fn daemon_run_is_hidden_but_parses() {
        let cli = Cli::try_parse_from(["propeller-clock", "daemon-run"]).unwrap();
        assert!(matches!(cli.command, Commands::DaemonRun));
    }

    #[test]
    fn console_parses() {
        let cli = Cli::try_parse_from(["propeller-clock", "console"]).unwrap();
        assert!(matches!(cli.command, Commands::Console));
    }
}
