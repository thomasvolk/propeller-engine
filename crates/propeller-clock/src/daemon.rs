// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::net::UnixListener;
use tokio::sync::oneshot;

use crate::engine::ClockEngine;
use crate::ipc::{ClockSettings, run_ipc_server};

pub async fn run(sock_path: PathBuf, engine: Arc<ClockEngine>, settings: Arc<ClockSettings>) {
    let listener = match UnixListener::bind(&sock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("propeller-clock: failed to bind socket {sock_path:?}: {e}");
            std::process::exit(1);
        }
    };

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let shutdown_tx = Arc::new(Mutex::new(Some(shutdown_tx)));

    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("failed to install SIGTERM handler");

    tokio::select! {
        _ = run_ipc_server(listener, Arc::clone(&engine), settings, shutdown_tx) => {}
        _ = shutdown_rx => {}
        _ = sigterm.recv() => {}
    }

    // Send MIDI Stop before removing the socket so connected devices don't hang.
    engine.stop_and_wait();
    let _ = std::fs::remove_file(&sock_path);
}
