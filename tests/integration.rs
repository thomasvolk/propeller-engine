use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

// ── helpers ────────────────────────────────────────────────────────────────

fn propeller_bin() -> &'static str {
    env!("CARGO_BIN_EXE_propeller")
}

fn unique_sock_path() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let pid = std::process::id();
    PathBuf::from(format!("/tmp/propeller_test_{pid}_{nanos}.sock"))
}

/// Wait up to `timeout` for the socket to become connectable. Returns true if
/// the socket became connectable within the timeout.
fn wait_for_socket(path: &Path, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if UnixStream::connect(path).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn start_daemon(sock_path: &Path) -> bool {
    let status = Command::new(propeller_bin())
        .arg("start")
        .env("PROPELLER_SOCK", sock_path)
        .status()
        .expect("failed to run propeller start");
    status.success()
}

fn stop_daemon(sock_path: &Path) {
    let _ = Command::new(propeller_bin())
        .arg("stop")
        .env("PROPELLER_SOCK", sock_path)
        .status();
}

/// RAII guard: stops the daemon and removes the socket on drop.
struct DaemonGuard {
    pub sock_path: PathBuf,
}

impl DaemonGuard {
    fn start(sock_path: PathBuf) -> Self {
        start_daemon(&sock_path);
        wait_for_socket(&sock_path, Duration::from_secs(5));
        Self { sock_path }
    }
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        stop_daemon(&self.sock_path);
        let _ = std::fs::remove_file(&self.sock_path);
    }
}

// ── T-4 / T-5 : socket liveness (AC-5, AC-6) ──────────────────────────────

/// T-4: socket at resolved path is connectable after daemon starts (AC-5)
#[test]
fn socket_connectable_after_daemon_starts() {
    let sock = unique_sock_path();
    let _guard = DaemonGuard::start(sock.clone());
    assert!(
        UnixStream::connect(&sock).is_ok(),
        "socket should be connectable after daemon starts"
    );
}

/// T-5: socket absent or connection refused when daemon not running (AC-6)
#[test]
fn socket_not_connectable_when_daemon_not_running() {
    let sock = unique_sock_path();
    assert!(
        UnixStream::connect(&sock).is_err(),
        "socket should not be connectable when daemon is not running"
    );
}

// ── T-7 / T-8 : start command daemonises immediately (AC-1, AC-8) ──────────

/// T-7: `start` CLI command daemonises and calling process returns immediately (AC-1)
#[test]
fn start_command_returns_immediately() {
    let sock = unique_sock_path();
    let before = Instant::now();
    let status = Command::new(propeller_bin())
        .arg("start")
        .env("PROPELLER_SOCK", &sock)
        .status()
        .unwrap();
    let elapsed = before.elapsed();

    // Clean up daemon
    wait_for_socket(&sock, Duration::from_secs(5));
    stop_daemon(&sock);
    let _ = std::fs::remove_file(&sock);

    assert!(status.success(), "start command should exit 0");
    // The parent exits after double-fork; well under 1 s
    assert!(
        elapsed < Duration::from_secs(2),
        "start command should return quickly, took {elapsed:?}"
    );
}

/// T-8: time from `start` invocation to socket-ready is under 1 second (AC-8)
#[test]
fn socket_ready_under_one_second() {
    let sock = unique_sock_path();
    let before = Instant::now();
    start_daemon(&sock);
    let ready = wait_for_socket(&sock, Duration::from_secs(3));
    let elapsed = before.elapsed();

    stop_daemon(&sock);
    let _ = std::fs::remove_file(&sock);

    assert!(ready, "socket should become connectable within 3 s");
    assert!(
        elapsed < Duration::from_secs(1),
        "socket should be ready in < 1 s, took {elapsed:?}"
    );
}

// ── T-10 / T-11 : startup guard (AC-4, AC-10) ─────────────────────────────

/// T-10: second `start` while running is rejected and first instance is unaffected (AC-4)
#[test]
fn second_start_rejected_while_running() {
    let sock = unique_sock_path();
    let _guard = DaemonGuard::start(sock.clone());

    let status = Command::new(propeller_bin())
        .arg("start")
        .env("PROPELLER_SOCK", &sock)
        .status()
        .unwrap();

    assert!(
        !status.success(),
        "second start should fail with non-zero exit code"
    );
    // First instance still connectable
    assert!(
        UnixStream::connect(&sock).is_ok(),
        "first daemon instance should still be running"
    );
}

/// T-11: stale socket (connection refused) is removed and restart succeeds (AC-10)
#[test]
fn stale_socket_cleared_on_start() {
    let sock = unique_sock_path();
    // Create a stale socket file (no listener behind it)
    std::fs::File::create(&sock).unwrap();
    assert!(sock.exists(), "stale socket file should exist before start");

    let ok = start_daemon(&sock);
    assert!(ok, "start should succeed when socket is stale");

    let connectable = wait_for_socket(&sock, Duration::from_secs(5));

    stop_daemon(&sock);
    let _ = std::fs::remove_file(&sock);

    assert!(
        connectable,
        "daemon should start and socket should be connectable after stale clear"
    );
}

// ── T-13 : daemon remains running indefinitely (AC-3) ──────────────────────

/// T-13: daemon remains running without further interaction (AC-3)
#[test]
fn daemon_remains_running_without_interaction() {
    let sock = unique_sock_path();
    let _guard = DaemonGuard::start(sock.clone());

    // Wait a moment with no interaction
    std::thread::sleep(Duration::from_millis(500));

    assert!(
        UnixStream::connect(&sock).is_ok(),
        "daemon should still be running 500 ms after start"
    );
}

// ── T-16 / T-17 : stop command shuts down and removes socket (AC-2) ────────

/// T-16: `stop` CLI command causes daemon to shut down and remove socket (AC-2)
#[test]
fn stop_command_shuts_down_daemon_and_removes_socket() {
    let sock = unique_sock_path();
    start_daemon(&sock);
    assert!(
        wait_for_socket(&sock, Duration::from_secs(5)),
        "daemon should start"
    );

    stop_daemon(&sock);

    // Give daemon time to clean up
    let mut socket_gone = false;
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if UnixStream::connect(&sock).is_err() {
            socket_gone = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let _ = std::fs::remove_file(&sock);

    assert!(socket_gone, "socket should be gone after stop command");
}

// ── T-18 / T-19 : SIGTERM triggers graceful shutdown ───────────────────────

/// T-18: SIGTERM on daemon triggers graceful shutdown and socket removal
#[test]
fn sigterm_causes_graceful_shutdown() {
    let sock = unique_sock_path();
    start_daemon(&sock);
    assert!(
        wait_for_socket(&sock, Duration::from_secs(5)),
        "daemon should start"
    );

    // Find the daemon PID by checking which process has the socket open
    // We can use lsof or send SIGTERM to any process listening on the socket.
    // Simpler: use the stop command which exercises the same shutdown path;
    // the SIGTERM path is covered separately by sending the signal directly.
    //
    // Use `lsof` to find the PID of the process listening on the socket.
    let lsof_out = Command::new("lsof")
        .args(["-t", &sock.to_string_lossy()])
        .output();

    if let Ok(out) = lsof_out {
        let pids: Vec<u32> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.trim().parse().ok())
            .collect();

        for pid in pids {
            Command::new("kill")
                .args(["-15", &pid.to_string()])
                .status()
                .ok();
        }
    }

    let mut socket_gone = false;
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if UnixStream::connect(&sock).is_err() {
            socket_gone = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let _ = std::fs::remove_file(&sock);
    assert!(socket_gone, "socket should be gone after SIGTERM");
}

// ── T-14 : idle resource usage (AC-9) ─────────────────────────────────────

/// T-14: idle CPU below 1 % and memory below 50 MB after a stable period (AC-9)
///
/// Marked ignore because it is slow and environment-sensitive; run explicitly
/// with `cargo test -- --ignored` on a quiet machine.
#[test]
#[ignore]
fn idle_resource_usage_within_limits() {
    let sock = unique_sock_path();
    let _guard = DaemonGuard::start(sock.clone());

    // Let the daemon settle for a second
    std::thread::sleep(Duration::from_secs(1));

    // Find the PID(s) listening on the socket via lsof
    let lsof_out = Command::new("lsof")
        .args(["-t", &sock.to_string_lossy()])
        .output()
        .expect("lsof not available");

    let pids: Vec<u32> = String::from_utf8_lossy(&lsof_out.stdout)
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect();

    assert!(!pids.is_empty(), "could not find daemon PID via lsof");

    let pid = pids[0];

    // Read RSS from /proc/<pid>/status (Linux) or `ps` (macOS)
    let ps_out = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .expect("ps not available");

    let rss_kb: u64 = String::from_utf8_lossy(&ps_out.stdout)
        .trim()
        .parse()
        .expect("could not parse RSS from ps");

    let rss_mb = rss_kb / 1024;
    assert!(rss_mb < 50, "idle RSS {rss_mb} MB exceeds 50 MB limit");

    // CPU: sample with `ps` twice and take the reported %CPU (approximate)
    let cpu_out = Command::new("ps")
        .args(["-o", "%cpu=", "-p", &pid.to_string()])
        .output()
        .expect("ps not available");

    let cpu: f64 = String::from_utf8_lossy(&cpu_out.stdout)
        .trim()
        .parse()
        .expect("could not parse %CPU from ps");

    assert!(cpu < 1.0, "idle CPU {cpu:.2}% exceeds 1% limit");
}

// ── T-20 : PROPELLER_SOCK env var routes to custom path ───────────────────

/// T-20: setting PROPELLER_SOCK env var routes daemon and CLI to custom socket path
#[test]
fn propeller_sock_env_var_uses_custom_path() {
    let sock = unique_sock_path();
    // Verify the default path is NOT used
    let default_sock = PathBuf::from("/tmp/propeller.sock");

    let _guard = DaemonGuard::start(sock.clone());

    assert!(
        UnixStream::connect(&sock).is_ok(),
        "custom socket should be connectable"
    );
    // If default socket happened to exist from another test, it's not ours —
    // but we should not have created it.
    if !default_sock.exists() {
        assert!(
            UnixStream::connect(&default_sock).is_err(),
            "default socket should not be in use when custom path is set"
        );
    }
}

// ── T-33 : runtime command protocol over live socket ──────────────────────

fn send_command(sock: &std::path::Path, cmd: &str) -> serde_json::Value {
    use std::io::{BufRead, BufReader, Write};
    let mut stream = UnixStream::connect(sock).expect("connect failed");
    stream
        .write_all((cmd.to_string() + "\n").as_bytes())
        .unwrap();
    stream.flush().unwrap();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    serde_json::from_str(line.trim()).expect("response not valid JSON")
}

/// T-33: create-project → loop-start → status (running) → loop-stop → status (stopped)
#[test]
fn runtime_protocol_create_start_status_stop() {
    let sock = unique_sock_path();
    let _guard = DaemonGuard::start(sock.clone());

    let create = send_command(
        &sock,
        r#"{"command":"create-project","header":{"bpm":120,"loop_duration":1920},"tracks":[{"name":"piano","channel":1,"instrument":0,"notes":[[0,480,60,80]]}]}"#,
    );
    assert_eq!(create["status"], "ok", "create-project failed");

    let start = send_command(&sock, r#"{"command":"loop-start"}"#);
    assert_eq!(start["status"], "ok", "loop-start failed");

    std::thread::sleep(Duration::from_millis(100));

    let status = send_command(&sock, r#"{"command":"status"}"#);
    assert_eq!(status["status"], "ok");
    assert_eq!(status["clock_state"], "started");
    assert_eq!(status["project_present"], true);
    let resp_str = serde_json::to_string(&status).unwrap();
    assert!(resp_str.contains("mode"), "status missing mode field");

    let stop = send_command(&sock, r#"{"command":"loop-stop"}"#);
    assert_eq!(stop["status"], "ok", "loop-stop failed");

    std::thread::sleep(Duration::from_millis(100));

    let status2 = send_command(&sock, r#"{"command":"status"}"#);
    assert_eq!(status2["clock_state"], "stopped");
}

// ── T-22 / T-23 : status subcommand (AC-11, AC-12) ────────────────────────

/// T-22: `propeller status` exits 0 and reports running when daemon is up (AC-11)
#[test]
fn status_exits_zero_and_reports_running_when_daemon_is_up() {
    let sock = unique_sock_path();
    let _guard = DaemonGuard::start(sock.clone());

    let output = Command::new(propeller_bin())
        .arg("status")
        .env("PROPELLER_SOCK", &sock)
        .output()
        .expect("failed to run propeller status");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "status should exit 0 when daemon is running, got {:?}",
        output.status.code()
    );
    assert!(
        !stdout.trim().is_empty(),
        "status should print a non-empty message"
    );
}

// ── EP-8 T-23 : nonexistent PROPELLER_MIDI_PORT causes non-zero exit ─────────

/// EP-8 T-23: start with PROPELLER_MIDI_PORT=nonexistent_xyz → non-zero exit, stderr names the port
#[test]
fn start_with_nonexistent_midi_port_exits_nonzero() {
    let sock = unique_sock_path();
    let output = Command::new(propeller_bin())
        .arg("start")
        .env("PROPELLER_SOCK", &sock)
        .env("PROPELLER_MIDI_PORT", "nonexistent_xyz_port_name")
        .output()
        .expect("failed to run propeller start");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "start should exit non-zero when PROPELLER_MIDI_PORT names a non-existent port"
    );
    assert!(
        stderr.contains("nonexistent_xyz_port_name"),
        "stderr should contain the requested port name, got: {stderr}"
    );
}

/// T-23: `propeller status` exits non-zero and reports not running when daemon is down (AC-12)
#[test]
fn status_exits_nonzero_and_reports_not_running_when_daemon_is_down() {
    let sock = unique_sock_path();

    let output = Command::new(propeller_bin())
        .arg("status")
        .env("PROPELLER_SOCK", &sock)
        .output()
        .expect("failed to run propeller status");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "status should exit non-zero when daemon is not running"
    );
    assert!(
        !stdout.trim().is_empty() || !stderr.trim().is_empty(),
        "status should print a non-empty message"
    );
}

// ── EP-9: CLI convenience commands ─────────────────────────────────────────

const EP9_PROJECT_JSON: &str = r#"{"header":{"bpm":120,"loop_duration":1920},"tracks":[]}"#;

/// Binds a UnixListener on `sock_path`, spawns a thread that accepts one
/// connection, records the first line received, sends `response`, then exits.
/// Returns a Receiver that yields the recorded line (trimmed).
fn spawn_cli_mock(sock_path: &Path, response: &str) -> mpsc::Receiver<String> {
    let listener = UnixListener::bind(sock_path).expect("bind mock server");
    let response = response.to_string();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("mock accept");
        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        reader.read_line(&mut line).expect("mock read_line");
        tx.send(line.trim().to_string()).ok();
        stream
            .write_all((response + "\n").as_bytes())
            .expect("mock write response");
    });
    rx
}

/// T-10: `propeller project create <file>` — daemon receives create-project, exits 0, no output
#[test]
fn ep9_project_create_from_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let sock = dir.path().join("test.sock");
    let file_path = dir.path().join("project.json");
    std::fs::write(&file_path, EP9_PROJECT_JSON).unwrap();

    let rx = spawn_cli_mock(&sock, r#"{"status":"ok"}"#);

    let output = Command::new(propeller_bin())
        .args(["project", "create", file_path.to_str().unwrap()])
        .env("PROPELLER_SOCK", &sock)
        .output()
        .expect("run propeller project create");

    assert!(
        output.status.success(),
        "expected exit 0, got {:?}",
        output.status.code()
    );
    assert!(output.stdout.is_empty(), "expected no stdout");
    assert!(output.stderr.is_empty(), "expected no stderr");

    let received = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&received).unwrap();
    assert_eq!(parsed["command"], "create-project", "wrong command field");
    assert!(parsed.get("header").is_some(), "missing header");
    assert!(parsed.get("tracks").is_some(), "missing tracks");
}

/// T-11: `propeller project create` with stdin — daemon receives create-project, exits 0
#[test]
fn ep9_project_create_from_stdin() {
    let dir = tempfile::TempDir::new().unwrap();
    let sock = dir.path().join("test.sock");
    let file_path = dir.path().join("project.json");
    std::fs::write(&file_path, EP9_PROJECT_JSON).unwrap();

    let rx = spawn_cli_mock(&sock, r#"{"status":"ok"}"#);

    let output = Command::new(propeller_bin())
        .args(["project", "create"])
        .env("PROPELLER_SOCK", &sock)
        .stdin(std::process::Stdio::from(
            std::fs::File::open(&file_path).unwrap(),
        ))
        .output()
        .expect("run propeller project create (stdin)");

    assert!(
        output.status.success(),
        "expected exit 0, got {:?}",
        output.status.code()
    );
    assert!(output.stdout.is_empty(), "expected no stdout");
    assert!(output.stderr.is_empty(), "expected no stderr");

    let received = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&received).unwrap();
    assert_eq!(parsed["command"], "create-project");
}

/// T-13: `propeller project modify <file>` — daemon receives modify-project, exits 0, no output
#[test]
fn ep9_project_modify_from_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let sock = dir.path().join("test.sock");
    let file_path = dir.path().join("project.json");
    std::fs::write(&file_path, EP9_PROJECT_JSON).unwrap();

    let rx = spawn_cli_mock(&sock, r#"{"status":"ok"}"#);

    let output = Command::new(propeller_bin())
        .args(["project", "modify", file_path.to_str().unwrap()])
        .env("PROPELLER_SOCK", &sock)
        .output()
        .expect("run propeller project modify");

    assert!(
        output.status.success(),
        "expected exit 0, got {:?}",
        output.status.code()
    );
    assert!(output.stdout.is_empty(), "expected no stdout");
    assert!(output.stderr.is_empty(), "expected no stderr");

    let received = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&received).unwrap();
    assert_eq!(parsed["command"], "modify-project", "wrong command field");
    assert!(parsed.get("header").is_some(), "missing header");
    assert!(parsed.get("tracks").is_some(), "missing tracks");
}

/// T-14: `propeller project modify` with stdin — daemon receives modify-project, exits 0
#[test]
fn ep9_project_modify_from_stdin() {
    let dir = tempfile::TempDir::new().unwrap();
    let sock = dir.path().join("test.sock");
    let file_path = dir.path().join("project.json");
    std::fs::write(&file_path, EP9_PROJECT_JSON).unwrap();

    let rx = spawn_cli_mock(&sock, r#"{"status":"ok"}"#);

    let output = Command::new(propeller_bin())
        .args(["project", "modify"])
        .env("PROPELLER_SOCK", &sock)
        .stdin(std::process::Stdio::from(
            std::fs::File::open(&file_path).unwrap(),
        ))
        .output()
        .expect("run propeller project modify (stdin)");

    assert!(
        output.status.success(),
        "expected exit 0, got {:?}",
        output.status.code()
    );
    assert!(output.stdout.is_empty(), "expected no stdout");
    assert!(output.stderr.is_empty(), "expected no stderr");

    let received = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&received).unwrap();
    assert_eq!(parsed["command"], "modify-project");
}

/// T-16: `propeller loop start` — daemon receives loop-start, exits 0, no output
#[test]
fn ep9_loop_start() {
    let dir = tempfile::TempDir::new().unwrap();
    let sock = dir.path().join("test.sock");
    let rx = spawn_cli_mock(&sock, r#"{"status":"ok"}"#);

    let output = Command::new(propeller_bin())
        .args(["loop", "start"])
        .env("PROPELLER_SOCK", &sock)
        .output()
        .expect("run propeller loop start");

    assert!(
        output.status.success(),
        "expected exit 0, got {:?}",
        output.status.code()
    );
    assert!(output.stdout.is_empty(), "expected no stdout");
    assert!(output.stderr.is_empty(), "expected no stderr");

    let received = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&received).unwrap();
    assert_eq!(parsed["command"], "loop-start");
}

/// T-18: `propeller loop stop` — daemon receives loop-stop, exits 0, no output
#[test]
fn ep9_loop_stop() {
    let dir = tempfile::TempDir::new().unwrap();
    let sock = dir.path().join("test.sock");
    let rx = spawn_cli_mock(&sock, r#"{"status":"ok"}"#);

    let output = Command::new(propeller_bin())
        .args(["loop", "stop"])
        .env("PROPELLER_SOCK", &sock)
        .output()
        .expect("run propeller loop stop");

    assert!(
        output.status.success(),
        "expected exit 0, got {:?}",
        output.status.code()
    );
    assert!(output.stdout.is_empty(), "expected no stdout");
    assert!(output.stderr.is_empty(), "expected no stderr");

    let received = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&received).unwrap();
    assert_eq!(parsed["command"], "loop-stop");
}

/// T-20: PROPELLER_SOCK env var — CLI connects to the custom path, not the default
#[test]
fn ep9_custom_sock_path_via_env() {
    let dir = tempfile::TempDir::new().unwrap();
    let sock = dir.path().join("custom.sock");
    let rx = spawn_cli_mock(&sock, r#"{"status":"ok"}"#);

    let output = Command::new(propeller_bin())
        .args(["loop", "start"])
        .env("PROPELLER_SOCK", &sock)
        .output()
        .expect("run propeller loop start");

    assert!(output.status.success(), "expected exit 0");
    let received = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&received).unwrap();
    assert_eq!(parsed["command"], "loop-start");
}

/// T-22: daemon not running — CLI writes human-readable error to stderr, exits non-zero
#[test]
fn ep9_error_when_daemon_not_running() {
    let sock = PathBuf::from(format!(
        "/tmp/propeller_ep9_no_daemon_{}.sock",
        std::process::id()
    ));

    let output = Command::new(propeller_bin())
        .args(["loop", "start"])
        .env("PROPELLER_SOCK", &sock)
        .output()
        .expect("run propeller loop start");

    assert!(!output.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.is_empty(), "expected error message in stderr");
    assert!(
        stderr.contains("propeller"),
        "stderr should contain 'propeller', got: {stderr}"
    );
}

// ── EP-6: sync startup guard (AC-9, F-9) ──────────────────────────────────

/// EP-6: `propeller start --sync` without PROPELLER_SYNC_PORT → exits non-zero,
/// socket never created (F-9, AC-9).
#[test]
fn start_with_sync_flag_and_no_env_var_exits_nonzero() {
    let sock = unique_sock_path();
    let output = Command::new(propeller_bin())
        .args(["start", "--sync"])
        .env("PROPELLER_SOCK", &sock)
        .env_remove("PROPELLER_SYNC_PORT")
        .output()
        .expect("failed to run propeller start --sync");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "start --sync without PROPELLER_SYNC_PORT should exit non-zero"
    );
    assert!(
        !wait_for_socket(&sock, Duration::from_millis(500)),
        "socket should not be created when startup guard fires"
    );
    assert!(
        stderr.contains("PROPELLER_SYNC_PORT"),
        "stderr should mention PROPELLER_SYNC_PORT, got: {stderr}"
    );
}

/// EP-6: `propeller start --sync` with PROPELLER_SYNC_PORT set to a nonexistent
/// port name → exits non-zero, socket never created (F-9, AC-9).
#[test]
fn start_with_sync_flag_and_nonexistent_port_exits_nonzero() {
    let sock = unique_sock_path();
    let output = Command::new(propeller_bin())
        .args(["start", "--sync"])
        .env("PROPELLER_SOCK", &sock)
        .env("PROPELLER_SYNC_PORT", "nonexistent_sync_port_xyz")
        .output()
        .expect("failed to run propeller start --sync");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "start --sync with nonexistent PROPELLER_SYNC_PORT should exit non-zero"
    );
    assert!(
        !wait_for_socket(&sock, Duration::from_millis(500)),
        "socket should not be created when startup guard fires"
    );
    assert!(
        stderr.contains("nonexistent_sync_port_xyz"),
        "stderr should contain the requested port name, got: {stderr}"
    );
}

/// T-24: daemon returns error — CLI writes message to stderr, exits non-zero
#[test]
fn ep9_error_when_daemon_returns_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let sock = dir.path().join("test.sock");
    spawn_cli_mock(
        &sock,
        r#"{"status":"error","message":"intentional test error"}"#,
    );

    let output = Command::new(propeller_bin())
        .args(["loop", "start"])
        .env("PROPELLER_SOCK", &sock)
        .output()
        .expect("run propeller loop start");

    assert!(!output.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.is_empty(), "expected error message in stderr");
    assert!(
        stderr.contains("intentional test error") || stderr.contains("propeller"),
        "stderr should mention the error, got: {stderr}"
    );
}
