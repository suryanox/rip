use std::io::{self, stdout};
use std::process::Command;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

#[derive(Clone, Debug)]
struct PortProcess {
    pid: u32,
    port: u16,
    protocol: String,
    name: String,
}

struct App {
    processes: Vec<PortProcess>,
    list_state: ListState,
    message: Option<String>,
    should_quit: bool,
}

impl App {
    fn new() -> Self {
        let mut app = App {
            processes: Vec::new(),
            list_state: ListState::default(),
            message: None,
            should_quit: false,
        };
        app.refresh_processes();
        if !app.processes.is_empty() {
            app.list_state.select(Some(0));
        }
        app
    }

    fn refresh_processes(&mut self) {
        self.processes = get_port_processes();
        self.message = Some(format!("Found {} processes", self.processes.len()));

        if self.processes.is_empty() {
            self.list_state.select(None);
        } else if let Some(selected) = self.list_state.selected() {
            if selected >= self.processes.len() {
                self.list_state.select(Some(self.processes.len() - 1));
            }
        } else {
            self.list_state.select(Some(0));
        }
    }

    fn next(&mut self) {
        if self.processes.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.processes.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn previous(&mut self) {
        if self.processes.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.processes.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn kill_selected(&mut self) {
        if let Some(selected) = self.list_state.selected() {
            if let Some(process) = self.processes.get(selected) {
                let pid = process.pid;
                let name = process.name.clone();

                match kill_process(pid) {
                    Ok(_) => {
                        self.message = Some(format!("Killed process {} (PID: {})", name, pid));
                        self.refresh_processes();
                    }
                    Err(e) => {
                        self.message = Some(format!("Failed to kill PID {}: {}", pid, e));
                    }
                }
            }
        }
    }
}

fn get_port_processes() -> Vec<PortProcess> {
    let output = Command::new("lsof")
        .args(["-iTCP", "-iUDP", "-sTCP:LISTEN", "-P", "-n"])
        .output();

    let output = match output {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut processes = Vec::new();
    let mut seen_pids: std::collections::HashSet<u32> = std::collections::HashSet::new();

    for line in stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 9 {
            continue;
        }

        let name = parts[0].to_string();
        let pid: u32 = match parts[1].parse() {
            Ok(p) => p,
            Err(_) => continue,
        };

        if seen_pids.contains(&pid) {
            continue;
        }

        let protocol = if parts[4].contains("TCP") || parts[7].contains("TCP") {
            "TCP".to_string()
        } else if parts[4].contains("UDP") || parts[7].contains("UDP") {
            "UDP".to_string()
        } else {
            "???".to_string()
        };

        let addr_field = parts[8];
        let port: u16 = if let Some(port_str) = addr_field.rsplit(':').next() {
            port_str.parse().unwrap_or(0)
        } else {
            0
        };

        if port > 0 {
            seen_pids.insert(pid);
            processes.push(PortProcess {
                pid,
                port,
                protocol,
                name,
            });
        }
    }

    processes.sort_by_key(|p| p.port);
    processes
}

fn kill_process(pid: u32) -> io::Result<()> {
    let status = Command::new("kill")
        .arg("-9")
        .arg(pid.to_string())
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("kill command failed with status: {}", status),
        ))
    }
}

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let mut app = App::new();

    loop {
        terminal.draw(|frame| ui(frame, &mut app))?;
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                        KeyCode::Down | KeyCode::Char('j') => app.next(),
                        KeyCode::Up | KeyCode::Char('k') => app.previous(),
                        KeyCode::Enter | KeyCode::Char('d') => app.kill_selected(),
                        KeyCode::Char('r') => app.refresh_processes(),
                        _ => {}
                    }
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

fn ui(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let title = Paragraph::new("rip - Kill processes on ports")
        .style(Style::default().fg(Color::Cyan).bold())
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(title, chunks[0]);

    let items: Vec<ListItem> = app
        .processes
        .iter()
        .map(|p| {
            let content = format!(
                ":{:<6} {:4} {:>6}  {}",
                p.port, p.protocol, p.pid, p.name
            );
            ListItem::new(content)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title("Processes (PORT | PROTO | PID | NAME)")
                .borders(Borders::ALL),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .fg(Color::White)
                .bold(),
        )
        .highlight_symbol(">> ");

    frame.render_stateful_widget(list, chunks[1], &mut app.list_state);

    let help_text = match &app.message {
        Some(msg) => format!("{} | ↑/↓:Navigate  Enter/d:Kill  r:Refresh  q:Quit", msg),
        None => "↑/↓:Navigate  Enter/d:Kill  r:Refresh  q:Quit".to_string(),
    };

    let status = Paragraph::new(help_text)
        .style(Style::default().fg(Color::Yellow))
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(status, chunks[2]);
}
