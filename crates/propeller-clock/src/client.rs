// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

pub enum ClientError {
    Connect(std::io::Error),
    Daemon { message: String },
    Input(String),
}

pub fn send_command(sock_path: &Path, cmd: Value) -> Result<Value, ClientError> {
    let mut stream = UnixStream::connect(sock_path).map_err(ClientError::Connect)?;

    let mut line = serde_json::to_string(&cmd).expect("command serialisation cannot fail");
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .map_err(|e| ClientError::Input(e.to_string()))?;

    let mut reader = BufReader::new(stream);
    let mut response_line = String::new();
    reader
        .read_line(&mut response_line)
        .map_err(|e| ClientError::Input(e.to_string()))?;

    let response: Value = serde_json::from_str(response_line.trim())
        .map_err(|e| ClientError::Input(format!("invalid response JSON: {e}")))?;

    if response.get("status").and_then(|s| s.as_str()) == Some("error") {
        let message = response
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error")
            .to_string();
        return Err(ClientError::Daemon { message });
    }

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;
    use std::time::Duration;
    use tempfile::TempDir;

    fn spawn_mock(sock_path: std::path::PathBuf, response: String) -> mpsc::Receiver<String> {
        let listener = UnixListener::bind(&sock_path).unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(&stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            tx.send(line.trim().to_string()).ok();
            stream.write_all((response + "\n").as_bytes()).unwrap();
        });
        rx
    }

    #[test]
    fn send_command_ok_path() {
        let dir = TempDir::new().unwrap();
        let sock_path = dir.path().join("test.sock");
        let rx = spawn_mock(sock_path.clone(), r#"{"status":"ok"}"#.to_string());

        let cmd = serde_json::json!({"command": "start"});
        let result = send_command(&sock_path, cmd);

        assert!(result.is_ok());
        let received = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let parsed: Value = serde_json::from_str(&received).unwrap();
        assert_eq!(parsed["command"], "start");
    }

    #[test]
    fn send_command_connect_error() {
        let result = send_command(
            Path::new("/tmp/propeller_clock_nonexistent_test_xyz_99999.sock"),
            serde_json::json!({"command": "start"}),
        );
        assert!(matches!(result, Err(ClientError::Connect(_))));
    }

    #[test]
    fn send_command_daemon_error() {
        let dir = TempDir::new().unwrap();
        let sock_path = dir.path().join("test.sock");
        spawn_mock(
            sock_path.clone(),
            r#"{"status":"error","message":"test daemon error"}"#.to_string(),
        );

        let result = send_command(&sock_path, serde_json::json!({"command": "start"}));
        match result {
            Err(ClientError::Daemon { message }) => assert_eq!(message, "test daemon error"),
            _ => panic!("expected ClientError::Daemon"),
        }
    }
}
