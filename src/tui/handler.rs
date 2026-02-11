use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use std::sync::Arc;

use crate::pikpak::{Entry, EntryKind};

use super::completion::PathInput;
use super::{handle_text_input, App, InputMode, LoginField, OpResult, PickerState};

impl App {
    pub(super) fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Result<bool> {
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
                    KeyCode::Char('n') | KeyCode::Esc => {
                        self.push_log("Remove cancelled".into());
                    }
                    _ => {
                        self.input = InputMode::ConfirmDelete;
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
                        let old_id =
                            std::mem::replace(&mut self.current_folder_id, entry.id);
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
}
