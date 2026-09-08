// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use std::os::unix::net::UnixStream;
use std::path::Path;

#[derive(Debug, PartialEq)]
pub enum StartupOutcome {
    Started,
    AlreadyRunning,
    StaleCleared,
}

pub fn check(sock_path: &Path) -> StartupOutcome {
    if !sock_path.exists() {
        return StartupOutcome::Started;
    }
    match UnixStream::connect(sock_path) {
        Ok(_) => StartupOutcome::AlreadyRunning,
        Err(_) => {
            let _ = std::fs::remove_file(sock_path);
            StartupOutcome::StaleCleared
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use tempfile::tempdir;

    #[test]
    fn returns_started_when_no_socket_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("no.sock");
        assert_eq!(check(&path), StartupOutcome::Started);
    }

    #[test]
    fn returns_already_running_when_socket_connectable() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("live.sock");
        let _listener = UnixListener::bind(&path).unwrap();
        assert_eq!(check(&path), StartupOutcome::AlreadyRunning);
    }

    #[test]
    fn returns_stale_cleared_and_removes_file_when_connection_refused() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("stale.sock");
        // Create the socket file but don't listen on it
        std::fs::File::create(&path).unwrap();
        let result = check(&path);
        assert_eq!(result, StartupOutcome::StaleCleared);
        assert!(!path.exists(), "stale socket file should be removed");
    }
}
