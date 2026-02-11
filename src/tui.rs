use crate::backend::{Backend, Entry};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use std::collections::VecDeque;
use std::io;
use std::time::Duration;

const DEFAULT_PATH: &str = "/My Pack";

pub fn run(backend: Box<dyn Backend>) -> Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut terminal = ratatui::init();

    let res = App::new(backend).run(&mut terminal);

    ratatui::restore();
    execute!(io::stdout(), LeaveAlternateScreen)?;
    disable_raw_mode()?;

    res
}

struct App {
    backend: Box<dyn Backend>,
    current_path: String,
    entries: Vec<Entry>,
    selected: usize,
    logs: VecDeque<String>,
    input: InputMode,
}

enum InputMode {
    None,
    Move { value: String },
    Copy { value: String },
    Rename { value: String },
    ConfirmDelete,
}

impl App {
    fn new(backend: Box<dyn Backend>) -> Self {
        let mut app = Self {
            backend,
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

    fn draw(&self, f: &mut Frame) {
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
            InputMode::None => {}
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
            InputMode::None => self.handle_normal_key(code),
            InputMode::Move { mut value } => {
                if let Some(done) = handle_text_input(&mut value, code) {
                    if done {
                        if let Some(entry) = self.current_entry().cloned() {
                            let target = value.trim().to_string();
                            if !target.is_empty() {
                                match self.backend.mv(&self.current_path, &entry.name, &target) {
                                    Ok(_) => self.push_log(format!(
                                        "Moved '{}' -> '{}'",
                                        entry.name, target
                                    )),
                                    Err(e) => self.push_log(format!("Move failed: {e:#}")),
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
                                match self.backend.cp(&self.current_path, &entry.name, &target) {
                                    Ok(_) => self.push_log(format!(
                                        "Copied '{}' -> '{}'",
                                        entry.name, target
                                    )),
                                    Err(e) => self.push_log(format!("Copy failed: {e:#}")),
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
                                match self.backend.rename(
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
                            match self.backend.remove(&self.current_path, &entry.name) {
                                Ok(_) => {
                                    self.push_log(format!("Removed '{}' (to trash)", entry.name))
                                }
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
        match self.backend.ls(&self.current_path) {
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
