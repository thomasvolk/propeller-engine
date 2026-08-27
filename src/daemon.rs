// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use tokio::net::UnixListener;
use tokio::sync::oneshot;
use tracing::info;

use crate::domain::ProjectStore;
use crate::ipc::{EngineMode, EngineSettings, run_ipc_server};
use crate::loop_engine::{LoopEngine, midi::MidiOutput};
use crate::midi_clock::MidiClockReceiver;

pub async fn run(
    sock_path: PathBuf,
    midi_output: Box<dyn MidiOutput>,
    midi_port_name: Option<String>,
    initial_mode: EngineMode,
    sync_port_name: Option<String>,
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
    {
        let mut settings_guard = settings.lock().unwrap();
        settings_guard.mode = initial_mode;
        settings_guard.midi_port_name = midi_port_name;
    }

    // If --sync was passed, start the MIDI clock receiver and store its state in settings.
    let _clock_receiver: Option<MidiClockReceiver> = if let Some(ref port_name) = sync_port_name {
        match MidiClockReceiver::new(port_name, Arc::clone(&engine)) {
            Ok(receiver) => {
                let mut settings_guard = settings.lock().unwrap();
                settings_guard.sync_clock_state = Some(receiver.state_arc());
                settings_guard.sync_port_name = Some(port_name.clone());
                Some(receiver)
            }
            Err(e) => {
                eprintln!("propeller: failed to start MIDI clock receiver on {port_name:?}: {e}");
                std::process::exit(1);
            }
        }
    } else {
        None
    };

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

    // Send MIDI Stop before removing socket so connected devices don't hang.
    engine_for_shutdown.clock_stop_on_shutdown();
    let _ = std::fs::remove_file(&sock_path);
    info!("daemon stopped");
}
