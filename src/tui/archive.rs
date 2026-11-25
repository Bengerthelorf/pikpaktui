use super::{App, InputMode, handle_text_input, operations::WriteGuard};
use crate::cmd::share::terminal_safe_text;
use crate::pikpak::{
    Entry, EntryKind, PikPak,
    archive::{ArchiveListing, ArchiveMember, ArchiveProgress},
};
use anyhow::{Result, anyhow};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Paragraph, Wrap},
};
use std::{
    collections::HashSet,
    sync::{
        Arc,
        mpsc::{self, Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};

enum Reply {
    Listing(String, Vec<String>, ArchiveListing),
    Submitted(String),
    Progress(ArchiveProgress),
}

#[derive(PartialEq)]
enum Stage {
    Browse,
    Password,
    ConfirmSelected,
    ConfirmAll,
    Progress,
}

pub(super) struct Archive {
    id: String,
    name: String,
    gcid: String,
    password: String,
    directories: Vec<String>,
    members: Vec<ArchiveMember>,
    cursor: usize,
    marked: HashSet<usize>,
    stage: Stage,
    task: String,
    next_refresh: Option<Instant>,
    pending: Option<Receiver<Result<Reply>>>,
    notice: String,
    listed: bool,
    complete: bool,
}

impl App {
    pub(super) fn open_archive(&mut self, entry: Entry) {
        if entry.kind != EntryKind::File {
            self.push_log("Select an archive file".into());
            return;
        }
        let mut archive = Archive {
            id: entry.id,
            name: entry.name,
            gcid: String::new(),
            password: String::new(),
            directories: Vec::new(),
            members: Vec::new(),
            cursor: 0,
            marked: HashSet::new(),
            stage: Stage::Browse,
            task: String::new(),
            next_refresh: None,
            pending: None,
            notice: String::new(),
            listed: false,
            complete: false,
        };
        archive.list(&self.client, vec![]);
        self.input = InputMode::Archive(Box::new(archive));
    }
}

impl Archive {
    fn request(&mut self, work: impl FnOnce() -> Result<Reply> + Send + 'static) {
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
    }

    fn list(&mut self, client: &Arc<PikPak>, path: Vec<String>) {
        let client = Arc::clone(client);
        let id = self.id.clone();
        let gcid = self.gcid.clone();
        let password = self.password.clone();
        self.notice = "Loading archive directory…".into();
        self.request(move || {
            let gcid = if gcid.is_empty() {
                client.archive_gcid(&id)?
            } else {
                gcid
            };
            let listing = client.archive_list(
                &id,
                &gcid,
                path.last().map(String::as_str).unwrap_or(""),
                &password,
            )?;
            if !listing.needs_password() {
                listing.check()?;
            }
            Ok(Reply::Listing(gcid, path, listing))
        });
    }

    fn refresh(&mut self, client: &Arc<PikPak>) {
        self.next_refresh = None;
        let client = Arc::clone(client);
        let task = self.task.clone();
        self.request(move || client.archive_progress(&task).map(Reply::Progress));
    }

    pub(super) fn poll(&mut self, client: &Arc<PikPak>) {
        if self.pending.is_none() && self.next_refresh.is_some_and(|when| Instant::now() >= when) {
            self.refresh(client);
        }
        let Some(rx) = &self.pending else { return };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(anyhow!("archive request interrupted")),
        };
        self.pending = None;
        match result {
            Ok(Reply::Listing(gcid, path, listing)) => {
                self.gcid = gcid;
                self.directories = path;
                self.cursor = 0;
                self.marked.clear();
                self.listed = !listing.needs_password();
                if listing.needs_password() {
                    self.password.clear();
                    self.stage = Stage::Password;
                    self.notice = format!("{} · Enter the archive password", listing.status);
                } else if !listing.task_id.is_empty() {
                    self.track(listing.task_id);
                } else {
                    self.members = listing.files;
                    self.stage = Stage::Browse;
                    self.notice.clear();
                }
            }
            Ok(Reply::Submitted(task)) => self.track(task),
            Ok(Reply::Progress(progress)) => {
                self.complete = progress.complete();
                self.notice = format!(
                    "{}%  {}  {}",
                    progress.progress, progress.phase, progress.file_id
                );
                if progress.failed() {
                    self.notice
                        .push_str(&format!(" · {}", progress.error_description));
                } else if !progress.complete() {
                    self.next_refresh = Some(
                        Instant::now()
                            + Duration::from_secs(progress.expires_in.unwrap_or(2).clamp(1, 300)),
                    );
                }
            }
            Err(e) => {
                self.notice = format!("{e:#}");
                if matches!(self.stage, Stage::ConfirmSelected | Stage::ConfirmAll) {
                    self.stage = Stage::Browse;
                }
            }
        }
    }

    fn track(&mut self, task: String) {
        self.stage = Stage::Progress;
        self.task = task;
        self.next_refresh = Some(Instant::now());
        self.notice = "Checking extraction progress…".into();
    }

    fn selected(&self) -> Vec<ArchiveMember> {
        self.members
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                if self.marked.is_empty() {
                    *i == self.cursor
                } else {
                    self.marked.contains(i)
                }
            })
            .map(|(_, m)| m.clone())
            .collect()
    }

    pub(super) fn key(&mut self, app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> bool {
        if self.pending.is_some() {
            // Wait for the submission's task ID so an extraction stays traceable.
            if matches!(self.stage, Stage::ConfirmSelected | Stage::ConfirmAll) {
                return true;
            }
            if code == KeyCode::Esc && !self.task.is_empty() {
                app.push_log(format!("Extraction task {}", self.task));
            }
            return code != KeyCode::Esc;
        }
        match self.stage {
            Stage::Password => {
                match handle_text_input(&mut self.password, &mut app.text_cursor, code, modifiers) {
                    Some(false) => return false,
                    Some(true) => self.list(&app.client, self.directories.clone()),
                    _ => {}
                }
            }
            Stage::ConfirmSelected | Stage::ConfirmAll => match code {
                KeyCode::Esc => self.stage = Stage::Browse,
                KeyCode::Enter => {
                    let members = if self.stage == Stage::ConfirmAll {
                        Vec::new()
                    } else {
                        self.selected()
                    };
                    let client = Arc::clone(&app.client);
                    let (id, gcid, password) =
                        (self.id.clone(), self.gcid.clone(), self.password.clone());
                    let guard = WriteGuard::new(&app.background_writes);
                    self.notice = "Submitting extraction…".into();
                    self.request(move || {
                        let _guard = guard;
                        client
                            .archive_extract(&id, &gcid, &password, &members)
                            .map(Reply::Submitted)
                    });
                }
                _ => {}
            },
            Stage::Progress => match code {
                KeyCode::Esc => {
                    if self.complete {
                        app.refresh();
                    }
                    app.push_log(format!("Extraction task {}", self.task));
                    return false;
                }
                KeyCode::Char('r') => self.refresh(&app.client),
                _ => {}
            },
            Stage::Browse => match code {
                KeyCode::Esc => return false,
                KeyCode::Char('j') | KeyCode::Down => {
                    self.cursor = (self.cursor + 1).min(self.members.len().saturating_sub(1))
                }
                KeyCode::Char('k') | KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
                KeyCode::Char(' ') if self.cursor < self.members.len() => {
                    if !self.marked.remove(&self.cursor) {
                        self.marked.insert(self.cursor);
                    }
                }
                KeyCode::Enter if self.members.get(self.cursor).is_some_and(|m| m.is_folder()) => {
                    let mut path = self.directories.clone();
                    path.push(self.members[self.cursor].path.clone());
                    self.list(&app.client, path);
                }
                KeyCode::Backspace if !self.directories.is_empty() => {
                    let mut path = self.directories.clone();
                    path.pop();
                    self.list(&app.client, path);
                }
                KeyCode::Char('r') => self.list(&app.client, self.directories.clone()),
                KeyCode::Char('x') if self.listed && !self.members.is_empty() => {
                    self.stage = Stage::ConfirmSelected
                }
                KeyCode::Char('A') if self.listed => self.stage = Stage::ConfirmAll,
                _ => {}
            },
        }
        true
    }

    pub(super) fn draw(&self, frame: &mut Frame) {
        let title = format!(
            " Archive · {}  {} ",
            terminal_safe_text(&self.name),
            terminal_safe_text(self.directories.last().map_or("/", String::as_str))
        );
        let block = Block::bordered().title(title);
        let inner = block.inner(frame.area());
        frame.render_widget(block, frame.area());
        let [body, notice, help] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(2),
            Constraint::Length(2),
        ])
        .areas(inner);
        let mut lines = Vec::new();
        let hint = match self.stage {
            Stage::Browse => {
                let visible = usize::from(body.height);
                let offset = (self.cursor + 1).saturating_sub(visible);
                for (i, m) in self.members.iter().enumerate().skip(offset).take(visible) {
                    let mut line = Line::from(format!(
                        " {} {}{}",
                        if self.marked.contains(&i) {
                            "[x]"
                        } else {
                            "[ ]"
                        },
                        terminal_safe_text(&m.filename),
                        if m.is_folder() { "/" } else { "" }
                    ));
                    if i == self.cursor {
                        line = line.style(Style::default().add_modifier(Modifier::REVERSED));
                    }
                    lines.push(line);
                }
                if self.listed && self.members.is_empty() {
                    lines.push(Line::from("Empty directory"));
                }
                "↑/↓ navigate · Enter open folder · Backspace parent · Space select · x extract selection · A extract all · r reload · Esc close"
            }
            Stage::Password => {
                lines.push(Line::from(format!(
                    "Password  {}",
                    "•".repeat(self.password.chars().count())
                )));
                "Enter unlock · Esc close"
            }
            Stage::ConfirmAll | Stage::ConfirmSelected => {
                let what = if self.stage == Stage::ConfirmAll {
                    "the whole archive".into()
                } else {
                    format!("{} selected member(s)", self.selected().len())
                };
                lines.push(Line::from(format!(
                    "Extract {what} to PikPak’s default cloud destination?"
                )));
                "Enter extract · Esc cancel"
            }
            Stage::Progress => {
                lines.push(Line::from(format!(
                    "Task  {}",
                    terminal_safe_text(&self.task)
                )));
                "r refresh · Esc close"
            }
        };
        frame.render_widget(Paragraph::new(lines), body);
        frame.render_widget(
            Paragraph::new(terminal_safe_text(&self.notice)).wrap(Wrap { trim: true }),
            notice,
        );
        frame.render_widget(Paragraph::new(hint).wrap(Wrap { trim: true }), help);
    }
}
