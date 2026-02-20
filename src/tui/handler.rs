use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::pikpak::{Entry, EntryKind};
use crate::theme;

use super::completion::PathInput;
use super::download::{DownloadTask, TaskStatus};
use super::local_completion::LocalPathInput;
use super::{App, InputMode, LoginField, OpResult, PickerState, PlayOption, PreviewState, handle_text_input};

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
            InputMode::Normal => self.handle_normal_key(code, modifiers),
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
            InputMode::ConfirmQuit => {
                match code {
                    KeyCode::Char('y') => {
                        return Ok(true);
                    }
                    KeyCode::Char('n') | KeyCode::Esc => {
                        // cancel, return to Normal
                    }
                    _ => {
                        self.input = InputMode::ConfirmQuit;
                    }
                }
                Ok(false)
            }
            InputMode::GotoPath { mut query } => {
                match handle_text_input(&mut query, code) {
                    Some(true) => {
                        // Enter — resolve path in background
                        let q = query.trim().to_string();
                        if !q.is_empty() {
                            self.loading = true;
                            let client = Arc::clone(&self.client);
                            let tx = self.result_tx.clone();
                            std::thread::spawn(move || {
                                let _ = tx.send(OpResult::GotoPath(client.resolve_path_nav(&q)));
                            });
                        }
                    }
                    Some(false) => { /* ESC — Normal already set by mem::replace */ }
                    None => {
                        self.input = InputMode::GotoPath { query };
                    }
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
                            self.push_log(
                                "Permanent delete cancelled (type 'yes' to confirm)".into(),
                            );
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
            InputMode::CartView => {
                self.handle_cart_view_key(code);
                Ok(false)
            }
            InputMode::DownloadInput { mut input } => {
                self.handle_download_input_key(code, &mut input);
                Ok(false)
            }
            InputMode::UploadInput { mut input } => {
                self.handle_upload_input_key(code, &mut input);
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
            InputMode::TrashView {
                mut entries,
                mut selected,
                expanded,
            } => {
                self.handle_trash_view_key(code, &mut entries, &mut selected, expanded);
                Ok(false)
            }
            InputMode::ConfirmPlay { name, url } => {
                match code {
                    KeyCode::Enter | KeyCode::Char('y') => {
                        if let Some(player) = self.config.player.clone() {
                            self.spawn_player(&player, &url);
                        } else {
                            self.input = InputMode::PlayerInput {
                                value: String::new(),
                                pending_url: url,
                            };
                        }
                    }
                    KeyCode::Esc | KeyCode::Char('n') => {}
                    _ => {
                        self.input = InputMode::ConfirmPlay { name, url };
                    }
                }
                Ok(false)
            }
            InputMode::PlayPicker {
                name,
                medias,
                mut selected,
            } => {
                match code {
                    KeyCode::Down | KeyCode::Char('j') => {
                        // Find next available item
                        let mut next = selected + 1;
                        while next < medias.len() && !medias[next].available {
                            next += 1;
                        }
                        if next < medias.len() {
                            selected = next;
                        }
                        self.input = InputMode::PlayPicker {
                            name,
                            medias,
                            selected,
                        };
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if selected > 0 {
                            let mut prev = selected - 1;
                            while prev > 0 && !medias[prev].available {
                                prev -= 1;
                            }
                            if medias[prev].available {
                                selected = prev;
                            }
                        }
                        self.input = InputMode::PlayPicker {
                            name,
                            medias,
                            selected,
                        };
                    }
                    KeyCode::Enter => {
                        if let Some(opt) = medias.get(selected) {
                            if opt.available {
                                let url = opt.url.clone();
                                if let Some(player) = self.config.player.clone() {
                                    self.spawn_player(&player, &url);
                                } else {
                                    self.input = InputMode::PlayerInput {
                                        value: String::new(),
                                        pending_url: url,
                                    };
                                }
                            } else {
                                self.push_log("Stream not available (cold storage)".into());
                                self.input = InputMode::PlayPicker {
                                    name,
                                    medias,
                                    selected,
                                };
                            }
                        }
                    }
                    KeyCode::Esc => {}
                    _ => {
                        self.input = InputMode::PlayPicker {
                            name,
                            medias,
                            selected,
                        };
                    }
                }
                Ok(false)
            }
            InputMode::PlayerInput {
                mut value,
                pending_url,
            } => {
                match code {
                    KeyCode::Esc => {}
                    KeyCode::Enter => {
                        let cmd = value.trim().to_string();
                        if !cmd.is_empty() {
                            self.config.player = Some(cmd.clone());
                            let _ = self.config.save();
                            self.push_log(format!("Player set to: {}", cmd));
                            self.spawn_player(&cmd, &pending_url);
                        } else {
                            self.input = InputMode::PlayerInput {
                                value,
                                pending_url,
                            };
                        }
                    }
                    KeyCode::Backspace => {
                        value.pop();
                        self.input = InputMode::PlayerInput {
                            value,
                            pending_url,
                        };
                    }
                    KeyCode::Char(c) => {
                        value.push(c);
                        self.input = InputMode::PlayerInput {
                            value,
                            pending_url,
                        };
                    }
                    _ => {
                        self.input = InputMode::PlayerInput {
                            value,
                            pending_url,
                        };
                    }
                }
                Ok(false)
            }
            InputMode::InfoLoading => {
                if code == KeyCode::Esc {
                    if !self.trash_entries.is_empty() {
                        self.input = InputMode::TrashView {
                            entries: std::mem::take(&mut self.trash_entries),
                            selected: self.trash_selected,
                            expanded: self.trash_expanded,
                        };
                    } else {
                        self.input = InputMode::Normal;
                    }
                    self.finish_loading();
                }
                Ok(false)
            }
            InputMode::InfoView { .. } => {
                // Any key closes info view; return to trash view if we came from there
                if !self.trash_entries.is_empty() {
                    self.input = InputMode::TrashView {
                        entries: std::mem::take(&mut self.trash_entries),
                        selected: self.trash_selected,
                        expanded: self.trash_expanded,
                    };
                }
                Ok(false)
            }
            InputMode::InfoFolderView { entries, .. } => {
                // Cache listing so Enter can reuse it
                self.preview_state = PreviewState::FolderListing(entries);
                Ok(false)
            }
            InputMode::TextPreviewView { .. } => {
                // Any key closes text preview view
                Ok(false)
            }
            InputMode::Settings {
                mut selected,
                mut editing,
                mut draft,
                mut modified,
            } => {
                let result = self.handle_settings_key(code, &mut selected, &mut editing, &mut draft, &mut modified);

                // Check if handle_settings_key changed the input mode (e.g., entered CustomColorSettings)
                if !matches!(self.input, InputMode::Normal) {
                    return Ok(false);
                }

                match result {
                    None => {
                        self.input = InputMode::Settings {
                            selected,
                            editing,
                            draft,
                            modified,
                        };
                    }
                    Some(should_save) => {
                        if should_save {
                            match draft.save() {
                                Ok(()) => {
                                    self.config = draft;
                                    self.resort_entries();
                                    self.push_log("Settings saved to config.toml".into());
                                    self.input = InputMode::Normal;
                                }
                                Err(e) => {
                                    self.push_log(format!("Failed to save config: {:#}", e));
                                    self.input = InputMode::Settings {
                                        selected,
                                        editing,
                                        draft,
                                        modified,
                                    };
                                }
                            }
                        } else {
                            self.input = InputMode::Normal;
                        }
                    }
                }
                Ok(false)
            }
            InputMode::CustomColorSettings {
                mut selected,
                mut draft,
                mut modified,
                mut editing_rgb,
                mut rgb_input,
                mut rgb_component,
            } => {
                self.handle_custom_color_key(
                    code,
                    &mut selected,
                    &mut draft,
                    &mut modified,
                    &mut editing_rgb,
                    &mut rgb_input,
                    &mut rgb_component,
                );
                Ok(false)
            }
            InputMode::ImageProtocolSettings {
                mut selected,
                mut draft,
                mut modified,
                current_terminal,
                terminals,
            } => {
                self.handle_image_protocol_key(
                    code,
                    &mut selected,
                    &mut draft,
                    &mut modified,
                    &current_terminal,
                    &terminals,
                );
                Ok(false)
            }
        }
    }

    fn handle_normal_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Result<bool> {
        match code {
            KeyCode::Char('q') => {
                if self.download_state.has_active() {
                    self.input = InputMode::ConfirmQuit;
                } else {
                    return Ok(true);
                }
            }
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
                        let cached_children =
                            if self.preview_target_id.as_deref() == Some(&entry.id) {
                                if let PreviewState::FolderListing(children) =
                                    std::mem::replace(&mut self.preview_state, PreviewState::Empty)
                                {
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
                        let old_id = std::mem::replace(&mut self.current_folder_id, entry.id);
                        self.breadcrumb.push((old_id, entry.name));
                        self.selected = 0;
                        self.clear_preview();

                        if let Some(children) = cached_children {
                            // Reuse cached preview data — no API call needed
                            self.entries = children;
                            self.push_log(format!("Refreshed {}", self.current_path_display()));
                            self.on_cursor_move();
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
                    } else if entry.kind == EntryKind::File
                        && theme::categorize(&entry) == theme::FileCategory::Video
                    {
                        // Video file: fetch info for playback
                        self.loading = true;
                        let client = Arc::clone(&self.client);
                        let tx = self.result_tx.clone();
                        let eid = entry.id.clone();
                        std::thread::spawn(move || {
                            let _ = tx.send(OpResult::PlayInfo(client.file_info(&eid)));
                        });
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some((parent_id, _)) = self.breadcrumb.pop() {
                    let leaving_id = std::mem::replace(&mut self.current_folder_id, parent_id);
                    let old_entries = std::mem::replace(
                        &mut self.entries,
                        std::mem::take(&mut self.parent_entries),
                    );
                    self.selected = self.parent_selected;

                    // Clamp selected to valid range
                    if !self.entries.is_empty() && self.selected >= self.entries.len() {
                        self.selected = self.entries.len() - 1;
                    }

                    // Preview: show children of the folder we just left
                    if self.config.show_preview {
                        self.preview_state = PreviewState::FolderListing(old_entries);
                        self.preview_target_id = Some(leaving_id);
                    } else {
                        self.clear_preview();
                    }
                    self.pending_preview_fetch = false;

                    if self.entries.is_empty() {
                        // parent_entries was empty (async fetch hadn't completed),
                        // do a full refresh to reload current directory
                        self.refresh();
                    } else {
                        // Only need to fetch grandparent entries
                        self.refresh_parent();
                    }
                }
            }
            KeyCode::Char('l') => {
                self.show_logs_overlay = !self.show_logs_overlay;
                self.logs_scroll = None;
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
                if modifiers.contains(KeyModifiers::CONTROL) {
                    if !self.entries.is_empty() {
                        let half = (self.list_area_height.get() / 2).max(1) as usize;
                        self.selected = (self.selected + half).min(self.entries.len() - 1);
                        self.on_cursor_move();
                    }
                } else if self.current_entry().is_some() {
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
            KeyCode::Char('u') => {
                if modifiers.contains(KeyModifiers::CONTROL) {
                    if !self.entries.is_empty() {
                        let half = (self.list_area_height.get() / 2).max(1) as usize;
                        self.selected = self.selected.saturating_sub(half);
                        self.on_cursor_move();
                    }
                } else {
                    self.input = InputMode::UploadInput {
                        input: LocalPathInput::new_for_upload(),
                    };
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
            KeyCode::Char('t') => {
                // Trash view
                self.open_trash_view();
            }
            KeyCode::Char('S') => {
                // Cycle sort field
                self.config.sort_field = self.config.sort_field.next();
                self.resort_entries();
                let _ = self.config.save();
            }
            KeyCode::Char('R') => {
                // Toggle reverse sort order
                self.config.sort_reverse = !self.config.sort_reverse;
                self.resort_entries();
                let _ = self.config.save();
            }
            KeyCode::Char('w') => {
                // Watch: open video stream/resolution picker
                if let Some(entry) = self.current_entry().cloned() {
                    if entry.kind == EntryKind::File
                        && theme::categorize(&entry) == theme::FileCategory::Video
                    {
                        self.loading = true;
                        let client = Arc::clone(&self.client);
                        let tx = self.result_tx.clone();
                        let eid = entry.id.clone();
                        std::thread::spawn(move || {
                            let result = client.file_info(&eid);
                            let _ = tx.send(match result {
                                Ok(info) => {
                                    let mut options = Vec::new();
                                    // Original is always available via web_content_link
                                    if let Some(ref url) = info.web_content_link {
                                        if !url.is_empty() {
                                            let size_str = info
                                                .size
                                                .as_deref()
                                                .and_then(|s| s.parse::<u64>().ok())
                                                .map(super::format_size)
                                                .unwrap_or_default();
                                            options.push(PlayOption {
                                                label: format!("Original ({})", size_str),
                                                url: url.clone(),
                                                available: true,
                                            });
                                        }
                                    }
                                    // Transcoded streams from medias
                                    if let Some(ref medias) = info.medias {
                                        for m in medias {
                                            if m.is_origin.unwrap_or(false) {
                                                continue; // skip origin duplicate
                                            }
                                            let url = m
                                                .link
                                                .as_ref()
                                                .and_then(|l| l.url.as_deref())
                                                .unwrap_or("")
                                                .to_string();
                                            if url.is_empty() {
                                                continue;
                                            }
                                            let label = m
                                                .media_name
                                                .as_deref()
                                                .unwrap_or("Unknown")
                                                .to_string();
                                            let available =
                                                crate::pikpak::PikPak::check_stream_available(&url);
                                            options.push(PlayOption {
                                                label,
                                                url,
                                                available,
                                            });
                                        }
                                    }
                                    OpResult::PlayPickerInfo(Ok((info, options)))
                                }
                                Err(e) => OpResult::PlayPickerInfo(Err(e)),
                            });
                        });
                    }
                }
            }
            KeyCode::Char('p') => {
                if let Some(entry) = self.current_entry().cloned() {
                    if entry.kind == EntryKind::File && theme::is_text_previewable(&entry) {
                        if self.config.show_preview {
                            // Fill right preview pane with text content
                            self.fetch_text_preview_for_selected();
                        } else {
                            // Popup overlay
                            self.input = InputMode::InfoLoading;
                            self.loading = true;
                            self.loading_label = Some("Loading preview...".into());
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
                }
            }
            KeyCode::Char(',') => {
                self.input = InputMode::Settings {
                    selected: 0,
                    editing: false,
                    draft: self.config.clone(),
                    modified: false,
                };
            }
            KeyCode::Char(' ') => {
                if let Some(entry) = self.current_entry().cloned() {
                    if self.config.show_preview {
                        // Fill right preview pane
                        self.fetch_preview_for_selected();
                    } else {
                        // Popup overlay (no preview pane)
                        match entry.kind {
                            EntryKind::File => self.open_info_popup(entry),
                            EntryKind::Folder => self.open_folder_info_popup(entry),
                        }
                    }
                }
            }
            // --- Navigation enhancements ---
            KeyCode::Char('g') => {
                if !self.entries.is_empty() {
                    self.selected = 0;
                    self.on_cursor_move();
                }
            }
            KeyCode::Char('G') => {
                if !self.entries.is_empty() {
                    self.selected = self.entries.len() - 1;
                    self.on_cursor_move();
                }
            }
            KeyCode::Char(':') => {
                self.input = InputMode::GotoPath {
                    query: String::new(),
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
                    input.clear_candidates();
                    self.restore_download_input(input);
                } else {
                    self.input = InputMode::CartView;
                }
            }
            // Tab: open candidates (if not open) or move forward
            KeyCode::Tab => {
                if input.candidates.is_empty() {
                    input.open_candidates();
                } else {
                    input.navigate_next();
                }
                self.restore_download_input(input);
            }
            // Shift+Tab: open candidates (if not open) then move backward
            KeyCode::BackTab => {
                if input.candidates.is_empty() {
                    input.open_candidates();
                }
                input.navigate_prev();
                self.restore_download_input(input);
            }
            KeyCode::Up => {
                input.navigate_prev();
                self.restore_download_input(input);
            }
            KeyCode::Down => {
                input.navigate_next();
                self.restore_download_input(input);
            }
            KeyCode::Enter => {
                // Apply selected candidate; if one was applied, stay in overlay
                // to let user navigate deeper or press Enter again to confirm.
                let applied = input.confirm_selected();
                if applied {
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
                if !input.candidates.is_empty() {
                    input.open_candidates(); // re-filter with shorter prefix
                }
                self.restore_download_input(input);
            }
            KeyCode::Char(c) => {
                input.value.push(c);
                if !input.candidates.is_empty() {
                    input.open_candidates(); // re-filter with new character
                }
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
            include_files: input.include_files,
        };
        self.input = InputMode::DownloadInput { input: owned };
    }

    fn restore_upload_input(&mut self, input: &mut LocalPathInput) {
        let owned = LocalPathInput {
            value: std::mem::take(&mut input.value),
            candidates: std::mem::take(&mut input.candidates),
            candidate_idx: input.candidate_idx,
            completion_base: std::mem::take(&mut input.completion_base),
            include_files: input.include_files,
        };
        self.input = InputMode::UploadInput { input: owned };
    }

    fn handle_upload_input_key(&mut self, code: KeyCode, input: &mut LocalPathInput) {
        match code {
            KeyCode::Esc => {
                if !input.candidates.is_empty() {
                    input.clear_candidates();
                    self.restore_upload_input(input);
                } else {
                    self.input = InputMode::Normal;
                }
            }
            // Tab: open candidates (if not open) or move forward
            KeyCode::Tab => {
                if input.candidates.is_empty() {
                    input.open_candidates();
                } else {
                    input.navigate_next();
                }
                self.restore_upload_input(input);
            }
            // Shift+Tab: open candidates (if not open) then move backward
            KeyCode::BackTab => {
                if input.candidates.is_empty() {
                    input.open_candidates();
                }
                input.navigate_prev();
                self.restore_upload_input(input);
            }
            KeyCode::Up => {
                input.navigate_prev();
                self.restore_upload_input(input);
            }
            KeyCode::Down => {
                input.navigate_next();
                self.restore_upload_input(input);
            }
            KeyCode::Enter => {
                // Apply selected candidate first
                let applied = input.confirm_selected();
                if applied && input.value.ends_with('/') {
                    // Entered a directory; stay in overlay for further navigation
                    self.restore_upload_input(input);
                } else {
                    let local_path = std::path::PathBuf::from(input.value.trim());
                    if !local_path.exists() {
                        self.push_log(format!("File not found: {}", local_path.display()));
                        self.restore_upload_input(input);
                    } else if !local_path.is_file() {
                        self.push_log(format!("Not a file: {}", local_path.display()));
                        self.restore_upload_input(input);
                    } else {
                        let folder_id = self.current_folder_id.clone();
                        let client = Arc::clone(&self.client);
                        let tx = self.result_tx.clone();
                        let name = local_path.file_name().unwrap_or_default().to_string_lossy().to_string();
                        self.loading = true;
                        self.loading_label = Some(format!("Uploading {}…", name));
                        self.input = InputMode::Normal;
                        std::thread::spawn(move || {
                            let result = client.upload_file(Some(&folder_id), &local_path)
                                .map(|(name, dedup)| {
                                    if dedup {
                                        format!("Uploaded '{}' (instant, dedup)", name)
                                    } else {
                                        format!("Uploaded '{}'", name)
                                    }
                                });
                            let _ = tx.send(OpResult::Upload(result));
                        });
                    }
                }
            }
            KeyCode::Backspace => {
                input.value.pop();
                if !input.candidates.is_empty() {
                    input.open_candidates(); // re-filter with shorter prefix
                }
                self.restore_upload_input(input);
            }
            KeyCode::Char(c) => {
                input.value.push(c);
                if !input.candidates.is_empty() {
                    input.open_candidates(); // re-filter with new character
                }
                self.restore_upload_input(input);
            }
            _ => {
                self.restore_upload_input(input);
            }
        }
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
            KeyCode::Enter => {
                // Toggle collapsed/expanded mode
                use crate::tui::DownloadViewMode;
                self.download_view_mode = match self.download_view_mode {
                    DownloadViewMode::Collapsed => DownloadViewMode::Expanded,
                    DownloadViewMode::Expanded => DownloadViewMode::Collapsed,
                };
                self.input = InputMode::DownloadView;
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
        let is_starred = entry.starred;
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        let eid = entry.id.clone();
        let name = entry.name.clone();
        self.loading = true;
        std::thread::spawn(move || {
            let result = if is_starred {
                client.unstar(&[eid.as_str()])
            } else {
                client.star(&[eid.as_str()])
            };
            let op = if is_starred { "Unstarred" } else { "Starred" };
            let _ = tx.send(match result {
                Ok(()) => OpResult::Ok(format!("{} '{}'", op, name)),
                Err(e) => OpResult::Err(format!("{} failed: {e:#}", op)),
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
        self.input = InputMode::InfoLoading;
        self.loading = true;
        self.loading_label = Some("Loading offline tasks...".into());
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        std::thread::spawn(move || {
            let phases = &[
                "PHASE_TYPE_RUNNING",
                "PHASE_TYPE_PENDING",
                "PHASE_TYPE_COMPLETE",
                "PHASE_TYPE_ERROR",
            ];
            let result = client.offline_list(50, phases).map(|r| r.tasks);
            let _ = tx.send(OpResult::OfflineTasks(result));
        });
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
                        let client = Arc::clone(&self.client);
                        let tx = self.result_tx.clone();
                        let task_id = task.id.clone();
                        let task_name = task.name.clone();
                        self.input = InputMode::InfoLoading;
                        self.loading = true;
                        self.loading_label = Some("Retrying task...".into());
                        std::thread::spawn(move || match client.offline_task_retry(&task_id) {
                            Ok(()) => {
                                let _ =
                                    tx.send(OpResult::Ok(format!("Retrying task: {}", task_name)));
                            }
                            Err(e) => {
                                let _ = tx.send(OpResult::Err(format!("Retry failed: {e:#}")));
                            }
                        });
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
                    let client = Arc::clone(&self.client);
                    let tx = self.result_tx.clone();
                    let task_id = task.id.clone();
                    let task_name = task.name.clone();
                    self.input = InputMode::InfoLoading;
                    self.loading = true;
                    self.loading_label = Some("Deleting task...".into());
                    std::thread::spawn(move || {
                        match client.delete_tasks(&[task_id.as_str()], false) {
                            Ok(()) => {
                                let _ =
                                    tx.send(OpResult::Ok(format!("Deleted task: {}", task_name)));
                            }
                            Err(e) => {
                                let _ =
                                    tx.send(OpResult::Err(format!("Delete task failed: {e:#}")));
                            }
                        }
                    });
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

    fn open_trash_view(&mut self) {
        self.trash_entries.clear();
        self.trash_selected = 0;
        self.trash_expanded = false;
        self.input = InputMode::TrashView {
            entries: vec![],
            selected: 0,
            expanded: false,
        };
        self.loading = true;
        self.loading_label = Some("Loading trash...".into());
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(OpResult::TrashList(client.ls_trash(200)));
        });
    }

    fn handle_trash_view_key(
        &mut self,
        code: KeyCode,
        entries: &mut Vec<Entry>,
        selected: &mut usize,
        expanded: bool,
    ) {
        if self.loading {
            if matches!(code, KeyCode::Esc) {
                self.finish_loading();
            }
            self.input = InputMode::TrashView {
                entries: std::mem::take(entries),
                selected: *selected,
                expanded,
            };
            return;
        }
        match code {
            KeyCode::Esc => {
                if expanded {
                    self.trash_expanded = false;
                    self.input = InputMode::TrashView {
                        entries: std::mem::take(entries),
                        selected: *selected,
                        expanded: false,
                    };
                } else {
                    self.trash_entries.clear();
                    self.trash_selected = 0;
                    self.trash_expanded = false;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !entries.is_empty() {
                    *selected = (*selected + 1).min(entries.len() - 1);
                }
                self.trash_selected = *selected;
                self.input = InputMode::TrashView {
                    entries: std::mem::take(entries),
                    selected: *selected,
                    expanded,
                };
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if *selected > 0 {
                    *selected -= 1;
                }
                self.trash_selected = *selected;
                self.input = InputMode::TrashView {
                    entries: std::mem::take(entries),
                    selected: *selected,
                    expanded,
                };
            }
            KeyCode::Enter => {
                let new_expanded = !expanded;
                self.trash_expanded = new_expanded;
                self.input = InputMode::TrashView {
                    entries: std::mem::take(entries),
                    selected: *selected,
                    expanded: new_expanded,
                };
            }
            KeyCode::Char('u') => {
                if let Some(entry) = entries.get(*selected) {
                    let client = Arc::clone(&self.client);
                    let tx = self.result_tx.clone();
                    let eid = entry.id.clone();
                    let name = entry.name.clone();
                    self.trash_entries = std::mem::take(entries);
                    self.trash_selected = *selected;
                    self.trash_expanded = expanded;
                    self.input = InputMode::TrashView {
                        entries: self.trash_entries.clone(),
                        selected: *selected,
                        expanded,
                    };
                    self.loading = true;
                    self.loading_label = Some("Restoring...".into());
                    std::thread::spawn(move || {
                        let _ = tx.send(match client.untrash(&[eid.as_str()]) {
                            Ok(()) => OpResult::TrashOp(format!("Restored '{}'", name)),
                            Err(e) => OpResult::TrashOp(format!("Untrash failed: {e:#}")),
                        });
                    });
                    return;
                }
                self.input = InputMode::TrashView {
                    entries: std::mem::take(entries),
                    selected: *selected,
                    expanded,
                };
            }
            KeyCode::Char('x') => {
                if let Some(entry) = entries.get(*selected) {
                    let client = Arc::clone(&self.client);
                    let tx = self.result_tx.clone();
                    let eid = entry.id.clone();
                    let name = entry.name.clone();
                    self.trash_entries = std::mem::take(entries);
                    self.trash_selected = *selected;
                    self.trash_expanded = expanded;
                    self.input = InputMode::TrashView {
                        entries: self.trash_entries.clone(),
                        selected: *selected,
                        expanded,
                    };
                    self.loading = true;
                    self.loading_label = Some("Deleting...".into());
                    std::thread::spawn(move || {
                        let _ = tx.send(match client.delete_permanent(&[eid.as_str()]) {
                            Ok(()) => {
                                OpResult::TrashOp(format!("Permanently deleted '{}'", name))
                            }
                            Err(e) => {
                                OpResult::TrashOp(format!("Permanent delete failed: {e:#}"))
                            }
                        });
                    });
                    return;
                }
                self.input = InputMode::TrashView {
                    entries: std::mem::take(entries),
                    selected: *selected,
                    expanded,
                };
            }
            KeyCode::Char(' ') => {
                if let Some(entry) = entries.get(*selected).cloned() {
                    self.trash_entries = std::mem::take(entries);
                    self.trash_selected = *selected;
                    self.trash_expanded = expanded;
                    let info = crate::pikpak::FileInfoResponse {
                        id: Some(entry.id),
                        name: entry.name,
                        kind: Some(match entry.kind {
                            crate::pikpak::EntryKind::Folder => "drive#folder".to_string(),
                            crate::pikpak::EntryKind::File => "drive#file".to_string(),
                        }),
                        size: if entry.size > 0 { Some(entry.size.to_string()) } else { None },
                        hash: None,
                        mime_type: None,
                        created_time: if entry.created_time.is_empty() { None } else { Some(entry.created_time) },
                        web_content_link: None,
                        thumbnail_link: entry.thumbnail_link,
                        links: None,
                        medias: None,
                    };
                    let thumb_url = info.thumbnail_link.clone();
                    self.input = InputMode::InfoView { info, image: None };
                    if let Some(url) = thumb_url {
                        if !url.is_empty() {
                            let client = Arc::clone(&self.client);
                            let tx = self.result_tx.clone();
                            std::thread::spawn(move || {
                                let _ = tx.send(super::OpResult::InfoThumbnail(
                                    super::fetch_and_render_thumbnail(&url, &client),
                                ));
                            });
                        }
                    }
                } else {
                    self.input = InputMode::TrashView {
                        entries: std::mem::take(entries),
                        selected: *selected,
                        expanded,
                    };
                }
            }
            KeyCode::Char('r') => {
                self.trash_expanded = expanded;
                self.open_trash_view_preserve_expanded();
            }
            _ => {
                self.input = InputMode::TrashView {
                    entries: std::mem::take(entries),
                    selected: *selected,
                    expanded,
                };
            }
        }
    }

    fn open_trash_view_preserve_expanded(&mut self) {
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

    fn open_info_popup(&mut self, entry: Entry) {
        self.input = InputMode::InfoLoading;
        self.loading = true;
        self.loading_label = Some("Loading file info...".into());
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        let eid = entry.id.clone();
        std::thread::spawn(move || {
            let _ = tx.send(OpResult::Info(client.file_info(&eid)));
        });
    }

    fn open_folder_info_popup(&mut self, entry: Entry) {
        self.input = InputMode::InfoLoading;
        self.loading = true;
        self.loading_label = Some("Loading folder...".into());
        self.preview_target_id = Some(entry.id.clone());
        self.preview_target_name = Some(entry.name.clone());
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        let eid = entry.id.clone();
        std::thread::spawn(move || {
            let _ = tx.send(OpResult::PreviewLs(eid.clone(), client.ls(&eid)));
        });
    }

    fn spawn_player(&mut self, cmd: &str, url: &str) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            self.push_log("Player command is empty".into());
            return;
        }
        let program = parts[0];
        let mut args: Vec<&str> = parts[1..].to_vec();
        args.push(url);
        match std::process::Command::new(program).args(&args).spawn() {
            Ok(_) => {
                self.push_log(format!("Launched {} with video URL", program));
            }
            Err(e) => {
                self.push_log(format!("Failed to launch {}: {}", program, e));
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

    pub(super) fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let up = matches!(mouse.kind, MouseEventKind::ScrollUp);
                self.handle_mouse_scroll(mouse.column, mouse.row, up);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let double = self.check_double_click(mouse.column, mouse.row);
                self.handle_mouse_click(mouse.column, mouse.row, double);
            }
            _ => {}
        }
    }

    fn check_double_click(&mut self, col: u16, row: u16) -> bool {
        let now = Instant::now();
        let is_double = now.duration_since(self.last_click_time) < Duration::from_millis(400)
            && self.last_click_pos == (col, row);
        self.last_click_time = now;
        self.last_click_pos = (col, row);
        is_double
    }

    fn handle_mouse_scroll(&mut self, col: u16, row: u16, up: bool) {
        // Normal mode: scroll in any of the three panes
        if matches!(self.input, InputMode::Normal) {
            // Log overlay scroll takes priority when visible
            if self.show_logs_overlay
                && self.is_in_rect(col, row, self.logs_overlay_area.get())
            {
                let area = self.logs_overlay_area.get();
                let visible = area.height.saturating_sub(2) as usize;
                let content_width = area.width.saturating_sub(2).max(1) as usize;
                let total_visual = super::wrap_logs(
                    self.logs.iter().map(|s| s.as_str()),
                    content_width,
                ).len();
                let max_scroll = total_visual.saturating_sub(visible);
                let current = self.logs_scroll.unwrap_or(max_scroll);
                if up {
                    let new_pos = current.saturating_sub(3);
                    self.logs_scroll = Some(new_pos);
                } else {
                    let new_pos = (current + 3).min(max_scroll);
                    if new_pos >= max_scroll {
                        self.logs_scroll = None;
                    } else {
                        self.logs_scroll = Some(new_pos);
                    }
                }
                return;
            }
            if self.is_in_rect(col, row, self.current_pane_area.get()) {
                if up {
                    if self.selected > 0 {
                        self.selected -= 1;
                        self.on_cursor_move();
                    }
                } else if !self.entries.is_empty() {
                    self.selected = (self.selected + 1).min(self.entries.len() - 1);
                    self.on_cursor_move();
                }
            } else if self.is_in_rect(col, row, self.parent_pane_area.get()) {
                if up {
                    if self.parent_selected > 0 {
                        self.parent_selected -= 1;
                    }
                } else if !self.parent_entries.is_empty() {
                    self.parent_selected =
                        (self.parent_selected + 1).min(self.parent_entries.len() - 1);
                }
            } else if self.is_in_rect(col, row, self.preview_pane_area.get()) {
                let area = self.preview_pane_area.get();
                let visible = area.height.saturating_sub(2) as usize;
                let max_scroll = match &self.preview_state {
                    PreviewState::FileTextPreview { lines, .. } => {
                        lines.len().saturating_sub(visible)
                    }
                    PreviewState::FolderListing(children) => {
                        children.len().saturating_sub(visible)
                    }
                    _ => 0,
                };
                if up {
                    self.preview_scroll = self.preview_scroll.saturating_sub(1);
                } else if self.preview_scroll < max_scroll {
                    self.preview_scroll += 1;
                }
            }
            return;
        }

        // Non-normal modes: scroll support for overlay views
        if let InputMode::OfflineTasksView { tasks, selected } = &mut self.input {
            if up {
                if *selected > 0 {
                    *selected -= 1;
                }
            } else if !tasks.is_empty() {
                *selected = (*selected + 1).min(tasks.len() - 1);
            }
        } else if matches!(self.input, InputMode::CartView) {
            if up {
                if self.cart_selected > 0 {
                    self.cart_selected -= 1;
                }
            } else if !self.cart.is_empty() {
                self.cart_selected = (self.cart_selected + 1).min(self.cart.len() - 1);
            }
        } else if matches!(self.input, InputMode::DownloadView) {
            let count = self.download_state.tasks.len();
            if up {
                if self.download_state.selected > 0 {
                    self.download_state.selected -= 1;
                }
            } else if count > 0 {
                self.download_state.selected =
                    (self.download_state.selected + 1).min(count - 1);
            }
        } else if let InputMode::TrashView {
            entries, selected, ..
        } = &mut self.input
        {
            if up {
                if *selected > 0 {
                    *selected -= 1;
                }
            } else if !entries.is_empty() {
                *selected = (*selected + 1).min(entries.len() - 1);
            }
            self.trash_selected = *selected;
        } else if let InputMode::Settings { selected, editing, draft, modified } = &mut self.input {
            // Settings overlay scroll support
            if up {
                if *selected > 0 {
                    *selected -= 1;
                }
            } else if *selected < 13 {
                *selected += 1;
            }
            self.input = InputMode::Settings {
                selected: *selected,
                editing: *editing,
                draft: draft.clone(),
                modified: *modified,
            };
        }
    }

    fn handle_mouse_click(&mut self, col: u16, row: u16, double: bool) {
        if matches!(self.input, InputMode::Settings { .. }) {
            let area = self.settings_area.get();
            if let InputMode::Settings { mut selected, mut editing, mut draft, mut modified } = std::mem::replace(&mut self.input, InputMode::Normal) {
                if self.is_in_rect(col, row, area) && !editing {
                    let content_y = row.saturating_sub(area.y + 1) as usize;
                    let content_x = col.saturating_sub(area.x + 1) as usize;

                    let categories = vec![
                        ("UI Settings", 5),
                        ("Preview Settings", 5),
                        ("Sort Settings", 2),
                        ("Interface Settings", 2),
                        ("Playback Settings", 1),
                    ];

                    let bool_items = vec![0, 3, 5, 6, 11, 13];
                    let mut current_line = 0;
                    let mut item_idx = 0;
                    let terminal_width = (area.width.saturating_sub(4)) as usize;

                    for (_cat_name, item_count) in categories {
                        current_line += 1;
                        for _ in 0..item_count {
                            if content_y >= current_line && content_y < current_line + 2 {
                                selected = item_idx;

                                if content_y == current_line && bool_items.contains(&item_idx) {
                                    if content_x + 10 >= terminal_width {
                                        match item_idx {
                                            0 => draft.nerd_font = !draft.nerd_font,
                                            3 => draft.show_help_bar = !draft.show_help_bar,
                                            5 => draft.show_preview = !draft.show_preview,
                                            6 => draft.lazy_preview = !draft.lazy_preview,
                                            11 => draft.sort_reverse = !draft.sort_reverse,
                                            13 => draft.cli_nerd_font = !draft.cli_nerd_font,
                                            _ => {}
                                        }
                                        modified = true;
                                    }
                                } else if double {
                                    editing = true;
                                }
                                break;
                            }
                            current_line += 2;
                            item_idx += 1;
                        }
                    }
                }
                self.input = InputMode::Settings {
                    selected,
                    editing,
                    draft,
                    modified,
                };
            }
            return;
        }

        if !matches!(self.input, InputMode::Normal) {
            return;
        }

        let current_area = self.current_pane_area.get();
        let parent_area = self.parent_pane_area.get();
        let preview_area = self.preview_pane_area.get();

        if self.is_in_rect(col, row, current_area) {
            // Click / double-click on current pane
            let content_y = row.saturating_sub(current_area.y + 1) as usize;
            let offset = self.scroll_offset.get();
            let clicked_idx = offset + content_y;
            if clicked_idx < self.entries.len() {
                self.selected = clicked_idx;
                self.on_cursor_move();
                if double {
                    let _ = self.handle_normal_key(KeyCode::Enter, KeyModifiers::NONE);
                }
            }
        } else if self.is_in_rect(col, row, parent_area) {
            // Click / double-click on parent pane
            let content_y = row.saturating_sub(parent_area.y + 1) as usize;
            let offset = self.parent_scroll_offset.get();
            let clicked_idx = offset + content_y;
            if clicked_idx < self.parent_entries.len() {
                self.parent_selected = clicked_idx;
                if double {
                    // Navigate back to parent, then enter the clicked folder
                    let _ = self.handle_normal_key(KeyCode::Backspace, KeyModifiers::NONE);
                    let is_folder = self
                        .entries
                        .get(self.selected)
                        .is_some_and(|e| e.kind == EntryKind::Folder);
                    if is_folder {
                        let _ = self.handle_normal_key(KeyCode::Enter, KeyModifiers::NONE);
                    }
                }
            }
        } else if self.is_in_rect(col, row, preview_area) && double {
            // Double-click on preview pane: Enter for folders, Space for files
            let is_folder = self
                .entries
                .get(self.selected)
                .is_some_and(|e| e.kind == EntryKind::Folder);
            let has_entry = self.selected < self.entries.len();
            if has_entry {
                if is_folder {
                    let _ = self.handle_normal_key(KeyCode::Enter, KeyModifiers::NONE);
                } else {
                    let _ = self.handle_normal_key(KeyCode::Char(' '), KeyModifiers::NONE);
                }
            }
        }
    }

    fn is_in_rect(&self, col: u16, row: u16, rect: ratatui::layout::Rect) -> bool {
        col >= rect.x && col < rect.x + rect.width && row >= rect.y && row < rect.y + rect.height
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

    fn handle_image_protocol_key(
        &mut self,
        code: KeyCode,
        selected: &mut usize,
        draft: &mut crate::config::TuiConfig,
        modified: &mut bool,
        current_terminal: &str,
        terminals: &[String],
    ) {
        match code {
            KeyCode::Down | KeyCode::Char('j') => {
                if !terminals.is_empty() {
                    *selected = (*selected + 1).min(terminals.len() - 1);
                }
                self.input = InputMode::ImageProtocolSettings {
                    selected: *selected,
                    draft: draft.clone(),
                    modified: *modified,
                    current_terminal: current_terminal.to_string(),
                    terminals: terminals.to_vec(),
                };
            }
            KeyCode::Up | KeyCode::Char('k') => {
                *selected = selected.saturating_sub(1);
                self.input = InputMode::ImageProtocolSettings {
                    selected: *selected,
                    draft: draft.clone(),
                    modified: *modified,
                    current_terminal: current_terminal.to_string(),
                    terminals: terminals.to_vec(),
                };
            }
            KeyCode::Left => {
                if let Some(term) = terminals.get(*selected) {
                    let proto = draft
                        .image_protocols
                        .get(term)
                        .copied()
                        .unwrap_or(crate::config::ImageProtocol::Auto);
                    draft.image_protocols.insert(term.clone(), proto.prev());
                    *modified = true;
                }
                self.input = InputMode::ImageProtocolSettings {
                    selected: *selected,
                    draft: draft.clone(),
                    modified: *modified,
                    current_terminal: current_terminal.to_string(),
                    terminals: terminals.to_vec(),
                };
            }
            KeyCode::Right => {
                if let Some(term) = terminals.get(*selected) {
                    let proto = draft
                        .image_protocols
                        .get(term)
                        .copied()
                        .unwrap_or(crate::config::ImageProtocol::Auto);
                    draft.image_protocols.insert(term.clone(), proto.next());
                    *modified = true;
                }
                self.input = InputMode::ImageProtocolSettings {
                    selected: *selected,
                    draft: draft.clone(),
                    modified: *modified,
                    current_terminal: current_terminal.to_string(),
                    terminals: terminals.to_vec(),
                };
            }
            KeyCode::Char('s') => {
                if *modified {
                    match draft.save() {
                        Ok(()) => {
                            self.config = draft.clone();
                            self.push_log("Image protocol settings saved to config.toml".into());
                            self.input = InputMode::Settings {
                                selected: 8,
                                editing: false,
                                draft: draft.clone(),
                                modified: false,
                            };
                        }
                        Err(e) => {
                            self.push_log(format!("Failed to save config: {:#}", e));
                            self.input = InputMode::ImageProtocolSettings {
                                selected: *selected,
                                draft: draft.clone(),
                                modified: *modified,
                                current_terminal: current_terminal.to_string(),
                                terminals: terminals.to_vec(),
                            };
                        }
                    }
                } else {
                    self.input = InputMode::ImageProtocolSettings {
                        selected: *selected,
                        draft: draft.clone(),
                        modified: *modified,
                        current_terminal: current_terminal.to_string(),
                        terminals: terminals.to_vec(),
                    };
                }
            }
            KeyCode::Esc | KeyCode::Backspace => {
                // Return to main settings at item #8
                self.input = InputMode::Settings {
                    selected: 8,
                    editing: false,
                    draft: draft.clone(),
                    modified: *modified,
                };
            }
            _ => {
                self.input = InputMode::ImageProtocolSettings {
                    selected: *selected,
                    draft: draft.clone(),
                    modified: *modified,
                    current_terminal: current_terminal.to_string(),
                    terminals: terminals.to_vec(),
                };
            }
        }
    }

    fn handle_custom_color_key(
        &mut self,
        code: KeyCode,
        selected: &mut usize,
        draft: &mut crate::config::TuiConfig,
        modified: &mut bool,
        editing_rgb: &mut bool,
        rgb_input: &mut String,
        rgb_component: &mut usize,
    ) {
        if *editing_rgb {
            match code {
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    if rgb_input.len() < 3 {
                        rgb_input.push(c);
                    }
                }
                KeyCode::Backspace => {
                    rgb_input.pop();
                }
                KeyCode::Enter => {
                    if let Ok(value) = rgb_input.parse::<u8>() {
                        let color_ref = match *selected {
                            0 => &mut draft.custom_colors.folder,
                            1 => &mut draft.custom_colors.archive,
                            2 => &mut draft.custom_colors.image,
                            3 => &mut draft.custom_colors.video,
                            4 => &mut draft.custom_colors.audio,
                            5 => &mut draft.custom_colors.document,
                            6 => &mut draft.custom_colors.code,
                            7 => &mut draft.custom_colors.default,
                            _ => return,
                        };
                        match *rgb_component {
                            0 => color_ref.0 = value,
                            1 => color_ref.1 = value,
                            2 => color_ref.2 = value,
                            _ => {}
                        }
                        *modified = true;
                    }
                    *editing_rgb = false;
                    rgb_input.clear();
                }
                KeyCode::Esc => {
                    *editing_rgb = false;
                    rgb_input.clear();
                }
                _ => {}
            }
            self.input = InputMode::CustomColorSettings {
                selected: *selected,
                draft: draft.clone(),
                modified: *modified,
                editing_rgb: *editing_rgb,
                rgb_input: rgb_input.clone(),
                rgb_component: *rgb_component,
            };
        } else {
            match code {
                KeyCode::Down | KeyCode::Char('j') => {
                    *selected = (*selected + 1).min(7);
                    self.input = InputMode::CustomColorSettings {
                        selected: *selected,
                        draft: draft.clone(),
                        modified: *modified,
                        editing_rgb: false,
                        rgb_input: String::new(),
                        rgb_component: 0,
                    };
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    *selected = selected.saturating_sub(1);
                    self.input = InputMode::CustomColorSettings {
                        selected: *selected,
                        draft: draft.clone(),
                        modified: *modified,
                        editing_rgb: false,
                        rgb_input: String::new(),
                        rgb_component: 0,
                    };
                }
                KeyCode::Char('r') => {
                    *editing_rgb = true;
                    *rgb_component = 0;
                    rgb_input.clear();
                    self.input = InputMode::CustomColorSettings {
                        selected: *selected,
                        draft: draft.clone(),
                        modified: *modified,
                        editing_rgb: true,
                        rgb_input: rgb_input.clone(),
                        rgb_component: 0,
                    };
                }
                KeyCode::Char('g') => {
                    *editing_rgb = true;
                    *rgb_component = 1;
                    rgb_input.clear();
                    self.input = InputMode::CustomColorSettings {
                        selected: *selected,
                        draft: draft.clone(),
                        modified: *modified,
                        editing_rgb: true,
                        rgb_input: rgb_input.clone(),
                        rgb_component: 1,
                    };
                }
                KeyCode::Char('b') => {
                    *editing_rgb = true;
                    *rgb_component = 2;
                    rgb_input.clear();
                    self.input = InputMode::CustomColorSettings {
                        selected: *selected,
                        draft: draft.clone(),
                        modified: *modified,
                        editing_rgb: true,
                        rgb_input: rgb_input.clone(),
                        rgb_component: 2,
                    };
                }
                KeyCode::Char('s') => {
                    if *modified {
                        match draft.save() {
                            Ok(()) => {
                                self.config = draft.clone();
                                self.push_log("Custom colors saved to config.toml".into());
                                self.input = InputMode::Settings {
                                    selected: 5, // Return to Color Scheme item
                                    editing: false,
                                    draft: draft.clone(),
                                    modified: false,
                                };
                            }
                            Err(e) => {
                                self.push_log(format!("Failed to save config: {:#}", e));
                                self.input = InputMode::CustomColorSettings {
                                    selected: *selected,
                                    draft: draft.clone(),
                                    modified: *modified,
                                    editing_rgb: false,
                                    rgb_input: String::new(),
                                    rgb_component: 0,
                                };
                            }
                        }
                    } else {
                        self.input = InputMode::CustomColorSettings {
                            selected: *selected,
                            draft: draft.clone(),
                            modified: *modified,
                            editing_rgb: false,
                            rgb_input: String::new(),
                            rgb_component: 0,
                        };
                    }
                }
                KeyCode::Esc | KeyCode::Backspace => {
                    // Return to main settings
                    self.input = InputMode::Settings {
                        selected: 5, // Return to Color Scheme item
                        editing: false,
                        draft: draft.clone(),
                        modified: *modified,
                    };
                }
                _ => {
                    self.input = InputMode::CustomColorSettings {
                        selected: *selected,
                        draft: draft.clone(),
                        modified: *modified,
                        editing_rgb: false,
                        rgb_input: String::new(),
                        rgb_component: 0,
                    };
                }
            }
        }
    }

    fn handle_settings_key(
        &mut self,
        code: KeyCode,
        selected: &mut usize,
        editing: &mut bool,
        draft: &mut crate::config::TuiConfig,
        modified: &mut bool,
    ) -> Option<bool> {

        if *editing {
            match *selected {
                0 => {
                    match code {
                        KeyCode::Char(' ')
                        | KeyCode::Enter
                        | KeyCode::Left
                        | KeyCode::Right => {
                            draft.nerd_font = !draft.nerd_font;
                            *modified = true;
                            *editing = false;
                        }
                        KeyCode::Esc => {
                            *editing = false;
                        }
                        _ => {}
                    }
                }
                1 => {
                    match code {
                        KeyCode::Left => {
                            draft.border_style = draft.border_style.prev();
                            *modified = true;
                        }
                        KeyCode::Right => {
                            draft.border_style = draft.border_style.next();
                            *modified = true;
                        }
                        KeyCode::Enter => {
                            *editing = false;
                        }
                        KeyCode::Esc => {
                            *editing = false;
                        }
                        _ => {}
                    }
                }
                2 => {
                    match code {
                        KeyCode::Left => {
                            draft.color_scheme = draft.color_scheme.prev();
                            *modified = true;
                        }
                        KeyCode::Right => {
                            draft.color_scheme = draft.color_scheme.next();
                            *modified = true;
                        }
                        KeyCode::Enter => {
                            use crate::config::ColorScheme;
                            if draft.color_scheme == ColorScheme::Custom {
                                self.input = InputMode::CustomColorSettings {
                                    selected: 0,
                                    draft: draft.clone(),
                                    modified: *modified,
                                    editing_rgb: false,
                                    rgb_input: String::new(),
                                    rgb_component: 0,
                                };
                                return None;
                            }
                            *editing = false;
                        }
                        KeyCode::Esc => {
                            *editing = false;
                        }
                        _ => {}
                    }
                }
                3 => {
                    match code {
                        KeyCode::Char(' ')
                        | KeyCode::Enter
                        | KeyCode::Left
                        | KeyCode::Right => {
                            draft.show_help_bar = !draft.show_help_bar;
                            *modified = true;
                            *editing = false;
                        }
                        KeyCode::Esc => {
                            *editing = false;
                        }
                        _ => {}
                    }
                }
                4 => {
                    // Quota Bar Style
                    match code {
                        KeyCode::Left => {
                            draft.quota_bar_style = draft.quota_bar_style.prev();
                            *modified = true;
                        }
                        KeyCode::Right => {
                            draft.quota_bar_style = draft.quota_bar_style.next();
                            *modified = true;
                        }
                        KeyCode::Enter => {
                            *editing = false;
                        }
                        KeyCode::Esc => {
                            *editing = false;
                        }
                        _ => {}
                    }
                }
                5 => {
                    match code {
                        KeyCode::Char(' ')
                        | KeyCode::Enter
                        | KeyCode::Left
                        | KeyCode::Right => {
                            draft.show_preview = !draft.show_preview;
                            *modified = true;
                            *editing = false;
                        }
                        KeyCode::Esc => {
                            *editing = false;
                        }
                        _ => {}
                    }
                }
                6 => {
                    match code {
                        KeyCode::Char(' ')
                        | KeyCode::Enter
                        | KeyCode::Left
                        | KeyCode::Right => {
                            draft.lazy_preview = !draft.lazy_preview;
                            *modified = true;
                            *editing = false;
                        }
                        KeyCode::Esc => {
                            *editing = false;
                        }
                        _ => {}
                    }
                }
                7 => {
                    match code {
                        KeyCode::Char('+') | KeyCode::Up => {
                            draft.preview_max_size = (draft.preview_max_size + 1024).min(10485760);
                            *modified = true;
                        }
                        KeyCode::Char('-') | KeyCode::Down => {
                            draft.preview_max_size = (draft.preview_max_size.saturating_sub(1024)).max(1024);
                            *modified = true;
                        }
                        KeyCode::Enter => {
                            *editing = false;
                        }
                        KeyCode::Esc => {
                            *editing = false;
                        }
                        _ => {}
                    }
                }
                8 => {
                    match code {
                        KeyCode::Left => {
                            draft.thumbnail_mode = draft.thumbnail_mode.prev();
                            *modified = true;
                        }
                        KeyCode::Right => {
                            draft.thumbnail_mode = draft.thumbnail_mode.next();
                            *modified = true;
                        }
                        KeyCode::Enter => {
                            *editing = false;
                        }
                        KeyCode::Esc => {
                            *editing = false;
                        }
                        _ => {}
                    }
                }
                9 => {
                    match code {
                        KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right => {
                            let current_terminal = draft.ensure_current_terminal();
                            let terminals: Vec<String> =
                                draft.image_protocols.keys().cloned().collect();
                            let sel = terminals
                                .iter()
                                .position(|t| t == &current_terminal)
                                .unwrap_or(0);
                            self.input = InputMode::ImageProtocolSettings {
                                selected: sel,
                                draft: draft.clone(),
                                modified: *modified,
                                current_terminal,
                                terminals,
                            };
                            return None;
                        }
                        KeyCode::Esc => {
                            *editing = false;
                        }
                        _ => {}
                    }
                }
                10 => {
                    // Sort Field
                    match code {
                        KeyCode::Left => {
                            draft.sort_field = draft.sort_field.prev();
                            *modified = true;
                        }
                        KeyCode::Right => {
                            draft.sort_field = draft.sort_field.next();
                            *modified = true;
                        }
                        KeyCode::Enter => {
                            *editing = false;
                        }
                        KeyCode::Esc => {
                            *editing = false;
                        }
                        _ => {}
                    }
                }
                11 => {
                    // Reverse Order
                    match code {
                        KeyCode::Char(' ')
                        | KeyCode::Enter
                        | KeyCode::Left
                        | KeyCode::Right => {
                            draft.sort_reverse = !draft.sort_reverse;
                            *modified = true;
                            *editing = false;
                        }
                        KeyCode::Esc => {
                            *editing = false;
                        }
                        _ => {}
                    }
                }
                12 => {
                    // Move Mode
                    match code {
                        KeyCode::Left => {
                            draft.move_mode = if draft.move_mode == "picker" {
                                "input".to_string()
                            } else {
                                "picker".to_string()
                            };
                            *modified = true;
                        }
                        KeyCode::Right => {
                            draft.move_mode = if draft.move_mode == "picker" {
                                "input".to_string()
                            } else {
                                "picker".to_string()
                            };
                            *modified = true;
                        }
                        KeyCode::Enter => {
                            *editing = false;
                        }
                        KeyCode::Esc => {
                            *editing = false;
                        }
                        _ => {}
                    }
                }
                13 => {
                    // CLI Nerd Font
                    match code {
                        KeyCode::Char(' ')
                        | KeyCode::Enter
                        | KeyCode::Left
                        | KeyCode::Right => {
                            draft.cli_nerd_font = !draft.cli_nerd_font;
                            *modified = true;
                            *editing = false;
                        }
                        KeyCode::Esc => {
                            *editing = false;
                        }
                        _ => {}
                    }
                }
                14 => {
                    // Player Command (text input)
                    match code {
                        KeyCode::Esc => {
                            *editing = false;
                        }
                        KeyCode::Enter => {
                            *editing = false;
                        }
                        KeyCode::Backspace => {
                            if let Some(ref mut p) = draft.player {
                                p.pop();
                                if p.is_empty() {
                                    draft.player = None;
                                }
                            }
                            *modified = true;
                        }
                        KeyCode::Char(c) => {
                            match draft.player {
                                Some(ref mut p) => p.push(c),
                                None => draft.player = Some(String::from(c)),
                            }
                            *modified = true;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            None
        } else {
            match code {
                KeyCode::Down | KeyCode::Char('j') => {
                    *selected = (*selected + 1).min(14);
                    None
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    *selected = selected.saturating_sub(1);
                    None
                }
                KeyCode::Char(' ') | KeyCode::Enter => {
                    if *selected == 9 {
                        // Directly enter image protocol sub-menu
                        let current_terminal = draft.ensure_current_terminal();
                        let terminals: Vec<String> =
                            draft.image_protocols.keys().cloned().collect();
                        let sel = terminals
                            .iter()
                            .position(|t| t == &current_terminal)
                            .unwrap_or(0);
                        self.input = InputMode::ImageProtocolSettings {
                            selected: sel,
                            draft: draft.clone(),
                            modified: *modified,
                            current_terminal,
                            terminals,
                        };
                        return None;
                    }
                    *editing = true;
                    None
                }
                KeyCode::Char('s') => {
                    if *modified {
                        Some(true) // Save and exit
                    } else {
                        None // Nothing to save, stay in settings
                    }
                }
                KeyCode::Esc => {
                    Some(false)
                }
                _ => None,
            }
        }
    }

}
