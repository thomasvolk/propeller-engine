use std::sync::{Arc, Mutex, RwLock};

use tokio::net::UnixListener;
use tokio::sync::oneshot;
use tracing::error;

use crate::domain::ProjectStore;
use crate::loop_engine::LoopEngine;
use crate::midi_clock::SyncClockState;

use super::handler::connection_handler;
use super::types::EngineSettings;

pub async fn run_ipc_server(
    listener: UnixListener,
    store: Arc<RwLock<ProjectStore>>,
    engine: Arc<LoopEngine>,
    settings: Arc<Mutex<EngineSettings>>,
    shutdown_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    sync_clock_state: Option<Arc<Mutex<SyncClockState>>>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let store = Arc::clone(&store);
                let engine = Arc::clone(&engine);
                let settings = Arc::clone(&settings);
                let shutdown_tx = Arc::clone(&shutdown_tx);
                let sync_clock_state = sync_clock_state.clone();
                tokio::spawn(async move {
                    connection_handler(stream, store, engine, settings, shutdown_tx, sync_clock_state).await;
                });
            }
            Err(e) => {
                error!("accept error: {e}");
                break;
            }
        }
    }
}
