mod daemon;
mod domain;
mod ipc;
mod loop_engine;
mod logger;
mod midi_port;
mod socket_path;
mod startup_guard;

use clap::{Parser, Subcommand};
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
    Start,
    /// Stop the running daemon
    Stop,
    /// Check whether the daemon is running
    Status,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Start => cmd_start(),
        Commands::Stop => cmd_stop(),
        Commands::Status => cmd_status(),
    }
}

fn cmd_start() {
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

    match daemonize::Daemonize::new().start() {
        Ok(_) => {}
        Err(e) => {
            eprintln!("propeller: daemonize failed: {e}");
            std::process::exit(1);
        }
    }

    // Open the MIDI connection after daemonize (in the child process).
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

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(daemon::run(sock_path, midi_out));
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
