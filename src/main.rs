mod client;
mod daemon;
mod domain;
mod ipc;
mod loop_engine;
mod logger;
mod midi_clock;
mod midi_port;
mod socket_path;
mod startup_guard;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use clap::{Parser, Subcommand};
use ipc::EngineMode;
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
        /// MIDI input port name for external clock sync
        #[arg(long)]
        sync_port: Option<String>,
        /// Start in clock mode (overrides the default standalone mode)
        #[arg(long)]
        clock: bool,
    },
    /// Stop the running daemon
    Stop,
    /// Check whether the daemon is running
    Status,
    /// List available MIDI input ports
    ListPorts,
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
}

#[derive(Subcommand)]
enum LoopCommand {
    /// Send a loop-start command to the daemon
    Start,
    /// Send a loop-stop command to the daemon
    Stop,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Start { sync_port, clock } => cmd_start(sync_port, clock),
        Commands::Stop => cmd_stop(),
        Commands::Status => cmd_status(),
        Commands::ListPorts => cmd_list_ports(),
        Commands::Project { command } => match command {
            ProjectCommand::Create { filename } => cmd_project_create(filename),
            ProjectCommand::Modify { filename } => cmd_project_modify(filename),
        },
        Commands::Loop { command } => match command {
            LoopCommand::Start => cmd_loop_start(),
            LoopCommand::Stop => cmd_loop_stop(),
        },
    }
}

fn cmd_start(sync_port: Option<String>, clock: bool) {
    let sock_path = socket_path::resolve();

    match startup_guard::check(&sock_path) {
        startup_guard::StartupOutcome::AlreadyRunning => {
            eprintln!("propeller: already running (socket {:?} is connectable)", sock_path);
            std::process::exit(1);
        }
        startup_guard::StartupOutcome::StaleCleared => {
            eprintln!("propeller: removed stale socket, starting fresh");
        }
        startup_guard::StartupOutcome::Started => {}
    }

    // Validate PROPELLER_MIDI_PORT before daemonize so errors are visible in the terminal.
    // The actual connection is opened after daemonize (in the child) to avoid CoreMIDI
    // fork-safety issues.
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

    // Validate --sync-port before daemonize
    if let Some(ref name) = sync_port {
        let ports = midi_clock::list_midi_input_ports();
        if !ports.iter().any(|p| p == name) {
            eprintln!(
                "propeller: sync MIDI port {:?} not found; available ports: [{}]",
                name,
                ports.join(", ")
            );
            std::process::exit(1);
        }
    }

    match daemonize::Daemonize::new().start() {
        Ok(_) => {}
        Err(e) => {
            eprintln!("propeller: daemonize failed: {e}");
            std::process::exit(1);
        }
    }

    // Open the MIDI output connection after daemonize (in the child process).
    let midi_out: Box<dyn loop_engine::midi::MidiOutput> = match midi_port_name {
        Some(ref name) => match midi_port::open_port(name) {
            Ok(out) => Box::new(out),
            Err(e) => {
                eprintln!("propeller: failed to open MIDI port after daemonize: {e}");
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

    let log_path = logger::platform_log_path();
    let _guard = logger::init(&log_path);

    // Open sync clock input if --sync-port was given.
    // The MidiInputBridge is kept alive by _bridge_guard until block_on returns.
    let _bridge_guard: Option<midi_clock::MidiInputBridge>;
    let clock_rx: Option<mpsc::Receiver<midi_clock::ClockMessage>>;
    let sync_clock_state: Option<Arc<Mutex<midi_clock::SyncClockState>>>;

    if let Some(ref name) = sync_port {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(midi_clock::SyncClockState::Waiting));
        let bridge = midi_clock::MidiInputBridge::open(name, tx).unwrap_or_else(|e| {
            eprintln!("propeller: failed to open sync MIDI port: {e}");
            std::process::exit(1);
        });
        _bridge_guard = Some(bridge);
        clock_rx = Some(rx);
        sync_clock_state = Some(state);
    } else {
        _bridge_guard = None;
        clock_rx = None;
        sync_clock_state = None;
    }

    let initial_mode = if clock { EngineMode::Clock } else { EngineMode::Standalone };

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(daemon::run(sock_path, midi_out, clock_rx, sync_clock_state, initial_mode));
}

fn cmd_list_ports() {
    let ports = midi_clock::list_midi_input_ports();
    for p in &ports {
        println!("{p}");
    }
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

fn cmd_loop_start() {
    let sock_path = socket_path::resolve();
    if let Err(e) = client::send_command(&sock_path, serde_json::json!({"command": "loop-start"}))
    {
        handle_client_error(e, &sock_path);
    }
}

fn cmd_loop_stop() {
    let sock_path = socket_path::resolve();
    if let Err(e) = client::send_command(&sock_path, serde_json::json!({"command": "loop-stop"}))
    {
        handle_client_error(e, &sock_path);
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
                    if let Some(mode) = v.get("mode") { println!("  mode: {mode}"); }
                    if let Some(bpm) = v.get("bpm") { println!("  bpm: {bpm}"); }
                    if let Some(cs) = v.get("clock_state") { println!("  clock: {cs}"); }
                    if let Some(pp) = v.get("project_present") { println!("  project: {pp}"); }
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
