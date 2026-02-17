mod completion;
pub(crate) mod download;
mod download_view;
mod draw;
mod handler;
mod local_completion;

pub use download_view::{DownloadViewMode, NetworkStats};

use crate::config::{AppConfig, TuiConfig};
use crate::pikpak::{Entry, EntryKind, FileInfoResponse, PikPak};
use crate::theme;
use anyhow::Result;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::DefaultTerminal;
use ratatui::layout::{Constraint, Direction, Layout};
use std::cell::Cell;
use std::collections::{HashSet, VecDeque};
use std::io;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, LazyLock};
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
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    let mut terminal = ratatui::init();
    let res = app.run(&mut terminal);
    ratatui::restore();
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
    disable_raw_mode()?;
    res
}

#[derive(Clone)]
enum LoginField {
    Email,
    Password,
}

enum PreviewState {
    Empty,
    Loading,
    FolderListing(Vec<Entry>),
    FileBasicInfo,
    FileDetailedInfo(FileInfoResponse),
    FileTextPreview {
        name: String,
        lines: Vec<ratatui::text::Line<'static>>,
        size: u64,
        truncated: bool,
    },
    ThumbnailImage {
        image: image::DynamicImage,
    },
}

pub(crate) struct PlayOption {
    pub label: String,
    pub url: String,
    pub available: bool,
}

enum OpResult {
    Ls(Result<Vec<Entry>>),
    Ok(String),
    Err(String),
    Info(Result<FileInfoResponse>),
    ParentLs(String, Result<Vec<Entry>>),
    PreviewLs(String, Result<Vec<Entry>>),
    PreviewInfo(String, Result<FileInfoResponse>),
    PreviewText(String, Result<(String, String, u64, bool)>),
    PreviewThumbnail(String, Result<image::DynamicImage>),
    OfflineTasks(Result<Vec<crate::pikpak::OfflineTask>>),
    PlayInfo(Result<FileInfoResponse>),
    PlayPickerInfo(Result<(FileInfoResponse, Vec<PlayOption>)>),
    TrashList(Result<Vec<Entry>>),
    TrashOp(String),
    TrashInfo(Result<FileInfoResponse>),
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
    // Info popup (show_preview=false mode)
    InfoLoading,
    InfoView {
        info: FileInfoResponse,
    },
    InfoFolderView {
        name: String,
        entries: Vec<Entry>,
    },
    TextPreviewView {
        name: String,
        lines: Vec<ratatui::text::Line<'static>>,
        truncated: bool,
    },
    ConfirmPlay {
        name: String,
        url: String,
    },
    PlayPicker {
        name: String,
        medias: Vec<PlayOption>,
        selected: usize,
    },
    PlayerInput {
        value: String,
        pending_url: String,
    },
    TrashView {
        entries: Vec<Entry>,
        selected: usize,
        expanded: bool,
    },
    ConfirmQuit,
    Settings {
        selected: usize,
        editing: bool,
        draft: TuiConfig,
        modified: bool,
    },
    CustomColorSettings {
        selected: usize,
        draft: TuiConfig,
        modified: bool,
        editing_rgb: bool,
        rgb_input: String,
        rgb_component: usize, // 0=R, 1=G, 2=B
    },
    ImageProtocolSettings {
        selected: usize,
        draft: TuiConfig,
        modified: bool,
        current_terminal: String,
        terminals: Vec<String>,
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
    // Miller columns
    parent_entries: Vec<Entry>,
    parent_selected: usize,
    preview_state: PreviewState,
    preview_target_id: Option<String>,
    preview_target_name: Option<String>,
    show_logs_overlay: bool,
    last_cursor_move: Instant,
    pending_preview_fetch: bool,
    // Cart
    cart: Vec<Entry>,
    cart_ids: HashSet<String>,
    cart_selected: usize,
    // Downloads
    download_state: DownloadState,
    download_view_mode: DownloadViewMode,
    network_stats: NetworkStats,
    last_network_update: Instant,
    // Mouse support: pane areas recorded during draw
    current_pane_area: Cell<ratatui::layout::Rect>,
    parent_pane_area: Cell<ratatui::layout::Rect>,
    preview_pane_area: Cell<ratatui::layout::Rect>,
    scroll_offset: Cell<usize>,
    parent_scroll_offset: Cell<usize>,
    // Double-click detection
    last_click_time: Instant,
    last_click_pos: (u16, u16),
    // Preview pane scroll offset
    preview_scroll: usize,
    // Log overlay scroll offset and area for mouse support
    /// `None` = auto-follow bottom; `Some(y)` = pinned at absolute scroll-from-top offset
    logs_scroll: Option<usize>,
    logs_overlay_area: Cell<ratatui::layout::Rect>,
    // Settings overlay area for mouse support
    settings_area: Cell<ratatui::layout::Rect>,
    // Trash view state (preserved across InfoView popup)
    trash_entries: Vec<Entry>,
    trash_selected: usize,
    trash_expanded: bool,
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
            parent_entries: Vec::new(),
            parent_selected: 0,
            preview_state: PreviewState::Empty,
            preview_target_id: None,
            preview_target_name: None,
            show_logs_overlay: false,
            last_cursor_move: Instant::now(),
            pending_preview_fetch: false,
            cart: Vec::new(),
            cart_ids: HashSet::new(),
            cart_selected: 0,
            download_state: dl_state,
            download_view_mode: DownloadViewMode::Collapsed,
            network_stats: NetworkStats::new(),
            last_network_update: Instant::now(),
            current_pane_area: Cell::new(ratatui::layout::Rect::default()),
            parent_pane_area: Cell::new(ratatui::layout::Rect::default()),
            preview_pane_area: Cell::new(ratatui::layout::Rect::default()),
            scroll_offset: Cell::new(0),
            parent_scroll_offset: Cell::new(0),
            last_click_time: Instant::now(),
            last_click_pos: (0, 0),
            preview_scroll: 0,
            logs_scroll: None,
            logs_overlay_area: Cell::new(ratatui::layout::Rect::default()),
            settings_area: Cell::new(ratatui::layout::Rect::default()),
            trash_entries: Vec::new(),
            trash_selected: 0,
            trash_expanded: false,
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
            parent_entries: Vec::new(),
            parent_selected: 0,
            preview_state: PreviewState::Empty,
            preview_target_id: None,
            preview_target_name: None,
            show_logs_overlay: false,
            last_cursor_move: Instant::now(),
            pending_preview_fetch: false,
            cart: Vec::new(),
            cart_ids: HashSet::new(),
            cart_selected: 0,
            download_state: DownloadState::new(),
            download_view_mode: DownloadViewMode::Collapsed,
            network_stats: NetworkStats::new(),
            last_network_update: Instant::now(),
            current_pane_area: Cell::new(ratatui::layout::Rect::default()),
            parent_pane_area: Cell::new(ratatui::layout::Rect::default()),
            preview_pane_area: Cell::new(ratatui::layout::Rect::default()),
            scroll_offset: Cell::new(0),
            parent_scroll_offset: Cell::new(0),
            last_click_time: Instant::now(),
            last_click_pos: (0, 0),
            preview_scroll: 0,
            logs_scroll: None,
            logs_overlay_area: Cell::new(ratatui::layout::Rect::default()),
            settings_area: Cell::new(ratatui::layout::Rect::default()),
            trash_entries: Vec::new(),
            trash_selected: 0,
            trash_expanded: false,
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
            if self.last_spinner.elapsed() >= Duration::from_millis(80) {
                self.spinner_idx = (self.spinner_idx + 1) % SPINNER_FRAMES.len();
                self.last_spinner = Instant::now();
            }
            self.poll_results();

            // Debounce: auto-fetch preview after 300ms if lazy_preview enabled
            if self.config.lazy_preview
                && self.pending_preview_fetch
                && self.last_cursor_move.elapsed() >= Duration::from_millis(300)
            {
                self.pending_preview_fetch = false;
                // Skip auto-loading for large text files
                let skip = self.entries.get(self.selected).is_some_and(|e| {
                    e.kind == EntryKind::File
                        && theme::is_text_previewable(e)
                        && e.size > self.config.preview_max_size
                });
                if !skip {
                    self.fetch_preview_for_selected();
                }
            }

            terminal.draw(|f| self.draw(f))?;

            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(key) => {
                        if key.kind != KeyEventKind::Press {
                            continue;
                        }
                        self.cursor_visible = true;
                        self.last_blink = Instant::now();
                        if self.handle_key(key.code, key.modifiers)? {
                            break;
                        }
                    }
                    Event::Mouse(mouse) => {
                        self.handle_mouse(mouse);
                    }
                    _ => {}
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
                OpResult::Ls(Ok(mut entries)) => {
                    self.loading = false;
                    crate::config::sort_entries(&mut entries, self.config.sort_field, self.config.sort_reverse);
                    self.entries = entries;
                    if self.selected >= self.entries.len() {
                        self.selected = self.entries.len().saturating_sub(1);
                    }
                    self.push_log(format!("Refreshed {}", self.current_path_display()));
                    self.on_cursor_move();
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
                OpResult::Info(Ok(info)) => {
                    self.loading = false;
                    if matches!(self.input, InputMode::InfoLoading) {
                        self.input = InputMode::InfoView { info };
                    }
                }
                OpResult::Info(Err(e)) => {
                    self.loading = false;
                    if matches!(self.input, InputMode::InfoLoading) {
                        self.input = InputMode::Normal;
                    }
                    self.push_log(format!("File info failed: {e:#}"));
                }
                OpResult::ParentLs(pid, Ok(mut entries)) => {
                    // Only accept if this is still the expected parent
                    let expected = self.breadcrumb.last().map(|(id, _)| id.as_str());
                    if expected == Some(&pid) {
                        crate::config::sort_entries(&mut entries, self.config.sort_field, self.config.sort_reverse);
                        self.parent_entries = entries;
                        if let Some(pos) = self
                            .parent_entries
                            .iter()
                            .position(|e| e.id == self.current_folder_id)
                        {
                            self.parent_selected = pos;
                        }
                    }
                }
                OpResult::ParentLs(pid, Err(e)) => {
                    let expected = self.breadcrumb.last().map(|(id, _)| id.as_str());
                    if expected == Some(&pid) {
                        self.push_log(format!("Parent listing failed: {e:#}"));
                    }
                }
                OpResult::PreviewLs(id, Ok(mut children)) => {
                    crate::config::sort_entries(&mut children, self.config.sort_field, self.config.sort_reverse);
                    if matches!(self.input, InputMode::InfoLoading) {
                        // Popup mode (show_preview=false)
                        self.loading = false;
                        let name = self.preview_target_name.take().unwrap_or_default();
                        self.preview_state = PreviewState::FolderListing(children.clone());
                        self.preview_target_id = Some(id);
                        self.input = InputMode::InfoFolderView {
                            name,
                            entries: children,
                        };
                    } else if self.preview_target_id.as_deref() == Some(&id) {
                        self.preview_state = PreviewState::FolderListing(children);
                    }
                }
                OpResult::PreviewLs(id, Err(e)) => {
                    if matches!(self.input, InputMode::InfoLoading) {
                        self.loading = false;
                        self.input = InputMode::Normal;
                    } else if self.preview_target_id.as_deref() == Some(&id) {
                        self.preview_state = PreviewState::Empty;
                    }
                    self.push_log(format!("Folder listing failed: {e:#}"));
                }
                OpResult::PreviewInfo(id, Ok(info)) => {
                    if self.preview_target_id.as_deref() == Some(&id) {
                        self.preview_state = PreviewState::FileDetailedInfo(info);
                    }
                }
                OpResult::PreviewInfo(id, Err(e)) => {
                    if self.preview_target_id.as_deref() == Some(&id) {
                        self.preview_state = PreviewState::Empty;
                    }
                    self.push_log(format!("Preview info failed: {e:#}"));
                }
                OpResult::PreviewText(id, Ok((name, content, size, truncated))) => {
                    let lines = highlight_content(&name, &content);
                    if matches!(self.input, InputMode::InfoLoading) {
                        self.loading = false;
                        self.input = InputMode::TextPreviewView {
                            name: name.clone(),
                            lines: lines.clone(),
                            truncated,
                        };
                        self.preview_state = PreviewState::FileTextPreview {
                            name,
                            lines,
                            size,
                            truncated,
                        };
                        self.preview_target_id = Some(id);
                    } else if self.preview_target_id.as_deref() == Some(&id) {
                        self.preview_state = PreviewState::FileTextPreview {
                            name,
                            lines,
                            size,
                            truncated,
                        };
                    }
                }
                OpResult::PreviewText(id, Err(e)) => {
                    if matches!(self.input, InputMode::InfoLoading) {
                        self.loading = false;
                        self.input = InputMode::Normal;
                    } else if self.preview_target_id.as_deref() == Some(&id) {
                        self.preview_state = PreviewState::FileBasicInfo;
                    }
                    self.push_log(format!("Text preview failed: {e:#}"));
                }
                OpResult::PreviewThumbnail(id, Ok(image)) => {
                    if self.preview_target_id.as_deref() == Some(&id) {
                        self.preview_state = PreviewState::ThumbnailImage { image };
                    }
                }
                OpResult::PreviewThumbnail(id, Err(e)) => {
                    if self.preview_target_id.as_deref() == Some(&id) {
                        self.preview_state = PreviewState::FileBasicInfo;
                    }
                    self.push_log(format!("Thumbnail preview failed: {e:#}"));
                }
                OpResult::OfflineTasks(Ok(tasks)) => {
                    self.loading = false;
                    if matches!(self.input, InputMode::InfoLoading) {
                        self.input = InputMode::OfflineTasksView { tasks, selected: 0 };
                    }
                }
                OpResult::OfflineTasks(Err(e)) => {
                    self.loading = false;
                    if matches!(self.input, InputMode::InfoLoading) {
                        self.input = InputMode::Normal;
                    }
                    self.push_log(format!("Failed to load offline tasks: {e:#}"));
                }
                OpResult::PlayInfo(Ok(info)) => {
                    self.loading = false;
                    let url = info
                        .web_content_link
                        .as_deref()
                        .or(info.links.as_ref().and_then(|l| {
                            l.get("application/octet-stream")
                                .and_then(|v| v.url.as_deref())
                        }))
                        .unwrap_or("")
                        .to_string();
                    if url.is_empty() {
                        self.push_log("No playback URL available".into());
                    } else {
                        self.input = InputMode::ConfirmPlay {
                            name: info.name.clone(),
                            url,
                        };
                    }
                }
                OpResult::PlayInfo(Err(e)) => {
                    self.loading = false;
                    self.push_log(format!("Play info failed: {e:#}"));
                }
                OpResult::PlayPickerInfo(Ok((info, medias))) => {
                    self.loading = false;
                    if medias.is_empty() {
                        self.push_log("No playback streams available".into());
                    } else {
                        // Select first available
                        let first_avail = medias.iter().position(|m| m.available).unwrap_or(0);
                        self.input = InputMode::PlayPicker {
                            name: info.name.clone(),
                            medias,
                            selected: first_avail,
                        };
                    }
                }
                OpResult::PlayPickerInfo(Err(e)) => {
                    self.loading = false;
                    self.push_log(format!("Play picker info failed: {e:#}"));
                }
                OpResult::TrashList(Ok(entries)) => {
                    self.loading = false;
                    let expanded = if let InputMode::TrashView { expanded, .. } = &self.input {
                        *expanded
                    } else {
                        self.trash_expanded
                    };
                    self.trash_entries = entries.clone();
                    self.trash_selected = 0;
                    self.trash_expanded = expanded;
                    self.input = InputMode::TrashView {
                        entries,
                        selected: 0,
                        expanded,
                    };
                }
                OpResult::TrashList(Err(e)) => {
                    self.loading = false;
                    if matches!(self.input, InputMode::InfoLoading) {
                        self.input = InputMode::Normal;
                    }
                    self.push_log(format!("Failed to load trash: {e:#}"));
                }
                OpResult::TrashOp(msg) => {
                    self.loading = false;
                    self.push_log(msg);
                    // Re-fetch trash list
                    self.open_trash_view_preserve();
                }
                OpResult::TrashInfo(Ok(info)) => {
                    self.loading = false;
                    if matches!(self.input, InputMode::InfoLoading) {
                        self.input = InputMode::InfoView { info };
                    }
                }
                OpResult::TrashInfo(Err(e)) => {
                    self.loading = false;
                    if matches!(self.input, InputMode::InfoLoading) {
                        // Restore trash view
                        self.input = InputMode::TrashView {
                            entries: std::mem::take(&mut self.trash_entries),
                            selected: self.trash_selected,
                            expanded: self.trash_expanded,
                        };
                    }
                    self.push_log(format!("Trash file info failed: {e:#}"));
                }
            }
        }

        // Poll download progress
        let logs = self.download_state.poll(&self.client);
        for msg in logs {
            self.push_log(msg);
        }

        // Update network stats (every 500ms)
        if self.last_network_update.elapsed() >= Duration::from_millis(500) {
            let current_speed: f64 = self
                .download_state
                .tasks
                .iter()
                .filter(|t| t.status == download::TaskStatus::Downloading)
                .map(|t| t.speed / 1_048_576.0) // Convert to MB/s
                .sum();
            self.network_stats.update(current_speed);
            self.last_network_update = Instant::now();
        }
    }

    fn attempt_login(&mut self, email: &str, password: &str) {
        let client =
            Arc::get_mut(&mut self.client).expect("no other references to client during login");
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
        self.refresh_parent();
    }

    fn refresh_parent(&mut self) {
        if let Some((parent_id, _)) = self.breadcrumb.last() {
            let client = Arc::clone(&self.client);
            let tx = self.result_tx.clone();
            let pid = parent_id.clone();
            std::thread::spawn(move || {
                let _ = tx.send(OpResult::ParentLs(pid.clone(), client.ls(&pid)));
            });
        } else {
            // At root — no parent
            self.parent_entries.clear();
            self.parent_selected = 0;
        }
    }

    fn clear_preview(&mut self) {
        self.preview_state = PreviewState::Empty;
        self.preview_target_id = None;
        self.preview_target_name = None;
        self.pending_preview_fetch = false;
        self.preview_scroll = 0;
    }

    fn on_cursor_move(&mut self) {
        self.preview_scroll = 0;
        // No preview pane when show_preview=false
        if !self.config.show_preview {
            return;
        }
        self.last_cursor_move = Instant::now();
        if let Some(entry) = self.entries.get(self.selected) {
            match entry.kind {
                EntryKind::File => {
                    self.preview_state = PreviewState::FileBasicInfo;
                    self.preview_target_id = Some(entry.id.clone());
                }
                EntryKind::Folder => {
                    self.preview_state = PreviewState::Empty;
                    self.preview_target_id = Some(entry.id.clone());
                }
            }
            if self.config.lazy_preview {
                self.pending_preview_fetch = true;
            }
        } else {
            self.clear_preview();
        }
    }

    fn fetch_preview_for_selected(&mut self) {
        let entry = match self.entries.get(self.selected) {
            Some(e) => e.clone(),
            None => return,
        };
        self.preview_target_id = Some(entry.id.clone());
        self.preview_state = PreviewState::Loading;
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        let eid = entry.id.clone();
        match entry.kind {
            EntryKind::Folder => {
                // Folders always show content listing, never thumbnails
                std::thread::spawn(move || {
                    let _ = tx.send(OpResult::PreviewLs(eid.clone(), client.ls(&eid)));
                });
            }
            EntryKind::File => {
                if let Some(ref thumb_url) = entry.thumbnail_link {
                    if !thumb_url.is_empty() {
                        let thumb_url = thumb_url.clone();
                        std::thread::spawn(move || {
                            let _ = tx.send(OpResult::PreviewThumbnail(
                                eid.clone(),
                                fetch_and_render_thumbnail(&thumb_url, &client),
                            ));
                        });
                        return;
                    }
                }
                if theme::is_text_previewable(&entry) {
                    let max_bytes = self.config.preview_max_size;
                    std::thread::spawn(move || {
                        let _ = tx.send(OpResult::PreviewText(
                            eid.clone(),
                            client.fetch_text_preview(&eid, max_bytes),
                        ));
                    });
                } else {
                    std::thread::spawn(move || {
                        let _ = tx.send(OpResult::PreviewInfo(eid.clone(), client.file_info(&eid)));
                    });
                }
            }
        }
    }

    fn open_trash_view_preserve(&mut self) {
        self.input = InputMode::InfoLoading;
        self.loading = true;
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(OpResult::TrashList(client.ls_trash(200)));
        });
    }

    fn resort_entries(&mut self) {
        crate::config::sort_entries(&mut self.entries, self.config.sort_field, self.config.sort_reverse);
        if self.selected >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
        let arrow = if self.config.sort_reverse { "\u{2193}" } else { "\u{2191}" };
        self.push_log(format!("Sort: {} {}", self.config.sort_field.as_str(), arrow));
    }

    fn fetch_text_preview_for_selected(&mut self) {
        let entry = match self.entries.get(self.selected) {
            Some(e) => e.clone(),
            None => return,
        };
        if entry.kind != EntryKind::File || !theme::is_text_previewable(&entry) {
            return;
        }
        self.preview_target_id = Some(entry.id.clone());
        self.preview_state = PreviewState::Loading;
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        let eid = entry.id.clone();
        let max_bytes = self.config.preview_max_size;
        std::thread::spawn(move || {
            let _ = tx.send(OpResult::PreviewText(
                eid.clone(),
                client.fetch_text_preview(&eid, max_bytes),
            ));
        });
    }
}

static SYNTAX_SET: LazyLock<syntect::parsing::SyntaxSet> =
    LazyLock::new(|| syntect::parsing::SyntaxSet::load_defaults_newlines());
static THEME_SET: LazyLock<syntect::highlighting::ThemeSet> =
    LazyLock::new(|| syntect::highlighting::ThemeSet::load_defaults());

fn highlight_content(name: &str, content: &str) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::style::{Color, Style};
    use ratatui::text::{Line, Span};
    use syntect::easy::HighlightLines;

    let ext = name.rsplit('.').next().unwrap_or("");
    let syntax = SYNTAX_SET
        .find_syntax_by_extension(ext)
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
    let theme = &THEME_SET.themes["base16-ocean.dark"];
    let mut h = HighlightLines::new(syntax, theme);

    content
        .lines()
        .enumerate()
        .map(|(i, line)| {
            let mut spans = vec![Span::styled(
                format!("{:>4} ", i + 1),
                Style::default().fg(Color::DarkGray),
            )];
            match h.highlight_line(line, &SYNTAX_SET) {
                Ok(ranges) => {
                    for (style, text) in ranges {
                        let fg = style.foreground;
                        spans.push(Span::styled(
                            text.to_string(),
                            Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b)),
                        ));
                    }
                }
                Err(_) => {
                    spans.push(Span::styled(
                        line.to_string(),
                        Style::default().fg(Color::White),
                    ));
                }
            }
            Line::from(spans)
        })
        .collect()
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

fn fetch_and_render_thumbnail(
    url: &str,
    client: &crate::pikpak::PikPak,
) -> Result<image::DynamicImage> {
    use anyhow::Context;
    use image::ImageReader;
    use std::io::Cursor;

    let response = client
        .http()
        .get(url)
        .send()
        .context("failed to download thumbnail")?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!("thumbnail download failed: {}", response.status()));
    }

    let bytes = response.bytes().context("failed to read thumbnail bytes")?;
    let img = ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .context("failed to guess image format")?
        .decode()
        .context("failed to decode thumbnail image")?;

    Ok(img)
}

/// Wrap a string into visual lines based on display width.
/// Each returned `String` fits within `max_width` display columns.
pub(crate) fn wrap_line(s: &str, max_width: usize) -> Vec<String> {
    use unicode_width::UnicodeWidthChar;
    if max_width == 0 {
        return vec![s.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width: usize = 0;
    for ch in s.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_width + ch_width > max_width && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push(ch);
        current_width += ch_width;
    }
    lines.push(current);
    lines
}

/// Wrap all log messages and return total visual line count.
pub(crate) fn wrap_logs<'a, I>(logs: I, max_width: usize) -> Vec<String>
where
    I: Iterator<Item = &'a str>,
{
    let mut all_lines = Vec::new();
    for msg in logs {
        all_lines.extend(wrap_line(msg, max_width));
    }
    all_lines
}

#[cfg(test)]
mod wrap_tests {
    use super::{wrap_line, wrap_logs};

    #[test]
    fn empty_string_gives_one_line() {
        assert_eq!(wrap_line("", 50), vec![""]);
    }

    #[test]
    fn short_string_no_wrap() {
        assert_eq!(wrap_line("hello", 50), vec!["hello"]);
    }

    #[test]
    fn exact_fit_no_wrap() {
        assert_eq!(wrap_line("abcde", 5), vec!["abcde"]);
    }

    #[test]
    fn simple_wrap() {
        assert_eq!(wrap_line("abcdefgh", 5), vec!["abcde", "fgh"]);
    }

    #[test]
    fn multiple_wraps() {
        assert_eq!(
            wrap_line("abcdefghijklm", 5),
            vec!["abcde", "fghij", "klm"]
        );
    }

    #[test]
    fn cjk_double_width() {
        // Each CJK char is width 2, so 3 chars = width 6
        // In a width-5 area, "三上" (width 4) fits, "悠" starts new line
        assert_eq!(wrap_line("三上悠", 5), vec!["三上", "悠"]);
    }

    #[test]
    fn cjk_exact_fit() {
        // "三上" = width 4, fits in width 4
        assert_eq!(wrap_line("三上", 4), vec!["三上"]);
    }

    #[test]
    fn mixed_ascii_cjk() {
        // "ab三" = 2 + 2 = 4 width, fits in 5
        // "cd" = 2, next line
        assert_eq!(wrap_line("ab三cd", 5), vec!["ab三c", "d"]);
    }

    #[test]
    fn long_url_wrap() {
        let url = "https://dl-z01a-0049.mypikpak.com/download/?fid=KKGF0zFia";
        let lines = wrap_line(url, 20);
        // Each line should be at most 20 chars wide
        for line in &lines {
            assert!(
                unicode_width::UnicodeWidthStr::width(line.as_str()) <= 20,
                "line too wide: {:?} (width {})",
                line,
                unicode_width::UnicodeWidthStr::width(line.as_str())
            );
        }
        // Rejoin should give original
        let rejoined: String = lines.concat();
        assert_eq!(rejoined, url);
    }

    #[test]
    fn wrap_logs_total_lines() {
        let logs = vec![
            "short",
            "a]medium length line here",
            "abcdefghijklmnopqrstuvwxyz",
        ];
        let wrapped = wrap_logs(logs.iter().copied(), 10);
        // "short" → 1 line
        // "a]medium length line here" (24 chars) → 3 lines
        // "abcdefghijklmnopqrstuvwxyz" (26 chars) → 3 lines
        assert_eq!(wrapped.len(), 7);
    }

    #[test]
    fn scroll_bottom_shows_last_lines() {
        let logs = vec!["line1", "line2", "line3", "line4", "line5"];
        let wrapped = wrap_logs(logs.iter().copied(), 50);
        let visible = 3;
        let max_scroll = wrapped.len().saturating_sub(visible); // 5 - 3 = 2
        // At bottom (scroll_y = max_scroll = 2), show lines 2..5
        let bottom: Vec<&str> = wrapped.iter().skip(max_scroll).take(visible).map(|s| s.as_str()).collect();
        assert_eq!(bottom, vec!["line3", "line4", "line5"]);
    }

    #[test]
    fn scroll_with_wrapped_lines_reaches_bottom() {
        let logs = vec![
            "short",
            "this is a very long line that will wrap multiple times in a narrow window!",
            "last line",
        ];
        let width = 20;
        let visible = 5;
        let wrapped = wrap_logs(logs.iter().copied(), width);
        let total = wrapped.len();
        let max_scroll = total.saturating_sub(visible);
        // At bottom, last visible line should be "last line"
        let bottom: Vec<&str> = wrapped.iter().skip(max_scroll).take(visible).map(|s| s.as_str()).collect();
        assert_eq!(bottom.last().unwrap(), &"last line");
    }
}

