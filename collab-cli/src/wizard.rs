use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, Paragraph},
    Frame, Terminal,
};
use std::io;

use crate::init::{ProjectConfig, WorkerConfig};

const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;

// ── State ─────────────────────────────────────────────────────────────────────

#[derive(PartialEq, Clone, Copy)]
enum Step {
    Welcome,
    Config,
    Workers,
    Review,
}

#[derive(PartialEq, Clone, Copy)]
enum Field {
    Server,
    OutputDir,
    WorkerName,
    WorkerRole,
}

struct WizardState {
    step: Step,
    server: String,
    output_dir: String,
    workers: Vec<WorkerConfig>,
    name_buf: String,
    role_buf: String,
    active: Field,
    error: Option<String>,
    quit: bool,
    done: bool,
}

impl WizardState {
    fn new() -> Self {
        Self {
            step: Step::Welcome,
            server: "http://localhost:8000".to_string(),
            output_dir: ".".to_string(),
            workers: Vec::new(),
            name_buf: String::new(),
            role_buf: String::new(),
            active: Field::Server,
            error: None,
            quit: false,
            done: false,
        }
    }

    fn active_buf(&mut self) -> &mut String {
        match self.active {
            Field::Server => &mut self.server,
            Field::OutputDir => &mut self.output_dir,
            Field::WorkerName => &mut self.name_buf,
            Field::WorkerRole => &mut self.role_buf,
        }
    }

    fn handle(&mut self, code: KeyCode, mods: KeyModifiers) {
        self.error = None;
        let ctrl = mods.contains(KeyModifiers::CONTROL);

        match self.step {
            Step::Welcome => match code {
                KeyCode::Enter | KeyCode::Char(' ') => {
                    self.step = Step::Config;
                    self.active = Field::Server;
                }
                KeyCode::Esc | KeyCode::Char('q') => self.quit = true,
                _ => {}
            },

            Step::Config => match code {
                KeyCode::Tab => {
                    self.active = match self.active {
                        Field::Server => Field::OutputDir,
                        _ => Field::Server,
                    };
                }
                KeyCode::Enter => {
                    if self.server.trim().is_empty() {
                        self.server = "http://localhost:8000".to_string();
                    }
                    if self.output_dir.trim().is_empty() {
                        self.output_dir = ".".to_string();
                    }
                    self.step = Step::Workers;
                    self.active = Field::WorkerName;
                }
                KeyCode::Esc => {
                    self.step = Step::Welcome;
                }
                KeyCode::Backspace => {
                    self.active_buf().pop();
                }
                KeyCode::Char(c) if !ctrl => {
                    self.active_buf().push(c);
                }
                _ => {}
            },

            Step::Workers => match code {
                KeyCode::Tab => {
                    self.active = match self.active {
                        Field::WorkerName => Field::WorkerRole,
                        _ => Field::WorkerName,
                    };
                }
                KeyCode::Enter => {
                    let name = self.name_buf.trim().to_string();
                    let role = self.role_buf.trim().to_string();

                    if name.is_empty() && role.is_empty() {
                        if self.workers.is_empty() {
                            self.error = Some("Add at least one worker before continuing.".into());
                        } else {
                            self.step = Step::Review;
                        }
                        return;
                    }
                    if name.is_empty() {
                        self.error = Some("Worker name is required.".into());
                        self.active = Field::WorkerName;
                        return;
                    }
                    if role.is_empty() {
                        self.active = Field::WorkerRole;
                        return;
                    }
                    if self.workers.iter().any(|w| w.name == name) {
                        self.error = Some(format!("'{}' is already in the list.", name));
                        return;
                    }
                    self.workers.push(WorkerConfig { name, role, tasks: None, avatar: None, color: None });
                    self.name_buf.clear();
                    self.role_buf.clear();
                    self.active = Field::WorkerName;
                }
                KeyCode::Esc => {
                    self.step = Step::Config;
                    self.active = Field::Server;
                }
                KeyCode::Backspace => {
                    self.active_buf().pop();
                }
                KeyCode::Char(c) if !ctrl => {
                    self.active_buf().push(c);
                }
                _ => {}
            },

            Step::Review => match code {
                KeyCode::Enter | KeyCode::Char('y') => self.done = true,
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('n') => {
                    self.step = Step::Workers;
                    self.active = Field::WorkerName;
                }
                _ => {}
            },
        }
    }

    fn to_config(&self) -> ProjectConfig {
        let output_dir = if self.output_dir == "." || self.output_dir.is_empty() {
            None
        } else {
            Some(self.output_dir.clone())
        };
        ProjectConfig::new(self.server.clone(), output_dir, self.workers.clone())
    }
}

// ── Layout helpers ────────────────────────────────────────────────────────────

fn centered_box(width_pct: u16, height: u16, area: Rect) -> Rect {
    let vert = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .split(area);

    let side = (100u16.saturating_sub(width_pct)) / 2;
    Layout::horizontal([
        Constraint::Percentage(side),
        Constraint::Percentage(width_pct),
        Constraint::Percentage(side),
    ])
    .split(vert[1])[1]
}

fn field_block<'a>(title: &'a str, active: bool) -> Block<'a> {
    let style = if active {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    Block::bordered()
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(Color::White),
        ))
        .border_style(style)
}

// ── Render ────────────────────────────────────────────────────────────────────

fn draw(f: &mut Frame, state: &WizardState) {
    let full = f.area();
    f.render_widget(Block::new().style(Style::default().bg(Color::Black)), full);
    match state.step {
        Step::Welcome => draw_welcome(f, state, full),
        Step::Config => draw_config(f, state, full),
        Step::Workers => draw_workers(f, state, full),
        Step::Review => draw_review(f, state, full),
    }
}

fn draw_welcome(f: &mut Frame, _: &WizardState, area: Rect) {
    let b = centered_box(56, 12, area);
    let block = Block::bordered()
        .title(Span::styled(
            " collab init ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(b);
    f.render_widget(block, b);

    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Worker Environment Wizard",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Creates a CLAUDE.md for each worker:",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            "    · Identity and role",
            Style::default().fg(DIM),
        )),
        Line::from(Span::styled(
            "    · Full collab messaging instructions",
            Style::default().fg(DIM),
        )),
        Line::from(Span::styled(
            "    · Team roster and operating rules",
            Style::default().fg(DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Enter to start  ·  q to quit",
            Style::default().fg(DIM),
        )),
    ];
    f.render_widget(Paragraph::new(text), inner);
}

fn draw_config(f: &mut Frame, state: &WizardState, area: Rect) {
    let b = centered_box(62, 16, area);
    let block = Block::bordered()
        .title(Span::styled(
            " collab init — Configuration ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(b);
    f.render_widget(block, b);

    let chunks = Layout::vertical([
        Constraint::Length(1), // label
        Constraint::Length(1), // gap
        Constraint::Length(3), // server field
        Constraint::Length(1), // gap
        Constraint::Length(3), // output dir field
        Constraint::Fill(1),
        Constraint::Length(1), // hint
    ])
    .split(inner);

    f.render_widget(
        Paragraph::new(Span::styled(
            "  Collab server URL and output directory:",
            Style::default().fg(Color::Gray),
        )),
        chunks[0],
    );

    let server_active = state.active == Field::Server;
    f.render_widget(
        Paragraph::new(format!(" {}", state.server))
            .block(field_block("Server URL", server_active)),
        chunks[2],
    );

    let dir_active = state.active == Field::OutputDir;
    f.render_widget(
        Paragraph::new(format!(" {}", state.output_dir))
            .block(field_block("Output Directory", dir_active)),
        chunks[4],
    );

    f.render_widget(
        Paragraph::new(Span::styled(
            "  Tab switch  ·  Enter continue  ·  Esc back",
            Style::default().fg(DIM),
        )),
        chunks[6],
    );

    render_error(f, state, b, area);
}

fn draw_workers(f: &mut Frame, state: &WizardState, area: Rect) {
    let list_h = (state.workers.len() as u16 + 2).clamp(3, 8);
    let total_h = list_h + 1 + 3 + 1 + 3 + 1 + 1 + 2; // list + gaps + fields + hint + border
    let b = centered_box(68, total_h.min(area.height.saturating_sub(2)), area);

    let block = Block::bordered()
        .title(Span::styled(
            " collab init — Workers ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(b);
    f.render_widget(block, b);

    let chunks = Layout::vertical([
        Constraint::Length(list_h),
        Constraint::Length(1),
        Constraint::Length(3), // name
        Constraint::Length(1),
        Constraint::Length(3), // role
        Constraint::Fill(1),
        Constraint::Length(1), // hint
    ])
    .split(inner);

    // Worker list
    if state.workers.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled("  No workers yet.", Style::default().fg(DIM))),
            chunks[0],
        );
    } else {
        let items: Vec<ListItem> = state
            .workers
            .iter()
            .map(|w| {
                ListItem::new(Line::from(vec![
                    Span::styled("  • ", Style::default().fg(ACCENT)),
                    Span::styled(
                        w.name.clone(),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" — ", Style::default().fg(DIM)),
                    Span::styled(w.role.clone(), Style::default().fg(Color::Gray)),
                ]))
            })
            .collect();
        f.render_widget(List::new(items), chunks[0]);
    }

    let name_active = state.active == Field::WorkerName;
    f.render_widget(
        Paragraph::new(format!(" {}", state.name_buf))
            .block(field_block("Name", name_active)),
        chunks[2],
    );

    let role_active = state.active == Field::WorkerRole;
    f.render_widget(
        Paragraph::new(format!(" {}", state.role_buf))
            .block(field_block("Role / Task Description", role_active)),
        chunks[4],
    );

    let hint = if state.workers.is_empty() {
        "  Tab switch  ·  Enter add worker  ·  Esc back"
    } else {
        "  Tab switch  ·  Enter add  ·  Empty Enter → review  ·  Esc back"
    };
    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(DIM))),
        chunks[6],
    );

    render_error(f, state, b, area);
}

fn draw_review(f: &mut Frame, state: &WizardState, area: Rect) {
    let content_h = (state.workers.len() as u16 * 2 + 12).min(area.height.saturating_sub(4));
    let b = centered_box(62, content_h, area);

    let block = Block::bordered()
        .title(Span::styled(
            " collab init — Review ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(b);
    f.render_widget(block, b);

    let mut lines = vec![
        Line::from(Span::styled(
            "  Ready to generate:",
            Style::default().fg(Color::Gray),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Server:  ", Style::default().fg(DIM)),
            Span::styled(state.server.clone(), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("  Output:  ", Style::default().fg(DIM)),
            Span::styled(state.output_dir.clone(), Style::default().fg(Color::White)),
        ]),
        Line::from(""),
        Line::from(Span::styled("  Workers:", Style::default().fg(DIM))),
    ];

    for w in &state.workers {
        lines.push(Line::from(vec![
            Span::styled("    • ", Style::default().fg(ACCENT)),
            Span::styled(
                w.name.clone(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" — ", Style::default().fg(DIM)),
            Span::styled(w.role.clone(), Style::default().fg(Color::Gray)),
        ]));
        lines.push(Line::from(Span::styled(
            format!("      → {}/{}/CLAUDE.md", state.output_dir, w.name),
            Style::default().fg(DIM),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Enter to generate  ·  Esc to go back",
        Style::default().fg(DIM),
    )));

    f.render_widget(Paragraph::new(lines), inner);
}

fn render_error(f: &mut Frame, state: &WizardState, b: Rect, area: Rect) {
    if let Some(err) = &state.error {
        let y = b.y.saturating_add(b.height);
        if y < area.height {
            let err_area = Rect { y, height: 1, x: b.x, width: b.width };
            f.render_widget(
                Paragraph::new(Span::styled(
                    format!("  ⚠  {}", err),
                    Style::default().fg(Color::Red),
                )),
                err_area,
            );
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn run() -> Result<Option<ProjectConfig>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = WizardState::new();

    let result = loop {
        terminal.draw(|f| draw(f, &state))?;

        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                state.handle(key.code, key.modifiers);
            }
        }

        if state.quit {
            break Ok(None);
        }
        if state.done {
            break Ok(Some(state.to_config()));
        }
    };

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}
