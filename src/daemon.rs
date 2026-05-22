use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use tokio::net::UnixListener;
use tokio::sync::oneshot;
use tracing::info;

use crate::domain::ProjectStore;
use crate::ipc::{run_ipc_server, EngineSettings};
use crate::loop_engine::{LoopEngine, midi::MockMidiOutput};

pub async fn run(sock_path: PathBuf) {
    let listener = match UnixListener::bind(&sock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("propeller: failed to bind socket {sock_path:?}: {e}");
            std::process::exit(1);
        }
    };

    info!("daemon started, listening on {sock_path:?}");

    let store = Arc::new(RwLock::new(ProjectStore::new()));
    let engine = Arc::new(LoopEngine::new(Arc::clone(&store), Box::new(MockMidiOutput::new())));
    let settings = Arc::new(Mutex::new(EngineSettings::new()));

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let shutdown_tx = Arc::new(Mutex::new(Some(shutdown_tx)));

    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("failed to install SIGTERM handler");

    tokio::select! {
        _ = run_ipc_server(listener, store, engine, settings, shutdown_tx) => {}
        _ = shutdown_rx => {
            info!("stop command received, shutting down");
        }
        _ = sigterm.recv() => {
            info!("SIGTERM received, shutting down");
        }
    }

    let _ = std::fs::remove_file(&sock_path);
    info!("daemon stopped");
}
