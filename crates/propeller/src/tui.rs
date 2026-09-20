// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use std::io;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::client::{self, ClientError};
use crate::midi_port;

const POLL_INTERVAL: Duration = Duration::from_millis(200);
const RESTART_TIMEOUT: Duration = Duration::from_secs(10);

const HELP_LINE: &str =
    "d start/stop daemon   o midi port   m mode   y sync port   c clear project   q/Esc quit";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Standalone,
    Clock,
    Sync,
}

impl Mode {
    fn next(self) -> Mode {
        match self {
            Mode::Standalone => Mode::Clock,
            Mode::Clock => Mode::Sync,
            Mode::Sync => Mode::Standalone,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Mode::Standalone => "standalone",
            Mode::Clock => "clock",
            Mode::Sync => "sync",
        }
    }

    fn from_str(s: &str) -> Mode {
        match s {
            "clock" => Mode::Clock,
            "sync" => Mode::Sync,
            _ => Mode::Standalone,
        }
    }
}

/// A port picker's value list. Index 0 is always the "default/none" sentinel:
/// for the MIDI output port that means the fallback virtual port, for the sync
/// port it means "no sync port chosen yet".
struct PortPicker {
    values: Vec<Option<String>>,
    index: usize,
    none_label: &'static str,
}

impl PortPicker {
    fn new(names: Vec<String>, none_label: &'static str) -> Self {
        let mut values = vec![None];
        values.extend(names.into_iter().map(Some));
        PortPicker {
            values,
            index: 0,
            none_label,
        }
    }

    fn selected(&self) -> Option<&str> {
        self.values[self.index].as_deref()
    }

    fn advance(&mut self) {
        self.index = (self.index + 1) % self.values.len();
    }

    fn select_by_name(&mut self, name: &str) {
        if let Some(i) = self.values.iter().position(|v| v.as_deref() == Some(name)) {
            self.index = i;
        }
    }

    fn label(&self) -> &str {
        self.selected().unwrap_or(self.none_label)
    }
}

struct Console {
    sock_path: PathBuf,
    daemon_running: bool,
    mode: Mode,
    output_port: PortPicker,
    sync_port: PortPicker,
    clock_state: String,
    bpm: Option<u32>,
    sync_clock_state: Option<String>,
    error_message: Option<String>,
}

impl Console {
    fn refresh_from_status(&mut self) {
        match client::send_command(&self.sock_path, serde_json::json!({"command": "status"})) {
            Ok(v) => {
                self.daemon_running = true;
                self.mode = v
                    .get("mode")
                    .and_then(|m| m.as_str())
                    .map(Mode::from_str)
                    .unwrap_or(self.mode);
                self.clock_state = v
                    .get("clock_state")
                    .and_then(|s| s.as_str())
                    .unwrap_or("?")
                    .to_string();
                self.bpm = v.get("bpm").and_then(|b| b.as_u64()).map(|b| b as u32);
                if let Some(name) = v.get("midi_port_name").and_then(|p| p.as_str()) {
                    self.output_port.select_by_name(name);
                }
                self.sync_clock_state = v
                    .get("sync_clock_state")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string());
                if let Some(name) = v.get("sync_port_name").and_then(|p| p.as_str()) {
                    self.sync_port.select_by_name(name);
                }
            }
            Err(ClientError::Connect(_)) => {
                self.daemon_running = false;
                self.clock_state = "stopped".to_string();
                self.bpm = None;
                self.sync_clock_state = None;
            }
            Err(_) => {}
        }
    }

    fn stop_daemon(&mut self) {
        let _ = client::send_command(&self.sock_path, serde_json::json!({"command": "stop"}));
        let deadline = Instant::now() + RESTART_TIMEOUT;
        while self.sock_path.exists() {
            if Instant::now() >= deadline {
                self.error_message = Some("timed out waiting for daemon to stop".to_string());
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        self.daemon_running = false;
    }

    fn spawn_daemon(&mut self) {
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                self.error_message = Some(format!("cannot determine executable path: {e}"));
                return;
            }
        };

        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("daemon-run");
        match self.mode {
            Mode::Clock => {
                cmd.arg("--clock");
            }
            Mode::Sync => {
                cmd.arg("--sync");
            }
            Mode::Standalone => {}
        }
        match self.output_port.selected() {
            Some(name) => {
                cmd.env("PROPELLER_MIDI_PORT", name);
            }
            None => {
                cmd.env_remove("PROPELLER_MIDI_PORT");
            }
        }
        match (self.mode, self.sync_port.selected()) {
            (Mode::Sync, Some(name)) => {
                cmd.env("PROPELLER_SYNC_PORT", name);
            }
            _ => {
                cmd.env_remove("PROPELLER_SYNC_PORT");
            }
        }

        use std::os::unix::process::CommandExt;
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0);

        if let Err(e) = cmd.spawn() {
            self.error_message = Some(format!("failed to start daemon: {e}"));
            return;
        }

        let deadline = Instant::now() + RESTART_TIMEOUT;
        loop {
            if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(&self.sock_path) {
                let probe = r#"{"command":"status"}"#.to_string() + "\n";
                if stream.write_all(probe.as_bytes()).is_ok() {
                    let mut reader = BufReader::new(stream);
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_ok() && !line.is_empty() {
                        self.daemon_running = true;
                        self.error_message = None;
                        return;
                    }
                }
            }
            if Instant::now() >= deadline {
                self.error_message =
                    Some("timed out waiting for daemon to become ready".to_string());
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Restarts the daemon with the currently selected mode/ports; a no-op if the daemon
    /// isn't running, since a stopped daemon has nothing to apply the change to yet.
    fn apply_if_running(&mut self) {
        if !self.daemon_running {
            return;
        }
        self.stop_daemon();
        self.spawn_daemon();
    }

    fn toggle_daemon(&mut self) {
        if self.daemon_running {
            self.stop_daemon();
        } else {
            self.spawn_daemon();
        }
    }

    /// Clears both the active and pending project immediately, even while the loop is
    /// running. A no-op (not an error) when the daemon isn't running, since there is no
    /// project state to clear.
    fn clear_project(&mut self) {
        match client::send_command(&self.sock_path, serde_json::json!({"command": "clear-project"}))
        {
            Ok(_) => self.error_message = None,
            Err(ClientError::Connect(_)) => {}
            Err(ClientError::Daemon { message }) => self.error_message = Some(message),
            Err(ClientError::Input(msg)) => self.error_message = Some(msg),
        }
    }

    fn cycle_output_port(&mut self) {
        self.output_port.advance();
        self.apply_if_running();
    }

    fn cycle_sync_port(&mut self) {
        self.sync_port.advance();
        if self.mode == Mode::Sync {
            self.apply_if_running();
        }
    }

    fn cycle_mode(&mut self) {
        let mut candidate = self.mode.next();
        let blocked = candidate == Mode::Sync && self.sync_port.selected().is_none();
        if blocked {
            candidate = candidate.next();
        }
        self.mode = candidate;
        // apply_if_running() may clear error_message on a successful restart, so the
        // blocked-switch message is set afterwards to make sure it survives that restart.
        self.apply_if_running();
        if blocked {
            self.error_message = Some("select a sync port first (y)".to_string());
        }
    }
}

/// Runs the interactive console against the daemon at `sock_path` until the user quits
/// (q/Esc). If the daemon isn't already running when the console opens, it is launched
/// in the background with default settings, mirroring `propeller-clock console`. Always
/// restores the terminal before returning, even on error.
pub fn run(sock_path: &Path) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, sock_path);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    sock_path: &Path,
) -> io::Result<()> {
    let mut console = Console {
        sock_path: sock_path.to_path_buf(),
        daemon_running: false,
        mode: Mode::Standalone,
        output_port: PortPicker::new(
            midi_port::list_ports()
                .into_iter()
                .map(|p| p.name)
                .collect(),
            "default (virtual)",
        ),
        sync_port: PortPicker::new(midi_port::list_input_port_names(), "none chosen"),
        clock_state: "?".to_string(),
        bpm: None,
        sync_clock_state: None,
        error_message: None,
    };

    console.refresh_from_status();
    if !console.daemon_running {
        console.spawn_daemon();
    }
    console.refresh_from_status();

    loop {
        terminal.draw(|f| draw(f, &console))?;

        if event::poll(POLL_INTERVAL)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('d') => console.toggle_daemon(),
                KeyCode::Char('o') => console.cycle_output_port(),
                KeyCode::Char('m') => console.cycle_mode(),
                KeyCode::Char('y') => console.cycle_sync_port(),
                KeyCode::Char('c') => console.clear_project(),
                _ => {}
            }
        }

        console.refresh_from_status();
    }
}

fn draw(f: &mut Frame, console: &Console) {
    let area = f.area();
    let block = Block::default().borders(Borders::ALL).title(" propeller ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let inner = inner.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(7),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

    let mut status_lines = vec![
        Line::from(format!(
            "daemon: {}",
            if console.daemon_running {
                "running"
            } else {
                "stopped"
            }
        )),
        Line::from(format!("mode:   {}", console.mode.as_str())),
        Line::from(format!("clock:  {}", console.clock_state)),
        Line::from(format!(
            "bpm:    {}",
            console
                .bpm
                .map(|b| b.to_string())
                .unwrap_or_else(|| "-".to_string())
        )),
        Line::from(format!("port:   {}", console.output_port.label())),
    ];
    if console.mode == Mode::Sync {
        status_lines.push(Line::from(format!(
            "sync:   {} ({})",
            console.sync_port.label(),
            console.sync_clock_state.as_deref().unwrap_or("-")
        )));
    } else {
        status_lines.push(Line::from(format!("sync:   {}", console.sync_port.label())));
    }
    f.render_widget(
        Paragraph::new(status_lines).alignment(Alignment::Center),
        rows[1],
    );

    if let Some(msg) = console.error_message.as_deref() {
        f.render_widget(
            Paragraph::new(msg)
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Red)),
            rows[2],
        );
    }

    f.render_widget(
        Paragraph::new(HELP_LINE)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray)),
        rows[4],
    );
}
