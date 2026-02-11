use crate::backend::{Backend, Entry};
use crate::config::AppConfig;
use crate::native::auth::NativeAuth;
use crate::native::NativeBackend;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use std::collections::VecDeque;
use std::io;
use std::time::Duration;

const DEFAULT_PATH: &str = "/My Pack";

/// Credentials passed from main: (email, password)
pub type Credentials = (String, String);

pub fn run_with_backend(backend: Box<dyn Backend>) -> Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut terminal = ratatui::init();

    let res = App::new_with_backend(backend).run(&mut terminal);

    ratatui::restore();
    execute!(io::stdout(), LeaveAlternateScreen)?;
    disable_raw_mode()?;

    res
}

pub fn run(credentials: Option<Credentials>) -> Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut terminal = ratatui::init();

    let res = App::new(credentials).run(&mut terminal);

    ratatui::restore();
    execute!(io::stdout(), LeaveAlternateScreen)?;
    disable_raw_mode()?;

    res
}

#[derive(Clone)]
enum LoginField {
    Email,
    Password,
}

struct App {
    backend: Option<Box<dyn Backend>>,
    current_path: String,
    entries: Vec<Entry>,
    selected: usize,
    logs: VecDeque<String>,
    input: InputMode,
}

enum InputMode {
    Login {
        field: LoginField,
        email: String,
        password: String,
        error: Option<String>,
        logging_in: bool,
    },
    None,
    Move { value: String },
    Copy { value: String },
    Rename { value: String },
    ConfirmDelete,
}

impl App {
    fn new(credentials: Option<Credentials>) -> Self {
        let input = match credentials {
            Some((email, password)) => InputMode::Login {
                field: LoginField::Email,
                email,
                password,
                error: None,
                logging_in: true, // auto-login with provided credentials
            },
            None => InputMode::Login {
                field: LoginField::Email,
                email: String::new(),
                password: String::new(),
                error: None,
                logging_in: false,
            },
        };

        Self {
            backend: None,
            current_path: DEFAULT_PATH.to_string(),
            entries: Vec::new(),
            selected: 0,
            logs: VecDeque::new(),
            input,
        }
    }

    fn new_with_backend(backend: Box<dyn Backend>) -> Self {
        let mut app = Self {
            backend: Some(backend),
            current_path: DEFAULT_PATH.to_string(),
            entries: Vec::new(),
            selected: 0,
            logs: VecDeque::new(),
            input: InputMode::None,
        };
        app.refresh();
        app
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        // If logging_in is set, attempt auto-login before first draw
        if let InputMode::Login {
            logging_in: true,
            ref email,
            ref password,
            ..
        } = self.input
        {
            let email = email.clone();
            let password = password.clone();
            self.attempt_login(&email, &password);
        }

        loop {
            terminal.draw(|f| self.draw(f))?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    if self.handle_key(key.code)? {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    fn attempt_login(&mut self, email: &str, password: &str) {
        match NativeAuth::new() {
            Ok(auth) => match auth.login_with_password(email, password) {
                Ok(_token) => {
                    // Save credentials to config.yaml
                    if let Err(e) = AppConfig::save_credentials(email, password) {
                        self.push_log(format!("Warning: failed to save config: {e:#}"));
                    }
                    // Create backend and enter file browser
                    match NativeBackend::new() {
                        Ok(backend) => {
                            self.backend = Some(Box::new(backend));
                            self.input = InputMode::None;
                            self.refresh();
                            self.push_log("Login successful".to_string());
                        }
                        Err(e) => {
                            self.input = InputMode::Login {
                                field: LoginField::Email,
                                email: email.to_string(),
                                password: password.to_string(),
                                error: Some(format!("Backend init failed: {e:#}")),
                                logging_in: false,
                            };
                        }
                    }
                }
                Err(e) => {
                    self.input = InputMode::Login {
                        field: LoginField::Email,
                        email: email.to_string(),
                        password: password.to_string(),
                        error: Some(format!("Login failed: {e:#}")),
                        logging_in: false,
                    };
                }
            },
            Err(e) => {
                self.input = InputMode::Login {
                    field: LoginField::Email,
                    email: email.to_string(),
                    password: password.to_string(),
                    error: Some(format!("Auth init failed: {e:#}")),
                    logging_in: false,
                };
            }
        }
    }

    fn draw(&self, f: &mut Frame) {
        match &self.input {
            InputMode::Login { .. } => self.draw_login_screen(f),
            _ => self.draw_main(f),
        }
    }

    fn draw_login_screen(&self, f: &mut Frame) {
        // Fill background
        let bg = Block::default()
            .style(Style::default().bg(Color::Black));
        f.render_widget(bg, f.area());

        let area = centered_rect(50, 40, f.area());

        if let InputMode::Login {
            field,
            email,
            password,
            error,
            logging_in,
        } = &self.input
        {
            f.render_widget(Clear, area);

            let email_style = match field {
                LoginField::Email => Style::default().fg(Color::Yellow),
                LoginField::Password => Style::default().fg(Color::White),
            };
            let pass_style = match field {
                LoginField::Password => Style::default().fg(Color::Yellow),
                LoginField::Email => Style::default().fg(Color::White),
            };

            let masked_password: String = "*".repeat(password.len());

            let email_cursor = match field {
                LoginField::Email => "_",
                _ => "",
            };
            let pass_cursor = match field {
                LoginField::Password => "_",
                _ => "",
            };

            let mut lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Email:    ", email_style),
                    Span::styled(format!("{}{}", email, email_cursor), email_style),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Password: ", pass_style),
                    Span::styled(format!("{}{}", masked_password, pass_cursor), pass_style),
                ]),
                Line::from(""),
            ];

            if *logging_in {
                lines.push(Line::from(Span::styled(
                    "  Logging in...",
                    Style::default().fg(Color::Cyan),
                )));
            } else if let Some(err) = error {
                lines.push(Line::from(Span::styled(
                    format!("  {}", err),
                    Style::default().fg(Color::Red),
                )));
                lines.push(Line::from(""));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  Tab: switch field | Enter: login | Esc: quit",
                Style::default().fg(Color::DarkGray),
            )));

            let p = Paragraph::new(Text::from(lines))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" PikPak Login ")
                        .border_style(Style::default().fg(Color::Cyan)),
                )
                .wrap(Wrap { trim: false });

            f.render_widget(p, area);
        }
    }

    fn draw_main(&self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(f.area());

        let left_title = format!("Path: {}", self.current_path);
        let items: Vec<ListItem> = self
            .entries
            .iter()
            .map(|e| {
                let kind = if e.size == 0 { "[D]" } else { "[F]" };
                ListItem::new(format!("{} {} ({})", kind, e.name, e.size))
            })
            .collect();

        let mut state = ListState::default();
        if !self.entries.is_empty() {
            state.select(Some(self.selected.min(self.entries.len() - 1)));
        }

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(left_title))
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        f.render_stateful_widget(list, chunks[0], &mut state);

        let log_lines: Vec<Line> = self
            .logs
            .iter()
            .rev()
            .take((chunks[1].height.saturating_sub(2)) as usize)
            .rev()
            .map(|s| Line::from(s.as_str()))
            .collect();

        let help = "j/k, ↑/↓: move | Enter: open dir | Backspace: up | r: refresh | c: copy | m: move | n: rename | d: remove | q: quit";
        let mut lines = log_lines;
        lines.push(Line::from(""));
        lines.push(Line::from(help));
        let logs = Paragraph::new(Text::from(lines))
            .block(Block::default().borders(Borders::ALL).title("Logs"))
            .wrap(Wrap { trim: false });

        f.render_widget(logs, chunks[1]);

        self.draw_overlay(f);
    }

    fn draw_overlay(&self, f: &mut Frame) {
        let area = centered_rect(60, 20, f.area());

        match &self.input {
            InputMode::None | InputMode::Login { .. } => {}
            InputMode::Move { value } => {
                f.render_widget(Clear, area);
                let p = Paragraph::new(format!(
                    "Move target path:\n{}\n\nEnter to confirm, Esc to cancel",
                    value
                ))
                .block(Block::default().borders(Borders::ALL).title("Move"));
                f.render_widget(p, area);
            }
            InputMode::Copy { value } => {
                f.render_widget(Clear, area);
                let p = Paragraph::new(format!(
                    "Copy target path:\n{}\n\nEnter to confirm, Esc to cancel",
                    value
                ))
                .block(Block::default().borders(Borders::ALL).title("Copy"));
                f.render_widget(p, area);
            }
            InputMode::Rename { value } => {
                f.render_widget(Clear, area);
                let p = Paragraph::new(format!(
                    "New name:\n{}\n\nEnter to confirm, Esc to cancel",
                    value
                ))
                .block(Block::default().borders(Borders::ALL).title("Rename"));
                f.render_widget(p, area);
            }
            InputMode::ConfirmDelete => {
                f.render_widget(Clear, area);
                let name = self
                    .current_entry()
                    .map(|e| e.name.as_str())
                    .unwrap_or("<none>");
                let p = Paragraph::new(format!(
                    "Delete `{}` to trash?\n\nPress y to confirm, n/Esc to cancel",
                    name
                ))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Confirm Remove"),
                );
                f.render_widget(p, area);
            }
        }
    }

    fn handle_key(&mut self, code: KeyCode) -> Result<bool> {
        let mode = std::mem::replace(&mut self.input, InputMode::None);
        match mode {
            InputMode::Login {
                mut field,
                mut email,
                mut password,
                logging_in,
                ..
            } => {
                if logging_in {
                    // Ignore keys while logging in
                    self.input = InputMode::Login {
                        field,
                        email,
                        password,
                        error: None,
                        logging_in: true,
                    };
                    return Ok(false);
                }
                match code {
                    KeyCode::Esc => return Ok(true),
                    KeyCode::Tab | KeyCode::BackTab => {
                        field = match field {
                            LoginField::Email => LoginField::Password,
                            LoginField::Password => LoginField::Email,
                        };
                        self.input = InputMode::Login {
                            field,
                            email,
                            password,
                            error: None,
                            logging_in: false,
                        };
                    }
                    KeyCode::Enter => {
                        let e = email.clone();
                        let p = password.clone();
                        if e.trim().is_empty() || p.is_empty() {
                            self.input = InputMode::Login {
                                field,
                                email,
                                password,
                                error: Some("Email and password are required".to_string()),
                                logging_in: false,
                            };
                        } else {
                            self.input = InputMode::Login {
                                field,
                                email: e.clone(),
                                password: p.clone(),
                                error: None,
                                logging_in: true,
                            };
                            self.attempt_login(&e, &p);
                        }
                    }
                    KeyCode::Backspace => {
                        match field {
                            LoginField::Email => { email.pop(); }
                            LoginField::Password => { password.pop(); }
                        }
                        self.input = InputMode::Login {
                            field,
                            email,
                            password,
                            error: None,
                            logging_in: false,
                        };
                    }
                    KeyCode::Char(c) => {
                        match field {
                            LoginField::Email => email.push(c),
                            LoginField::Password => password.push(c),
                        }
                        self.input = InputMode::Login {
                            field,
                            email,
                            password,
                            error: None,
                            logging_in: false,
                        };
                    }
                    _ => {
                        self.input = InputMode::Login {
                            field,
                            email,
                            password,
                            error: None,
                            logging_in: false,
                        };
                    }
                }
                Ok(false)
            }
            InputMode::None => self.handle_normal_key(code),
            InputMode::Move { mut value } => {
                if let Some(done) = handle_text_input(&mut value, code) {
                    if done {
                        if let Some(entry) = self.current_entry().cloned() {
                            let target = value.trim().to_string();
                            if !target.is_empty() {
                                if let Some(ref backend) = self.backend {
                                    match backend.mv(&self.current_path, &entry.name, &target) {
                                        Ok(_) => self.push_log(format!(
                                            "Moved '{}' -> '{}'",
                                            entry.name, target
                                        )),
                                        Err(e) => self.push_log(format!("Move failed: {e:#}")),
                                    }
                                }
                                self.refresh();
                            }
                        }
                    }
                } else {
                    self.input = InputMode::Move { value };
                }
                Ok(false)
            }
            InputMode::Copy { mut value } => {
                if let Some(done) = handle_text_input(&mut value, code) {
                    if done {
                        if let Some(entry) = self.current_entry().cloned() {
                            let target = value.trim().to_string();
                            if !target.is_empty() {
                                if let Some(ref backend) = self.backend {
                                    match backend.cp(&self.current_path, &entry.name, &target) {
                                        Ok(_) => self.push_log(format!(
                                            "Copied '{}' -> '{}'",
                                            entry.name, target
                                        )),
                                        Err(e) => self.push_log(format!("Copy failed: {e:#}")),
                                    }
                                }
                                self.refresh();
                            }
                        }
                    }
                } else {
                    self.input = InputMode::Copy { value };
                }
                Ok(false)
            }
            InputMode::Rename { mut value } => {
                if let Some(done) = handle_text_input(&mut value, code) {
                    if done {
                        if let Some(entry) = self.current_entry().cloned() {
                            let new_name = value.trim().to_string();
                            if !new_name.is_empty() {
                                if let Some(ref backend) = self.backend {
                                    match backend.rename(
                                        &self.current_path,
                                        &entry.name,
                                        &new_name,
                                    ) {
                                        Ok(_) => self.push_log(format!(
                                            "Renamed '{}' -> '{}'",
                                            entry.name, new_name
                                        )),
                                        Err(e) => self.push_log(format!("Rename failed: {e:#}")),
                                    }
                                }
                                self.refresh();
                            }
                        }
                    }
                } else {
                    self.input = InputMode::Rename { value };
                }
                Ok(false)
            }
            InputMode::ConfirmDelete => {
                match code {
                    KeyCode::Char('y') => {
                        if let Some(entry) = self.current_entry().cloned() {
                            if let Some(ref backend) = self.backend {
                                match backend.remove(&self.current_path, &entry.name) {
                                    Ok(_) => {
                                        self.push_log(format!(
                                            "Removed '{}' (to trash)",
                                            entry.name
                                        ))
                                    }
                                    Err(e) => self.push_log(format!("Remove failed: {e:#}")),
                                }
                            }
                            self.refresh();
                        }
                    }
                    KeyCode::Char('n') | KeyCode::Esc => {
                        self.push_log("Remove cancelled".to_string());
                    }
                    _ => {
                        self.input = InputMode::ConfirmDelete;
                    }
                }
                Ok(false)
            }
        }
    }

    fn handle_normal_key(&mut self, code: KeyCode) -> Result<bool> {
        match code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.entries.is_empty() {
                    self.selected = (self.selected + 1).min(self.entries.len() - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            KeyCode::Enter => {
                if let Some(entry) = self.current_entry() {
                    if entry.size == 0 {
                        self.current_path = join_path(&self.current_path, &entry.name);
                        self.selected = 0;
                        self.refresh();
                    }
                }
            }
            KeyCode::Backspace => {
                self.current_path = parent_path(&self.current_path);
                self.selected = 0;
                self.refresh();
            }
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('c') => {
                if self.current_entry().is_some() {
                    self.input = InputMode::Copy {
                        value: String::new(),
                    };
                }
            }
            KeyCode::Char('m') => {
                if self.current_entry().is_some() {
                    self.input = InputMode::Move {
                        value: String::new(),
                    };
                }
            }
            KeyCode::Char('n') => {
                if self.current_entry().is_some() {
                    self.input = InputMode::Rename {
                        value: String::new(),
                    };
                }
            }
            KeyCode::Char('d') => {
                if self.current_entry().is_some() {
                    self.input = InputMode::ConfirmDelete;
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn current_entry(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
    }

    fn push_log(&mut self, msg: String) {
        self.logs.push_back(msg);
        if self.logs.len() > 500 {
            self.logs.pop_front();
        }
    }

    fn refresh(&mut self) {
        if let Some(ref backend) = self.backend {
            match backend.ls(&self.current_path) {
                Ok(entries) => {
                    self.entries = entries;
                    if self.selected >= self.entries.len() {
                        self.selected = self.entries.len().saturating_sub(1);
                    }
                    self.push_log(format!("Refreshed {}", self.current_path));
                }
                Err(e) => {
                    self.push_log(format!("Refresh failed: {e:#}"));
                }
            }
        }
    }
}

fn handle_text_input(value: &mut String, code: KeyCode) -> Option<bool> {
    match code {
        KeyCode::Esc => Some(false),
        KeyCode::Enter => Some(true),
        KeyCode::Backspace => {
            value.pop();
            None
        }
        KeyCode::Char(c) => {
            value.push(c);
            None
        }
        _ => None,
    }
}

fn parent_path(path: &str) -> String {
    if path == "/" || path.is_empty() {
        return "/".to_string();
    }
    let trimmed = path.trim_end_matches('/');
    if let Some((head, _)) = trimmed.rsplit_once('/') {
        if head.is_empty() {
            "/".to_string()
        } else {
            head.to_string()
        }
    } else {
        "/".to_string()
    }
}

fn join_path(base: &str, name: &str) -> String {
    if base == "/" {
        format!("/{}", name)
    } else {
        format!("{}/{}", base.trim_end_matches('/'), name)
    }
}

fn centered_rect(
    percent_x: u16,
    percent_y: u16,
    area: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
