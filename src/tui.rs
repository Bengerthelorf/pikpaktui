use crate::config::{AppConfig, TuiConfig};
use crate::pikpak::{Entry, EntryKind, PikPak};
use crate::theme;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
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
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub type Credentials = (String, String);

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn run(client: PikPak, config: TuiConfig) -> Result<()> {
    run_terminal(App::new_authed(client, config))
}

pub fn run_with_credentials(
    client: PikPak,
    credentials: Option<Credentials>,
    config: TuiConfig,
) -> Result<()> {
    run_terminal(App::new_login(client, credentials, config))
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

enum OpResult {
    Ls(Result<Vec<Entry>>),
    Ok(String),
    Err(String),
}

struct PickerState {
    folder_id: String,
    breadcrumb: Vec<(String, String)>,
    entries: Vec<Entry>,
    selected: usize,
    loading: bool,
}

struct PathInput {
    value: String,
    candidates: Vec<String>,
    candidate_idx: Option<usize>,
}

impl PathInput {
    fn new() -> Self {
        Self {
            value: String::new(),
            candidates: Vec::new(),
            candidate_idx: None,
        }
    }
}

struct App {
    client: Arc<PikPak>,
    config: TuiConfig,
    current_folder_id: String,
    breadcrumb: Vec<(String, String)>,
    entries: Vec<Entry>,
    selected: usize,
    logs: VecDeque<String>,
    input: InputMode,
    cursor_visible: bool,
    last_blink: Instant,
    loading: bool,
    spinner_idx: usize,
    last_spinner: Instant,
    result_rx: Receiver<OpResult>,
    result_tx: Sender<OpResult>,
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
    Rename {
        value: String,
    },
    Mkdir {
        value: String,
    },
    ConfirmDelete,
    // Text input with tab completion
    MoveInput {
        source: Entry,
        input: PathInput,
    },
    CopyInput {
        source: Entry,
        input: PathInput,
    },
    // Two-pane picker
    MovePicker {
        source: Entry,
        picker: PickerState,
    },
    CopyPicker {
        source: Entry,
        picker: PickerState,
    },
}

impl App {
    fn new_authed(client: PikPak, config: TuiConfig) -> Self {
        let (tx, rx) = mpsc::channel();
        let mut app = Self {
            client: Arc::new(client),
            config,
            current_folder_id: String::new(),
            breadcrumb: Vec::new(),
            entries: Vec::new(),
            selected: 0,
            logs: VecDeque::new(),
            input: InputMode::Normal,
            cursor_visible: true,
            last_blink: Instant::now(),
            loading: false,
            spinner_idx: 0,
            last_spinner: Instant::now(),
            result_rx: rx,
            result_tx: tx,
        };
        app.refresh();
        app
    }

    fn new_login(client: PikPak, credentials: Option<Credentials>, config: TuiConfig) -> Self {
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

        let (tx, rx) = mpsc::channel();
        Self {
            client: Arc::new(client),
            config,
            current_folder_id: String::new(),
            breadcrumb: Vec::new(),
            entries: Vec::new(),
            selected: 0,
            logs: VecDeque::new(),
            input,
            cursor_visible: true,
            last_blink: Instant::now(),
            loading: false,
            spinner_idx: 0,
            last_spinner: Instant::now(),
            result_rx: rx,
            result_tx: tx,
        }
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
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
            if self.last_blink.elapsed() >= Duration::from_millis(500) {
                self.cursor_visible = !self.cursor_visible;
                self.last_blink = Instant::now();
            }
            if self.loading && self.last_spinner.elapsed() >= Duration::from_millis(80) {
                self.spinner_idx = (self.spinner_idx + 1) % SPINNER_FRAMES.len();
                self.last_spinner = Instant::now();
            }
            self.poll_results();

            terminal.draw(|f| self.draw(f))?;

            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    self.cursor_visible = true;
                    self.last_blink = Instant::now();
                    if self.handle_key(key.code, key.modifiers)? {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    fn poll_results(&mut self) {
        while let Ok(result) = self.result_rx.try_recv() {
            match result {
                OpResult::Ls(Ok(entries)) => {
                    self.loading = false;
                    self.entries = entries;
                    if self.selected >= self.entries.len() {
                        self.selected = self.entries.len().saturating_sub(1);
                    }
                    self.push_log(format!("Refreshed {}", self.current_path_display()));
                }
                OpResult::Ls(Err(e)) => {
                    self.loading = false;
                    self.push_log(format!("Refresh failed: {e:#}"));
                }
                OpResult::Ok(msg) => {
                    self.push_log(msg);
                    self.refresh();
                }
                OpResult::Err(msg) => {
                    self.push_log(msg);
                    self.loading = false;
                }
            }
        }
    }

    fn attempt_login(&mut self, email: &str, password: &str) {
        let client = Arc::get_mut(&mut self.client)
            .expect("no other references to client during login");
        match client.login(email, password) {
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
            let path: Vec<&str> = self.breadcrumb.iter().map(|(_, n)| n.as_str()).collect();
            format!("/{}", path.join("/"))
        }
    }

    fn picker_path_display(picker: &PickerState) -> String {
        if picker.breadcrumb.is_empty() {
            "/".to_string()
        } else {
            let path: Vec<&str> = picker.breadcrumb.iter().map(|(_, n)| n.as_str()).collect();
            format!("/{}", path.join("/"))
        }
    }

    // --- Drawing ---

    fn draw(&self, f: &mut Frame) {
        match &self.input {
            InputMode::Login { .. } => self.draw_login_screen(f),
            InputMode::MovePicker { .. } | InputMode::CopyPicker { .. } => self.draw_picker(f),
            _ => self.draw_main(f),
        }
    }

    fn draw_login_screen(&self, f: &mut Frame) {
        let bg = Block::default().style(Style::default().bg(Color::Black));
        f.render_widget(bg, f.area());
        let area = centered_rect(50, 40, f.area());

        if let InputMode::Login {
            field, email, password, error, logging_in,
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
            let masked: String = "*".repeat(password.len());
            let cur = if self.cursor_visible { "\u{2588}" } else { " " };
            let ec = if matches!(field, LoginField::Email) { cur } else { "" };
            let pc = if matches!(field, LoginField::Password) { cur } else { "" };

            let mut lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Email:    ", email_style),
                    Span::styled(format!("{}{}", email, ec), email_style),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Password: ", pass_style),
                    Span::styled(format!("{}{}", masked, pc), pass_style),
                ]),
                Line::from(""),
            ];
            if *logging_in {
                lines.push(Line::from(Span::styled("  Logging in...", Style::default().fg(Color::Cyan))));
            } else if let Some(err) = error {
                lines.push(Line::from(Span::styled(format!("  {}", err), Style::default().fg(Color::Red))));
                lines.push(Line::from(""));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  Tab: switch field | Enter: login | Esc: quit",
                Style::default().fg(Color::DarkGray),
            )));

            let p = Paragraph::new(Text::from(lines))
                .block(Block::default().borders(Borders::ALL).title(" PikPak Login ")
                    .border_style(Style::default().fg(Color::Cyan)))
                .wrap(Wrap { trim: false });
            f.render_widget(p, area);
        }
    }

    fn draw_main(&self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(f.area());

        // File list
        let path_display = self.current_path_display();
        let left_title = if self.loading {
            format!(" {} {} ", SPINNER_FRAMES[self.spinner_idx], path_display)
        } else {
            format!(" {} ", path_display)
        };

        let items: Vec<ListItem> = self.entries.iter().map(|e| {
            let cat = theme::categorize(e);
            let ico = theme::icon(cat, self.config.nerd_font);
            let c = theme::color(cat);
            let size_str = match e.kind {
                EntryKind::Folder => String::new(),
                EntryKind::File => format!("  {}", format_size(e.size)),
            };
            ListItem::new(Line::from(vec![
                Span::styled(ico, Style::default().fg(c)),
                Span::styled(" ", Style::default()),
                Span::styled(&e.name, Style::default().fg(c)),
                Span::styled(size_str, Style::default().fg(Color::DarkGray)),
            ]))
        }).collect();

        let mut state = ListState::default();
        if !self.entries.is_empty() {
            state.select(Some(self.selected.min(self.entries.len() - 1)));
        }

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(left_title)
                .title_style(Style::default().fg(Color::Green))
                .border_style(Style::default().fg(Color::Cyan)))
            .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            .highlight_symbol("> ");
        f.render_stateful_widget(list, chunks[0], &mut state);

        // Logs
        let log_lines: Vec<Line> = self.logs.iter().rev()
            .take(chunks[1].height.saturating_sub(4) as usize)
            .rev().map(|s| Line::from(s.as_str())).collect();
        let help = "j/k:move Enter:open Bksp:back r:refresh c:copy m:move n:rename d:rm f:mkdir q:quit";
        let mut lines = log_lines;
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(help, Style::default().fg(Color::DarkGray))));
        let logs = Paragraph::new(Text::from(lines))
            .block(Block::default().borders(Borders::ALL).title(" Logs ")
                .title_style(Style::default().fg(Color::Green))
                .border_style(Style::default().fg(Color::Cyan)))
            .wrap(Wrap { trim: false });
        f.render_widget(logs, chunks[1]);

        self.draw_overlay(f);
    }

    fn draw_overlay(&self, f: &mut Frame) {
        let cur = if self.cursor_visible { "\u{2588}" } else { " " };

        match &self.input {
            InputMode::Normal | InputMode::Login { .. }
            | InputMode::MovePicker { .. } | InputMode::CopyPicker { .. } => {}

            InputMode::MoveInput { input, .. } => {
                self.draw_path_input_overlay(f, "Move", "Move to path", input, cur);
            }
            InputMode::CopyInput { input, .. } => {
                self.draw_path_input_overlay(f, "Copy", "Copy to path", input, cur);
            }
            InputMode::Rename { value } => {
                let area = centered_rect(60, 20, f.area());
                f.render_widget(Clear, area);
                let p = Paragraph::new(Text::from(vec![
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("  New name: ", Style::default().fg(Color::Cyan)),
                        Span::styled(format!("{}{}", value, cur), Style::default().fg(Color::Yellow)),
                    ]),
                    Line::from(""),
                    Line::from(Span::styled("  Enter: confirm | Esc: cancel", Style::default().fg(Color::DarkGray))),
                ])).block(Block::default().borders(Borders::ALL).title(" Rename ")
                    .title_style(Style::default().fg(Color::Yellow))
                    .border_style(Style::default().fg(Color::Cyan)));
                f.render_widget(p, area);
            }
            InputMode::Mkdir { value } => {
                let area = centered_rect(60, 20, f.area());
                f.render_widget(Clear, area);
                let p = Paragraph::new(Text::from(vec![
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("  Folder name: ", Style::default().fg(Color::Cyan)),
                        Span::styled(format!("{}{}", value, cur), Style::default().fg(Color::Yellow)),
                    ]),
                    Line::from(""),
                    Line::from(Span::styled("  Enter: confirm | Esc: cancel", Style::default().fg(Color::DarkGray))),
                ])).block(Block::default().borders(Borders::ALL).title(" New Folder ")
                    .title_style(Style::default().fg(Color::Yellow))
                    .border_style(Style::default().fg(Color::Cyan)));
                f.render_widget(p, area);
            }
            InputMode::ConfirmDelete => {
                let area = centered_rect(60, 20, f.area());
                f.render_widget(Clear, area);
                let name = self.current_entry().map(|e| e.name.as_str()).unwrap_or("<none>");
                let p = Paragraph::new(Text::from(vec![
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("  Delete ", Style::default().fg(Color::Red)),
                        Span::styled(format!("`{}`", name), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                        Span::styled(" to trash?", Style::default().fg(Color::Red)),
                    ]),
                    Line::from(""),
                    Line::from(Span::styled("  y: confirm | n/Esc: cancel", Style::default().fg(Color::DarkGray))),
                ])).block(Block::default().borders(Borders::ALL).title(" Confirm Remove ")
                    .title_style(Style::default().fg(Color::Red))
                    .border_style(Style::default().fg(Color::Red)));
                f.render_widget(p, area);
            }
        }
    }

    fn draw_path_input_overlay(&self, f: &mut Frame, title: &str, label: &str, input: &PathInput, cur: &str) {
        // Determine overlay height based on candidates
        let candidate_lines = input.candidates.len().min(8);
        let base_height = 6; // padding + input line + help line
        let total_lines = base_height + if candidate_lines > 0 { candidate_lines + 1 } else { 0 };
        let pct = ((total_lines as u16 * 100) / f.area().height.max(1)).max(20).min(60);
        let area = centered_rect(70, pct, f.area());
        f.render_widget(Clear, area);

        let mut lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled(format!("  {}: ", label), Style::default().fg(Color::Cyan)),
                Span::styled(format!("{}{}", input.value, cur), Style::default().fg(Color::Yellow)),
            ]),
        ];

        // Show candidates
        if !input.candidates.is_empty() {
            lines.push(Line::from(""));
            for (i, name) in input.candidates.iter().enumerate().take(8) {
                let is_sel = input.candidate_idx == Some(i);
                let prefix = if is_sel { "  > " } else { "    " };
                let style = if is_sel {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Blue)
                };
                lines.push(Line::from(Span::styled(format!("{}{}/", prefix, name), style)));
            }
            if input.candidates.len() > 8 {
                lines.push(Line::from(Span::styled(
                    format!("    ... and {} more", input.candidates.len() - 8),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Tab: complete | Enter: confirm | Ctrl+B: picker | Esc: cancel",
            Style::default().fg(Color::DarkGray),
        )));

        let p = Paragraph::new(Text::from(lines))
            .block(Block::default().borders(Borders::ALL)
                .title(format!(" {} ", title))
                .title_style(Style::default().fg(Color::Yellow))
                .border_style(Style::default().fg(Color::Cyan)));
        f.render_widget(p, area);
    }

    fn draw_picker(&self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(f.area());

        // Left: source (read-only)
        let source_items: Vec<ListItem> = self.entries.iter().map(|e| {
            let cat = theme::categorize(e);
            let ico = theme::icon(cat, self.config.nerd_font);
            let c = theme::color(cat);
            ListItem::new(Line::from(vec![
                Span::styled(ico, Style::default().fg(c)),
                Span::styled(" ", Style::default()),
                Span::styled(&e.name, Style::default().fg(c)),
            ]))
        }).collect();

        let mut source_state = ListState::default();
        if !self.entries.is_empty() {
            source_state.select(Some(self.selected.min(self.entries.len() - 1)));
        }
        let source_list = List::new(source_items)
            .block(Block::default().borders(Borders::ALL)
                .title(format!(" Source: {} ", self.current_path_display()))
                .title_style(Style::default().fg(Color::DarkGray))
                .border_style(Style::default().fg(Color::DarkGray)))
            .highlight_style(Style::default().fg(Color::DarkGray))
            .highlight_symbol("  ");
        f.render_stateful_widget(source_list, chunks[0], &mut source_state);

        // Right: picker
        let (is_move, source_entry, picker) = match &self.input {
            InputMode::MovePicker { source, picker } => (true, source, picker),
            InputMode::CopyPicker { source, picker } => (false, source, picker),
            _ => return,
        };

        let op = if is_move { "Move" } else { "Copy" };
        let pp = Self::picker_path_display(picker);
        let title = if picker.loading {
            format!(" {} to: {} {} ", op, pp, SPINNER_FRAMES[self.spinner_idx])
        } else {
            format!(" {} to: {} ", op, pp)
        };

        let folders: Vec<&Entry> = picker.entries.iter()
            .filter(|e| e.kind == EntryKind::Folder).collect();

        let picker_items: Vec<ListItem> = folders.iter().map(|e| {
            let ico = theme::icon(theme::FileCategory::Folder, self.config.nerd_font);
            ListItem::new(Line::from(vec![
                Span::styled(ico, Style::default().fg(Color::Blue)),
                Span::styled(" ", Style::default()),
                Span::styled(&e.name, Style::default().fg(Color::Blue)),
            ]))
        }).collect();

        let mut picker_state = ListState::default();
        if !folders.is_empty() {
            picker_state.select(Some(picker.selected.min(folders.len() - 1)));
        }

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(3)])
            .split(chunks[1]);

        let plist = List::new(picker_items)
            .block(Block::default().borders(Borders::ALL).title(title)
                .title_style(Style::default().fg(Color::Yellow))
                .border_style(Style::default().fg(Color::Yellow)))
            .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            .highlight_symbol("> ");
        f.render_stateful_widget(plist, right_chunks[0], &mut picker_state);

        let help = format!(
            " {} '{}' | j/k:nav Enter:open Bksp:back Space:confirm /:input Esc:cancel",
            op, source_entry.name
        );
        let hw = Paragraph::new(Span::styled(help, Style::default().fg(Color::DarkGray)))
            .block(Block::default().borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)));
        f.render_widget(hw, right_chunks[1]);
    }

    // --- Key handling ---

    fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Result<bool> {
        let mode = std::mem::replace(&mut self.input, InputMode::Normal);
        match mode {
            InputMode::Login {
                mut field, mut email, mut password, logging_in, ..
            } => {
                if logging_in {
                    self.input = InputMode::Login { field, email, password, error: None, logging_in: true };
                    return Ok(false);
                }
                match code {
                    KeyCode::Esc => return Ok(true),
                    KeyCode::Tab | KeyCode::BackTab => {
                        field = match field {
                            LoginField::Email => LoginField::Password,
                            LoginField::Password => LoginField::Email,
                        };
                        self.input = InputMode::Login { field, email, password, error: None, logging_in: false };
                    }
                    KeyCode::Enter => {
                        let (e, p) = (email.clone(), password.clone());
                        if e.trim().is_empty() || p.is_empty() {
                            self.input = InputMode::Login { field, email, password, error: Some("Email and password are required".into()), logging_in: false };
                        } else {
                            self.input = InputMode::Login { field, email: e.clone(), password: p.clone(), error: None, logging_in: true };
                            self.attempt_login(&e, &p);
                        }
                    }
                    KeyCode::Backspace => {
                        match field { LoginField::Email => { email.pop(); } LoginField::Password => { password.pop(); } }
                        self.input = InputMode::Login { field, email, password, error: None, logging_in: false };
                    }
                    KeyCode::Char(c) => {
                        match field { LoginField::Email => email.push(c), LoginField::Password => password.push(c) }
                        self.input = InputMode::Login { field, email, password, error: None, logging_in: false };
                    }
                    _ => {
                        self.input = InputMode::Login { field, email, password, error: None, logging_in: false };
                    }
                }
                Ok(false)
            }
            InputMode::Normal => self.handle_normal_key(code),
            InputMode::Rename { mut value } => {
                if let Some(done) = handle_text_input(&mut value, code) {
                    if done {
                        if let Some(entry) = self.current_entry().cloned() {
                            let new_name = value.trim().to_string();
                            if !new_name.is_empty() {
                                self.spawn_rename(entry, new_name);
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
                            self.spawn_mkdir(name);
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
                            self.spawn_delete(entry);
                        }
                    }
                    KeyCode::Char('n') | KeyCode::Esc => {
                        self.push_log("Remove cancelled".into());
                    }
                    _ => { self.input = InputMode::ConfirmDelete; }
                }
                Ok(false)
            }
            InputMode::MoveInput { source, mut input } => {
                self.handle_path_input_key(code, modifiers, source, &mut input, true);
                Ok(false)
            }
            InputMode::CopyInput { source, mut input } => {
                self.handle_path_input_key(code, modifiers, source, &mut input, false);
                Ok(false)
            }
            InputMode::MovePicker { source, mut picker } => {
                self.handle_picker_key(code, source, &mut picker, true);
                Ok(false)
            }
            InputMode::CopyPicker { source, mut picker } => {
                self.handle_picker_key(code, source, &mut picker, false);
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
                if self.selected > 0 { self.selected -= 1; }
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
                if let Some((parent_id, _)) = self.breadcrumb.pop() {
                    self.current_folder_id = parent_id;
                    self.selected = 0;
                    self.refresh();
                }
            }
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('m') => {
                if let Some(entry) = self.current_entry().cloned() {
                    self.start_move_copy(entry, true);
                }
            }
            KeyCode::Char('c') => {
                if let Some(entry) = self.current_entry().cloned() {
                    self.start_move_copy(entry, false);
                }
            }
            KeyCode::Char('n') => {
                if self.current_entry().is_some() {
                    self.input = InputMode::Rename { value: String::new() };
                }
            }
            KeyCode::Char('d') => {
                if self.current_entry().is_some() {
                    self.input = InputMode::ConfirmDelete;
                }
            }
            KeyCode::Char('f') => {
                self.input = InputMode::Mkdir { value: String::new() };
            }
            _ => {}
        }
        Ok(false)
    }

    fn start_move_copy(&mut self, source: Entry, is_move: bool) {
        if self.config.use_picker() {
            self.init_picker(source, is_move);
        } else {
            self.init_path_input(source, is_move);
        }
    }

    // --- Path input with tab completion ---

    fn init_path_input(&mut self, source: Entry, is_move: bool) {
        let input = PathInput::new();
        if is_move {
            self.input = InputMode::MoveInput { source, input };
        } else {
            self.input = InputMode::CopyInput { source, input };
        }
    }

    fn handle_path_input_key(
        &mut self, code: KeyCode, modifiers: KeyModifiers,
        source: Entry, input: &mut PathInput, is_move: bool,
    ) {
        // Ctrl+B: switch to picker
        if code == KeyCode::Char('b') && modifiers.contains(KeyModifiers::CONTROL) {
            self.init_picker(source, is_move);
            return;
        }

        match code {
            KeyCode::Esc => {
                let op = if is_move { "Move" } else { "Copy" };
                self.push_log(format!("{} cancelled", op));
            }
            KeyCode::Enter => {
                let target = input.value.trim().to_string();
                if !target.is_empty() {
                    self.execute_move_copy(source, &target, is_move);
                }
            }
            KeyCode::Tab => {
                self.tab_complete(input);
                self.restore_path_input(source, input, is_move);
            }
            KeyCode::Backspace => {
                input.value.pop();
                input.candidates.clear();
                input.candidate_idx = None;
                self.restore_path_input(source, input, is_move);
            }
            KeyCode::Char(c) => {
                input.value.push(c);
                input.candidates.clear();
                input.candidate_idx = None;
                self.restore_path_input(source, input, is_move);
            }
            _ => {
                self.restore_path_input(source, input, is_move);
            }
        }
    }

    fn tab_complete(&self, input: &mut PathInput) {
        let val = &input.value;

        // If we have candidates and user presses Tab again, cycle through them
        if !input.candidates.is_empty() {
            let idx = match input.candidate_idx {
                Some(i) => (i + 1) % input.candidates.len(),
                None => 0,
            };
            input.candidate_idx = Some(idx);
            // Apply selected candidate to value
            let (parent, _) = split_path_prefix(val);
            let selected = &input.candidates[idx];
            input.value = if parent.is_empty() {
                format!("{}/", selected)
            } else if parent == "/" {
                format!("/{}/", selected)
            } else {
                format!("{}/{}/", parent, selected)
            };
            return;
        }

        // Parse: split into parent path + prefix to complete
        let (parent_path, prefix) = split_path_prefix(val);

        // Resolve parent folder
        let parent_id = if parent_path.is_empty() {
            // Relative: use current folder
            self.current_folder_id.clone()
        } else {
            match self.client.resolve_path(&parent_path) {
                Ok(id) => id,
                Err(_) => return,
            }
        };

        // List entries in parent
        let entries = match self.client.ls(&parent_id) {
            Ok(e) => e,
            Err(_) => return,
        };

        // Filter folders matching prefix
        let prefix_lower = prefix.to_lowercase();
        let matches: Vec<String> = entries.iter()
            .filter(|e| e.kind == EntryKind::Folder)
            .filter(|e| e.name.to_lowercase().starts_with(&prefix_lower))
            .map(|e| e.name.clone())
            .collect();

        if matches.is_empty() {
            return;
        }

        if matches.len() == 1 {
            // Single match: autocomplete directly
            let name = &matches[0];
            input.value = if parent_path.is_empty() {
                format!("{}/", name)
            } else if parent_path == "/" {
                format!("/{}/", name)
            } else {
                format!("{}/{}/", parent_path, name)
            };
            input.candidates.clear();
            input.candidate_idx = None;
        } else {
            // Multiple: show candidates, apply first
            input.candidates = matches;
            input.candidate_idx = Some(0);
            let first = &input.candidates[0];
            input.value = if parent_path.is_empty() {
                format!("{}/", first)
            } else if parent_path == "/" {
                format!("/{}/", first)
            } else {
                format!("{}/{}/", parent_path, first)
            };
        }
    }

    fn restore_path_input(&mut self, source: Entry, input: &mut PathInput, is_move: bool) {
        let owned = PathInput {
            value: std::mem::take(&mut input.value),
            candidates: std::mem::take(&mut input.candidates),
            candidate_idx: input.candidate_idx,
        };
        if is_move {
            self.input = InputMode::MoveInput { source, input: owned };
        } else {
            self.input = InputMode::CopyInput { source, input: owned };
        }
    }

    // --- Picker ---

    fn init_picker(&mut self, source: Entry, is_move: bool) {
        let folder_id = self.current_folder_id.clone();
        let breadcrumb = self.breadcrumb.clone();
        let entries = match self.client.ls(&folder_id) {
            Ok(e) => e,
            Err(e) => {
                self.push_log(format!("Picker load failed: {e:#}"));
                return;
            }
        };
        let picker = PickerState { folder_id, breadcrumb, entries, selected: 0, loading: false };
        if is_move {
            self.input = InputMode::MovePicker { source, picker };
        } else {
            self.input = InputMode::CopyPicker { source, picker };
        }
    }

    fn handle_picker_key(&mut self, code: KeyCode, source: Entry, picker: &mut PickerState, is_move: bool) {
        let folder_count = picker.entries.iter().filter(|e| e.kind == EntryKind::Folder).count();

        match code {
            KeyCode::Esc => {
                let op = if is_move { "Move" } else { "Copy" };
                self.push_log(format!("{} cancelled", op));
            }
            // Switch to text input
            KeyCode::Char('/') => {
                self.init_path_input(source, is_move);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if folder_count > 0 {
                    picker.selected = (picker.selected + 1).min(folder_count - 1);
                }
                self.restore_picker(source, picker, is_move);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if picker.selected > 0 { picker.selected -= 1; }
                self.restore_picker(source, picker, is_move);
            }
            KeyCode::Enter => {
                let folders: Vec<Entry> = picker.entries.iter()
                    .filter(|e| e.kind == EntryKind::Folder).cloned().collect();
                if let Some(entry) = folders.get(picker.selected).cloned() {
                    let old_id = std::mem::replace(&mut picker.folder_id, entry.id.clone());
                    picker.breadcrumb.push((old_id, entry.name.clone()));
                    picker.selected = 0;
                    picker.loading = true;
                    match self.client.ls(&entry.id) {
                        Ok(entries) => { picker.entries = entries; picker.loading = false; }
                        Err(e) => { self.push_log(format!("Picker load failed: {e:#}")); picker.loading = false; }
                    }
                }
                self.restore_picker(source, picker, is_move);
            }
            KeyCode::Backspace => {
                if let Some((parent_id, _)) = picker.breadcrumb.pop() {
                    picker.folder_id = parent_id.clone();
                    picker.selected = 0;
                    picker.loading = true;
                    match self.client.ls(&parent_id) {
                        Ok(entries) => { picker.entries = entries; picker.loading = false; }
                        Err(e) => { self.push_log(format!("Picker load failed: {e:#}")); picker.loading = false; }
                    }
                }
                self.restore_picker(source, picker, is_move);
            }
            KeyCode::Char(' ') => {
                let dest_id = picker.folder_id.clone();
                let dest_path = Self::picker_path_display(picker);
                self.spawn_move_copy(source, dest_id, dest_path, is_move);
            }
            _ => {
                self.restore_picker(source, picker, is_move);
            }
        }
    }

    fn restore_picker(&mut self, source: Entry, picker: &mut PickerState, is_move: bool) {
        let owned = PickerState {
            folder_id: std::mem::take(&mut picker.folder_id),
            breadcrumb: std::mem::take(&mut picker.breadcrumb),
            entries: std::mem::take(&mut picker.entries),
            selected: picker.selected,
            loading: picker.loading,
        };
        if is_move {
            self.input = InputMode::MovePicker { source, picker: owned };
        } else {
            self.input = InputMode::CopyPicker { source, picker: owned };
        }
    }

    // --- Operations ---

    fn execute_move_copy(&mut self, source: Entry, target: &str, is_move: bool) {
        match self.client.resolve_path(target) {
            Ok(dest_id) => {
                self.spawn_move_copy(source, dest_id, target.to_string(), is_move);
            }
            Err(e) => {
                self.push_log(format!("Invalid path: {e:#}"));
            }
        }
    }

    fn spawn_move_copy(&mut self, source: Entry, dest_id: String, dest_path: String, is_move: bool) {
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        let source_id = source.id.clone();
        let source_name = source.name.clone();
        let op = if is_move { "Move" } else { "Copy" };
        self.loading = true;
        std::thread::spawn(move || {
            let result = if is_move {
                client.mv(&[source_id.as_str()], &dest_id)
            } else {
                client.cp(&[source_id.as_str()], &dest_id)
            };
            let _ = tx.send(match result {
                Ok(()) => OpResult::Ok(format!("{}d '{}' -> '{}'", op, source_name, dest_path)),
                Err(e) => OpResult::Err(format!("{} failed: {e:#}", op)),
            });
        });
    }

    fn spawn_rename(&mut self, entry: Entry, new_name: String) {
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        let eid = entry.id.clone();
        let old = entry.name.clone();
        self.loading = true;
        std::thread::spawn(move || {
            let _ = tx.send(match client.rename(&eid, &new_name) {
                Ok(()) => OpResult::Ok(format!("Renamed '{}' -> '{}'", old, new_name)),
                Err(e) => OpResult::Err(format!("Rename failed: {e:#}")),
            });
        });
    }

    fn spawn_mkdir(&mut self, name: String) {
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        let fid = self.current_folder_id.clone();
        self.loading = true;
        std::thread::spawn(move || {
            let _ = tx.send(match client.mkdir(&fid, &name) {
                Ok(created) => OpResult::Ok(format!("Created folder '{}'", created.name)),
                Err(e) => OpResult::Err(format!("Mkdir failed: {e:#}")),
            });
        });
    }

    fn spawn_delete(&mut self, entry: Entry) {
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        let eid = entry.id.clone();
        let name = entry.name.clone();
        self.loading = true;
        std::thread::spawn(move || {
            let _ = tx.send(match client.remove(&[eid.as_str()]) {
                Ok(()) => OpResult::Ok(format!("Removed '{}' (to trash)", name)),
                Err(e) => OpResult::Err(format!("Remove failed: {e:#}")),
            });
        });
    }

    fn current_entry(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
    }

    fn push_log(&mut self, msg: String) {
        self.logs.push_back(msg);
        if self.logs.len() > 500 { self.logs.pop_front(); }
    }

    fn refresh(&mut self) {
        self.loading = true;
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        let fid = self.current_folder_id.clone();
        std::thread::spawn(move || {
            let _ = tx.send(OpResult::Ls(client.ls(&fid)));
        });
    }
}

/// Split a path input into (parent_path, prefix).
/// "/My Pack/sub" -> ("/My Pack", "sub")
/// "/My Pack/"    -> ("/My Pack", "")
/// "/"            -> ("/", "")
/// ""             -> ("", "")
/// "sub"          -> ("", "sub")
fn split_path_prefix(input: &str) -> (String, String) {
    if input.is_empty() {
        return (String::new(), String::new());
    }
    if input == "/" {
        return ("/".to_string(), String::new());
    }
    // If ends with '/', the prefix is empty, parent is the whole path (without trailing /)
    if input.ends_with('/') {
        let trimmed = input.trim_end_matches('/');
        return (trimmed.to_string(), String::new());
    }
    match input.rsplit_once('/') {
        Some(("", name)) => ("/".to_string(), name.to_string()),
        Some((parent, name)) => (parent.to_string(), name.to_string()),
        None => (String::new(), input.to_string()),
    }
}

fn handle_text_input(value: &mut String, code: KeyCode) -> Option<bool> {
    match code {
        KeyCode::Esc => Some(false),
        KeyCode::Enter => Some(true),
        KeyCode::Backspace => { value.pop(); None }
        KeyCode::Char(c) => { value.push(c); None }
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

fn centered_rect(percent_x: u16, percent_y: u16, area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ]).split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ]).split(v[1])[1]
}
