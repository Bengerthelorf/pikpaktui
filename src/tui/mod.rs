mod completion;
pub(crate) mod download;
mod download_view;
mod draw;
mod handler;
mod image_render;
mod local_completion;
mod widgets;

pub use download_view::{DownloadViewMode, NetworkStats};

use crate::config::{AppConfig, TuiConfig};
use crate::pikpak::{Entry, EntryKind, FileInfoResponse, PikPak};
use crate::theme;
use anyhow::Result;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
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

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
}

fn run_terminal(mut app: App) -> Result<()> {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        original_hook(info);
    }));

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    let backend = ratatui::backend::CrosstermBackend::new(io::stdout());
    let mut terminal = ratatui::Terminal::new(backend)?;
    let res = app.run(&mut terminal);
    restore_terminal();
    res
}

#[derive(Clone)]
enum LoginField {
    Email,
    Password,
}

#[allow(clippy::large_enum_variant)]
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

#[derive(Clone, Copy)]
struct ActionItem {
    key: KeyCode,
    shortcut: &'static str,
    label: &'static str,
}

const NORMAL_ACTIONS: &[ActionItem] = &[
    ActionItem {
        key: KeyCode::Enter,
        shortcut: "Enter",
        label: "Open folder / play video",
    },
    ActionItem {
        key: KeyCode::Char(' '),
        shortcut: "Space",
        label: "Show file or folder information",
    },
    ActionItem {
        key: KeyCode::Char('p'),
        shortcut: "p",
        label: "Preview selected file",
    },
    ActionItem {
        key: KeyCode::Char('w'),
        shortcut: "w",
        label: "Choose playback stream",
    },
    ActionItem {
        key: KeyCode::Char('a'),
        shortcut: "a",
        label: "Add or remove selected item from cart",
    },
    ActionItem {
        key: KeyCode::Char('A'),
        shortcut: "A",
        label: "Open cart",
    },
    ActionItem {
        key: KeyCode::Char('D'),
        shortcut: "D",
        label: "Open downloads",
    },
    ActionItem {
        key: KeyCode::Char('M'),
        shortcut: "M",
        label: "Open my shares",
    },
    ActionItem {
        key: KeyCode::Char('m'),
        shortcut: "m",
        label: "Move selected item",
    },
    ActionItem {
        key: KeyCode::Char('c'),
        shortcut: "c",
        label: "Copy selected item",
    },
    ActionItem {
        key: KeyCode::Char('n'),
        shortcut: "n",
        label: "Rename selected item",
    },
    ActionItem {
        key: KeyCode::Char('d'),
        shortcut: "d",
        label: "Delete selected item",
    },
    ActionItem {
        key: KeyCode::Char('f'),
        shortcut: "f",
        label: "Create folder",
    },
    ActionItem {
        key: KeyCode::Char('s'),
        shortcut: "s",
        label: "Star or unstar selected item",
    },
    ActionItem {
        key: KeyCode::Char('y'),
        shortcut: "y",
        label: "Copy download link",
    },
    ActionItem {
        key: KeyCode::Char('u'),
        shortcut: "u",
        label: "Upload file or folder",
    },
    ActionItem {
        key: KeyCode::Char('o'),
        shortcut: "o",
        label: "Create cloud download",
    },
    ActionItem {
        key: KeyCode::Char('O'),
        shortcut: "O",
        label: "Open offline tasks",
    },
    ActionItem {
        key: KeyCode::Char('t'),
        shortcut: "t",
        label: "Open trash",
    },
    ActionItem {
        key: KeyCode::Char(','),
        shortcut: ",",
        label: "Open settings",
    },
    ActionItem {
        key: KeyCode::Char('l'),
        shortcut: "l",
        label: "Toggle logs",
    },
    ActionItem {
        key: KeyCode::Char('r'),
        shortcut: "r",
        label: "Refresh current folder",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AsyncRequestKind {
    Info,
    FolderPreview,
    FilePreview,
    OfflineTasks,
    Play,
    PlayPicker,
    GotoPath,
    ParentListing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AsyncRequest {
    id: u64,
    kind: AsyncRequestKind,
    target: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StatusKind {
    Info,
    Warning,
    Error,
}

struct StatusMessage {
    text: String,
    kind: StatusKind,
    expires_at: Instant,
}

enum OpResult {
    /// Main-pane folder listing. Folder and request ids prevent a delayed
    /// reply from replacing a newer listing, including A -> B -> A navigation.
    Ls(u64, String, Result<Vec<Entry>>),
    Ok(String),
    Err(String),
    Info(AsyncRequest, Result<FileInfoResponse>, Option<String>),
    ParentLs(AsyncRequest, Result<Vec<Entry>>),
    PreviewLs(AsyncRequest, Result<Vec<Entry>>),
    PreviewInfo(AsyncRequest, Result<FileInfoResponse>),
    PreviewText(AsyncRequest, Result<(String, String, u64, bool)>),
    PreviewThumbnail(AsyncRequest, Result<image::DynamicImage>),
    OfflineTasks(AsyncRequest, Result<Vec<crate::pikpak::OfflineTask>>),
    PlayInfo(AsyncRequest, Result<FileInfoResponse>),
    PlayPickerInfo(AsyncRequest, Result<(FileInfoResponse, Vec<PlayOption>)>),
    TrashList(Result<Vec<Entry>>),
    TrashOp(String),
    OfflineOp(String),
    InfoThumbnail(AsyncRequest, Result<image::DynamicImage>),
    GotoPath(AsyncRequest, Result<(String, Vec<(String, String)>)>),
    Quota(Result<crate::pikpak::QuotaInfo>),
    Upload(Result<String>),
    ShareCreated {
        title: String,
        url: String,
        pass_code: String,
    },
    MyShares(Result<Vec<crate::pikpak::MyShare>>),
    UpdateAvailable(Option<String>),
    /// Folder listing for the move/copy picker; request and folder ids guard
    /// against replies from both older navigation and cancelled dialogs.
    PickerLs(u64, String, Result<Vec<Entry>>),
    /// Tab-completion candidates computed off-thread; `value` is the input
    /// text they were computed for.
    PathCandidates {
        request_id: u64,
        value: String,
        folder_context: String,
        parent: String,
        matches: Vec<String>,
    },
    LoginDone {
        email: String,
        password: String,
        result: std::result::Result<(), String>,
    },
}

#[derive(Default)]
struct PickerState {
    folder_id: String,
    listing_request_id: u64,
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
    ActionMenu {
        selected: usize,
    },
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
    MoveInput {
        source: Entry,
        input: PathInput,
    },
    CopyInput {
        source: Entry,
        input: PathInput,
    },
    MovePicker {
        source: Entry,
        picker: PickerState,
    },
    CopyPicker {
        source: Entry,
        picker: PickerState,
    },
    CartView,
    CartMoveInput {
        input: PathInput,
    },
    CartCopyInput {
        input: PathInput,
    },
    CartMovePicker {
        picker: PickerState,
    },
    CartCopyPicker {
        picker: PickerState,
    },
    ConfirmCartDelete,
    DownloadInput {
        input: LocalPathInput,
    },
    UploadInput {
        input: LocalPathInput,
    },
    DownloadView,
    OfflineInput {
        value: String,
    },
    OfflineTasksView {
        tasks: Vec<crate::pikpak::OfflineTask>,
        selected: usize,
    },
    InfoLoading,
    InfoView {
        request_id: u64,
        target_id: String,
        info: FileInfoResponse,
        image: Option<image::DynamicImage>,
        has_thumbnail: bool,
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
    SharePrompt,
    ShareCreatedView {
        shares: Vec<(String, String, String)>, // (title, url, pass_code)
    },
    MySharesView {
        shares: Vec<crate::pikpak::MyShare>,
        selected: usize,
        confirm_delete: Option<String>, // share_id pending delete confirmation
    },
    ConfirmQuit,
    GotoPath {
        query: String,
    },
    Settings {
        selected: usize,
        editing: bool,
        draft: TuiConfig,
        modified: bool,
    },
    ConfirmDiscardSettings {
        selected: usize,
        draft: TuiConfig,
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
    main_listing_request_id: u64,
    async_request_generation: u64,
    modal_request: Option<AsyncRequest>,
    preview_request: Option<AsyncRequest>,
    parent_listing_request: Option<AsyncRequest>,
    path_completion_in_flight: Option<u64>,
    breadcrumb: Vec<(String, String)>,
    entries: Vec<Entry>,
    selected: usize,
    logs: VecDeque<String>,
    status_message: Option<StatusMessage>,
    input: InputMode,
    cursor_visible: bool,
    /// UTF-8 byte offset for the active text field. `usize::MAX` means end.
    text_cursor: usize,
    last_blink: Instant,
    loading: bool,
    spinner_idx: usize,
    last_spinner: Instant,
    show_help_sheet: bool,
    help_scroll: usize,
    help_scroll_max: Cell<usize>,
    result_rx: Receiver<OpResult>,
    result_tx: Sender<OpResult>,
    parent_entries: Vec<Entry>,
    parent_selected: usize,
    preview_state: PreviewState,
    preview_target_id: Option<String>,
    preview_target_name: Option<String>,
    show_logs_overlay: bool,
    last_cursor_move: Instant,
    pending_preview_fetch: bool,
    cart: Vec<Entry>,
    cart_ids: HashSet<String>,
    cart_selected: usize,
    download_state: DownloadState,
    download_view_mode: DownloadViewMode,
    network_stats: NetworkStats,
    last_network_update: Instant,
    current_pane_area: Cell<ratatui::layout::Rect>,
    parent_pane_area: Cell<ratatui::layout::Rect>,
    preview_pane_area: Cell<ratatui::layout::Rect>,
    scroll_offset: Cell<usize>,
    parent_scroll_offset: Cell<usize>,
    list_area_height: Cell<u16>,
    last_click_time: Instant,
    last_click_pos: (u16, u16),
    preview_scroll: usize,
    /// `None` = auto-follow bottom; `Some(y)` = pinned at absolute scroll-from-top offset
    logs_scroll: Option<usize>,
    logs_overlay_area: Cell<ratatui::layout::Rect>,
    settings_area: Cell<ratatui::layout::Rect>,
    mouse_list_area: Cell<ratatui::layout::Rect>,
    mouse_list_first_row: Cell<u16>,
    mouse_list_offset: Cell<usize>,
    mouse_list_visible: Cell<usize>,
    trash_entries: Vec<Entry>,
    trash_selected: usize,
    trash_expanded: bool,
    loading_label: Option<String>,
    quota_used: Option<u64>,
    quota_limit: Option<u64>,
    shares_pending: bool,
    update_available: Option<String>,
    /// Terminal image-protocol picker, queried once at startup. Querying reads
    /// stdin, so it must NOT happen during draw — that races with key input.
    image_picker: Option<ratatui_image::picker::Picker>,
}

impl App {
    fn new_authed(client: PikPak, config: TuiConfig) -> Self {
        let (tx, rx) = mpsc::channel();
        let mut dl_state = DownloadState::new(config.download_jobs);
        dl_state.load_tasks(download::load_download_state());
        let mut app = Self {
            client: Arc::new(client),
            config,
            current_folder_id: String::new(),
            main_listing_request_id: 0,
            async_request_generation: 0,
            modal_request: None,
            preview_request: None,
            parent_listing_request: None,
            path_completion_in_flight: None,
            breadcrumb: Vec::new(),
            entries: Vec::new(),
            selected: 0,
            logs: VecDeque::new(),
            status_message: None,
            input: InputMode::Normal,
            cursor_visible: true,
            text_cursor: usize::MAX,
            last_blink: Instant::now(),
            loading: false,
            spinner_idx: 0,
            last_spinner: Instant::now(),
            show_help_sheet: false,
            help_scroll: 0,
            help_scroll_max: Cell::new(0),
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
            list_area_height: Cell::new(0),
            last_click_time: Instant::now(),
            last_click_pos: (0, 0),
            preview_scroll: 0,
            logs_scroll: None,
            logs_overlay_area: Cell::new(ratatui::layout::Rect::default()),
            settings_area: Cell::new(ratatui::layout::Rect::default()),
            mouse_list_area: Cell::new(ratatui::layout::Rect::default()),
            mouse_list_first_row: Cell::new(0),
            mouse_list_offset: Cell::new(0),
            mouse_list_visible: Cell::new(0),
            trash_entries: Vec::new(),
            trash_selected: 0,
            trash_expanded: false,
            loading_label: None,
            quota_used: None,
            quota_limit: None,
            shares_pending: false,
            update_available: None,
            image_picker: None,
        };
        app.refresh();
        app.fetch_quota();
        app.check_for_update_async();
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
        let download_jobs = config.download_jobs;
        Self {
            client: Arc::new(client),
            config,
            current_folder_id: String::new(),
            main_listing_request_id: 0,
            async_request_generation: 0,
            modal_request: None,
            preview_request: None,
            parent_listing_request: None,
            path_completion_in_flight: None,
            breadcrumb: Vec::new(),
            entries: Vec::new(),
            selected: 0,
            logs: VecDeque::new(),
            status_message: None,
            input,
            cursor_visible: true,
            text_cursor: usize::MAX,
            last_blink: Instant::now(),
            loading: false,
            spinner_idx: 0,
            last_spinner: Instant::now(),
            show_help_sheet: false,
            help_scroll: 0,
            help_scroll_max: Cell::new(0),
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
            download_state: DownloadState::new(download_jobs),
            download_view_mode: DownloadViewMode::Collapsed,
            network_stats: NetworkStats::new(),
            last_network_update: Instant::now(),
            current_pane_area: Cell::new(ratatui::layout::Rect::default()),
            parent_pane_area: Cell::new(ratatui::layout::Rect::default()),
            preview_pane_area: Cell::new(ratatui::layout::Rect::default()),
            scroll_offset: Cell::new(0),
            parent_scroll_offset: Cell::new(0),
            list_area_height: Cell::new(0),
            last_click_time: Instant::now(),
            last_click_pos: (0, 0),
            preview_scroll: 0,
            logs_scroll: None,
            logs_overlay_area: Cell::new(ratatui::layout::Rect::default()),
            settings_area: Cell::new(ratatui::layout::Rect::default()),
            mouse_list_area: Cell::new(ratatui::layout::Rect::default()),
            mouse_list_first_row: Cell::new(0),
            mouse_list_offset: Cell::new(0),
            mouse_list_visible: Cell::new(0),
            trash_entries: Vec::new(),
            trash_selected: 0,
            trash_expanded: false,
            loading_label: None,
            quota_used: None,
            quota_limit: None,
            shares_pending: false,
            update_available: None,
            image_picker: None,
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

        // Query the terminal's image protocol and font size ONCE, before the
        // input loop. Doing it during draw reads stdin every frame and steals
        // keypresses — a race with event::read().
        self.image_picker = ratatui_image::picker::Picker::from_query_stdio().ok();

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
        download::save_download_state(&self.download_state.tasks);
        Ok(())
    }

    fn poll_results(&mut self) {
        while let Ok(result) = self.result_rx.try_recv() {
            match result {
                OpResult::Ls(request_id, requested_folder_id, Ok(mut entries)) => {
                    if !folder_listing_matches(
                        &self.current_folder_id,
                        self.main_listing_request_id,
                        requested_folder_id.as_str(),
                        request_id,
                    ) {
                        continue;
                    }
                    self.finish_loading();
                    crate::config::sort_entries(
                        &mut entries,
                        self.config.sort_field,
                        self.config.sort_reverse,
                    );
                    // Keep the cursor on the same entry across a refresh — a
                    // re-sort or insert/delete shifts indices, so a fixed index
                    // would jump to a different file. Fall back to a clamp.
                    let prev_id = self.entries.get(self.selected).map(|e| e.id.clone());
                    self.entries = entries;
                    self.selected = prev_id
                        .and_then(|id| self.entries.iter().position(|e| e.id == id))
                        .unwrap_or_else(|| self.selected.min(self.entries.len().saturating_sub(1)));
                    self.push_log(format!("Refreshed {}", self.current_path_display()));
                    self.on_cursor_move();
                }
                OpResult::Ls(request_id, requested_folder_id, Err(e)) => {
                    if !folder_listing_matches(
                        &self.current_folder_id,
                        self.main_listing_request_id,
                        requested_folder_id.as_str(),
                        request_id,
                    ) {
                        continue;
                    }
                    self.finish_loading();
                    self.push_log(format!("Refresh failed: {e:#}"));
                }
                OpResult::Ok(msg) => {
                    self.push_log(msg);
                    self.refresh();
                }
                OpResult::Err(msg) => {
                    self.push_log(msg);
                    self.finish_loading();
                }
                OpResult::Info(request, Ok(info), thumb_fallback) => {
                    if !self.modal_request_matches(&request)
                        || !matches!(self.input, InputMode::InfoLoading)
                    {
                        continue;
                    }
                    self.finish_loading();
                    let thumb_url = info
                        .thumbnail_link
                        .clone()
                        .filter(|u| !u.is_empty())
                        .or_else(|| thumb_fallback.filter(|u| !u.is_empty()));
                    let has_thumbnail = thumb_url.is_some();
                    self.input = InputMode::InfoView {
                        request_id: request.id,
                        target_id: request.target.clone(),
                        info,
                        image: None,
                        has_thumbnail,
                    };
                    if let Some(url) = thumb_url {
                        let thumbnail_request = request.clone();
                        self.spawn_thumbnail_fetch(url, move |result| {
                            OpResult::InfoThumbnail(thumbnail_request, result)
                        });
                    } else {
                        self.modal_request = None;
                    }
                }
                OpResult::Info(request, Err(e), _) => {
                    if !self.modal_request_matches(&request)
                        || !matches!(self.input, InputMode::InfoLoading)
                    {
                        continue;
                    }
                    self.modal_request = None;
                    self.finish_loading();
                    self.input = InputMode::Normal;
                    self.push_log(format!("File info failed: {e:#}"));
                }
                OpResult::ParentLs(request, Ok(mut entries)) => {
                    let expected = self.breadcrumb.last().map(|(id, _)| id.as_str());
                    if self.parent_listing_request.as_ref() == Some(&request)
                        && expected == Some(request.target.as_str())
                    {
                        self.parent_listing_request = None;
                        crate::config::sort_entries(
                            &mut entries,
                            self.config.sort_field,
                            self.config.sort_reverse,
                        );
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
                OpResult::ParentLs(request, Err(e)) => {
                    let expected = self.breadcrumb.last().map(|(id, _)| id.as_str());
                    if self.parent_listing_request.as_ref() == Some(&request)
                        && expected == Some(request.target.as_str())
                    {
                        self.parent_listing_request = None;
                        self.push_log(format!("Parent listing failed: {e:#}"));
                    }
                }
                OpResult::PreviewLs(request, Ok(mut children)) => {
                    let is_modal = self.modal_request_matches(&request)
                        && matches!(self.input, InputMode::InfoLoading);
                    let is_preview = self.preview_request_matches(&request);
                    if !is_modal && !is_preview {
                        continue;
                    }
                    crate::config::sort_entries(
                        &mut children,
                        self.config.sort_field,
                        self.config.sort_reverse,
                    );
                    if is_modal {
                        self.modal_request = None;
                        self.finish_loading();
                        let name = self.preview_target_name.take().unwrap_or_default();
                        self.preview_state = PreviewState::FolderListing(children.clone());
                        self.preview_target_id = Some(request.target);
                        self.input = InputMode::InfoFolderView {
                            name,
                            entries: children,
                        };
                    } else {
                        self.preview_request = None;
                        self.preview_state = PreviewState::FolderListing(children);
                    }
                }
                OpResult::PreviewLs(request, Err(e)) => {
                    let is_modal = self.modal_request_matches(&request)
                        && matches!(self.input, InputMode::InfoLoading);
                    let is_preview = self.preview_request_matches(&request);
                    if !is_modal && !is_preview {
                        continue;
                    }
                    if is_modal {
                        self.modal_request = None;
                        self.finish_loading();
                        self.input = InputMode::Normal;
                    } else {
                        self.preview_request = None;
                        self.preview_state = PreviewState::Empty;
                    }
                    self.push_log(format!("Folder listing failed: {e:#}"));
                }
                OpResult::PreviewInfo(request, Ok(info)) => {
                    if !self.preview_request_matches(&request) {
                        continue;
                    }
                    self.preview_request = None;
                    self.preview_state = PreviewState::FileDetailedInfo(info);
                }
                OpResult::PreviewInfo(request, Err(e)) => {
                    if !self.preview_request_matches(&request) {
                        continue;
                    }
                    self.preview_request = None;
                    self.preview_state = PreviewState::Empty;
                    self.push_log(format!("Preview info failed: {e:#}"));
                }
                OpResult::PreviewText(request, Ok((name, content, size, truncated))) => {
                    let is_modal = self.modal_request_matches(&request)
                        && matches!(self.input, InputMode::InfoLoading);
                    let is_preview = self.preview_request_matches(&request);
                    if !is_modal && !is_preview {
                        continue;
                    }
                    let lines = highlight_content(&name, &content);
                    if is_modal {
                        self.modal_request = None;
                        self.finish_loading();
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
                        self.preview_target_id = Some(request.target);
                    } else {
                        self.preview_request = None;
                        self.preview_state = PreviewState::FileTextPreview {
                            name,
                            lines,
                            size,
                            truncated,
                        };
                    }
                }
                OpResult::PreviewText(request, Err(e)) => {
                    let is_modal = self.modal_request_matches(&request)
                        && matches!(self.input, InputMode::InfoLoading);
                    let is_preview = self.preview_request_matches(&request);
                    if !is_modal && !is_preview {
                        continue;
                    }
                    if is_modal {
                        self.modal_request = None;
                        self.finish_loading();
                        self.input = InputMode::Normal;
                    } else {
                        self.preview_request = None;
                        self.preview_state = PreviewState::FileBasicInfo;
                    }
                    self.push_log(format!("Text preview failed: {e:#}"));
                }
                OpResult::PreviewThumbnail(request, Ok(image)) => {
                    if !self.preview_request_matches(&request) {
                        continue;
                    }
                    self.preview_request = None;
                    self.preview_state = PreviewState::ThumbnailImage { image };
                }
                OpResult::PreviewThumbnail(request, Err(e)) => {
                    if !self.preview_request_matches(&request) {
                        continue;
                    }
                    self.preview_request = None;
                    self.preview_state = PreviewState::FileBasicInfo;
                    self.push_log(format!("Thumbnail preview failed: {e:#}"));
                }
                OpResult::OfflineTasks(request, Ok(tasks)) => {
                    if !self.modal_request_matches(&request)
                        || !matches!(self.input, InputMode::InfoLoading)
                    {
                        continue;
                    }
                    self.modal_request = None;
                    self.finish_loading();
                    self.input = InputMode::OfflineTasksView { tasks, selected: 0 };
                }
                OpResult::OfflineTasks(request, Err(e)) => {
                    if !self.modal_request_matches(&request)
                        || !matches!(self.input, InputMode::InfoLoading)
                    {
                        continue;
                    }
                    self.modal_request = None;
                    self.finish_loading();
                    self.input = InputMode::Normal;
                    self.push_log(format!("Failed to load offline tasks: {e:#}"));
                }
                OpResult::PlayInfo(request, Ok(info)) => {
                    if !self.modal_request_matches(&request)
                        || !matches!(self.input, InputMode::Normal)
                    {
                        continue;
                    }
                    self.modal_request = None;
                    self.finish_loading();
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
                OpResult::PlayInfo(request, Err(e)) => {
                    if !self.modal_request_matches(&request)
                        || !matches!(self.input, InputMode::Normal)
                    {
                        continue;
                    }
                    self.modal_request = None;
                    self.finish_loading();
                    self.push_log(format!("Play info failed: {e:#}"));
                }
                OpResult::PlayPickerInfo(request, Ok((info, medias))) => {
                    if !self.modal_request_matches(&request)
                        || !matches!(self.input, InputMode::Normal)
                    {
                        continue;
                    }
                    self.modal_request = None;
                    self.finish_loading();
                    if medias.is_empty() {
                        self.push_log("No playback streams available".into());
                    } else {
                        let first_avail = medias.iter().position(|m| m.available).unwrap_or(0);
                        self.input = InputMode::PlayPicker {
                            name: info.name.clone(),
                            medias,
                            selected: first_avail,
                        };
                    }
                }
                OpResult::PlayPickerInfo(request, Err(e)) => {
                    if !self.modal_request_matches(&request)
                        || !matches!(self.input, InputMode::Normal)
                    {
                        continue;
                    }
                    self.modal_request = None;
                    self.finish_loading();
                    self.push_log(format!("Play picker info failed: {e:#}"));
                }
                OpResult::TrashList(Ok(entries)) => {
                    self.finish_loading();
                    let expanded = if let InputMode::TrashView { expanded, .. } = &self.input {
                        *expanded
                    } else {
                        self.trash_expanded
                    };
                    // Keep the cursor near where it was: after restoring or
                    // deleting item #12, jumping back to the top loses the
                    // user's place. Clamp because the list just shrank.
                    let selected = self.trash_selected.min(entries.len().saturating_sub(1));
                    self.trash_entries = entries.clone();
                    self.trash_selected = selected;
                    self.trash_expanded = expanded;
                    self.input = InputMode::TrashView {
                        entries,
                        selected,
                        expanded,
                    };
                }
                OpResult::TrashList(Err(e)) => {
                    self.finish_loading();
                    if matches!(self.input, InputMode::TrashView { .. }) {
                        self.input = InputMode::Normal;
                    }
                    self.push_log(format!("Failed to load trash: {e:#}"));
                }
                OpResult::TrashOp(msg) => {
                    self.finish_loading();
                    self.push_log(msg);
                    self.open_trash_view_preserve();
                }
                OpResult::OfflineOp(msg) => {
                    self.push_log(msg);
                    self.open_offline_tasks_view();
                }
                OpResult::InfoThumbnail(request, Ok(img)) => {
                    if !self.modal_request_matches(&request) {
                        continue;
                    }
                    let InputMode::InfoView {
                        request_id,
                        target_id,
                        image,
                        ..
                    } = &mut self.input
                    else {
                        continue;
                    };
                    if *request_id != request.id || target_id != &request.target {
                        continue;
                    }
                    *image = Some(img);
                    self.modal_request = None;
                }
                OpResult::InfoThumbnail(request, Err(e)) => {
                    if !self.modal_request_matches(&request) {
                        continue;
                    }
                    let matches_view = matches!(
                        &self.input,
                        InputMode::InfoView {
                            request_id,
                            target_id,
                            ..
                        } if *request_id == request.id && target_id == &request.target
                    );
                    if !matches_view {
                        continue;
                    }
                    self.modal_request = None;
                    self.push_log(format!("Info thumbnail failed: {e:#}"));
                }
                OpResult::GotoPath(request, Ok((folder_id, new_breadcrumb))) => {
                    if !self.modal_request_matches(&request)
                        || !matches!(self.input, InputMode::Normal)
                    {
                        continue;
                    }
                    self.modal_request = None;
                    self.finish_loading();
                    self.breadcrumb = new_breadcrumb;
                    self.current_folder_id = folder_id.clone();
                    self.selected = 0;
                    self.parent_entries.clear();
                    self.parent_selected = 0;
                    // Fill the parent pane like normal navigation does — goto
                    // otherwise leaves it blank until the next move.
                    self.refresh_parent();
                    self.clear_preview();
                    self.loading = true;
                    self.request_main_listing(folder_id);
                }
                OpResult::GotoPath(request, Err(e)) => {
                    if !self.modal_request_matches(&request)
                        || !matches!(self.input, InputMode::Normal)
                    {
                        continue;
                    }
                    self.modal_request = None;
                    self.finish_loading();
                    self.push_log(format!("Go to path failed: {e:#}"));
                }
                OpResult::Quota(Ok(info)) => {
                    if let Some(detail) = info.quota {
                        self.quota_used = detail.usage.as_deref().and_then(|s| s.parse().ok());
                        self.quota_limit = detail.limit.as_deref().and_then(|s| s.parse().ok());
                    }
                }
                OpResult::Quota(Err(e)) => {
                    self.push_log(format!("Quota fetch failed: {e:#}"));
                }
                OpResult::Upload(Ok(msg)) => {
                    self.finish_loading();
                    self.push_log(msg);
                    self.refresh();
                }
                OpResult::Upload(Err(e)) => {
                    self.finish_loading();
                    self.push_log(format!("Upload failed: {e:#}"));
                }
                OpResult::ShareCreated {
                    title,
                    url,
                    pass_code,
                } => {
                    self.push_log(format!("Share created: {url}"));
                    if let InputMode::ShareCreatedView { ref mut shares } = self.input {
                        shares.push((title, url, pass_code));
                    } else {
                        self.input = InputMode::ShareCreatedView {
                            shares: vec![(title, url, pass_code)],
                        };
                    }
                }
                OpResult::MyShares(Ok(shares)) => {
                    self.finish_loading();
                    if self.shares_pending || matches!(self.input, InputMode::MySharesView { .. }) {
                        self.shares_pending = false;
                        let selected = if let InputMode::MySharesView { selected, .. } = &self.input
                        {
                            (*selected).min(shares.len().saturating_sub(1))
                        } else {
                            0
                        };
                        self.input = InputMode::MySharesView {
                            shares,
                            selected,
                            confirm_delete: None,
                        };
                    }
                }
                OpResult::MyShares(Err(e)) => {
                    self.finish_loading();
                    self.shares_pending = false;
                    self.push_log(format!("Failed to load shares: {e:#}"));
                    if matches!(self.input, InputMode::MySharesView { .. }) {
                        self.input = InputMode::Normal;
                    }
                }
                OpResult::UpdateAvailable(Some(version)) => {
                    self.push_log(format!(
                        "Update available: v{} → v{} (run `pikpaktui update`)",
                        env!("CARGO_PKG_VERSION"),
                        version
                    ));
                    self.update_available = Some(version);
                }
                OpResult::UpdateAvailable(None) => {}
                OpResult::PickerLs(request_id, folder_id, result) => {
                    let picker = match &mut self.input {
                        InputMode::MovePicker { picker, .. }
                        | InputMode::CopyPicker { picker, .. }
                        | InputMode::CartMovePicker { picker }
                        | InputMode::CartCopyPicker { picker } => Some(picker),
                        _ => None,
                    };
                    let mut log = None;
                    if let Some(p) = picker
                        && p.folder_id == folder_id
                        && p.listing_request_id == request_id
                    {
                        match result {
                            Ok(mut entries) => {
                                crate::config::sort_entries(
                                    &mut entries,
                                    self.config.sort_field,
                                    self.config.sort_reverse,
                                );
                                p.entries = entries;
                            }
                            Err(e) => log = Some(format!("Picker load failed: {e:#}")),
                        }
                        p.loading = false;
                    }
                    if let Some(msg) = log {
                        self.push_log(msg);
                    }
                }
                OpResult::PathCandidates {
                    request_id,
                    value,
                    folder_context,
                    parent,
                    matches,
                } => {
                    if self.path_completion_in_flight != Some(request_id) {
                        continue;
                    }
                    self.path_completion_in_flight = None;
                    let current_folder_id = self.current_folder_id.clone();
                    let input = match &mut self.input {
                        InputMode::MoveInput { input, .. }
                        | InputMode::CopyInput { input, .. }
                        | InputMode::CartMoveInput { input }
                        | InputMode::CartCopyInput { input } => Some(input),
                        _ => None,
                    };
                    // Request identity protects a newly opened dialog from an
                    // old dialog's response; value and folder protect edits
                    // and navigation within the same dialog.
                    if let Some(inp) = input
                        && completion::path_candidate_result_matches(
                            inp,
                            request_id,
                            value.as_str(),
                            current_folder_id.as_str(),
                            folder_context.as_str(),
                        )
                        && !matches.is_empty()
                    {
                        completion::apply_path_candidates(inp, parent, matches);
                        self.text_cursor = usize::MAX;
                    }
                }
                OpResult::LoginDone {
                    email,
                    password,
                    result,
                } => match result {
                    Ok(()) => {
                        if let Err(e) = AppConfig::save_credentials(&email, &password) {
                            self.push_log(format!("Warning: failed to save config: {e:#}"));
                        }
                        // Adopt the session (and device identity) the login
                        // client just wrote.
                        match PikPak::new() {
                            Ok(fresh) => self.client = Arc::new(fresh),
                            Err(e) => self.push_log(format!("client rebuild failed: {e:#}")),
                        }
                        self.input = InputMode::Normal;
                        self.refresh();
                        self.push_log("Login successful".to_string());
                    }
                    Err(e) => {
                        self.input = InputMode::Login {
                            field: LoginField::Email,
                            email,
                            password,
                            error: Some(format!("Login failed: {e}")),
                            logging_in: false,
                        };
                    }
                },
            }
        }

        let logs = self.download_state.poll(&self.client);
        for msg in logs {
            self.push_log(msg);
        }

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

    /// Log in on a worker thread so the "Logging in..." frame actually
    /// renders (the event loop used to block inside the HTTP calls). A fresh
    /// client does the signin — it writes the same session file — and the
    /// result comes back through poll_results as LoginDone.
    fn attempt_login(&mut self, email: &str, password: &str) {
        let email = email.to_string();
        let password = password.to_string();
        let tx = self.result_tx.clone();
        std::thread::spawn(move || {
            let result = PikPak::new()
                .and_then(|mut fresh| fresh.login(&email, &password))
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(OpResult::LoginDone {
                email,
                password,
                result,
            });
        });
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

    fn finish_loading(&mut self) {
        self.loading = false;
        self.loading_label = None;
    }

    fn push_log(&mut self, msg: String) {
        let lower = msg.to_ascii_lowercase();
        let kind = if [
            "failed",
            "error",
            "invalid",
            "not found",
            "cannot",
            "unavailable",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
        {
            StatusKind::Error
        } else if lower.contains("cancelled")
            || lower.starts_with("no ")
            || lower.contains("warning")
        {
            StatusKind::Warning
        } else {
            StatusKind::Info
        };
        self.status_message = Some(StatusMessage {
            text: msg.clone(),
            kind,
            expires_at: Instant::now() + Duration::from_secs(4),
        });
        self.logs.push_back(msg);
        if self.logs.len() > 500 {
            self.logs.pop_front();
        }
    }

    fn check_for_update_async(&self) {
        if self.config.update_check == crate::config::UpdateCheck::Off {
            return;
        }
        let tx = self.result_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(OpResult::UpdateAvailable(
                crate::cmd::update::check_for_update(),
            ));
        });
    }

    fn fetch_quota(&mut self) {
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(OpResult::Quota(client.quota()));
        });
    }

    fn refresh(&mut self) {
        self.loading = true;
        let fid = self.current_folder_id.clone();
        self.request_main_listing(fid);
        self.refresh_parent();
        self.fetch_quota();
    }

    fn request_main_listing(&mut self, folder_id: String) {
        self.invalidate_main_listing();
        let request_id = self.main_listing_request_id;
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        std::thread::spawn(move || {
            let result = client.ls(&folder_id);
            let _ = tx.send(OpResult::Ls(request_id, folder_id, result));
        });
    }

    fn invalidate_main_listing(&mut self) {
        self.main_listing_request_id = self.main_listing_request_id.wrapping_add(1);
    }

    fn next_async_request_id(&mut self) -> u64 {
        self.async_request_generation = self.async_request_generation.wrapping_add(1);
        self.async_request_generation
    }

    fn new_async_request(
        &mut self,
        kind: AsyncRequestKind,
        target: impl Into<String>,
    ) -> AsyncRequest {
        AsyncRequest {
            id: self.next_async_request_id(),
            kind,
            target: target.into(),
        }
    }

    fn begin_modal_request(
        &mut self,
        kind: AsyncRequestKind,
        target: impl Into<String>,
    ) -> AsyncRequest {
        let request = self.new_async_request(kind, target);
        self.modal_request = Some(request.clone());
        request
    }

    fn begin_preview_request(
        &mut self,
        kind: AsyncRequestKind,
        target: impl Into<String>,
    ) -> AsyncRequest {
        let request = self.new_async_request(kind, target);
        self.preview_request = Some(request.clone());
        request
    }

    fn modal_request_matches(&self, request: &AsyncRequest) -> bool {
        self.modal_request.as_ref() == Some(request)
    }

    fn preview_request_matches(&self, request: &AsyncRequest) -> bool {
        self.preview_request.as_ref() == Some(request)
            && self.preview_target_id.as_deref() == Some(request.target.as_str())
    }

    fn invalidate_modal_request(&mut self) {
        let request_owned_loading = self.modal_request.as_ref().is_some_and(|request| {
            matches!(
                request.kind,
                AsyncRequestKind::Play | AsyncRequestKind::PlayPicker | AsyncRequestKind::GotoPath
            )
        });
        self.modal_request = None;
        if request_owned_loading {
            self.finish_loading();
        }
    }

    fn refresh_parent(&mut self) {
        if let Some(parent_id) = self.breadcrumb.last().map(|(id, _)| id.clone()) {
            let request =
                self.new_async_request(AsyncRequestKind::ParentListing, parent_id.clone());
            self.parent_listing_request = Some(request.clone());
            let client = Arc::clone(&self.client);
            let tx = self.result_tx.clone();
            let pid = parent_id;
            std::thread::spawn(move || {
                let _ = tx.send(OpResult::ParentLs(request, client.ls(&pid)));
            });
        } else {
            self.parent_listing_request = None;
            self.parent_entries.clear();
            self.parent_selected = 0;
        }
    }

    fn clear_preview(&mut self) {
        self.preview_state = PreviewState::Empty;
        self.preview_target_id = None;
        self.preview_target_name = None;
        self.preview_request = None;
        self.pending_preview_fetch = false;
        self.preview_scroll = 0;
    }

    fn on_cursor_move(&mut self) {
        self.preview_scroll = 0;
        self.preview_request = None;
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

    fn spawn_thumbnail_fetch<F>(&self, url: String, make_result: F)
    where
        F: FnOnce(Result<image::DynamicImage>) -> OpResult + Send + 'static,
    {
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(make_result(fetch_and_render_thumbnail(&url, &client)));
        });
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
                let request =
                    self.begin_preview_request(AsyncRequestKind::FolderPreview, eid.clone());
                // Folders always show content listing, never thumbnails
                std::thread::spawn(move || {
                    let _ = tx.send(OpResult::PreviewLs(request, client.ls(&eid)));
                });
            }
            EntryKind::File => {
                if let Some(ref thumb_url) = entry.thumbnail_link
                    && !thumb_url.is_empty()
                {
                    let request =
                        self.begin_preview_request(AsyncRequestKind::FilePreview, eid.clone());
                    self.spawn_thumbnail_fetch(thumb_url.clone(), move |r| {
                        OpResult::PreviewThumbnail(request, r)
                    });
                    return;
                }
                if theme::is_text_previewable(&entry) {
                    let request =
                        self.begin_preview_request(AsyncRequestKind::FilePreview, eid.clone());
                    let max_bytes = self.config.preview_max_size;
                    std::thread::spawn(move || {
                        let _ = tx.send(OpResult::PreviewText(
                            request,
                            client.fetch_text_preview(&eid, max_bytes),
                        ));
                    });
                } else {
                    let request =
                        self.begin_preview_request(AsyncRequestKind::FilePreview, eid.clone());
                    std::thread::spawn(move || {
                        let _ = tx.send(OpResult::PreviewInfo(request, client.file_info(&eid)));
                    });
                }
            }
        }
    }

    fn open_trash_view_preserve(&mut self) {
        self.input = InputMode::TrashView {
            entries: self.trash_entries.clone(),
            selected: self.trash_selected,
            expanded: self.trash_expanded,
        };
        self.loading = true;
        self.loading_label = Some("Loading trash...".into());
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(OpResult::TrashList(client.ls_trash(200)));
        });
    }

    fn open_my_shares_view(&mut self) {
        self.shares_pending = true;
        self.loading = true;
        self.loading_label = Some("Loading shares...".into());
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(OpResult::MyShares(client.list_shares()));
        });
    }

    fn resort_entries(&mut self) {
        crate::config::sort_entries(
            &mut self.entries,
            self.config.sort_field,
            self.config.sort_reverse,
        );
        if self.selected >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
        let arrow = if self.config.sort_reverse {
            "\u{2193}"
        } else {
            "\u{2191}"
        };
        self.push_log(format!(
            "Sort: {} {}",
            self.config.sort_field.as_str(),
            arrow
        ));
    }
}

static SYNTAX_SET: LazyLock<syntect::parsing::SyntaxSet> =
    LazyLock::new(syntect::parsing::SyntaxSet::load_defaults_newlines);
static THEME_SET: LazyLock<syntect::highlighting::ThemeSet> =
    LazyLock::new(syntect::highlighting::ThemeSet::load_defaults);

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
    const TB: u64 = 1024 * GB;
    if bytes >= TB {
        format!("{:.1} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn truncate_name(name: &str, max_width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    if UnicodeWidthStr::width(name) <= max_width {
        name.to_string()
    } else {
        let mut w = 0;
        let mut out = String::new();
        for ch in name.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if w + cw + 3 > max_width {
                break;
            }
            out.push(ch);
            w += cw;
        }
        out.push_str("...");
        out
    }
}

fn folder_listing_matches(
    current_folder_id: &str,
    current_request_id: u64,
    requested_folder_id: &str,
    request_id: u64,
) -> bool {
    current_folder_id == requested_folder_id && current_request_id == request_id
}

#[cfg(test)]
mod folder_listing_result_tests {
    use super::{
        App, AsyncRequest, AsyncRequestKind, InputMode, NORMAL_ACTIONS, OpResult, PreviewState,
        StatusKind, folder_listing_matches,
    };
    use crate::config::TuiConfig;
    use crate::pikpak::{Entry, EntryKind, FileInfoResponse, PikPak};
    use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    fn test_app() -> App {
        App::new_login(PikPak::new().unwrap(), None, TuiConfig::default())
    }

    fn info(id: &str, name: &str) -> FileInfoResponse {
        FileInfoResponse {
            id: Some(id.to_string()),
            name: name.to_string(),
            kind: Some("drive#file".to_string()),
            size: None,
            hash: None,
            mime_type: None,
            created_time: None,
            modified_time: None,
            web_content_link: Some("https://example.invalid/video".to_string()),
            thumbnail_link: None,
            links: None,
            medias: None,
        }
    }

    fn folder(id: &str, name: &str) -> Entry {
        Entry {
            id: id.to_string(),
            name: name.to_string(),
            kind: EntryKind::Folder,
            size: 0,
            created_time: String::new(),
            modified_time: String::new(),
            starred: false,
            thumbnail_link: None,
        }
    }

    fn request(id: u64, kind: AsyncRequestKind, target: &str) -> AsyncRequest {
        AsyncRequest {
            id,
            kind,
            target: target.to_string(),
        }
    }

    #[test]
    fn main_listing_result_is_bound_to_its_requested_folder() {
        let result = OpResult::Ls(7, "folder-a".to_string(), Ok(Vec::new()));
        let OpResult::Ls(request_id, requested_folder_id, _) = result else {
            panic!("expected a main folder listing result");
        };

        assert!(folder_listing_matches(
            "folder-a",
            7,
            requested_folder_id.as_str(),
            request_id,
        ));
        assert!(!folder_listing_matches(
            "folder-b",
            7,
            requested_folder_id.as_str(),
            request_id,
        ));
    }

    #[test]
    fn older_listing_for_same_folder_cannot_replace_newer_listing() {
        assert!(!folder_listing_matches("folder-a", 8, "folder-a", 7));
        assert!(folder_listing_matches("folder-a", 8, "folder-a", 8));
    }

    #[test]
    fn older_parent_listing_for_the_same_parent_cannot_replace_a_newer_one() {
        let mut app = test_app();
        app.breadcrumb = vec![("parent".to_string(), "child".to_string())];
        let old_request = request(1, AsyncRequestKind::ParentListing, "parent");
        let new_request = request(2, AsyncRequestKind::ParentListing, "parent");
        app.parent_listing_request = Some(new_request.clone());
        app.result_tx
            .send(OpResult::ParentLs(
                new_request,
                Ok(vec![folder("new", "new")]),
            ))
            .unwrap();
        app.result_tx
            .send(OpResult::ParentLs(
                old_request,
                Ok(vec![folder("old", "old")]),
            ))
            .unwrap();

        app.poll_results();

        assert_eq!(app.parent_entries[0].id, "new");
    }

    #[test]
    fn older_picker_listing_for_same_folder_cannot_replace_newer_dialog_result() {
        let mut app = test_app();
        app.input = InputMode::CopyPicker {
            source: folder("source", "source"),
            picker: super::PickerState {
                folder_id: "folder-a".to_string(),
                listing_request_id: 2,
                breadcrumb: Vec::new(),
                entries: Vec::new(),
                selected: 0,
                loading: true,
            },
        };
        // The current dialog's response lands first, then the older request
        // from a cancelled dialog returns for the same folder.
        app.result_tx
            .send(OpResult::PickerLs(
                2,
                "folder-a".to_string(),
                Ok(vec![folder("new", "new")]),
            ))
            .unwrap();
        app.result_tx
            .send(OpResult::PickerLs(
                1,
                "folder-a".to_string(),
                Ok(vec![folder("old", "old")]),
            ))
            .unwrap();

        app.poll_results();

        let InputMode::CopyPicker { picker, .. } = app.input else {
            panic!("picker unexpectedly closed");
        };
        assert_eq!(picker.entries[0].id, "new");
    }

    #[test]
    fn cached_child_navigation_finishes_the_invalidated_network_loading_state() {
        let mut app = test_app();
        app.input = InputMode::Normal;
        app.entries = vec![folder("child", "child")];
        app.selected = 0;
        app.preview_target_id = Some("child".to_string());
        app.preview_state = PreviewState::FolderListing(Vec::new());
        app.loading = true;

        app.handle_key(KeyCode::Enter, KeyModifiers::NONE).unwrap();

        assert!(!app.loading);
    }

    #[test]
    fn cached_parent_navigation_finishes_the_invalidated_network_loading_state() {
        let mut app = test_app();
        app.input = InputMode::Normal;
        app.current_folder_id = "child".to_string();
        app.breadcrumb = vec![("parent".to_string(), "child".to_string())];
        app.parent_entries = vec![folder("sibling", "sibling")];
        app.loading = true;

        app.handle_key(KeyCode::Backspace, KeyModifiers::NONE)
            .unwrap();

        assert!(!app.loading);
    }

    #[test]
    fn stale_info_result_cannot_replace_a_newer_loading_target() {
        let mut app = test_app();
        app.input = InputMode::InfoLoading;
        app.modal_request = Some(request(2, AsyncRequestKind::Info, "new-target"));
        app.result_tx
            .send(OpResult::Info(
                request(1, AsyncRequestKind::Info, "old-target"),
                Ok(info("old-target", "old")),
                None,
            ))
            .unwrap();

        app.poll_results();

        assert!(matches!(app.input, InputMode::InfoLoading));
    }

    #[test]
    fn stale_folder_preview_cannot_replace_a_newer_loading_target() {
        let mut app = test_app();
        app.input = InputMode::InfoLoading;
        app.preview_target_id = Some("new-target".to_string());
        app.preview_target_name = Some("new".to_string());
        app.modal_request = Some(request(2, AsyncRequestKind::FolderPreview, "new-target"));
        app.result_tx
            .send(OpResult::PreviewLs(
                request(1, AsyncRequestKind::FolderPreview, "old-target"),
                Ok(Vec::new()),
            ))
            .unwrap();

        app.poll_results();

        assert!(matches!(app.input, InputMode::InfoLoading));
    }

    #[test]
    fn stale_thumbnail_cannot_land_on_a_different_info_view() {
        let mut app = test_app();
        app.modal_request = Some(request(2, AsyncRequestKind::Info, "new-target"));
        app.input = InputMode::InfoView {
            request_id: 2,
            target_id: "new-target".to_string(),
            info: info("new-target", "new"),
            image: None,
            has_thumbnail: true,
        };
        app.result_tx
            .send(OpResult::InfoThumbnail(
                request(1, AsyncRequestKind::Info, "old-target"),
                Ok(image::DynamicImage::new_rgb8(1, 1)),
            ))
            .unwrap();

        app.poll_results();

        let InputMode::InfoView { image, .. } = app.input else {
            panic!("info view was unexpectedly replaced");
        };
        assert!(image.is_none());
    }

    #[test]
    fn delayed_play_result_cannot_replace_a_later_modal() {
        let mut app = test_app();
        app.input = InputMode::Settings {
            selected: 0,
            editing: false,
            draft: TuiConfig::default(),
            modified: false,
        };
        app.modal_request = Some(request(1, AsyncRequestKind::Play, "old-target"));
        app.result_tx
            .send(OpResult::PlayInfo(
                request(1, AsyncRequestKind::Play, "old-target"),
                Ok(info("old-target", "old")),
            ))
            .unwrap();

        app.poll_results();

        assert!(matches!(app.input, InputMode::Settings { .. }));
    }

    #[test]
    fn keyboard_invalidation_of_pending_interactive_request_finishes_its_loading_state() {
        for kind in [
            AsyncRequestKind::Play,
            AsyncRequestKind::PlayPicker,
            AsyncRequestKind::GotoPath,
        ] {
            let mut app = test_app();
            app.input = InputMode::Normal;
            app.loading = true;
            app.modal_request = Some(request(1, kind, "target"));

            app.handle_key(KeyCode::Down, KeyModifiers::NONE).unwrap();

            assert!(app.modal_request.is_none(), "{kind:?}");
            assert!(!app.loading, "{kind:?}");
        }
    }

    #[test]
    fn mouse_invalidation_of_pending_play_finishes_its_loading_state() {
        let mut app = test_app();
        app.input = InputMode::Normal;
        app.loading = true;
        app.modal_request = Some(request(1, AsyncRequestKind::Play, "video"));

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });

        assert!(app.modal_request.is_none());
        assert!(!app.loading);
    }

    #[test]
    fn delayed_goto_result_cannot_navigate_after_a_later_modal_opened() {
        let mut app = test_app();
        app.current_folder_id = "before".to_string();
        app.input = InputMode::Settings {
            selected: 0,
            editing: false,
            draft: TuiConfig::default(),
            modified: false,
        };
        app.modal_request = Some(request(1, AsyncRequestKind::GotoPath, "/after"));
        app.result_tx
            .send(OpResult::GotoPath(
                request(1, AsyncRequestKind::GotoPath, "/after"),
                Ok((
                    "after".to_string(),
                    vec![("".to_string(), "after".to_string())],
                )),
            ))
            .unwrap();

        app.poll_results();

        assert_eq!(app.current_folder_id, "before");
        assert!(matches!(app.input, InputMode::Settings { .. }));
    }

    #[test]
    fn unsaved_settings_require_explicit_discard_confirmation() {
        let mut app = test_app();
        app.input = InputMode::Settings {
            selected: 4,
            editing: false,
            draft: TuiConfig::default(),
            modified: true,
        };

        app.handle_key(KeyCode::Esc, KeyModifiers::NONE).unwrap();
        assert!(matches!(
            app.input,
            InputMode::ConfirmDiscardSettings { selected: 4, .. }
        ));

        app.handle_key(KeyCode::Esc, KeyModifiers::NONE).unwrap();
        assert!(matches!(
            app.input,
            InputMode::Settings {
                selected: 4,
                modified: true,
                ..
            }
        ));
    }

    #[test]
    fn settings_subpages_return_focus_to_their_origin_rows() {
        let mut app = test_app();
        app.input = InputMode::CustomColorSettings {
            selected: 0,
            draft: TuiConfig::default(),
            modified: false,
            editing_rgb: false,
            rgb_input: String::new(),
            rgb_component: 0,
        };
        app.handle_key(KeyCode::Esc, KeyModifiers::NONE).unwrap();
        assert!(matches!(app.input, InputMode::Settings { selected: 2, .. }));

        app.input = InputMode::ImageProtocolSettings {
            selected: 0,
            draft: TuiConfig::default(),
            modified: false,
            current_terminal: "test".to_string(),
            terminals: vec!["test".to_string()],
        };
        app.handle_key(KeyCode::Esc, KeyModifiers::NONE).unwrap();
        assert!(matches!(app.input, InputMode::Settings { selected: 9, .. }));
    }

    #[test]
    fn log_messages_also_create_visible_typed_status() {
        let mut app = test_app();

        app.push_log("Upload failed: network timeout".to_string());
        let status = app.status_message.as_ref().unwrap();
        assert_eq!(status.kind, StatusKind::Error);
        assert_eq!(status.text, "Upload failed: network timeout");

        app.push_log("Download cancelled".to_string());
        assert_eq!(
            app.status_message.as_ref().unwrap().kind,
            StatusKind::Warning
        );

        app.push_log("Uploaded file".to_string());
        assert_eq!(app.status_message.as_ref().unwrap().kind, StatusKind::Info);
    }

    #[test]
    fn action_menu_runs_the_selected_normal_mode_action() {
        let mut app = test_app();
        app.input = InputMode::Normal;
        app.handle_key(KeyCode::Char('?'), KeyModifiers::NONE)
            .unwrap();
        assert!(matches!(app.input, InputMode::ActionMenu { selected: 0 }));

        let settings_index = NORMAL_ACTIONS
            .iter()
            .position(|action| action.key == KeyCode::Char(','))
            .unwrap();
        app.input = InputMode::ActionMenu {
            selected: settings_index,
        };
        app.handle_key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
        assert!(matches!(app.input, InputMode::Settings { .. }));
    }
}

/// Truncate and pad to an exact number of display columns. `format!("{:<w$}")`
/// pads by char count and never truncates, so it misaligns columns whenever
/// content is wide (CJK) or longer than the column — this is the safe
/// replacement for hand-rolled column padding.
pub(crate) fn pad_to_width(s: &str, width: usize) -> String {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
    let w = UnicodeWidthStr::width(s);
    if w <= width {
        return format!("{}{}", s, " ".repeat(width - w));
    }
    if width < 4 {
        // No room for an ellipsis; hard-cut by display width.
        let mut out = String::new();
        let mut used = 0;
        for ch in s.chars() {
            let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + cw > width {
                break;
            }
            out.push(ch);
            used += cw;
        }
        return format!("{}{}", out, " ".repeat(width - used));
    }
    let t = truncate_name(s, width);
    let tw = UnicodeWidthStr::width(t.as_str());
    format!("{}{}", t, " ".repeat(width.saturating_sub(tw)))
}

#[cfg(test)]
mod pad_tests {
    use super::pad_to_width;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn pads_short_ascii() {
        assert_eq!(pad_to_width("abc", 5), "abc  ");
    }

    #[test]
    fn exact_width_untouched() {
        assert_eq!(pad_to_width("abcde", 5), "abcde");
    }

    #[test]
    fn truncates_overflow_with_ellipsis() {
        let out = pad_to_width("abcdefghij", 7);
        assert_eq!(UnicodeWidthStr::width(out.as_str()), 7);
        assert!(out.ends_with("..."));
    }

    #[test]
    fn cjk_pads_by_display_width() {
        // 3 CJK chars = 6 columns; pad to 8 needs 2 spaces
        assert_eq!(pad_to_width("三上悠", 8), "三上悠  ");
    }

    #[test]
    fn cjk_truncates_by_display_width() {
        let out = pad_to_width("三上悠亚三上悠亚", 9);
        assert_eq!(UnicodeWidthStr::width(out.as_str()), 9);
    }

    #[test]
    fn tiny_width_hard_cuts() {
        assert_eq!(pad_to_width("abcdef", 2), "ab");
        // A double-width char that doesn't fit in 1 column becomes padding
        assert_eq!(pad_to_width("三", 1), " ");
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

fn handle_text_input(
    value: &mut String,
    cursor: &mut usize,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Option<bool> {
    *cursor = (*cursor).min(value.len());
    while !value.is_char_boundary(*cursor) {
        *cursor = cursor.saturating_sub(1);
    }

    let previous_boundary = |text: &str, at: usize| {
        text[..at]
            .char_indices()
            .next_back()
            .map(|(idx, _)| idx)
            .unwrap_or(0)
    };
    let next_boundary = |text: &str, at: usize| {
        text[at..]
            .chars()
            .next()
            .map(|c| at + c.len_utf8())
            .unwrap_or(text.len())
    };

    match code {
        KeyCode::Esc => Some(false),
        KeyCode::Enter => Some(true),
        KeyCode::Home | KeyCode::Char('a') if modifiers.contains(KeyModifiers::CONTROL) => {
            *cursor = 0;
            None
        }
        KeyCode::End | KeyCode::Char('e') if modifiers.contains(KeyModifiers::CONTROL) => {
            *cursor = value.len();
            None
        }
        KeyCode::Left => {
            *cursor = previous_boundary(value, *cursor);
            None
        }
        KeyCode::Right => {
            *cursor = next_boundary(value, *cursor);
            None
        }
        KeyCode::Backspace => {
            let start = previous_boundary(value, *cursor);
            value.replace_range(start..*cursor, "");
            *cursor = start;
            None
        }
        KeyCode::Delete => {
            let end = next_boundary(value, *cursor);
            value.replace_range(*cursor..end, "");
            None
        }
        KeyCode::Char('w') if modifiers.contains(KeyModifiers::CONTROL) => {
            let mut start = *cursor;
            while start > 0 {
                let prev = previous_boundary(value, start);
                let ch = value[prev..start].chars().next().unwrap_or(' ');
                if !ch.is_whitespace() {
                    break;
                }
                start = prev;
            }
            while start > 0 {
                let prev = previous_boundary(value, start);
                let ch = value[prev..start].chars().next().unwrap_or(' ');
                if ch.is_whitespace() {
                    break;
                }
                start = prev;
            }
            value.replace_range(start..*cursor, "");
            *cursor = start;
            None
        }
        KeyCode::Char(c) if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            value.insert(*cursor, c);
            *cursor += c.len_utf8();
            None
        }
        _ => None,
    }
}

fn text_input_view(value: &str, cursor: usize, max_width: usize, cursor_visible: bool) -> String {
    use unicode_width::UnicodeWidthStr;

    if max_width == 0 {
        return String::new();
    }
    let mut cursor = cursor.min(value.len());
    while !value.is_char_boundary(cursor) {
        cursor = cursor.saturating_sub(1);
    }

    let before = &value[..cursor];
    let after = &value[cursor..];
    let marker = if cursor_visible { "\u{2588}" } else { " " };
    let content_budget = max_width.saturating_sub(1);

    let suffix = |text: &str, width: usize| {
        let mut chars = Vec::new();
        let mut used = 0;
        for ch in text.chars().rev() {
            let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + w > width {
                break;
            }
            used += w;
            chars.push(ch);
        }
        chars.into_iter().rev().collect::<String>()
    };
    let prefix = |text: &str, width: usize| {
        let mut out = String::new();
        let mut used = 0;
        for ch in text.chars() {
            let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + w > width {
                break;
            }
            used += w;
            out.push(ch);
        }
        out
    };

    let before_width = UnicodeWidthStr::width(before);
    let (left_marker, before_visible, after_budget) = if before_width > content_budget {
        let visible = suffix(before, content_budget.saturating_sub(1));
        ("\u{2026}", visible, 0)
    } else {
        (
            "",
            before.to_string(),
            content_budget.saturating_sub(before_width),
        )
    };

    let after_width = UnicodeWidthStr::width(after);
    let (after_visible, right_marker) = if after_width > after_budget {
        (
            prefix(after, after_budget.saturating_sub(1)),
            if after_budget > 0 { "\u{2026}" } else { "" },
        )
    } else {
        (after.to_string(), "")
    };

    format!("{left_marker}{before_visible}{marker}{after_visible}{right_marker}")
}

#[cfg(test)]
mod text_input_tests {
    use super::{handle_text_input, text_input_view};
    use crossterm::event::{KeyCode, KeyModifiers};
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn unicode_cursor_edits_at_character_boundaries() {
        let mut value = "a界b".to_string();
        let mut cursor = value.len();

        handle_text_input(&mut value, &mut cursor, KeyCode::Left, KeyModifiers::NONE);
        handle_text_input(
            &mut value,
            &mut cursor,
            KeyCode::Backspace,
            KeyModifiers::NONE,
        );
        handle_text_input(
            &mut value,
            &mut cursor,
            KeyCode::Char('中'),
            KeyModifiers::NONE,
        );

        assert_eq!(value, "a中b");
        assert_eq!(cursor, "a中".len());
    }

    #[test]
    fn control_word_delete_removes_the_previous_word() {
        let mut value = "open -a IINA".to_string();
        let mut cursor = value.len();

        handle_text_input(
            &mut value,
            &mut cursor,
            KeyCode::Char('w'),
            KeyModifiers::CONTROL,
        );

        assert_eq!(value, "open -a ");
        assert_eq!(cursor, value.len());
    }

    #[test]
    fn long_input_view_keeps_cursor_visible_within_width() {
        let value = "/this/is/a/very/long/path/that/keeps/growing";
        let rendered = text_input_view(value, value.len(), 18, true);

        assert!(rendered.starts_with('\u{2026}'));
        assert!(rendered.contains('\u{2588}'));
        assert!(rendered.contains("growing"));
        assert!(UnicodeWidthStr::width(rendered.as_str()) <= 18);
    }

    #[test]
    fn input_view_shows_text_after_a_midline_cursor() {
        let value = "alpha界omega";
        let rendered = text_input_view(value, "alpha".len(), 12, true);

        assert!(rendered.contains("alpha"));
        assert!(rendered.contains('\u{2588}'));
        assert!(rendered.contains('\u{754c}'));
        assert!(UnicodeWidthStr::width(rendered.as_str()) <= 12);
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
        return Err(anyhow::anyhow!(
            "thumbnail download failed: {}",
            response.status()
        ));
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
        assert_eq!(wrap_line("abcdefghijklm", 5), vec!["abcde", "fghij", "klm"]);
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
        let logs = [
            "short",
            "a]medium length line here",
            "abcdefghijklmnopqrstuvwxyz",
        ];
        let wrapped = wrap_logs(logs.iter().copied(), 10);
        assert_eq!(wrapped.len(), 7);
    }

    #[test]
    fn scroll_bottom_shows_last_lines() {
        let logs = ["line1", "line2", "line3", "line4", "line5"];
        let wrapped = wrap_logs(logs.iter().copied(), 50);
        let visible = 3;
        let max_scroll = wrapped.len().saturating_sub(visible);
        let bottom: Vec<&str> = wrapped
            .iter()
            .skip(max_scroll)
            .take(visible)
            .map(|s| s.as_str())
            .collect();
        assert_eq!(bottom, vec!["line3", "line4", "line5"]);
    }

    #[test]
    fn scroll_with_wrapped_lines_reaches_bottom() {
        let logs = [
            "short",
            "this is a very long line that will wrap multiple times in a narrow window!",
            "last line",
        ];
        let width = 20;
        let visible = 5;
        let wrapped = wrap_logs(logs.iter().copied(), width);
        let total = wrapped.len();
        let max_scroll = total.saturating_sub(visible);
        let bottom: Vec<&str> = wrapped
            .iter()
            .skip(max_scroll)
            .take(visible)
            .map(|s| s.as_str())
            .collect();
        assert_eq!(bottom.last().unwrap(), &"last line");
    }
}
