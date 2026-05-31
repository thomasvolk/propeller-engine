// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use serde_json::Value;

pub(crate) enum ClientError {
    Connect(std::io::Error),
    Daemon { message: String },
    Input(String),
}

pub(crate) fn send_command(sock_path: &Path, cmd: Value) -> Result<Value, ClientError> {
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

pub(crate) fn read_project_input(filename: Option<PathBuf>) -> Result<Value, ClientError> {
    let content = match filename {
        Some(path) => std::fs::read_to_string(&path)
            .map_err(|e| ClientError::Input(format!("cannot read {path:?}: {e}")))?,
        None => {
            let mut content = String::new();
            std::io::stdin()
                .lock()
                .read_to_string(&mut content)
                .map_err(|e| ClientError::Input(format!("cannot read stdin: {e}")))?;
            content
        }
    };

    serde_json::from_str(&content)
        .map_err(|e| ClientError::Input(format!("invalid project JSON: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;
    use std::time::Duration;
    use tempfile::TempDir;

    fn spawn_mock(sock_path: PathBuf, response: String) -> mpsc::Receiver<String> {
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

    // T-1: send_command writes the correct JSON line and returns Ok(Value)
    #[test]
    fn send_command_ok_path() {
        let dir = TempDir::new().unwrap();
        let sock_path = dir.path().join("test.sock");
        let rx = spawn_mock(sock_path.clone(), r#"{"status":"ok"}"#.to_string());

        let cmd = serde_json::json!({"command": "loop-start"});
        let result = send_command(&sock_path, cmd);

        assert!(result.is_ok(), "expected Ok");
        let received = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let parsed: Value = serde_json::from_str(&received).unwrap();
        assert_eq!(parsed["command"], "loop-start");
    }

    // T-3: send_command returns ClientError::Connect when no socket exists
    #[test]
    fn send_command_connect_error() {
        let result = send_command(
            Path::new("/tmp/propeller_nonexistent_test_xyz_99999.sock"),
            serde_json::json!({"command": "loop-start"}),
        );
        assert!(matches!(result, Err(ClientError::Connect(_))));
    }

    // T-5: send_command returns ClientError::Daemon when server replies with error status
    #[test]
    fn send_command_daemon_error() {
        let dir = TempDir::new().unwrap();
        let sock_path = dir.path().join("test.sock");
        spawn_mock(
            sock_path.clone(),
            r#"{"status":"error","message":"test daemon error"}"#.to_string(),
        );

        let result = send_command(&sock_path, serde_json::json!({"command": "loop-start"}));
        match result {
            Err(ClientError::Daemon { message }) => {
                assert_eq!(message, "test daemon error");
            }
            _ => panic!("expected ClientError::Daemon"),
        }
    }

    // T-7: read_project_input(Some(path)) reads a temp file and returns the parsed Value
    #[test]
    fn read_project_input_from_file() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("project.json");
        let json_content = r#"{"header":{"bpm":120,"time_signature":{"numerator":4,"denominator":4}},"tracks":[]}"#;
        std::fs::write(&file_path, json_content).unwrap();

        let result = read_project_input(Some(file_path));
        let v = result.unwrap_or_else(|e| match e {
            ClientError::Input(s) => panic!("Input error: {s}"),
            _ => panic!("unexpected error"),
        });
        assert!(v.get("header").is_some(), "missing 'header' field");
        assert!(v.get("tracks").is_some(), "missing 'tracks' field");
    }
}
