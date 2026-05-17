use std::path::PathBuf;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixListener;
use tracing::{error, info};

use crate::ipc::IpcMessage;

pub async fn run(sock_path: PathBuf) {
    let listener = match UnixListener::bind(&sock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("propeller: failed to bind socket {sock_path:?}: {e}");
            std::process::exit(1);
        }
    };

    info!("daemon started, listening on {sock_path:?}");

    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("failed to install SIGTERM handler");

    loop {
        tokio::select! {
            conn = listener.accept() => {
                match conn {
                    Ok((stream, _)) => {
                        let mut lines = BufReader::new(stream).lines();
                        match lines.next_line().await {
                            Ok(Some(line)) => {
                                match serde_json::from_str::<IpcMessage>(&line) {
                                    Ok(IpcMessage::Stop) => {
                                        info!("stop command received, shutting down");
                                        break;
                                    }
                                    Err(e) => error!("unrecognised IPC message: {e}"),
                                }
                            }
                            Ok(None) => {}
                            Err(e) => error!("read error: {e}"),
                        }
                    }
                    Err(e) => error!("accept error: {e}"),
                }
            }
            _ = sigterm.recv() => {
                info!("SIGTERM received, shutting down");
                break;
            }
        }
    }

    let _ = std::fs::remove_file(&sock_path);
    info!("daemon stopped");
}
