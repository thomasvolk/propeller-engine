// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

mod client;
mod daemon;
mod domain;
mod ipc;
mod logger;
mod loop_engine;
mod midi_clock;
mod midi_port;
mod socket_path;
mod startup_guard;

use clap::{Parser, Subcommand};
use ipc::EngineMode;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Parser)]
#[command(name = "propeller", about = "Propeller engine daemon")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the daemon
    Start {
        /// Start in clock mode (overrides the default standalone mode)
        #[arg(long)]
        clock: bool,
        /// Start in sync mode (reads PROPELLER_SYNC_PORT env var for the MIDI input port name)
        #[arg(long)]
        sync: bool,
    },
    /// Stop the running daemon
    Stop,
    /// Check whether the daemon is running
    Status,
    /// Manage projects
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    /// Control loop playback
    Loop {
        #[command(subcommand)]
        command: LoopCommand,
    },
    /// MIDI utilities
    Midi {
        #[command(subcommand)]
        command: MidiCommand,
    },
    #[command(hide = true)]
    DaemonRun {
        #[arg(long)]
        clock: bool,
        #[arg(long)]
        sync: bool,
    },
}

#[derive(Subcommand)]
enum ProjectCommand {
    /// Send a create-project command to the daemon (reads file or stdin)
    Create {
        /// Path to the project JSON file; reads from stdin if omitted
        filename: Option<PathBuf>,
    },
    /// Send a modify-project command to the daemon (reads file or stdin)
    Modify {
        /// Path to the project JSON file; reads from stdin if omitted
        filename: Option<PathBuf>,
    },
    /// Print the current/pending project state as compact JSON
    Get,
}

#[derive(Subcommand)]
enum LoopCommand {
    /// Send a loop-start command to the daemon
    Start,
    /// Send a loop-stop command to the daemon
    Stop,
    /// Query current tick position
    Position {
        /// Poll continuously until interrupted
        #[arg(long)]
        poll: bool,
        /// Poll interval in milliseconds (only meaningful with --poll)
        #[arg(long, default_value_t = 50)]
        interval_ms: u64,
    },
}

#[derive(Subcommand)]
enum MidiCommand {
    /// List all available MIDI output ports
    Ports,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Start { clock, sync } => cmd_start(clock, sync),
        Commands::Stop => cmd_stop(),
        Commands::Status => cmd_status(),
        Commands::Project { command } => match command {
            ProjectCommand::Create { filename } => cmd_project_create(filename),
            ProjectCommand::Modify { filename } => cmd_project_modify(filename),
            ProjectCommand::Get => cmd_project_get(),
        },
        Commands::Loop { command } => match command {
            LoopCommand::Start => cmd_loop_start(),
            LoopCommand::Stop => cmd_loop_stop(),
            LoopCommand::Position { poll, interval_ms } => cmd_loop_position(poll, interval_ms),
        },
        Commands::Midi { command } => match command {
            MidiCommand::Ports => cmd_midi_ports(),
        },
        Commands::DaemonRun { clock, sync } => cmd_daemon_run(clock, sync),
    }
}

fn cmd_start(clock: bool, sync: bool) {
    let sock_path = socket_path::resolve();

    match startup_guard::check(&sock_path) {
        startup_guard::StartupOutcome::AlreadyRunning => {
            eprintln!(
                "propeller: already running (socket {:?} is connectable)",
                sock_path
            );
            std::process::exit(1);
        }
        startup_guard::StartupOutcome::StaleCleared => {
            eprintln!("propeller: removed stale socket, starting fresh");
        }
        startup_guard::StartupOutcome::Started => {}
    }

    // Validate PROPELLER_MIDI_PORT before spawning so errors are visible in the terminal.
    let midi_port_name: Option<String> = std::env::var("PROPELLER_MIDI_PORT").ok();
    if let Some(ref name) = midi_port_name {
        let ports = midi_port::list_ports();
        let names: Vec<String> = ports.iter().map(|p| p.name.clone()).collect();
        if midi_port::find_port_by_name(&names, name).is_none() {
            eprintln!(
                "propeller: MIDI port {:?} not found; available ports: [{}]",
                name,
                names.join(", ")
            );
            std::process::exit(1);
        }
    }

    // Validate PROPELLER_SYNC_PORT when --sync is passed.
    if sync {
        match std::env::var("PROPELLER_SYNC_PORT") {
            Err(_) => {
                eprintln!("propeller: --sync requires PROPELLER_SYNC_PORT to be set");
                std::process::exit(1);
            }
            Ok(ref name) => {
                let ports = midi_port::list_ports();
                let names: Vec<String> = ports.iter().map(|p| p.name.clone()).collect();
                if midi_port::find_port_by_name(&names, name).is_none() {
                    eprintln!(
                        "propeller: sync MIDI port {:?} not found; available ports: [{}]",
                        name,
                        names.join(", ")
                    );
                    std::process::exit(1);
                }
            }
        }
    }

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("propeller: cannot determine executable path: {e}");
            std::process::exit(1);
        }
    };

    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("daemon-run");
    if clock {
        cmd.arg("--clock");
    }
    if sync {
        cmd.arg("--sync");
    }
    use std::os::unix::process::CommandExt;
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0);

    if let Err(e) = cmd.spawn() {
        eprintln!("propeller: failed to start daemon: {e}");
        std::process::exit(1);
    }

    // Block until the IPC server is accepting and processing commands.
    // A plain connect() check is insufficient: the socket becomes connectable as soon as
    // UnixListener::bind() is called, which is before run_ipc_server starts. We send a
    // status command and wait for a valid JSON response so that callers can safely send
    // project and loop commands immediately after this function returns.
    use std::io::{BufRead, BufReader, Write};
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(&sock_path) {
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
            eprintln!("propeller: timed out waiting for daemon to become ready");
            std::process::exit(1);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn cmd_daemon_run(clock: bool, sync: bool) {
    let sock_path = socket_path::resolve();

    let midi_port_name: Option<String> = std::env::var("PROPELLER_MIDI_PORT").ok();
    let midi_out: Box<dyn loop_engine::midi::MidiOutput> = match midi_port_name {
        Some(ref name) => match midi_port::open_port(name) {
            Ok(out) => Box::new(out),
            Err(e) => {
                eprintln!("propeller: failed to open MIDI port: {e}");
                std::process::exit(1);
            }
        },
        None => match midi_port::open_virtual() {
            Ok(out) => Box::new(out),
            Err(e) => {
                eprintln!("propeller: failed to open virtual MIDI port: {e}");
                std::process::exit(1);
            }
        },
    };

    let sync_port_name: Option<String> = if sync {
        match std::env::var("PROPELLER_SYNC_PORT") {
            Ok(name) => Some(name),
            Err(_) => {
                eprintln!("propeller: --sync requires PROPELLER_SYNC_PORT to be set");
                std::process::exit(1);
            }
        }
    } else {
        None
    };

    let log_path = logger::platform_log_path();
    let _guard = logger::init(&log_path);

    let initial_mode = if sync {
        EngineMode::Sync
    } else if clock {
        EngineMode::Clock
    } else {
        EngineMode::Standalone
    };

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(daemon::run(
        sock_path,
        midi_out,
        midi_port_name,
        initial_mode,
        sync_port_name,
    ));
}

fn cmd_stop() {
    let sock_path = socket_path::resolve();
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        let mut stream = match tokio::net::UnixStream::connect(&sock_path).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("propeller: not running or cannot connect to {sock_path:?}: {e}");
                std::process::exit(1);
            }
        };
        let msg = r#"{"command":"stop"}"#.to_string() + "\n";
        stream.write_all(msg.as_bytes()).await.unwrap();
        let mut buf = Vec::new();
        let _ = stream.read_to_end(&mut buf).await;
    });

    // Block until the socket file is removed (daemon has fully shut down) (F-23)
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if !sock_path.exists() {
            return;
        }
        if std::time::Instant::now() >= deadline {
            eprintln!("propeller: timed out waiting for daemon to stop");
            std::process::exit(1);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn handle_client_error(e: client::ClientError, sock_path: &std::path::Path) -> ! {
    match e {
        client::ClientError::Connect(err) => {
            eprintln!("propeller: cannot connect to {sock_path:?}: {err}");
            std::process::exit(1);
        }
        client::ClientError::Daemon { message } => {
            eprintln!("propeller: daemon error: {message}");
            std::process::exit(1);
        }
        client::ClientError::Input(msg) => {
            eprintln!("propeller: {msg}");
            std::process::exit(1);
        }
    }
}

fn cmd_project_create(filename: Option<PathBuf>) {
    let sock_path = socket_path::resolve();
    let mut project = match client::read_project_input(filename) {
        Ok(v) => v,
        Err(e) => handle_client_error(e, &sock_path),
    };
    project["command"] = serde_json::json!("create-project");
    if let Err(e) = client::send_command(&sock_path, project) {
        handle_client_error(e, &sock_path);
    }
}

fn cmd_project_modify(filename: Option<PathBuf>) {
    let sock_path = socket_path::resolve();
    let mut project = match client::read_project_input(filename) {
        Ok(v) => v,
        Err(e) => handle_client_error(e, &sock_path),
    };
    project["command"] = serde_json::json!("modify-project");
    if let Err(e) = client::send_command(&sock_path, project) {
        handle_client_error(e, &sock_path);
    }
}

fn cmd_project_get() {
    let sock_path = socket_path::resolve();
    match client::send_command(&sock_path, serde_json::json!({"command": "project"})) {
        Ok(response) => println!("{}", client::format_project_get_output(&response)),
        Err(e) => handle_client_error(e, &sock_path),
    }
}

fn cmd_loop_start() {
    let sock_path = socket_path::resolve();
    if let Err(e) = client::send_command(&sock_path, serde_json::json!({"command": "loop-start"})) {
        handle_client_error(e, &sock_path);
    }
}

fn cmd_loop_position(poll: bool, interval_ms: u64) {
    let sock_path = socket_path::resolve();
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");

    if !poll {
        rt.block_on(async {
            match client::query_position(&sock_path).await {
                Ok((tick, loop_duration)) => {
                    println!("{}", client::format_position_output(tick, loop_duration));
                }
                Err(e) => handle_client_error(e, &sock_path),
            }
        });
        return;
    }

    let poll_result: Result<(), client::ClientError> = rt.block_on(async {
        use tokio::signal::unix::{SignalKind, signal};

        let mut interval = tokio::time::interval(std::time::Duration::from_millis(interval_ms));
        let mut sigint = signal(SignalKind::interrupt()).expect("failed to install SIGINT handler");
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    match client::query_position(&sock_path).await {
                        Ok((tick, loop_duration)) => {
                            println!("{}", client::format_position_output(tick, loop_duration));
                        }
                        Err(e) => return Err(e),
                    }
                }
                _ = sigint.recv() => {
                    return Ok(());
                }
            }
        }
    });

    if let Err(e) = poll_result {
        handle_client_error(e, &sock_path);
    }
}

fn cmd_loop_stop() {
    let sock_path = socket_path::resolve();
    if let Err(e) = client::send_command(&sock_path, serde_json::json!({"command": "loop-stop"})) {
        handle_client_error(e, &sock_path);
    }
}

fn cmd_midi_ports() {
    for port in midi_port::list_ports() {
        println!("{}", port.name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // EP-2 T-6: `project get` parses to Commands::Project { command: ProjectCommand::Get } (F-6, NF-1)
    #[test]
    fn project_get_parses() {
        let cli = Cli::try_parse_from(["propeller", "project", "get"]).unwrap();
        match cli.command {
            Commands::Project {
                command: ProjectCommand::Get,
            } => {}
            _ => panic!("expected Project Get"),
        }
    }

    // EP-3 T-1: `loop position` parses with defaults poll=false, interval_ms=50
    #[test]
    fn loop_position_parses_with_defaults() {
        let cli = Cli::try_parse_from(["propeller", "loop", "position"]).unwrap();
        match cli.command {
            Commands::Loop {
                command: LoopCommand::Position { poll, interval_ms },
            } => {
                assert!(!poll);
                assert_eq!(interval_ms, 50);
            }
            _ => panic!("expected Loop Position"),
        }
    }

    // EP-3 T-1: `--poll` sets poll=true
    #[test]
    fn loop_position_parses_poll_flag() {
        let cli = Cli::try_parse_from(["propeller", "loop", "position", "--poll"]).unwrap();
        match cli.command {
            Commands::Loop {
                command: LoopCommand::Position { poll, .. },
            } => assert!(poll),
            _ => panic!("expected Loop Position"),
        }
    }

    // EP-3 T-1: `--interval-ms 100` sets interval_ms=100
    #[test]
    fn loop_position_parses_interval_ms() {
        let cli =
            Cli::try_parse_from(["propeller", "loop", "position", "--interval-ms", "100"]).unwrap();
        match cli.command {
            Commands::Loop {
                command: LoopCommand::Position { interval_ms, .. },
            } => assert_eq!(interval_ms, 100),
            _ => panic!("expected Loop Position"),
        }
    }
}

fn cmd_status() {
    let sock_path = socket_path::resolve();
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        match tokio::net::UnixStream::connect(&sock_path).await {
            Ok(mut stream) => {
                let msg = r#"{"command":"status"}"#.to_string() + "\n";
                stream.write_all(msg.as_bytes()).await.unwrap();
                let mut buf = String::new();
                stream.read_to_string(&mut buf).await.unwrap();
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(buf.trim()) {
                    println!("propeller is running");
                    if let Some(mode) = v.get("mode") {
                        println!("  mode: {mode}");
                    }
                    if let Some(bpm) = v.get("bpm") {
                        println!("  bpm: {bpm}");
                    }
                    if let Some(cs) = v.get("clock_state") {
                        println!("  clock: {cs}");
                    }
                    if let Some(pp) = v.get("project_present") {
                        println!("  project: {pp}");
                    }
                } else {
                    println!("propeller is running");
                }
            }
            Err(_) => {
                println!("propeller is not running");
                std::process::exit(1);
            }
        }
    });
}
