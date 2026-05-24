use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock, mpsc};

use tokio::net::UnixListener;
use tokio::sync::oneshot;
use tracing::info;

use crate::domain::ProjectStore;
use crate::ipc::{run_ipc_server, EngineMode, EngineSettings};
use crate::loop_engine::{LoopEngine, midi::MidiOutput};
use crate::midi_clock::{ClockMessage, MidiClockReceiver, SyncClockState};

pub async fn run(
    sock_path: PathBuf,
    midi_output: Box<dyn MidiOutput>,
    clock_rx: Option<mpsc::Receiver<ClockMessage>>,
    sync_clock_state: Option<Arc<Mutex<SyncClockState>>>,
    initial_mode: EngineMode,
) {
    let listener = match UnixListener::bind(&sock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("propeller: failed to bind socket {sock_path:?}: {e}");
            std::process::exit(1);
        }
    };

    info!("daemon started, listening on {sock_path:?}");

    let store = Arc::new(RwLock::new(ProjectStore::new()));
    let engine = Arc::new(LoopEngine::new(Arc::clone(&store), midi_output));
    let engine_for_shutdown = Arc::clone(&engine);
    let settings = Arc::new(Mutex::new(EngineSettings::new()));
    settings.lock().unwrap().mode = initial_mode;

    // Wire up sync clock receiver if --sync-port was provided (overrides initial_mode)
    if let (Some(rx), Some(ref state)) = (clock_rx, sync_clock_state.clone()) {
        settings.lock().unwrap().mode = EngineMode::Sync;
        MidiClockReceiver::new(rx, Arc::clone(&engine), Arc::clone(state));
    }

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let shutdown_tx = Arc::new(Mutex::new(Some(shutdown_tx)));

    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("failed to install SIGTERM handler");

    tokio::select! {
        _ = run_ipc_server(listener, store, engine, settings, shutdown_tx, sync_clock_state) => {}
        _ = shutdown_rx => {
            info!("stop command received, shutting down");
        }
        _ = sigterm.recv() => {
            info!("SIGTERM received, shutting down");
        }
    }

    // T-30 (EP-5): send MIDI Stop before removing socket so connected devices don't hang
    engine_for_shutdown.clock_stop_on_shutdown();
    let _ = std::fs::remove_file(&sock_path);
    info!("daemon stopped");
}
