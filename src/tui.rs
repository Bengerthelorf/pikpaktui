use crate::config::AppConfig;
use crate::pikpak::{Entry, EntryKind, PikPak};
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

/// Credentials: (email, password)
pub type Credentials = (String, String);

/// Start TUI with a client that already has a valid session.
pub fn run(client: PikPak) -> Result<()> {
    run_terminal(App::new_authed(client))
}

/// Start TUI with optional credentials for auto-login.
pub fn run_with_credentials(client: PikPak, credentials: Option<Credentials>) -> Result<()> {
    run_terminal(App::new_login(client, credentials))
}

fn run_terminal(mut app: App) -> Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut terminal = ratatui::init();

    let res = app.run(&mut terminal);

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
    client: PikPak,
    current_folder_id: String,             // "" = root
    breadcrumb: Vec<(String, String)>,     // [(id, name), ...]
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
    Normal,
    Move { value: String },
    Copy { value: String },
    Rename { value: String },
    Mkdir { value: String },
    ConfirmDelete,
}

impl App {
    fn new_authed(client: PikPak) -> Self {
        let mut app = Self {
            client,
            current_folder_id: String::new(),
            breadcrumb: Vec::new(),
            entries: Vec::new(),
            selected: 0,
            logs: VecDeque::new(),
            input: InputMode::Normal,
        };
        app.refresh();
        app
    }

    fn new_login(client: PikPak, credentials: Option<Credentials>) -> Self {
        let input = match credentials {
            Some((email, password)) => InputMode::Login {
                field: LoginField::Email,
                email,
                password,
                error: None,
                logging_in: true,
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
            client,
            current_folder_id: String::new(),
            breadcrumb: Vec::new(),
            entries: Vec::new(),
            selected: 0,
            logs: VecDeque::new(),
            input,
        }
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        // Auto-login if credentials provided
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
        match self.client.login(email, password) {
            Ok(()) => {
                if let Err(e) = AppConfig::save_credentials(email, password) {
                    self.push_log(format!("Warning: failed to save config: {e:#}"));
                }
                self.input = InputMode::Normal;
                self.refresh();
                self.push_log("Login successful".to_string());
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
        }
    }

    fn current_path_display(&self) -> String {
        if self.breadcrumb.is_empty() {
            "/".to_string()
        } else {
            let path: Vec<&str> = self.breadcrumb.iter().map(|(_, name)| name.as_str()).collect();
            format!("/{}", path.join("/"))
        }
    }

    fn draw(&self, f: &mut Frame) {
        match &self.input {
            InputMode::Login { .. } => self.draw_login_screen(f),
            _ => self.draw_main(f),
        }
    }

    fn draw_login_screen(&self, f: &mut Frame) {
        let bg = Block::default().style(Style::default().bg(Color::Black));
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

        let path_display = self.current_path_display();
        let left_title = format!("Path: {}", path_display);
        let items: Vec<ListItem> = self
            .entries
            .iter()
            .map(|e| {
                let (kind, size_str) = match e.kind {
                    EntryKind::Folder => ("[D]", String::new()),
                    EntryKind::File => ("[F]", format!(" ({})", format_size(e.size))),
                };
                ListItem::new(format!("{} {}{}", kind, e.name, size_str))
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

        let help = "j/k: move | Enter: open | Bksp: back | r: refresh | c: copy | m: move | n: rename | d: rm | f: mkdir | q: quit";
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
            InputMode::Normal | InputMode::Login { .. } => {}
            InputMode::Move { value } => {
                f.render_widget(Clear, area);
                let p = Paragraph::new(format!(
                    "Move to folder path:\n{}\n\nEnter to confirm, Esc to cancel",
                    value
                ))
                .block(Block::default().borders(Borders::ALL).title("Move"));
                f.render_widget(p, area);
            }
            InputMode::Copy { value } => {
                f.render_widget(Clear, area);
                let p = Paragraph::new(format!(
                    "Copy to folder path:\n{}\n\nEnter to confirm, Esc to cancel",
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
            InputMode::Mkdir { value } => {
                f.render_widget(Clear, area);
                let p = Paragraph::new(format!(
                    "Folder name:\n{}\n\nEnter to confirm, Esc to cancel",
                    value
                ))
                .block(Block::default().borders(Borders::ALL).title("New Folder"));
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
        let mode = std::mem::replace(&mut self.input, InputMode::Normal);
        match mode {
            InputMode::Login {
                mut field,
                mut email,
                mut password,
                logging_in,
                ..
            } => {
                if logging_in {
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
            InputMode::Normal => self.handle_normal_key(code),
            InputMode::Move { mut value } => {
                if let Some(done) = handle_text_input(&mut value, code) {
                    if done {
                        if let Some(entry) = self.current_entry().cloned() {
                            let target = value.trim().to_string();
                            if !target.is_empty() {
                                match self.client.resolve_path(&target) {
                                    Ok(dest_id) => {
                                        match self.client.mv(&[entry.id.as_str()], &dest_id) {
                                            Ok(()) => self.push_log(format!(
                                                "Moved '{}' -> '{}'",
                                                entry.name, target
                                            )),
                                            Err(e) => self.push_log(format!("Move failed: {e:#}")),
                                        }
                                    }
                                    Err(e) => self.push_log(format!("Invalid path: {e:#}")),
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
                                match self.client.resolve_path(&target) {
                                    Ok(dest_id) => {
                                        match self.client.cp(&[entry.id.as_str()], &dest_id) {
                                            Ok(()) => self.push_log(format!(
                                                "Copied '{}' -> '{}'",
                                                entry.name, target
                                            )),
                                            Err(e) => self.push_log(format!("Copy failed: {e:#}")),
                                        }
                                    }
                                    Err(e) => self.push_log(format!("Invalid path: {e:#}")),
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
                                match self.client.rename(&entry.id, &new_name) {
                                    Ok(()) => self.push_log(format!(
                                        "Renamed '{}' -> '{}'",
                                        entry.name, new_name
                                    )),
                                    Err(e) => self.push_log(format!("Rename failed: {e:#}")),
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
            InputMode::Mkdir { mut value } => {
                if let Some(done) = handle_text_input(&mut value, code) {
                    if done {
                        let name = value.trim().to_string();
                        if !name.is_empty() {
                            match self.client.mkdir(&self.current_folder_id, &name) {
                                Ok(created) => self.push_log(format!(
                                    "Created folder '{}'",
                                    created.name
                                )),
                                Err(e) => self.push_log(format!("Mkdir failed: {e:#}")),
                            }
                            self.refresh();
                        }
                    }
                } else {
                    self.input = InputMode::Mkdir { value };
                }
                Ok(false)
            }
            InputMode::ConfirmDelete => {
                match code {
                    KeyCode::Char('y') => {
                        if let Some(entry) = self.current_entry().cloned() {
                            match self.client.remove(&[entry.id.as_str()]) {
                                Ok(()) => self.push_log(format!(
                                    "Removed '{}' (to trash)",
                                    entry.name
                                )),
                                Err(e) => self.push_log(format!("Remove failed: {e:#}")),
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
                if let Some(entry) = self.current_entry().cloned() {
                    if entry.kind == EntryKind::Folder {
                        let old_id = std::mem::replace(&mut self.current_folder_id, entry.id);
                        self.breadcrumb.push((old_id, entry.name));
                        self.selected = 0;
                        self.refresh();
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some((parent_id, _name)) = self.breadcrumb.pop() {
                    self.current_folder_id = parent_id;
                    self.selected = 0;
                    self.refresh();
                }
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
            KeyCode::Char('f') => {
                self.input = InputMode::Mkdir {
                    value: String::new(),
                };
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
        match self.client.ls(&self.current_folder_id) {
            Ok(entries) => {
                self.entries = entries;
                if self.selected >= self.entries.len() {
                    self.selected = self.entries.len().saturating_sub(1);
                }
                self.push_log(format!("Refreshed {}", self.current_path_display()));
            }
            Err(e) => {
                self.push_log(format!("Refresh failed: {e:#}"));
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

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
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
