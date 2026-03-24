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
    memory_kb: u64,
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

fn get_memory_for_pid(pid: u32) -> u64 {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output();

    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout.trim().parse().unwrap_or(0)
        }
        Err(_) => 0,
    }
}

fn format_memory(kb: u64) -> String {
    if kb >= 1_048_576 {
        format!("{:.1}G", kb as f64 / 1_048_576.0)
    } else if kb >= 1024 {
        format!("{:.1}M", kb as f64 / 1024.0)
    } else {
        format!("{}K", kb)
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
            let memory_kb = get_memory_for_pid(pid);
            processes.push(PortProcess {
                pid,
                port,
                protocol,
                name,
                memory_kb,
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
    let area = frame.area();
    
    frame.render_widget(
        Block::default().style(Style::default().bg(Color::Rgb(15, 15, 25))),
        area,
    );

    let outer_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    let main_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(outer_layout[1])[1];

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(1),
            Constraint::Length(3),
        ])
        .split(main_area);

    let title_block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(138, 43, 226)))
        .style(Style::default().bg(Color::Rgb(20, 20, 35)));

    let title_text = vec![
        Line::from(vec![
            Span::styled("⚡ ", Style::default().fg(Color::Rgb(255, 165, 0))),
            Span::styled("rip", Style::default().fg(Color::Rgb(255, 100, 100)).bold()),
            Span::styled(" — ", Style::default().fg(Color::Rgb(100, 100, 120))),
            Span::styled("Process Killer", Style::default().fg(Color::Rgb(200, 200, 220))),
        ]),
        Line::from(vec![
            Span::styled("   Kill processes hogging your ports", Style::default().fg(Color::Rgb(120, 120, 140)).italic()),
        ]),
    ];

    let title = Paragraph::new(title_text)
        .block(title_block)
        .alignment(Alignment::Center);
    frame.render_widget(title, chunks[0]);

    let header = Line::from(vec![
        Span::styled("   ", Style::default()),
        Span::styled("PORT", Style::default().fg(Color::Rgb(100, 200, 255)).bold()),
        Span::styled("      ", Style::default()),
        Span::styled("PROTO", Style::default().fg(Color::Rgb(100, 200, 255)).bold()),
        Span::styled("   ", Style::default()),
        Span::styled("PID", Style::default().fg(Color::Rgb(100, 200, 255)).bold()),
        Span::styled("       ", Style::default()),
        Span::styled("MEMORY", Style::default().fg(Color::Rgb(100, 200, 255)).bold()),
        Span::styled("       ", Style::default()),
        Span::styled("NAME", Style::default().fg(Color::Rgb(100, 200, 255)).bold()),
    ]);
    
    let header_widget = Paragraph::new(header)
        .style(Style::default().bg(Color::Rgb(30, 30, 50)));
    frame.render_widget(header_widget, chunks[1]);

    let items: Vec<ListItem> = app
        .processes
        .iter()
        .enumerate()
        .map(|(idx, p)| {
            let is_selected = app.list_state.selected() == Some(idx);
            
            let port_color = if p.port < 1024 {
                Color::Rgb(255, 100, 100)
            } else if p.port < 10000 {
                Color::Rgb(255, 200, 100)
            } else {
                Color::Rgb(100, 255, 150)
            };

            let proto_color = if p.protocol == "TCP" {
                Color::Rgb(100, 200, 255)
            } else {
                Color::Rgb(200, 150, 255)
            };

            let mem_color = if p.memory_kb >= 1_048_576 {
                Color::Rgb(255, 100, 100)
            } else if p.memory_kb >= 524_288 {
                Color::Rgb(255, 200, 100)
            } else if p.memory_kb >= 102_400 {
                Color::Rgb(255, 255, 100)
            } else {
                Color::Rgb(100, 255, 150)
            };

            let line = Line::from(vec![
                Span::styled(
                    format!(":{:<5}", p.port),
                    Style::default().fg(port_color).bold(),
                ),
                Span::styled("   ", Style::default()),
                Span::styled(
                    format!("{:5}", p.protocol),
                    Style::default().fg(proto_color),
                ),
                Span::styled("   ", Style::default()),
                Span::styled(
                    format!("{:>7}", p.pid),
                    Style::default().fg(Color::Rgb(180, 180, 200)),
                ),
                Span::styled("   ", Style::default()),
                Span::styled(
                    format!("{:>7}", format_memory(p.memory_kb)),
                    Style::default().fg(mem_color),
                ),
                Span::styled("   ", Style::default()),
                Span::styled(
                    p.name.clone(),
                    Style::default().fg(if is_selected {
                        Color::White
                    } else {
                        Color::Rgb(220, 220, 240)
                    }),
                ),
            ]);

            ListItem::new(line)
        })
        .collect();

    let process_count = app.processes.len();
    let selected_info = match app.list_state.selected() {
        Some(i) => format!(" {}/{} ", i + 1, process_count),
        None => format!(" {} ", process_count),
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(Line::from(vec![
                    Span::styled(" ", Style::default()),
                    Span::styled("●", Style::default().fg(Color::Rgb(100, 255, 150))),
                    Span::styled(" Listening Processes ", Style::default().fg(Color::Rgb(200, 200, 220)).bold()),
                ]))
                .title_bottom(Line::from(vec![
                    Span::styled(selected_info, Style::default().fg(Color::Rgb(150, 150, 170))),
                ]).alignment(Alignment::Right))
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(Style::default().fg(Color::Rgb(70, 70, 100)))
                .style(Style::default().bg(Color::Rgb(20, 20, 35))),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(60, 60, 100))
                .fg(Color::White)
                .bold(),
        )
        .highlight_symbol(" ▸ ");

    frame.render_stateful_widget(list, chunks[2], &mut app.list_state);

    let status_block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(70, 70, 100)))
        .style(Style::default().bg(Color::Rgb(20, 20, 35)));

    let (msg_text, msg_style) = match &app.message {
        Some(msg) if msg.contains("Killed") => (
            msg.clone(),
            Style::default().fg(Color::Rgb(100, 255, 150)),
        ),
        Some(msg) if msg.contains("Failed") => (
            msg.clone(),
            Style::default().fg(Color::Rgb(255, 100, 100)),
        ),
        Some(msg) => (
            msg.clone(),
            Style::default().fg(Color::Rgb(180, 180, 200)),
        ),
        None => (String::new(), Style::default()),
    };

    let keybinds = vec![
        ("↑↓", "navigate"),
        ("d", "kill"),
        ("r", "refresh"),
        ("q", "quit"),
    ];

    let mut help_spans = Vec::new();
    if !msg_text.is_empty() {
        help_spans.push(Span::styled(msg_text, msg_style));
        help_spans.push(Span::styled("  │  ", Style::default().fg(Color::Rgb(70, 70, 100))));
    }

    for (i, (key, action)) in keybinds.iter().enumerate() {
        help_spans.push(Span::styled(
            format!(" {} ", key),
            Style::default()
                .fg(Color::Rgb(30, 30, 50))
                .bg(Color::Rgb(138, 43, 226))
                .bold(),
        ));
        help_spans.push(Span::styled(
            format!(" {} ", action),
            Style::default().fg(Color::Rgb(150, 150, 170)),
        ));
        if i < keybinds.len() - 1 {
            help_spans.push(Span::styled(" ", Style::default()));
        }
    }

    let help_line = Line::from(help_spans);
    let status = Paragraph::new(help_line)
        .block(status_block)
        .alignment(Alignment::Center);
    frame.render_widget(status, chunks[4]);
}
