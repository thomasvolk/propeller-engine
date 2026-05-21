mod daemon;
mod domain;
mod ipc;
mod logger;
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

    // Double-fork via `daemonize`; this returns only in the grandchild.
    // Fork must happen before tokio runtime is created (fork in multithreaded context is unsafe).
    match daemonize::Daemonize::new().start() {
        Ok(_) => {}
        Err(e) => {
            eprintln!("propeller: daemonize failed: {e}");
            std::process::exit(1);
        }
    }

    // --- grandchild process starts here ---
    let log_path = logger::platform_log_path();
    let _guard = logger::init(&log_path);

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(daemon::run(sock_path));
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
        let msg = serde_json::to_string(&ipc::IpcMessage::Stop).unwrap() + "\n";
        stream.write_all(msg.as_bytes()).await.unwrap();
        // Wait for connection close as confirmation of shutdown
        let mut buf = Vec::new();
        let _ = stream.read_to_end(&mut buf).await;
    });
}

fn cmd_status() {
    let sock_path = socket_path::resolve();
    match std::os::unix::net::UnixStream::connect(&sock_path) {
        Ok(_) => {
            println!("propeller is running");
        }
        Err(_) => {
            println!("propeller is not running");
            std::process::exit(1);
        }
    }
}
