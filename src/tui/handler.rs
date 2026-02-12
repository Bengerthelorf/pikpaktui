use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::pikpak::{Entry, EntryKind};

use super::completion::PathInput;
use super::download::{DownloadTask, TaskStatus};
use super::local_completion::LocalPathInput;
use super::{handle_text_input, App, InputMode, LoginField, OpResult, PickerState, PreviewState};

impl App {
    pub(super) fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Result<bool> {
        // Help sheet: any key closes it
        if self.show_help_sheet {
            self.show_help_sheet = false;
            return Ok(false);
        }

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
                        let (e, p) = (email.clone(), password.clone());
                        if e.trim().is_empty() || p.is_empty() {
                            self.input = InputMode::Login {
                                field,
                                email,
                                password,
                                error: Some("Email and password are required".into()),
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
                            LoginField::Email => {
                                email.pop();
                            }
                            LoginField::Password => {
                                password.pop();
                            }
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
                    KeyCode::Char('p') => {
                        self.input = InputMode::ConfirmPermanentDelete {
                            value: String::new(),
                        };
                    }
                    KeyCode::Char('n') | KeyCode::Esc => {
                        self.push_log("Remove cancelled".into());
                    }
                    _ => {
                        self.input = InputMode::ConfirmDelete;
                    }
                }
                Ok(false)
            }
            InputMode::ConfirmPermanentDelete { mut value } => {
                match code {
                    KeyCode::Esc => {
                        self.push_log("Permanent delete cancelled".into());
                    }
                    KeyCode::Enter => {
                        if value == "yes" {
                            if let Some(entry) = self.current_entry().cloned() {
                                self.spawn_permanent_delete(entry);
                            }
                        } else {
                            self.push_log("Permanent delete cancelled (type 'yes' to confirm)".into());
                        }
                    }
                    KeyCode::Backspace => {
                        value.pop();
                        self.input = InputMode::ConfirmPermanentDelete { value };
                    }
                    KeyCode::Char(c) => {
                        value.push(c);
                        self.input = InputMode::ConfirmPermanentDelete { value };
                    }
                    _ => {
                        self.input = InputMode::ConfirmPermanentDelete { value };
                    }
                }
                Ok(false)
            }
            InputMode::MoveInput {
                source,
                mut input,
            } => {
                self.handle_path_input_key(code, modifiers, source, &mut input, true);
                Ok(false)
            }
            InputMode::CopyInput {
                source,
                mut input,
            } => {
                self.handle_path_input_key(code, modifiers, source, &mut input, false);
                Ok(false)
            }
            InputMode::MovePicker {
                source,
                mut picker,
            } => {
                self.handle_picker_key(code, source, &mut picker, true);
                Ok(false)
            }
            InputMode::CopyPicker {
                source,
                mut picker,
            } => {
                self.handle_picker_key(code, source, &mut picker, false);
                Ok(false)
            }
            InputMode::CartView => {
                self.handle_cart_view_key(code);
                Ok(false)
            }
            InputMode::DownloadInput { mut input } => {
                self.handle_download_input_key(code, &mut input);
                Ok(false)
            }
            InputMode::DownloadView => {
                self.handle_download_view_key(code);
                Ok(false)
            }
            InputMode::OfflineInput { mut value } => {
                self.handle_offline_input_key(code, &mut value);
                Ok(false)
            }
            InputMode::OfflineTasksView {
                mut tasks,
                mut selected,
            } => {
                self.handle_offline_tasks_key(code, &mut tasks, &mut selected);
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
                    self.on_cursor_move();
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.on_cursor_move();
                }
            }
            KeyCode::Enter => {
                if let Some(entry) = self.current_entry().cloned() {
                    if entry.kind == EntryKind::Folder {
                        // Check if preview already has this folder's children cached
                        let cached_children = if self.preview_target_id.as_deref() == Some(&entry.id) {
                            if let PreviewState::FolderListing(children) = std::mem::replace(
                                &mut self.preview_state,
                                PreviewState::Empty,
                            ) {
                                Some(children)
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        // Cache current entries as parent (avoid re-fetching)
                        self.parent_entries = std::mem::take(&mut self.entries);
                        self.parent_selected = self.selected;
                        let old_id =
                            std::mem::replace(&mut self.current_folder_id, entry.id);
                        self.breadcrumb.push((old_id, entry.name));
                        self.selected = 0;
                        self.clear_preview();

                        if let Some(children) = cached_children {
                            // Reuse cached preview data — no API call needed
                            self.entries = children;
                            self.push_log(format!("Refreshed {}", self.current_path_display()));
                        } else {
                            // Request current directory listing
                            self.loading = true;
                            let client = Arc::clone(&self.client);
                            let tx = self.result_tx.clone();
                            let fid = self.current_folder_id.clone();
                            std::thread::spawn(move || {
                                let _ = tx.send(OpResult::Ls(client.ls(&fid)));
                            });
                        }
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some((parent_id, _)) = self.breadcrumb.pop() {
                    self.current_folder_id = parent_id;
                    self.selected = 0;
                    self.clear_preview();
                    self.refresh();
                }
            }
            KeyCode::Char('l') => {
                self.show_logs_overlay = !self.show_logs_overlay;
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
            KeyCode::Char('h') => {
                self.show_help_sheet = true;
            }
            KeyCode::Char('a') => {
                // Toggle current file in/out of cart
                if let Some(entry) = self.current_entry().cloned() {
                    if entry.kind == EntryKind::File {
                        if self.cart_ids.contains(&entry.id) {
                            self.cart_ids.remove(&entry.id);
                            self.cart.retain(|e| e.id != entry.id);
                            self.push_log(format!("Removed '{}' from cart", entry.name));
                        } else {
                            self.cart_ids.insert(entry.id.clone());
                            self.push_log(format!("Added '{}' to cart", entry.name));
                            self.cart.push(entry);
                        }
                    }
                }
            }
            KeyCode::Char('A') => {
                self.input = InputMode::CartView;
            }
            KeyCode::Char('D') => {
                self.input = InputMode::DownloadView;
            }
            KeyCode::Char('s') => {
                // Star/unstar current entry
                if let Some(entry) = self.current_entry().cloned() {
                    self.spawn_star_toggle(entry);
                }
            }
            KeyCode::Char('o') => {
                // Offline download URL input
                self.input = InputMode::OfflineInput {
                    value: String::new(),
                };
            }
            KeyCode::Char('O') => {
                // Offline tasks view
                self.open_offline_tasks_view();
            }
            KeyCode::Char('i') => {
                // Load preview into right pane
                if self.current_entry().is_some() {
                    self.fetch_preview_for_selected();
                }
            }
            _ => {}
        }
        Ok(false)
    }

    pub(super) fn start_move_copy(&mut self, source: Entry, is_move: bool) {
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
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        source: Entry,
        input: &mut PathInput,
        is_move: bool,
    ) {
        // Ctrl+B: switch to picker
        if code == KeyCode::Char('b') && modifiers.contains(KeyModifiers::CONTROL) {
            self.init_picker(source, is_move);
            return;
        }

        match code {
            KeyCode::Esc => {
                if !input.candidates.is_empty() {
                    // Close candidate list
                    input.candidates.clear();
                    input.candidate_idx = None;
                    input.completion_base.clear();
                    self.restore_path_input(source, input, is_move);
                } else {
                    let op = if is_move { "Move" } else { "Copy" };
                    self.push_log(format!("{} cancelled", op));
                }
            }
            KeyCode::Enter => {
                if !input.candidates.is_empty() {
                    // Select current candidate: close candidate list, keep value
                    input.candidates.clear();
                    input.candidate_idx = None;
                    self.restore_path_input(source, input, is_move);
                } else {
                    let target = input.value.trim().to_string();
                    if !target.is_empty() {
                        self.execute_move_copy(source, &target, is_move);
                    }
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
                input.completion_base.clear();
                self.restore_path_input(source, input, is_move);
            }
            KeyCode::Char(c) => {
                input.value.push(c);
                input.candidates.clear();
                input.candidate_idx = None;
                input.completion_base.clear();
                self.restore_path_input(source, input, is_move);
            }
            _ => {
                self.restore_path_input(source, input, is_move);
            }
        }
    }

    fn restore_path_input(&mut self, source: Entry, input: &mut PathInput, is_move: bool) {
        let owned = PathInput {
            value: std::mem::take(&mut input.value),
            candidates: std::mem::take(&mut input.candidates),
            candidate_idx: input.candidate_idx,
            completion_base: std::mem::take(&mut input.completion_base),
        };
        if is_move {
            self.input = InputMode::MoveInput {
                source,
                input: owned,
            };
        } else {
            self.input = InputMode::CopyInput {
                source,
                input: owned,
            };
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
        let picker = PickerState {
            folder_id,
            breadcrumb,
            entries,
            selected: 0,
            loading: false,
        };
        if is_move {
            self.input = InputMode::MovePicker { source, picker };
        } else {
            self.input = InputMode::CopyPicker { source, picker };
        }
    }

    fn handle_picker_key(
        &mut self,
        code: KeyCode,
        source: Entry,
        picker: &mut PickerState,
        is_move: bool,
    ) {
        let folder_count = picker
            .entries
            .iter()
            .filter(|e| e.kind == EntryKind::Folder)
            .count();

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
                if picker.selected > 0 {
                    picker.selected -= 1;
                }
                self.restore_picker(source, picker, is_move);
            }
            KeyCode::Enter => {
                let folders: Vec<Entry> = picker
                    .entries
                    .iter()
                    .filter(|e| e.kind == EntryKind::Folder)
                    .cloned()
                    .collect();
                if let Some(entry) = folders.get(picker.selected).cloned() {
                    let old_id = std::mem::replace(&mut picker.folder_id, entry.id.clone());
                    picker.breadcrumb.push((old_id, entry.name.clone()));
                    picker.selected = 0;
                    picker.loading = true;
                    match self.client.ls(&entry.id) {
                        Ok(entries) => {
                            picker.entries = entries;
                            picker.loading = false;
                        }
                        Err(e) => {
                            self.push_log(format!("Picker load failed: {e:#}"));
                            picker.loading = false;
                        }
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
                        Ok(entries) => {
                            picker.entries = entries;
                            picker.loading = false;
                        }
                        Err(e) => {
                            self.push_log(format!("Picker load failed: {e:#}"));
                            picker.loading = false;
                        }
                    }
                }
                self.restore_picker(source, picker, is_move);
            }
            KeyCode::Char(' ') => {
                let dest_id = picker.folder_id.clone();
                let dest_path = Self::picker_path_display(picker);
                self.spawn_move_copy(source, dest_id, dest_path, is_move);
            }
            KeyCode::Char('h') => {
                self.show_help_sheet = true;
                self.restore_picker(source, picker, is_move);
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
            self.input = InputMode::MovePicker {
                source,
                picker: owned,
            };
        } else {
            self.input = InputMode::CopyPicker {
                source,
                picker: owned,
            };
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

    fn spawn_move_copy(
        &mut self,
        source: Entry,
        dest_id: String,
        dest_path: String,
        is_move: bool,
    ) {
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

    pub(super) fn spawn_rename(&mut self, entry: Entry, new_name: String) {
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

    pub(super) fn spawn_mkdir(&mut self, name: String) {
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

    // --- Cart View ---

    fn handle_cart_view_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => {
                // Back to normal
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.cart.is_empty() {
                    self.cart_selected = (self.cart_selected + 1).min(self.cart.len() - 1);
                }
                self.input = InputMode::CartView;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.cart_selected > 0 {
                    self.cart_selected -= 1;
                }
                self.input = InputMode::CartView;
            }
            KeyCode::Char('x') | KeyCode::Char('d') => {
                // Remove selected from cart
                if !self.cart.is_empty() && self.cart_selected < self.cart.len() {
                    let removed = self.cart.remove(self.cart_selected);
                    self.cart_ids.remove(&removed.id);
                    self.push_log(format!("Removed '{}' from cart", removed.name));
                    if self.cart_selected >= self.cart.len() && self.cart_selected > 0 {
                        self.cart_selected -= 1;
                    }
                }
                self.input = InputMode::CartView;
            }
            KeyCode::Char('a') => {
                // Clear all
                let count = self.cart.len();
                self.cart.clear();
                self.cart_ids.clear();
                self.cart_selected = 0;
                self.push_log(format!("Cleared {} items from cart", count));
                self.input = InputMode::CartView;
            }
            KeyCode::Enter => {
                if self.cart.is_empty() {
                    self.push_log("Cart is empty".into());
                    self.input = InputMode::CartView;
                } else {
                    self.input = InputMode::DownloadInput {
                        input: LocalPathInput::new(),
                    };
                }
            }
            _ => {
                self.input = InputMode::CartView;
            }
        }
    }

    // --- Download Input ---

    fn handle_download_input_key(&mut self, code: KeyCode, input: &mut LocalPathInput) {
        match code {
            KeyCode::Esc => {
                if !input.candidates.is_empty() {
                    input.candidates.clear();
                    input.candidate_idx = None;
                    input.completion_base.clear();
                    self.restore_download_input(input);
                } else {
                    self.input = InputMode::CartView;
                }
            }
            KeyCode::Tab => {
                input.tab_complete();
                self.restore_download_input(input);
            }
            KeyCode::Enter => {
                if !input.candidates.is_empty() {
                    input.candidates.clear();
                    input.candidate_idx = None;
                    self.restore_download_input(input);
                } else {
                    let dest = input.value.trim().to_string();
                    if dest.is_empty() {
                        self.push_log("No destination path specified".into());
                        self.restore_download_input(input);
                    } else {
                        self.start_cart_download(&dest);
                        self.input = InputMode::DownloadView;
                    }
                }
            }
            KeyCode::Backspace => {
                input.value.pop();
                input.candidates.clear();
                input.candidate_idx = None;
                input.completion_base.clear();
                self.restore_download_input(input);
            }
            KeyCode::Char(c) => {
                input.value.push(c);
                input.candidates.clear();
                input.candidate_idx = None;
                input.completion_base.clear();
                self.restore_download_input(input);
            }
            _ => {
                self.restore_download_input(input);
            }
        }
    }

    fn restore_download_input(&mut self, input: &mut LocalPathInput) {
        let owned = LocalPathInput {
            value: std::mem::take(&mut input.value),
            candidates: std::mem::take(&mut input.candidates),
            candidate_idx: input.candidate_idx,
            completion_base: std::mem::take(&mut input.completion_base),
        };
        self.input = InputMode::DownloadInput { input: owned };
    }

    fn start_cart_download(&mut self, dest_dir: &str) {
        let dest = PathBuf::from(dest_dir);
        let cart_items: Vec<Entry> = self.cart.drain(..).collect();
        self.cart_ids.clear();
        self.cart_selected = 0;

        let count = cart_items.len();
        for item in cart_items {
            let file_dest = dest.join(&item.name);
            let task = DownloadTask {
                file_id: item.id,
                name: item.name,
                total_size: item.size,
                downloaded: 0,
                dest_path: file_dest,
                status: TaskStatus::Pending,
                pause_flag: Arc::new(AtomicBool::new(false)),
                cancel_flag: Arc::new(AtomicBool::new(false)),
                speed: 0.0,
            };
            self.download_state.tasks.push(task);
        }

        self.push_log(format!("Queued {} files for download", count));
        self.download_state.start_next(&self.client);
    }

    // --- Download View ---

    fn handle_download_view_key(&mut self, code: KeyCode) {
        let task_count = self.download_state.tasks.len();

        match code {
            KeyCode::Esc => {
                // Back to normal, downloads continue in background
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if task_count > 0 {
                    self.download_state.selected =
                        (self.download_state.selected + 1).min(task_count - 1);
                }
                self.input = InputMode::DownloadView;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.download_state.selected > 0 {
                    self.download_state.selected -= 1;
                }
                self.input = InputMode::DownloadView;
            }
            KeyCode::Char('p') => {
                // Pause/resume selected
                let sel = self.download_state.selected;
                let mut log_msg = None;
                let mut need_start = false;
                if let Some(task) = self.download_state.tasks.get_mut(sel) {
                    match task.status {
                        TaskStatus::Downloading => {
                            task.pause_flag.store(true, Ordering::Relaxed);
                            task.status = TaskStatus::Paused;
                            log_msg = Some(format!("Paused '{}'", task.name));
                        }
                        TaskStatus::Paused => {
                            task.pause_flag.store(false, Ordering::Relaxed);
                            task.status = TaskStatus::Pending;
                            log_msg = Some(format!("Resumed '{}'", task.name));
                            need_start = true;
                        }
                        _ => {}
                    }
                }
                if let Some(msg) = log_msg {
                    self.push_log(msg);
                }
                if need_start {
                    self.download_state.start_next(&self.client);
                }
                self.input = InputMode::DownloadView;
            }
            KeyCode::Char('x') => {
                // Cancel selected
                let sel = self.download_state.selected;
                if let Some(task) = self.download_state.tasks.get_mut(sel) {
                    if matches!(
                        task.status,
                        TaskStatus::Downloading | TaskStatus::Paused | TaskStatus::Pending
                    ) {
                        task.cancel_flag.store(true, Ordering::Relaxed);
                        let name = task.name.clone();
                        self.download_state.tasks.remove(sel);
                        if self.download_state.selected >= self.download_state.tasks.len()
                            && self.download_state.selected > 0
                        {
                            self.download_state.selected -= 1;
                        }
                        self.push_log(format!("Cancelled '{}'", name));
                        self.download_state.start_next(&self.client);
                    }
                }
                self.input = InputMode::DownloadView;
            }
            KeyCode::Char('r') => {
                // Retry failed task
                let sel = self.download_state.selected;
                let mut log_msg = None;
                let mut need_start = false;
                if let Some(task) = self.download_state.tasks.get_mut(sel) {
                    if matches!(task.status, TaskStatus::Failed(_)) {
                        task.status = TaskStatus::Pending;
                        task.cancel_flag.store(false, Ordering::Relaxed);
                        task.pause_flag.store(false, Ordering::Relaxed);
                        log_msg = Some(format!("Retrying '{}'", task.name));
                        need_start = true;
                    }
                }
                if let Some(msg) = log_msg {
                    self.push_log(msg);
                }
                if need_start {
                    self.download_state.start_next(&self.client);
                }
                self.input = InputMode::DownloadView;
            }
            _ => {
                self.input = InputMode::DownloadView;
            }
        }
    }

    // --- Star/Unstar ---

    fn spawn_star_toggle(&mut self, entry: Entry) {
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        let eid = entry.id.clone();
        let name = entry.name.clone();
        self.loading = true;
        // We always star — there's no local state tracking starred status.
        // If already starred, the API is idempotent.
        std::thread::spawn(move || {
            let _ = tx.send(match client.star(&[eid.as_str()]) {
                Ok(()) => OpResult::Ok(format!("Starred '{}'", name)),
                Err(e) => OpResult::Err(format!("Star failed: {e:#}")),
            });
        });
    }

    // --- Offline download input ---

    fn handle_offline_input_key(&mut self, code: KeyCode, value: &mut String) {
        match code {
            KeyCode::Esc => {
                self.push_log("Offline download cancelled".into());
            }
            KeyCode::Enter => {
                let url = value.trim().to_string();
                if url.is_empty() {
                    self.push_log("No URL provided".into());
                    self.input = InputMode::OfflineInput {
                        value: std::mem::take(value),
                    };
                } else {
                    self.spawn_offline_download(url);
                }
            }
            KeyCode::Backspace => {
                value.pop();
                self.input = InputMode::OfflineInput {
                    value: std::mem::take(value),
                };
            }
            KeyCode::Char(c) => {
                value.push(c);
                self.input = InputMode::OfflineInput {
                    value: std::mem::take(value),
                };
            }
            _ => {
                self.input = InputMode::OfflineInput {
                    value: std::mem::take(value),
                };
            }
        }
    }

    fn spawn_offline_download(&mut self, url: String) {
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        let parent_id = if self.current_folder_id.is_empty() {
            None
        } else {
            Some(self.current_folder_id.clone())
        };
        self.loading = true;
        std::thread::spawn(move || {
            let result = client.offline_download(&url, parent_id.as_deref(), None);
            let _ = tx.send(match result {
                Ok(resp) => {
                    let name = resp
                        .task
                        .as_ref()
                        .map(|t| t.name.as_str())
                        .unwrap_or("unknown");
                    OpResult::Ok(format!("Offline task created: {}", name))
                }
                Err(e) => OpResult::Err(format!("Offline download failed: {e:#}")),
            });
        });
    }

    // --- Offline tasks view ---

    fn open_offline_tasks_view(&mut self) {
        let phases = &[
            "PHASE_TYPE_RUNNING",
            "PHASE_TYPE_PENDING",
            "PHASE_TYPE_COMPLETE",
            "PHASE_TYPE_ERROR",
        ];
        match self.client.offline_list(50, phases) {
            Ok(resp) => {
                self.input = InputMode::OfflineTasksView {
                    tasks: resp.tasks,
                    selected: 0,
                };
            }
            Err(e) => {
                self.push_log(format!("Failed to load offline tasks: {e:#}"));
            }
        }
    }

    fn handle_offline_tasks_key(
        &mut self,
        code: KeyCode,
        tasks: &mut Vec<crate::pikpak::OfflineTask>,
        selected: &mut usize,
    ) {
        match code {
            KeyCode::Esc => {
                // Back to normal
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !tasks.is_empty() {
                    *selected = (*selected + 1).min(tasks.len() - 1);
                }
                self.input = InputMode::OfflineTasksView {
                    tasks: std::mem::take(tasks),
                    selected: *selected,
                };
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if *selected > 0 {
                    *selected -= 1;
                }
                self.input = InputMode::OfflineTasksView {
                    tasks: std::mem::take(tasks),
                    selected: *selected,
                };
            }
            KeyCode::Char('r') => {
                // Refresh
                self.open_offline_tasks_view();
            }
            KeyCode::Char('R') => {
                // Retry selected task
                if let Some(task) = tasks.get(*selected) {
                    if task.phase == "PHASE_TYPE_ERROR" {
                        let task_id = task.id.clone();
                        match self.client.offline_task_retry(&task_id) {
                            Ok(()) => self.push_log(format!("Retrying task: {}", task.name)),
                            Err(e) => self.push_log(format!("Retry failed: {e:#}")),
                        }
                        self.open_offline_tasks_view();
                        return;
                    }
                }
                self.input = InputMode::OfflineTasksView {
                    tasks: std::mem::take(tasks),
                    selected: *selected,
                };
            }
            KeyCode::Char('x') => {
                // Delete selected task
                if let Some(task) = tasks.get(*selected) {
                    let task_id = task.id.clone();
                    match self.client.delete_tasks(&[task_id.as_str()], false) {
                        Ok(()) => self.push_log(format!("Deleted task: {}", task.name)),
                        Err(e) => self.push_log(format!("Delete task failed: {e:#}")),
                    }
                    self.open_offline_tasks_view();
                    return;
                }
                self.input = InputMode::OfflineTasksView {
                    tasks: std::mem::take(tasks),
                    selected: *selected,
                };
            }
            _ => {
                self.input = InputMode::OfflineTasksView {
                    tasks: std::mem::take(tasks),
                    selected: *selected,
                };
            }
        }
    }

    pub(super) fn spawn_delete(&mut self, entry: Entry) {
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

    pub(super) fn spawn_permanent_delete(&mut self, entry: Entry) {
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        let eid = entry.id.clone();
        let name = entry.name.clone();
        self.loading = true;
        std::thread::spawn(move || {
            let _ = tx.send(match client.delete_permanent(&[eid.as_str()]) {
                Ok(()) => OpResult::Ok(format!("Permanently deleted '{}'", name)),
                Err(e) => OpResult::Err(format!("Permanent delete failed: {e:#}")),
            });
        });
    }
}
