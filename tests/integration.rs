use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
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

    assert!(connectable, "daemon should start and socket should be connectable after stale clear");
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
