// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use std::io;
use std::path::Path;
use std::time::Duration;

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

const POLL_INTERVAL: Duration = Duration::from_millis(200);
const BPM_MIN: i64 = 20;
const BPM_MAX: i64 = 300;

const HELP_LINE: &str =
    "\u{2191}/k bpm+1   \u{2193}/j bpm-1   s start   p pause   r resume   x stop   q/Esc quit";

struct Status {
    state: String,
    bpm: f64,
    port: String,
    tick: u64,
    position: u64,
    spp_enabled: bool,
}

fn fetch_status(sock_path: &Path) -> Result<Status, ClientError> {
    let v = client::send_command(sock_path, serde_json::json!({"command": "status"}))?;
    Ok(Status {
        state: v
            .get("state")
            .and_then(|s| s.as_str())
            .unwrap_or("?")
            .to_string(),
        bpm: v.get("bpm").and_then(|b| b.as_f64()).unwrap_or(0.0),
        port: v
            .get("port")
            .and_then(|p| p.as_str())
            .unwrap_or("?")
            .to_string(),
        tick: v.get("tick").and_then(|t| t.as_u64()).unwrap_or(0),
        position: v.get("position").and_then(|p| p.as_u64()).unwrap_or(0),
        spp_enabled: v
            .get("spp_enabled")
            .and_then(|s| s.as_bool())
            .unwrap_or(false),
    })
}

fn dispatch_action(sock_path: &Path, cmd: serde_json::Value, error_message: &mut Option<String>) {
    match client::send_command(sock_path, cmd) {
        Ok(_) => *error_message = None,
        Err(ClientError::Daemon { message }) => *error_message = Some(message),
        Err(_) => {}
    }
}

fn adjust_bpm(
    sock_path: &Path,
    status: &Option<Status>,
    delta: i64,
    error_message: &mut Option<String>,
) {
    let current = status
        .as_ref()
        .map(|s| s.bpm.round() as i64)
        .unwrap_or(BPM_MIN);
    let new_bpm = (current + delta).clamp(BPM_MIN, BPM_MAX);
    dispatch_action(
        sock_path,
        serde_json::json!({"command": "bpm", "bpm": new_bpm}),
        error_message,
    );
}

/// Runs the interactive TUI against the daemon at `sock_path` until the user quits
/// (q/Esc). Stopping the clock (`x`) and a daemon disconnect both keep the console
/// open. Always restores the terminal before returning, even on error.
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
    let mut status = fetch_status(sock_path).ok();
    let mut error_message: Option<String> = None;
    let mut disconnected = false;

    loop {
        terminal.draw(|f| draw(f, status.as_ref(), error_message.as_deref(), disconnected))?;

        if event::poll(POLL_INTERVAL)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Up | KeyCode::Char('k') => {
                    adjust_bpm(sock_path, &status, 1, &mut error_message);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    adjust_bpm(sock_path, &status, -1, &mut error_message);
                }
                KeyCode::Char('s') => {
                    dispatch_action(
                        sock_path,
                        serde_json::json!({"command": "start"}),
                        &mut error_message,
                    );
                }
                KeyCode::Char('p') => {
                    dispatch_action(
                        sock_path,
                        serde_json::json!({"command": "pause"}),
                        &mut error_message,
                    );
                }
                KeyCode::Char('r') => {
                    dispatch_action(
                        sock_path,
                        serde_json::json!({"command": "resume"}),
                        &mut error_message,
                    );
                }
                KeyCode::Char('x') => {
                    dispatch_action(
                        sock_path,
                        serde_json::json!({"command": "halt"}),
                        &mut error_message,
                    );
                }
                _ => {}
            }
        }

        match fetch_status(sock_path) {
            Ok(s) => {
                status = Some(s);
                disconnected = false;
            }
            Err(ClientError::Connect(_)) => disconnected = true,
            Err(_) => {}
        }
    }
}

fn draw(f: &mut Frame, status: Option<&Status>, error_message: Option<&str>, disconnected: bool) {
    let area = f.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" propeller-clock ");
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
            Constraint::Length(6),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

    let status_lines: Vec<Line> = match status {
        Some(s) => vec![
            Line::from(format!("state: {}", s.state)),
            Line::from(format!("bpm:   {}", s.bpm)),
            Line::from(format!("port:  {}", s.port)),
            Line::from(format!("tick:  {}", s.tick)),
            Line::from(format!("pos:   {}", s.position)),
            Line::from(format!(
                "spp:   {}",
                if s.spp_enabled { "on" } else { "off" }
            )),
        ],
        None => vec![Line::from("connecting...")],
    };
    f.render_widget(
        Paragraph::new(status_lines).alignment(Alignment::Center),
        rows[1],
    );

    if disconnected {
        f.render_widget(
            Paragraph::new("daemon disconnected \u{2014} press q to quit")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Red)),
            rows[2],
        );
    } else if let Some(msg) = error_message {
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
