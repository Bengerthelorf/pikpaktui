mod completion;
pub(crate) mod download;
mod draw;
mod handler;
mod local_completion;

use crate::config::{AppConfig, TuiConfig};
use crate::pikpak::{Entry, PikPak};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::DefaultTerminal;
use std::collections::{HashSet, VecDeque};
use std::io;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use completion::PathInput;
use download::DownloadState;
use local_completion::LocalPathInput;

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
    ConfirmPermanentDelete {
        value: String,
    },
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
    // Cart & Downloads
    CartView,
    DownloadInput {
        input: LocalPathInput,
    },
    DownloadView,
    // Offline download URL input
    OfflineInput {
        value: String,
    },
    // Offline tasks list
    OfflineTasksView {
        tasks: Vec<crate::pikpak::OfflineTask>,
        selected: usize,
    },
    // File info popup
    InfoView {
        info: crate::pikpak::FileInfoResponse,
    },
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
    show_help_sheet: bool,
    result_rx: Receiver<OpResult>,
    result_tx: Sender<OpResult>,
    // Cart
    cart: Vec<Entry>,
    cart_ids: HashSet<String>,
    cart_selected: usize,
    // Downloads
    download_state: DownloadState,
}

impl App {
    fn new_authed(client: PikPak, config: TuiConfig) -> Self {
        let (tx, rx) = mpsc::channel();
        let mut dl_state = DownloadState::new();
        dl_state.tasks = download::load_download_state();
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
            show_help_sheet: false,
            result_rx: rx,
            result_tx: tx,
            cart: Vec::new(),
            cart_ids: HashSet::new(),
            cart_selected: 0,
            download_state: dl_state,
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
            show_help_sheet: false,
            result_rx: rx,
            result_tx: tx,
            cart: Vec::new(),
            cart_ids: HashSet::new(),
            cart_selected: 0,
            download_state: DownloadState::new(),
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
        // Save download state on exit
        download::save_download_state(&self.download_state.tasks);
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

        // Poll download progress
        let logs = self.download_state.poll(&self.client);
        for msg in logs {
            self.push_log(msg);
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
        self.loading = true;
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        let fid = self.current_folder_id.clone();
        std::thread::spawn(move || {
            let _ = tx.send(OpResult::Ls(client.ls(&fid)));
        });
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
    let v = Layout::default()
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
        .split(v[1])[1]
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
