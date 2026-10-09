use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::pikpak::{Entry, EntryKind};
use crate::theme;

use super::completion::PathInput;
use super::download::TaskStatus;
use super::local_completion::LocalPathInput;
use super::operations::{Destination, FileAction};
use super::{
    App, AsyncRequestKind, InputMode, LoginField, NORMAL_ACTIONS, OpResult, PickerState,
    PreviewState, handle_text_input, widgets,
};

/// Index of the last selectable Settings row. MUST match the item layout in
/// `draw::draw_settings_overlay`, the index match in `handle_settings_key`, and
/// the click map / `bool_items` in `handle_mouse_click` — keep all four in sync.
const SETTINGS_LAST_INDEX: usize = 23;
const SETTINGS_COLOR_SCHEME_INDEX: usize = 2;
const SETTINGS_IMAGE_PROTOCOL_INDEX: usize = 9;

enum PickerKeyResult {
    Navigated,
    Confirmed(String), // dest_id
    Cancelled,
    ShowHelp,
    SwitchToTextInput,
}

enum PathInputKeyResult {
    Updated,
    Confirmed(String), // target path
    SwitchToPicker,
    Cancelled,
}

enum LocalPathInputResult {
    Updated,
    Confirmed(String), // final path value
    Cancelled,
}

fn mouse_list_index(
    col: u16,
    row: u16,
    area: ratatui::layout::Rect,
    first_row: u16,
    offset: usize,
    visible: usize,
) -> Option<usize> {
    let inside = col > area.x
        && col < area.x.saturating_add(area.width).saturating_sub(1)
        && row >= area.y
        && row < area.y.saturating_add(area.height);
    if inside && row >= first_row && (row - first_row) < visible as u16 {
        Some(offset + (row - first_row) as usize)
    } else {
        None
    }
}

enum PathInputContext {
    SingleItem { source: Entry },
    Cart,
}

impl App {
    pub(super) fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Result<bool> {
        if self.show_help_sheet {
            let can_scroll = self.help_scroll_max.get() > 0;
            match code {
                KeyCode::Down | KeyCode::Char('j') if can_scroll => {
                    self.help_scroll = (self.help_scroll + 1).min(self.help_scroll_max.get());
                }
                KeyCode::Up | KeyCode::Char('k') if can_scroll => {
                    self.help_scroll = self.help_scroll.saturating_sub(1);
                }
                KeyCode::PageDown if can_scroll => {
                    self.help_scroll = (self.help_scroll + 5).min(self.help_scroll_max.get());
                }
                KeyCode::PageUp if can_scroll => {
                    self.help_scroll = self.help_scroll.saturating_sub(5);
                }
                KeyCode::Home if can_scroll => self.help_scroll = 0,
                KeyCode::End if can_scroll => self.help_scroll = self.help_scroll_max.get(),
                _ => {
                    self.show_help_sheet = false;
                    self.help_scroll = 0;
                }
            }
            return Ok(false);
        }

        // A pending play/goto response is only allowed to open its modal while
        // the user has not performed another action in the meantime.
        if !self.modal_request_survives_input() {
            self.invalidate_modal_request();
        }

        if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
            if self.download_state.has_active() {
                self.input = InputMode::ConfirmQuit;
                return Ok(false);
            } else {
                return Ok(true);
            }
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
                        self.text_cursor = usize::MAX;
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
                    _ => {
                        let value = match field {
                            LoginField::Email => &mut email,
                            LoginField::Password => &mut password,
                        };
                        let _ = handle_text_input(value, &mut self.text_cursor, code, modifiers);
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
            InputMode::Normal => {
                self.text_cursor = usize::MAX;
                self.handle_normal_key(code, modifiers)
            }
            InputMode::ActionMenu { mut selected } => {
                match code {
                    KeyCode::Esc | KeyCode::Char('?') => {}
                    KeyCode::Down | KeyCode::Char('j') => {
                        selected = (selected + 1).min(NORMAL_ACTIONS.len().saturating_sub(1));
                        self.input = InputMode::ActionMenu { selected };
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        selected = selected.saturating_sub(1);
                        self.input = InputMode::ActionMenu { selected };
                    }
                    KeyCode::Home => {
                        self.input = InputMode::ActionMenu { selected: 0 };
                    }
                    KeyCode::End => {
                        self.input = InputMode::ActionMenu {
                            selected: NORMAL_ACTIONS.len().saturating_sub(1),
                        };
                    }
                    KeyCode::Enter => {
                        if let Some(action) = NORMAL_ACTIONS.get(selected) {
                            return self.handle_normal_key(action.key, KeyModifiers::NONE);
                        }
                    }
                    _ => {
                        self.input = InputMode::ActionMenu { selected };
                    }
                }
                Ok(false)
            }
            InputMode::Rename { mut value } => {
                if let Some(done) =
                    handle_text_input(&mut value, &mut self.text_cursor, code, modifiers)
                {
                    if done && let Some(entry) = self.current_entry().cloned() {
                        let new_name = value.trim().to_string();
                        if !new_name.is_empty() {
                            self.spawn_rename(entry, new_name);
                        }
                    }
                } else {
                    self.input = InputMode::Rename { value };
                }
                Ok(false)
            }
            InputMode::Mkdir { mut value } => {
                if let Some(done) =
                    handle_text_input(&mut value, &mut self.text_cursor, code, modifiers)
                {
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
                    KeyCode::Char('n') | KeyCode::Esc => {}
                    _ => {
                        self.input = InputMode::ConfirmQuit;
                    }
                }
                Ok(false)
            }
            InputMode::GotoPath { mut query } => {
                match handle_text_input(&mut query, &mut self.text_cursor, code, modifiers) {
                    Some(true) => {
                        let q = query.trim().to_string();
                        if !q.is_empty() {
                            self.loading = true;
                            let request =
                                self.begin_modal_request(AsyncRequestKind::GotoPath, q.clone());
                            let client = Arc::clone(&self.client);
                            let tx = self.result_tx.clone();
                            std::thread::spawn(move || {
                                let _ = tx
                                    .send(OpResult::GotoPath(request, client.resolve_path_nav(&q)));
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
                        self.text_cursor = 0;
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
                match handle_text_input(&mut value, &mut self.text_cursor, code, modifiers) {
                    Some(false) => {
                        self.push_log("Permanent delete cancelled".into());
                    }
                    Some(true) => {
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
                    None => {
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
            InputMode::CartMoveInput { mut input } => {
                self.handle_cart_path_input_key(code, modifiers, &mut input, true);
                Ok(false)
            }
            InputMode::CartCopyInput { mut input } => {
                self.handle_cart_path_input_key(code, modifiers, &mut input, false);
                Ok(false)
            }
            InputMode::CartMovePicker { mut picker } => {
                self.handle_cart_picker_key(code, &mut picker, true);
                Ok(false)
            }
            InputMode::CartCopyPicker { mut picker } => {
                self.handle_cart_picker_key(code, &mut picker, false);
                Ok(false)
            }
            InputMode::ConfirmCartDelete => {
                self.handle_confirm_cart_delete_key(code);
                Ok(false)
            }
            InputMode::DownloadInput { mut input } => {
                self.handle_download_input_key(code, modifiers, &mut input);
                Ok(false)
            }
            InputMode::UploadInput { mut input } => {
                self.handle_upload_input_key(code, modifiers, &mut input);
                Ok(false)
            }
            InputMode::DownloadView => {
                self.handle_download_view_key(code);
                Ok(false)
            }
            InputMode::OfflineInput { mut value } => {
                self.handle_offline_input_key(code, modifiers, &mut value);
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
            InputMode::SharePrompt => {
                self.handle_share_prompt_key(code);
                Ok(false)
            }
            InputMode::ShareCreatedView { mut shares } => {
                self.handle_share_created_view_key(code, modifiers, &mut shares);
                Ok(false)
            }
            InputMode::MySharesView {
                mut shares,
                mut selected,
                mut confirm_delete,
            } => {
                self.handle_my_shares_key(code, &mut shares, &mut selected, &mut confirm_delete);
                Ok(false)
            }
            InputMode::ConfirmPlay { dialog } => {
                self.handle_play_dialog_key(dialog, code);
                Ok(false)
            }
            InputMode::PlayPicker {
                name,
                medias,
                mut selected,
            } => {
                match code {
                    KeyCode::Down | KeyCode::Char('j') => {
                        selected = super::playback::move_selection(&medias, selected, false);
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        selected = super::playback::move_selection(&medias, selected, true);
                    }
                    KeyCode::Enter => {
                        if let Some(option) = medias.get(selected)
                            && self.launch_play_option(option)
                        {
                            return Ok(false);
                        }
                    }
                    KeyCode::Esc => return Ok(false),
                    _ => {}
                }
                self.input = InputMode::PlayPicker {
                    name,
                    medias,
                    selected,
                };
                Ok(false)
            }
            InputMode::PlayerInput {
                mut value,
                pending_url,
            } => {
                match handle_text_input(&mut value, &mut self.text_cursor, code, modifiers) {
                    Some(false) => {}
                    Some(true) => {
                        let cmd = value.trim().to_string();
                        if !cmd.is_empty() {
                            self.push_log(format!("Player set to: {}", cmd));
                            self.spawn_player(&cmd, &pending_url);
                            self.config.player = Some(cmd);
                            let _ = self.config.save();
                        } else {
                            self.input = InputMode::PlayerInput { value, pending_url };
                        }
                    }
                    None => {
                        self.input = InputMode::PlayerInput { value, pending_url };
                    }
                }
                Ok(false)
            }
            InputMode::InfoLoading => {
                self.input = InputMode::InfoLoading;
                if code == KeyCode::Esc {
                    self.invalidate_modal_request();
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
                self.preview_state = PreviewState::FolderListing(entries);
                Ok(false)
            }
            InputMode::TextPreviewView { .. } => Ok(false),
            InputMode::Settings {
                mut selected,
                mut editing,
                mut draft,
                mut modified,
            } => {
                if code == KeyCode::Esc && !editing && modified {
                    self.input = InputMode::ConfirmDiscardSettings { selected, draft };
                    return Ok(false);
                }
                let result = self.handle_settings_key(
                    code,
                    modifiers,
                    &mut selected,
                    &mut editing,
                    &mut draft,
                    &mut modified,
                );

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
                                    if self.apply_settings(draft) {
                                        self.refresh();
                                    }
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
            InputMode::ConfirmDiscardSettings { selected, draft } => {
                match code {
                    KeyCode::Char('y') | KeyCode::Enter => {
                        self.input = InputMode::Normal;
                    }
                    KeyCode::Char('n') | KeyCode::Esc => {
                        self.input = InputMode::Settings {
                            selected,
                            editing: false,
                            draft,
                            modified: true,
                        };
                    }
                    _ => {
                        self.input = InputMode::ConfirmDiscardSettings { selected, draft };
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

    #[allow(clippy::collapsible_match)]
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
            KeyCode::PageDown => {
                if !self.entries.is_empty() {
                    let page = self.browser_page_size();
                    self.selected = (self.selected + page).min(self.entries.len() - 1);
                    self.on_cursor_move();
                }
            }
            KeyCode::PageUp => {
                if !self.entries.is_empty() {
                    let page = self.browser_page_size();
                    self.selected = self.selected.saturating_sub(page);
                    self.on_cursor_move();
                }
            }
            KeyCode::Home => {
                if !self.entries.is_empty() {
                    self.selected = 0;
                    self.on_cursor_move();
                }
            }
            KeyCode::End => {
                if !self.entries.is_empty() {
                    self.selected = self.entries.len() - 1;
                    self.on_cursor_move();
                }
            }
            KeyCode::Char('g') if !modifiers.contains(KeyModifiers::CONTROL) => {
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
            KeyCode::Enter | KeyCode::Right => {
                if let Some(entry) = self.current_entry().cloned() {
                    if entry.kind == EntryKind::Folder {
                        self.enter_directory(entry);
                    } else if entry.kind == EntryKind::File
                        && theme::categorize(&entry) == theme::FileCategory::Video
                    {
                        self.loading = true;
                        let client = Arc::clone(&self.client);
                        let tx = self.result_tx.clone();
                        let eid = entry.id.clone();
                        let request = self.begin_modal_request(AsyncRequestKind::Play, eid.clone());
                        std::thread::spawn(move || {
                            let _ = tx.send(OpResult::PlayInfo(request, client.file_info(&eid)));
                        });
                    }
                }
            }
            KeyCode::Backspace | KeyCode::Left => self.leave_directory(),
            KeyCode::Char('v') => {
                self.config.inline_thumbnails = !self.config.inline_thumbnails;
                self.save_browser_shortcut();
            }
            KeyCode::Char('[') => {
                self.config.inline_thumbnail_size = self.config.inline_thumbnail_size.smaller();
                self.save_browser_shortcut();
            }
            KeyCode::Char(']') => {
                self.config.inline_thumbnail_size = self.config.inline_thumbnail_size.larger();
                self.save_browser_shortcut();
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
                        let half = (self.browser_page_size() / 2).max(1);
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
                self.help_scroll = 0;
            }
            KeyCode::Char('a') => {
                if let Some(entry) = self.current_entry().cloned() {
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
            KeyCode::Char('A') => {
                self.input = InputMode::CartView;
            }
            KeyCode::Char('D') => {
                self.input = InputMode::DownloadView;
            }
            KeyCode::Char('M') => {
                self.open_my_shares_view();
            }
            KeyCode::Char('s') => {
                if let Some(entry) = self.current_entry().cloned() {
                    self.spawn_star_toggle(entry);
                }
            }
            KeyCode::Char('y') => {
                if let Some(entry) = self.current_entry().cloned()
                    && entry.kind == EntryKind::File
                {
                    let client = Arc::clone(&self.client);
                    let tx = self.result_tx.clone();
                    let eid = entry.id;
                    let ename = entry.name;
                    std::thread::spawn(move || {
                        let _ = tx.send(match client.download_url(&eid) {
                            Ok((url, _)) => match write_clipboard(&url) {
                                Ok(()) => OpResult::Ok(format!("Copied link: '{}'", ename)),
                                Err(e) => OpResult::Err(format!("Clipboard failed: {e:#}")),
                            },
                            Err(e) => OpResult::Err(format!("Link failed: {e:#}")),
                        });
                    });
                }
            }
            KeyCode::Char('u') => {
                if modifiers.contains(KeyModifiers::CONTROL) {
                    if !self.entries.is_empty() {
                        let half = (self.browser_page_size() / 2).max(1);
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
                self.input = InputMode::OfflineInput {
                    value: String::new(),
                };
            }
            KeyCode::Char('O') => {
                self.open_offline_tasks_view();
            }
            KeyCode::Char('t') => {
                self.open_trash_view();
            }
            KeyCode::Char('S') => {
                self.config.sort_field = self.config.sort_field.next();
                self.resort_entries();
                let _ = self.config.save();
            }
            KeyCode::Char('R') => {
                self.config.sort_reverse = !self.config.sort_reverse;
                self.resort_entries();
                let _ = self.config.save();
            }
            KeyCode::Char('w') => {
                if let Some(entry) = self.current_entry().cloned()
                    && entry.kind == EntryKind::File
                    && theme::categorize(&entry) == theme::FileCategory::Video
                {
                    self.loading = true;
                    let client = Arc::clone(&self.client);
                    let tx = self.result_tx.clone();
                    let eid = entry.id.clone();
                    let request =
                        self.begin_modal_request(AsyncRequestKind::PlayPicker, eid.clone());
                    std::thread::spawn(move || {
                        let result = client.file_info(&eid);
                        let _ = tx.send(match result {
                            Ok(info) => {
                                let options = info.play_options();
                                OpResult::PlayPickerInfo(request, Ok((info, options)))
                            }
                            Err(e) => OpResult::PlayPickerInfo(request, Err(e)),
                        });
                    });
                }
            }
            KeyCode::Char('p') => {
                if let Some(entry) = self.current_entry().cloned() {
                    if self.config.show_preview {
                        self.fetch_preview_for_selected();
                    } else if entry.kind == EntryKind::File && theme::is_text_previewable(&entry) {
                        self.input = InputMode::InfoLoading;
                        self.loading = true;
                        self.loading_label = Some("Loading preview...".into());
                        let client = Arc::clone(&self.client);
                        let tx = self.result_tx.clone();
                        let eid = entry.id.clone();
                        let request =
                            self.begin_modal_request(AsyncRequestKind::FilePreview, eid.clone());
                        let max_bytes = self.config.preview_max_size;
                        std::thread::spawn(move || {
                            let _ = tx.send(OpResult::PreviewText(
                                request,
                                client.fetch_text_preview(&eid, max_bytes),
                            ));
                        });
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
            KeyCode::Char('?') => {
                self.input = InputMode::ActionMenu { selected: 0 };
            }
            KeyCode::Char(' ') => {
                if let Some(entry) = self.current_entry().cloned() {
                    match entry.kind {
                        EntryKind::File => self.open_info_popup(entry),
                        EntryKind::Folder => self.open_folder_info_popup(entry),
                    }
                }
            }
            KeyCode::Char(':') => {
                self.input = InputMode::GotoPath {
                    query: String::new(),
                };
            }
            KeyCode::Esc => {}
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

    fn init_path_input(&mut self, source: Entry, is_move: bool) {
        let input = PathInput::new();
        if is_move {
            self.input = InputMode::MoveInput { source, input };
        } else {
            self.input = InputMode::CopyInput { source, input };
        }
    }

    /// Shared text-input editing logic for all move/copy path inputs.
    fn apply_path_input_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        input: &mut PathInput,
    ) -> PathInputKeyResult {
        if code == KeyCode::Char('b') && modifiers.contains(KeyModifiers::CONTROL) {
            return PathInputKeyResult::SwitchToPicker;
        }
        match code {
            KeyCode::Esc => {
                if !input.candidates.is_empty() {
                    input.clear_completion();
                    PathInputKeyResult::Updated
                } else {
                    // Closing the dialog also invalidates any in-flight
                    // completion result without requiring a second Esc.
                    input.pending_request_id = None;
                    PathInputKeyResult::Cancelled
                }
            }
            KeyCode::Enter => {
                if !input.candidates.is_empty() {
                    input.candidates.clear();
                    input.candidate_idx = None;
                    PathInputKeyResult::Updated
                } else {
                    let target = input.value.trim().to_string();
                    if !target.is_empty() {
                        PathInputKeyResult::Confirmed(target)
                    } else {
                        PathInputKeyResult::Updated
                    }
                }
            }
            KeyCode::Tab => {
                self.tab_complete(input);
                self.text_cursor = usize::MAX;
                PathInputKeyResult::Updated
            }
            _ => {
                let old = input.value.clone();
                let _ = handle_text_input(&mut input.value, &mut self.text_cursor, code, modifiers);
                if input.value != old {
                    input.clear_completion();
                }
                PathInputKeyResult::Updated
            }
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
        self.handle_generic_path_input_key(
            code,
            modifiers,
            input,
            is_move,
            PathInputContext::SingleItem { source },
        );
    }

    fn handle_generic_path_input_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        input: &mut PathInput,
        is_move: bool,
        context: PathInputContext,
    ) {
        match self.apply_path_input_key(code, modifiers, input) {
            PathInputKeyResult::Updated => match &context {
                PathInputContext::SingleItem { source } => {
                    self.restore_path_input(source.clone(), input, is_move)
                }
                PathInputContext::Cart => self.restore_cart_path_input(input, is_move),
            },
            PathInputKeyResult::Confirmed(target) => match context {
                PathInputContext::SingleItem { source } => {
                    self.execute_move_copy(source, &target, is_move);
                }
                PathInputContext::Cart => {
                    self.execute_cart_move_copy(&target, is_move);
                }
            },
            PathInputKeyResult::SwitchToPicker => match context {
                PathInputContext::SingleItem { source } => self.init_picker(source, is_move),
                PathInputContext::Cart => self.init_cart_picker(is_move),
            },
            PathInputKeyResult::Cancelled => {
                let op = if is_move { "Move" } else { "Copy" };
                self.push_log(format!("{} cancelled", op));
                if matches!(context, PathInputContext::Cart) {
                    self.input = InputMode::CartView;
                }
            }
        }
    }

    fn restore_path_input(&mut self, source: Entry, input: &mut PathInput, is_move: bool) {
        let owned = std::mem::take(input);
        self.input = if is_move {
            InputMode::MoveInput {
                source,
                input: owned,
            }
        } else {
            InputMode::CopyInput {
                source,
                input: owned,
            }
        };
    }

    /// Start a picker on the current folder; the listing arrives via
    /// OpResult::PickerLs so a slow link never freezes keystrokes.
    fn build_picker_state(&mut self) -> Option<PickerState> {
        let folder_id = self.current_folder_id.clone();
        let breadcrumb = self.breadcrumb.clone();
        let listing_request_id = self.spawn_picker_ls(folder_id.clone());
        Some(PickerState {
            folder_id,
            listing_request_id,
            breadcrumb,
            entries: Vec::new(),
            selected: 0,
            loading: true,
        })
    }

    fn spawn_picker_ls(&mut self, folder_id: String) -> u64 {
        let request_id = self.next_async_request_id();
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(OpResult::PickerLs(
                request_id,
                folder_id.clone(),
                client.ls(&folder_id),
            ));
        });
        request_id
    }

    fn init_picker(&mut self, source: Entry, is_move: bool) {
        if let Some(picker) = self.build_picker_state() {
            self.input = if is_move {
                InputMode::MovePicker { source, picker }
            } else {
                InputMode::CopyPicker { source, picker }
            };
        }
    }

    /// Shared navigation logic for all picker modes. Mutates `picker` in place
    /// and returns what action should be taken by the caller.
    fn apply_picker_key(&mut self, code: KeyCode, picker: &mut PickerState) -> PickerKeyResult {
        let folder_count = picker
            .entries
            .iter()
            .filter(|e| e.kind == EntryKind::Folder)
            .count();

        match code {
            KeyCode::Down | KeyCode::Char('j') => {
                if folder_count > 0 {
                    picker.selected = (picker.selected + 1).min(folder_count - 1);
                }
                PickerKeyResult::Navigated
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if picker.selected > 0 {
                    picker.selected -= 1;
                }
                PickerKeyResult::Navigated
            }
            KeyCode::Enter => {
                if let Some(entry) = picker
                    .entries
                    .iter()
                    .filter(|e| e.kind == EntryKind::Folder)
                    .nth(picker.selected)
                {
                    let old_id = std::mem::replace(&mut picker.folder_id, entry.id.clone());
                    picker.breadcrumb.push((old_id, entry.name.clone()));
                    picker.selected = 0;
                    picker.loading = true;
                    picker.entries.clear();
                    picker.listing_request_id = self.spawn_picker_ls(picker.folder_id.clone());
                }
                PickerKeyResult::Navigated
            }
            KeyCode::Backspace => {
                if let Some((parent_id, _)) = picker.breadcrumb.pop() {
                    picker.folder_id = parent_id;
                    picker.selected = 0;
                    picker.loading = true;
                    picker.entries.clear();
                    picker.listing_request_id = self.spawn_picker_ls(picker.folder_id.clone());
                }
                PickerKeyResult::Navigated
            }
            KeyCode::Char(' ') => PickerKeyResult::Confirmed(picker.folder_id.clone()),
            KeyCode::Char('/') => PickerKeyResult::SwitchToTextInput,
            KeyCode::Char('h') => PickerKeyResult::ShowHelp,
            KeyCode::Esc => PickerKeyResult::Cancelled,
            _ => PickerKeyResult::Navigated,
        }
    }

    fn handle_picker_key(
        &mut self,
        code: KeyCode,
        source: Entry,
        picker: &mut PickerState,
        is_move: bool,
    ) {
        self.handle_generic_picker_key(
            code,
            picker,
            is_move,
            PathInputContext::SingleItem { source },
        );
    }

    fn handle_generic_picker_key(
        &mut self,
        code: KeyCode,
        picker: &mut PickerState,
        is_move: bool,
        context: PathInputContext,
    ) {
        match self.apply_picker_key(code, picker) {
            PickerKeyResult::Navigated => match &context {
                PathInputContext::SingleItem { source } => {
                    self.restore_picker(source.clone(), picker, is_move)
                }
                PathInputContext::Cart => self.restore_cart_picker(picker, is_move),
            },
            PickerKeyResult::Confirmed(dest_id) => {
                let dest_path = Self::picker_path_display(picker);
                match context {
                    PathInputContext::SingleItem { source } => {
                        self.spawn_move_copy(source, dest_id, dest_path, is_move);
                    }
                    PathInputContext::Cart => {
                        self.spawn_cart_move_copy(dest_id, dest_path, is_move);
                    }
                }
            }
            PickerKeyResult::Cancelled => {
                let op = if is_move { "Move" } else { "Copy" };
                self.push_log(format!("{} cancelled", op));
                if matches!(context, PathInputContext::Cart) {
                    self.input = InputMode::CartView;
                }
            }
            PickerKeyResult::ShowHelp => {
                self.show_help_sheet = true;
                self.help_scroll = 0;
                match &context {
                    PathInputContext::SingleItem { source } => {
                        self.restore_picker(source.clone(), picker, is_move)
                    }
                    PathInputContext::Cart => self.restore_cart_picker(picker, is_move),
                }
            }
            PickerKeyResult::SwitchToTextInput => match context {
                PathInputContext::SingleItem { source } => self.init_path_input(source, is_move),
                PathInputContext::Cart => self.init_cart_path_input(is_move),
            },
        }
    }

    fn restore_picker(&mut self, source: Entry, picker: &mut PickerState, is_move: bool) {
        let owned = std::mem::take(picker);
        self.input = if is_move {
            InputMode::MovePicker {
                source,
                picker: owned,
            }
        } else {
            InputMode::CopyPicker {
                source,
                picker: owned,
            }
        };
    }

    fn execute_move_copy(&mut self, source: Entry, target: &str, is_move: bool) {
        let destination = Destination::Path(target.to_owned());
        self.queue_move_copy(source, destination, is_move);
    }

    fn spawn_move_copy(
        &mut self,
        source: Entry,
        dest_id: String,
        dest_path: String,
        is_move: bool,
    ) {
        self.queue_move_copy(
            source,
            Destination::Folder {
                id: dest_id,
                path: dest_path,
            },
            is_move,
        );
    }

    fn queue_move_copy(&mut self, source: Entry, destination: Destination, is_move: bool) {
        let path = destination.display().to_owned();
        let action = if is_move {
            FileAction::Move(destination)
        } else {
            FileAction::Copy(destination)
        };
        let success = format!(
            "{} '{}' -> '{}'",
            action.completed_label(),
            source.name,
            path
        );
        self.spawn_file_mutation(action, vec![source.id], success, false);
    }

    pub(super) fn spawn_rename(&mut self, entry: Entry, new_name: String) {
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        let eid = entry.id;
        let old = entry.name;
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

    fn handle_cart_view_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => {}
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
            // Clear-all is 'C', not 'a': 'a' is the global add-to-cart key, so
            // binding clear here would wipe the cart on a stray keystroke.
            KeyCode::Char('C') => {
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
            KeyCode::Char('m') => {
                if self.cart.is_empty() {
                    self.push_log("Cart is empty".into());
                    self.input = InputMode::CartView;
                } else {
                    self.init_cart_picker(true);
                }
            }
            KeyCode::Char('c') => {
                if self.cart.is_empty() {
                    self.push_log("Cart is empty".into());
                    self.input = InputMode::CartView;
                } else {
                    self.init_cart_picker(false);
                }
            }
            KeyCode::Char('t') => {
                if self.cart.is_empty() {
                    self.push_log("Cart is empty".into());
                    self.input = InputMode::CartView;
                } else {
                    self.input = InputMode::ConfirmCartDelete;
                }
            }
            KeyCode::Char('s') => {
                if self.cart.is_empty() {
                    self.push_log("Cart is empty".into());
                    self.input = InputMode::CartView;
                } else {
                    self.input = InputMode::SharePrompt;
                }
            }
            KeyCode::Char('S') => {
                if self.cart.is_empty() {
                    self.push_log("Cart is empty".into());
                    self.input = InputMode::CartView;
                } else {
                    self.spawn_create_shares(false);
                }
            }
            _ => {
                self.input = InputMode::CartView;
            }
        }
    }

    fn init_cart_path_input(&mut self, is_move: bool) {
        let input = PathInput::new();
        self.input = if is_move {
            InputMode::CartMoveInput { input }
        } else {
            InputMode::CartCopyInput { input }
        };
    }

    fn handle_cart_path_input_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        input: &mut PathInput,
        is_move: bool,
    ) {
        self.handle_generic_path_input_key(code, modifiers, input, is_move, PathInputContext::Cart);
    }

    fn restore_cart_path_input(&mut self, input: &mut PathInput, is_move: bool) {
        let owned = std::mem::take(input);
        self.input = if is_move {
            InputMode::CartMoveInput { input: owned }
        } else {
            InputMode::CartCopyInput { input: owned }
        };
    }

    fn execute_cart_move_copy(&mut self, target: &str, is_move: bool) {
        self.queue_cart_move_copy(Destination::Path(target.to_owned()), is_move);
    }

    fn init_cart_picker(&mut self, is_move: bool) {
        match self.build_picker_state() {
            Some(picker) => {
                self.input = if is_move {
                    InputMode::CartMovePicker { picker }
                } else {
                    InputMode::CartCopyPicker { picker }
                };
            }
            None => {
                self.input = InputMode::CartView;
            }
        }
    }

    fn handle_cart_picker_key(&mut self, code: KeyCode, picker: &mut PickerState, is_move: bool) {
        self.handle_generic_picker_key(code, picker, is_move, PathInputContext::Cart);
    }

    fn restore_cart_picker(&mut self, picker: &mut PickerState, is_move: bool) {
        let owned = std::mem::take(picker);
        self.input = if is_move {
            InputMode::CartMovePicker { picker: owned }
        } else {
            InputMode::CartCopyPicker { picker: owned }
        };
    }

    fn spawn_cart_move_copy(&mut self, dest_id: String, dest_path: String, is_move: bool) {
        self.queue_cart_move_copy(
            Destination::Folder {
                id: dest_id,
                path: dest_path,
            },
            is_move,
        );
    }

    fn queue_cart_move_copy(&mut self, destination: Destination, is_move: bool) {
        let path = destination.display().to_owned();
        let ids = self.cart.iter().map(|e| e.id.clone()).collect();
        let action = if is_move {
            FileAction::Move(destination)
        } else {
            FileAction::Copy(destination)
        };
        let success = format!(
            "{} {} item(s) -> '{}'",
            action.completed_label(),
            self.cart.len(),
            path
        );
        self.spawn_file_mutation(action, ids, success, true);
    }

    fn handle_confirm_cart_delete_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('y') | KeyCode::Enter => {
                self.spawn_cart_delete();
            }
            _ => {
                self.input = InputMode::CartView;
            }
        }
    }

    fn spawn_cart_delete(&mut self) {
        let ids = self.cart.iter().map(|e| e.id.clone()).collect();
        let success = format!("Trashed {} item(s)", self.cart.len());
        self.spawn_file_mutation(FileAction::Trash, ids, success, true);
    }

    fn apply_local_path_input_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        input: &mut LocalPathInput,
    ) -> LocalPathInputResult {
        match code {
            KeyCode::Esc => {
                if !input.candidates.is_empty() {
                    input.clear_candidates();
                    LocalPathInputResult::Updated
                } else {
                    LocalPathInputResult::Cancelled
                }
            }
            KeyCode::Tab => {
                if input.candidates.is_empty() {
                    input.open_candidates();
                } else {
                    input.navigate_next();
                }
                LocalPathInputResult::Updated
            }
            KeyCode::BackTab => {
                if input.candidates.is_empty() {
                    input.open_candidates();
                }
                input.navigate_prev();
                LocalPathInputResult::Updated
            }
            KeyCode::Up => {
                input.navigate_prev();
                LocalPathInputResult::Updated
            }
            KeyCode::Down => {
                input.navigate_next();
                LocalPathInputResult::Updated
            }
            KeyCode::Enter => {
                let applied = input.confirm_selected();
                if applied {
                    self.text_cursor = usize::MAX;
                    LocalPathInputResult::Updated
                } else {
                    LocalPathInputResult::Confirmed(input.value.trim().to_string())
                }
            }
            _ => {
                let old = input.value.clone();
                let _ = handle_text_input(&mut input.value, &mut self.text_cursor, code, modifiers);
                if input.value != old && !input.candidates.is_empty() {
                    input.open_candidates();
                }
                LocalPathInputResult::Updated
            }
        }
    }

    fn handle_download_input_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        input: &mut LocalPathInput,
    ) {
        match self.apply_local_path_input_key(code, modifiers, input) {
            LocalPathInputResult::Updated => self.restore_download_input(input),
            LocalPathInputResult::Confirmed(dest) => {
                if dest.is_empty() {
                    self.push_log("No destination path specified".into());
                    self.restore_download_input(input);
                } else {
                    self.start_cart_download(&dest);
                    self.input = InputMode::DownloadView;
                }
            }
            LocalPathInputResult::Cancelled => {
                self.input = InputMode::CartView;
            }
        }
    }

    fn restore_download_input(&mut self, input: &mut LocalPathInput) {
        let owned = std::mem::take(input);
        self.input = InputMode::DownloadInput { input: owned };
    }

    fn restore_upload_input(&mut self, input: &mut LocalPathInput) {
        let owned = std::mem::take(input);
        self.input = InputMode::UploadInput { input: owned };
    }

    fn handle_upload_input_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        input: &mut LocalPathInput,
    ) {
        match self.apply_local_path_input_key(code, modifiers, input) {
            LocalPathInputResult::Updated => self.restore_upload_input(input),
            LocalPathInputResult::Confirmed(path_str) => {
                let local_path = std::path::PathBuf::from(&path_str);
                if !local_path.exists() {
                    self.push_log(format!("File not found: {}", local_path.display()));
                    self.restore_upload_input(input);
                } else if local_path.is_dir() {
                    let folder_id = self.current_folder_id.clone();
                    let client = Arc::clone(&self.client);
                    let tx = self.result_tx.clone();
                    let name = local_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    self.loading = true;
                    self.loading_label = Some(format!("Uploading folder {}…", name));
                    self.input = InputMode::Normal;
                    std::thread::spawn(move || {
                        let result =
                            client
                                .upload_dir(&folder_id, &local_path)
                                .map(|(ok, failed)| {
                                    if failed == 0 {
                                        format!("Uploaded folder '{}' ({} files)", name, ok)
                                    } else {
                                        format!(
                                            "Uploaded folder '{}' ({} ok, {} failed)",
                                            name, ok, failed
                                        )
                                    }
                                });
                        let _ = tx.send(OpResult::Upload(result));
                    });
                } else if local_path.is_file() {
                    let folder_id = self.current_folder_id.clone();
                    let client = Arc::clone(&self.client);
                    let tx = self.result_tx.clone();
                    let name = local_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    self.loading = true;
                    self.loading_label = Some(format!("Uploading {}…", name));
                    self.input = InputMode::Normal;
                    std::thread::spawn(move || {
                        let result = client.upload_file(Some(&folder_id), &local_path).map(
                            |(name, dedup)| {
                                if dedup {
                                    format!("Uploaded '{}' (instant, dedup)", name)
                                } else {
                                    format!("Uploaded '{}'", name)
                                }
                            },
                        );
                        let _ = tx.send(OpResult::Upload(result));
                    });
                } else {
                    self.push_log(format!("Not a file or directory: {}", local_path.display()));
                    self.restore_upload_input(input);
                }
            }
            LocalPathInputResult::Cancelled => {
                self.input = InputMode::Normal;
            }
        }
    }

    fn start_cart_download(&mut self, dest_dir: &str) {
        let dest = PathBuf::from(dest_dir);
        let cart_items = std::mem::take(&mut self.cart);
        self.cart_ids.clear();
        self.cart_selected = 0;

        let mut count = 0;
        let mut folders = 0;
        // Reserve unique local names: two cart entries may share a name
        // (different folders, or duplicates within one), and concurrent
        // workers writing one path interleave chunks into a corrupt file.
        // Names already queued for this directory count as taken too.
        let mut taken = self.download_state.reserved_names_in(dest.as_path());
        for item in cart_items {
            // Sanitized: the name is server data and must not escape dest.
            let local_name = crate::pikpak::unique_local_name(
                &mut taken,
                &crate::pikpak::sanitize_filename(&item.name),
            );
            let item_dest = dest.join(&local_name);
            if item.kind == EntryKind::Folder {
                // Expanded off-thread into one task per file (OpResult::FolderExpanded).
                folders += 1;
                let client = Arc::clone(&self.client);
                let tx = self.result_tx.clone();
                std::thread::spawn(move || {
                    let result = client
                        .list_folder_files(&item.id, &item_dest)
                        .map(|files| (item.name, files));
                    let _ = tx.send(OpResult::FolderExpanded(result));
                });
            } else {
                count += 1;
                self.download_state.queue(item, item_dest);
            }
        }

        self.push_log(format!(
            "Queued {count} files for download{}",
            if folders > 0 {
                format!(" ({folders} folders expanding)")
            } else {
                String::new()
            }
        ));
        self.download_state.start_next(&self.client);
    }

    fn handle_download_view_key(&mut self, code: KeyCode) {
        let task_count = self.download_state.tasks.len();

        // Per-task keys (j/k/p/x/r) need the Expanded list's visible selection
        // cursor. The collapsed view is a summary with no cursor, so there only
        // Enter (expand) and Esc (close) act — otherwise p/x would hit a task
        // the user can't see.
        if matches!(
            code,
            KeyCode::Char('j')
                | KeyCode::Char('k')
                | KeyCode::Char('p')
                | KeyCode::Char('x')
                | KeyCode::Char('r')
                | KeyCode::Down
                | KeyCode::Up
        ) && self.download_view_mode != crate::tui::DownloadViewMode::Expanded
        {
            self.input = InputMode::DownloadView;
            return;
        }

        match code {
            KeyCode::Esc => {}
            KeyCode::Enter => {
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
                let sel = self.download_state.selected;
                let mut log_msg = None;
                let mut need_start = false;
                let info = self
                    .download_state
                    .tasks
                    .get(sel)
                    .map(|t| (t.status.clone(), t.name.clone()));
                if let Some((status, name)) = info {
                    match status {
                        TaskStatus::Downloading => {
                            // Stop the worker instead of parking it: a parked
                            // thread pins an HTTP connection and, once other
                            // tasks filled its slot, resuming it would exceed
                            // max_concurrent. The slot frees when the worker
                            // acknowledges with Stopped.
                            let task = &mut self.download_state.tasks[sel];
                            task.cancel_flag.store(true, Ordering::Relaxed);
                            task.status = TaskStatus::Paused;
                            log_msg = Some(format!("Paused '{}'", name));
                        }
                        TaskStatus::Paused => {
                            // Fresh flag: the old worker may still be draining
                            // and must keep seeing its stop signal. start_next
                            // skips this id until Stopped frees it, so no
                            // second worker can write the same file.
                            let task = &mut self.download_state.tasks[sel];
                            task.cancel_flag = Arc::new(AtomicBool::new(false));
                            task.status = TaskStatus::Pending;
                            need_start = true;
                            log_msg = Some(format!("Resumed '{}'", name));
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
                let sel = self.download_state.selected;
                if let Some(name) = self.download_state.cancel_task(sel) {
                    self.push_log(format!("Cancelled '{}'", name));
                    // If it had no worker (plain Pending), a slot is available
                    // now. Otherwise active_ids retains the slot until Stopped.
                    self.download_state.start_next(&self.client);
                }
                self.input = InputMode::DownloadView;
            }
            KeyCode::Char('r') => {
                let sel = self.download_state.selected;
                let mut log_msg = None;
                let mut need_start = false;
                if let Some(task) = self.download_state.tasks.get_mut(sel)
                    && matches!(task.status, TaskStatus::Failed(_))
                {
                    task.status = TaskStatus::Pending;
                    task.cancel_flag = Arc::new(AtomicBool::new(false));
                    log_msg = Some(format!("Retrying '{}'", task.name));
                    need_start = true;
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

    fn spawn_star_toggle(&mut self, entry: Entry) {
        let action = FileAction::Star(!entry.starred);
        let success = format!("{} '{}'", action.completed_label(), entry.name);
        self.spawn_file_mutation(action, vec![entry.id], success, false);
    }

    fn handle_offline_input_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        value: &mut String,
    ) {
        match handle_text_input(value, &mut self.text_cursor, code, modifiers) {
            Some(false) => {
                self.push_log("Offline download cancelled".into());
            }
            Some(true) => {
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
            None => {
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

    pub(super) fn open_info_popup(&mut self, entry: Entry) {
        self.input = InputMode::InfoLoading;
        self.loading = true;
        self.loading_label = Some("Loading file info...".into());
        let request = self.begin_modal_request(AsyncRequestKind::Info, entry.id.clone());
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        let eid = entry.id.clone();
        let thumb_fallback = entry.thumbnail_link.clone();
        std::thread::spawn(move || {
            let _ = tx.send(OpResult::Info(
                request,
                client.file_info(&eid),
                thumb_fallback,
            ));
        });
    }

    fn open_folder_info_popup(&mut self, entry: Entry) {
        self.input = InputMode::InfoLoading;
        self.loading = true;
        self.loading_label = Some("Loading folder...".into());
        self.preview_target_id = Some(entry.id.clone());
        self.preview_target_name = Some(entry.name.clone());
        let request = self.begin_modal_request(AsyncRequestKind::FolderPreview, entry.id.clone());
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        let eid = entry.id.clone();
        std::thread::spawn(move || {
            let _ = tx.send(OpResult::PreviewLs(request, client.ls(&eid)));
        });
    }

    pub(super) fn spawn_player(&mut self, cmd: &str, url: &str) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            self.push_log("Player command is empty".into());
            return;
        }
        let program = parts[0];
        let mut args: Vec<&str> = parts[1..].to_vec();
        args.push("--");
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
        let success = format!("Removed '{}' (to trash)", entry.name);
        self.spawn_file_mutation(FileAction::Trash, vec![entry.id], success, false);
    }

    pub(super) fn handle_mouse(&mut self, mouse: MouseEvent) {
        if self.show_help_sheet {
            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    self.help_scroll = self.help_scroll.saturating_sub(3);
                }
                MouseEventKind::ScrollDown => {
                    self.help_scroll = (self.help_scroll + 3).min(self.help_scroll_max.get());
                }
                MouseEventKind::Down(_) => {
                    self.show_help_sheet = false;
                    self.help_scroll = 0;
                }
                _ => {}
            }
            return;
        }
        if !self.modal_request_survives_input() {
            self.invalidate_modal_request();
        }
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
        if matches!(self.input, InputMode::Normal) {
            if self.show_logs_overlay && self.is_in_rect(col, row, self.logs_overlay_area.get()) {
                let area = self.logs_overlay_area.get();
                let visible = area.height.saturating_sub(2) as usize;
                let content_width = area.width.saturating_sub(2).max(1) as usize;
                let total_visual =
                    super::wrap_logs(self.logs.iter().map(|s| s.as_str()), content_width).len();
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
            self.handle_browser_scroll(col, row, up);
            return;
        }

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
                self.download_state.selected = (self.download_state.selected + 1).min(count - 1);
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
        } else if let InputMode::Settings { selected, .. } = &mut self.input {
            // Mutate the selection in place — no need to clone the whole draft
            // config just to bump a usize each wheel notch.
            if up {
                if *selected > 0 {
                    *selected -= 1;
                }
            } else if *selected < SETTINGS_LAST_INDEX {
                *selected += 1;
            }
        } else if let InputMode::MySharesView {
            shares, selected, ..
        } = &mut self.input
        {
            if up {
                *selected = selected.saturating_sub(1);
            } else if !shares.is_empty() {
                *selected = (*selected + 1).min(shares.len() - 1);
            }
        } else if let InputMode::ConfirmPlay { dialog } = &mut self.input {
            if let Some(cursor) = &mut dialog.quality_cursor {
                *cursor = super::playback::move_selection(&dialog.medias, *cursor, up);
            }
        } else if let InputMode::PlayPicker {
            medias, selected, ..
        } = &mut self.input
        {
            *selected = super::playback::move_selection(medias, *selected, up);
        } else if let InputMode::CustomColorSettings { selected, .. } = &mut self.input {
            if up {
                *selected = selected.saturating_sub(1);
            } else {
                *selected = (*selected + 1).min(7);
            }
        } else if let InputMode::ImageProtocolSettings {
            selected,
            terminals,
            ..
        } = &mut self.input
        {
            if up {
                *selected = selected.saturating_sub(1);
            } else if !terminals.is_empty() {
                *selected = (*selected + 1).min(terminals.len() - 1);
            }
        } else if let InputMode::ActionMenu { selected } = &mut self.input {
            if up {
                *selected = selected.saturating_sub(1);
            } else {
                *selected = (*selected + 1).min(NORMAL_ACTIONS.len().saturating_sub(1));
            }
        }
    }

    fn handle_mouse_click(&mut self, col: u16, row: u16, double: bool) {
        // Same guard the keyboard path has: with the help sheet open, a click
        // must close it — not land on whatever pane sits underneath.
        if self.show_help_sheet {
            self.show_help_sheet = false;
            self.help_scroll = 0;
            return;
        }
        // The logs overlay floats above a pane; don't click through it.
        if self.show_logs_overlay && self.is_in_rect(col, row, self.logs_overlay_area.get()) {
            return;
        }

        if matches!(self.input, InputMode::ConfirmPlay { .. }) {
            self.handle_play_dialog_click(col, row);
            return;
        }
        let mouse_list_area = self.mouse_list_area.get();
        let first_row = self.mouse_list_first_row.get();
        let visible = self.mouse_list_visible.get();
        let clicked_list_index = mouse_list_index(
            col,
            row,
            mouse_list_area,
            first_row,
            self.mouse_list_offset.get(),
            visible,
        );
        if let Some(clicked_idx) = clicked_list_index {
            let mut handled = true;
            let mut activate = false;
            match &mut self.input {
                InputMode::CartView if clicked_idx < self.cart.len() => {
                    self.cart_selected = clicked_idx;
                    activate = double;
                }
                InputMode::DownloadView
                    if self.download_view_mode == crate::tui::DownloadViewMode::Expanded
                        && clicked_idx < self.download_state.tasks.len() =>
                {
                    self.download_state.selected = clicked_idx;
                }
                InputMode::OfflineTasksView {
                    tasks, selected, ..
                } if clicked_idx < tasks.len() => {
                    *selected = clicked_idx;
                }
                InputMode::TrashView {
                    entries, selected, ..
                } if clicked_idx < entries.len() => {
                    *selected = clicked_idx;
                    self.trash_selected = clicked_idx;
                    activate = double;
                }
                InputMode::MySharesView {
                    shares, selected, ..
                } if clicked_idx < shares.len() => {
                    *selected = clicked_idx;
                }
                InputMode::PlayPicker {
                    medias, selected, ..
                } if clicked_idx < medias.len() => {
                    if medias[clicked_idx].available {
                        *selected = clicked_idx;
                        activate = double;
                    }
                }
                InputMode::CustomColorSettings { selected, .. } if clicked_idx < 8 => {
                    *selected = clicked_idx;
                }
                InputMode::ImageProtocolSettings {
                    terminals,
                    selected,
                    ..
                } if clicked_idx < terminals.len() => {
                    *selected = clicked_idx;
                }
                InputMode::ActionMenu { selected } if clicked_idx < NORMAL_ACTIONS.len() => {
                    *selected = clicked_idx;
                    activate = double;
                }
                _ => handled = false,
            }
            if handled {
                if activate {
                    let _ = self.handle_key(KeyCode::Enter, KeyModifiers::NONE);
                }
                return;
            }
        }

        if matches!(self.input, InputMode::Settings { .. }) {
            let area = self.settings_area.get();
            if let InputMode::Settings {
                mut selected,
                mut editing,
                mut draft,
                mut modified,
            } = std::mem::replace(&mut self.input, InputMode::Normal)
            {
                if self.is_in_rect(col, row, area) && !editing {
                    let content_y = row.saturating_sub(area.y + 1) as usize;
                    let content_x = col.saturating_sub(area.x + 1) as usize;

                    // Derive the hit-test layout from the single settings source
                    // (settings_items), so it can't drift from what
                    // draw_settings_overlay renders. Bool toggles are exactly the
                    // checkbox-valued items.
                    let layout = Self::settings_items(&draft);
                    let bool_items: Vec<usize> = layout
                        .iter()
                        .flat_map(|(_, items)| items.iter())
                        .enumerate()
                        .filter_map(|(idx, item)| {
                            let value = item.2.as_str();
                            (value == "[\u{2713}]" || value == "[ ]").then_some(idx)
                        })
                        .collect();

                    // Reverse-map the click through the same layout draw uses,
                    // compensating for the leading blank line and the active
                    // scroll offset so the hit lands on the drawn item.
                    let item_counts: Vec<usize> =
                        layout.iter().map(|(_, items)| items.len()).collect();
                    let item_line_map = widgets::settings_item_line_map(&item_counts);
                    let inner_height = area.height.saturating_sub(4) as usize;
                    let scroll_offset =
                        widgets::settings_scroll_offset(&item_line_map, selected, inner_height);
                    let terminal_width = (area.width.saturating_sub(4)) as usize;

                    if let Some((item_idx, on_name_row)) =
                        widgets::settings_item_at_row(&item_line_map, scroll_offset, content_y)
                    {
                        selected = item_idx;

                        if on_name_row
                            && bool_items.contains(&item_idx)
                            && content_x + 10 >= terminal_width
                        {
                            match item_idx {
                                0 => draft.nerd_font = !draft.nerd_font,
                                3 => draft.show_help_bar = !draft.show_help_bar,
                                5 => draft.show_preview = !draft.show_preview,
                                6 => draft.lazy_preview = !draft.lazy_preview,
                                11 => draft.sort_reverse = !draft.sort_reverse,
                                13 => draft.cli_nerd_font = !draft.cli_nerd_font,
                                20 => draft.inline_thumbnails = !draft.inline_thumbnails,
                                22 => draft.thumbnail_placeholders = !draft.thumbnail_placeholders,
                                _ => {}
                            }
                            modified = true;
                        } else if double {
                            self.settings_number_input.clear();
                            editing = true;
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

        self.handle_browser_click(col, row, double);
    }

    fn is_in_rect(&self, col: u16, row: u16, rect: ratatui::layout::Rect) -> bool {
        rect.contains((col, row).into())
    }

    pub(super) fn spawn_permanent_delete(&mut self, entry: Entry) {
        let success = format!("Permanently deleted '{}'", entry.name);
        self.spawn_file_mutation(FileAction::Delete, vec![entry.id], success, false);
    }

    #[allow(clippy::collapsible_match)]
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
                                selected: SETTINGS_IMAGE_PROTOCOL_INDEX,
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
                self.input = InputMode::Settings {
                    selected: SETTINGS_IMAGE_PROTOCOL_INDEX,
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

    #[allow(clippy::collapsible_match, clippy::too_many_arguments)]
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
                                    selected: SETTINGS_COLOR_SCHEME_INDEX,
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
                    self.input = InputMode::Settings {
                        selected: SETTINGS_COLOR_SCHEME_INDEX,
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
        modifiers: KeyModifiers,
        selected: &mut usize,
        editing: &mut bool,
        draft: &mut crate::config::TuiConfig,
        modified: &mut bool,
    ) -> Option<bool> {
        if *editing {
            match *selected {
                0 => match code {
                    KeyCode::Char(' ') | KeyCode::Enter | KeyCode::Left | KeyCode::Right => {
                        draft.nerd_font = !draft.nerd_font;
                        *modified = true;
                        *editing = false;
                    }
                    KeyCode::Esc => {
                        *editing = false;
                    }
                    _ => {}
                },
                1 => match code {
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
                },
                2 => match code {
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
                },
                3 => match code {
                    KeyCode::Char(' ') | KeyCode::Enter | KeyCode::Left | KeyCode::Right => {
                        draft.show_help_bar = !draft.show_help_bar;
                        *modified = true;
                        *editing = false;
                    }
                    KeyCode::Esc => {
                        *editing = false;
                    }
                    _ => {}
                },
                4 => match code {
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
                },
                5 => match code {
                    KeyCode::Char(' ') | KeyCode::Enter | KeyCode::Left | KeyCode::Right => {
                        draft.show_preview = !draft.show_preview;
                        *modified = true;
                        *editing = false;
                    }
                    KeyCode::Esc => {
                        *editing = false;
                    }
                    _ => {}
                },
                6 => match code {
                    KeyCode::Char(' ') | KeyCode::Enter | KeyCode::Left | KeyCode::Right => {
                        draft.lazy_preview = !draft.lazy_preview;
                        *modified = true;
                        *editing = false;
                    }
                    KeyCode::Esc => {
                        *editing = false;
                    }
                    _ => {}
                },
                7 => match code {
                    KeyCode::Char('+') | KeyCode::Up => {
                        draft.preview_max_size = (draft.preview_max_size + 1024).min(10485760);
                        *modified = true;
                    }
                    KeyCode::Char('-') | KeyCode::Down => {
                        draft.preview_max_size =
                            (draft.preview_max_size.saturating_sub(1024)).max(1024);
                        *modified = true;
                    }
                    KeyCode::Enter => {
                        *editing = false;
                    }
                    KeyCode::Esc => {
                        *editing = false;
                    }
                    _ => {}
                },
                8 => match code {
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
                },
                9 => match code {
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
                },
                10 => match code {
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
                },
                11 => match code {
                    KeyCode::Char(' ') | KeyCode::Enter | KeyCode::Left | KeyCode::Right => {
                        draft.sort_reverse = !draft.sort_reverse;
                        *modified = true;
                        *editing = false;
                    }
                    KeyCode::Esc => {
                        *editing = false;
                    }
                    _ => {}
                },
                12 => match code {
                    KeyCode::Left => {
                        draft.move_mode = draft.move_mode.toggle();
                        *modified = true;
                    }
                    KeyCode::Right => {
                        draft.move_mode = draft.move_mode.toggle();
                        *modified = true;
                    }
                    KeyCode::Enter => {
                        *editing = false;
                    }
                    KeyCode::Esc => {
                        *editing = false;
                    }
                    _ => {}
                },
                13 => match code {
                    KeyCode::Char(' ') | KeyCode::Enter | KeyCode::Left | KeyCode::Right => {
                        draft.cli_nerd_font = !draft.cli_nerd_font;
                        *modified = true;
                        *editing = false;
                    }
                    KeyCode::Esc => {
                        *editing = false;
                    }
                    _ => {}
                },
                14 => match code {
                    KeyCode::Esc => {
                        *editing = false;
                    }
                    KeyCode::Enter => {
                        *editing = false;
                    }
                    _ => {
                        let player = draft.player.get_or_insert_default();
                        let old = player.clone();
                        let _ = handle_text_input(player, &mut self.text_cursor, code, modifiers);
                        if player != &old {
                            *modified = true;
                        }
                        if player.is_empty() {
                            draft.player = None;
                        }
                    }
                },
                15 => match code {
                    KeyCode::Right | KeyCode::Char('+') => {
                        draft.playback_quality = draft.playback_quality.next();
                        *modified = true;
                    }
                    KeyCode::Left | KeyCode::Char('-') => {
                        draft.playback_quality = draft.playback_quality.prev();
                        *modified = true;
                    }
                    KeyCode::Enter | KeyCode::Esc => *editing = false,
                    _ => {}
                },
                16 => match code {
                    KeyCode::Char('+') | KeyCode::Up | KeyCode::Right => {
                        draft.download_jobs = (draft.download_jobs + 1).min(16);
                        *modified = true;
                    }
                    KeyCode::Char('-') | KeyCode::Down | KeyCode::Left => {
                        draft.download_jobs = draft.download_jobs.saturating_sub(1).max(1);
                        *modified = true;
                    }
                    KeyCode::Enter | KeyCode::Esc => {
                        *editing = false;
                    }
                    _ => {}
                },
                17 => match code {
                    KeyCode::Right | KeyCode::Char('+') | KeyCode::Char('l') => {
                        draft.update_check = draft.update_check.next();
                        *modified = true;
                    }
                    KeyCode::Left | KeyCode::Char('-') | KeyCode::Char('h') => {
                        draft.update_check = draft.update_check.prev();
                        *modified = true;
                    }
                    KeyCode::Enter | KeyCode::Esc => {
                        *editing = false;
                    }
                    _ => {}
                },
                18 => match code {
                    KeyCode::Char('a') => {
                        draft.columns = crate::config::ColumnCount::Auto;
                        self.settings_number_input.clear();
                        *modified = true;
                    }
                    KeyCode::Char(c) if c.is_ascii_digit() => {
                        self.settings_number_input.push(c);
                        if let Ok(count) = self.settings_number_input.parse::<usize>()
                            && count > 0
                        {
                            draft.columns = crate::config::ColumnCount::Fixed(count);
                            *modified = true;
                        }
                    }
                    KeyCode::Backspace => {
                        self.settings_number_input.pop();
                        draft.columns = self
                            .settings_number_input
                            .parse::<usize>()
                            .ok()
                            .filter(|count| *count > 0)
                            .map(crate::config::ColumnCount::Fixed)
                            .unwrap_or(crate::config::ColumnCount::Auto);
                        *modified = true;
                    }
                    KeyCode::Right | KeyCode::Char('+') => {
                        draft.columns = match draft.columns {
                            crate::config::ColumnCount::Auto => {
                                crate::config::ColumnCount::Fixed(3)
                            }
                            crate::config::ColumnCount::Fixed(n) => {
                                crate::config::ColumnCount::Fixed(n.saturating_add(1))
                            }
                        };
                        self.settings_number_input.clear();
                        *modified = true;
                    }
                    KeyCode::Left | KeyCode::Char('-') => {
                        draft.columns = match draft.columns {
                            crate::config::ColumnCount::Fixed(n) if n > 1 => {
                                crate::config::ColumnCount::Fixed(n - 1)
                            }
                            _ => crate::config::ColumnCount::Auto,
                        };
                        self.settings_number_input.clear();
                        *modified = true;
                    }
                    KeyCode::Enter | KeyCode::Esc => *editing = false,
                    _ => {}
                },
                19 => match code {
                    KeyCode::Char(c) if c.is_ascii_digit() => {
                        self.settings_number_input.push(c);
                        if let Ok(width) = self.settings_number_input.parse::<u16>() {
                            draft.column_min_width = width.max(16);
                            *modified = true;
                        }
                    }
                    KeyCode::Backspace => {
                        self.settings_number_input.pop();
                        draft.column_min_width = self
                            .settings_number_input
                            .parse::<u16>()
                            .unwrap_or(28)
                            .max(16);
                        *modified = true;
                    }
                    KeyCode::Right | KeyCode::Char('+') => {
                        draft.column_min_width = draft.column_min_width.saturating_add(1).max(16);
                        self.settings_number_input.clear();
                        *modified = true;
                    }
                    KeyCode::Left | KeyCode::Char('-') => {
                        draft.column_min_width = draft.column_min_width.saturating_sub(1).max(16);
                        self.settings_number_input.clear();
                        *modified = true;
                    }
                    KeyCode::Enter | KeyCode::Esc => *editing = false,
                    _ => {}
                },
                20 => match code {
                    KeyCode::Char(' ') | KeyCode::Enter | KeyCode::Left | KeyCode::Right => {
                        draft.inline_thumbnails = !draft.inline_thumbnails;
                        *modified = true;
                        *editing = false;
                    }
                    KeyCode::Esc => *editing = false,
                    _ => {}
                },
                21 => match code {
                    KeyCode::Right | KeyCode::Char('+') | KeyCode::Char(']') => {
                        draft.inline_thumbnail_size = draft.inline_thumbnail_size.larger();
                        *modified = true;
                    }
                    KeyCode::Left | KeyCode::Char('-') | KeyCode::Char('[') => {
                        draft.inline_thumbnail_size = draft.inline_thumbnail_size.smaller();
                        *modified = true;
                    }
                    KeyCode::Enter | KeyCode::Esc => *editing = false,
                    _ => {}
                },
                22 => match code {
                    KeyCode::Char(' ') | KeyCode::Enter | KeyCode::Left | KeyCode::Right => {
                        draft.thumbnail_placeholders = !draft.thumbnail_placeholders;
                        *modified = true;
                        *editing = false;
                    }
                    KeyCode::Esc => *editing = false,
                    _ => {}
                },
                23 => match code {
                    KeyCode::Right | KeyCode::Char('+') => {
                        draft.thumbnail_size = draft.thumbnail_size.next();
                        *modified = true;
                    }
                    KeyCode::Left | KeyCode::Char('-') => {
                        draft.thumbnail_size = draft.thumbnail_size.prev();
                        *modified = true;
                    }
                    KeyCode::Enter | KeyCode::Esc => *editing = false,
                    _ => {}
                },
                _ => {}
            }
            None
        } else {
            match code {
                KeyCode::Down | KeyCode::Char('j') => {
                    *selected = (*selected + 1).min(SETTINGS_LAST_INDEX);
                    None
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    *selected = selected.saturating_sub(1);
                    None
                }
                KeyCode::Char(' ') | KeyCode::Enter => {
                    if *selected == 9 {
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
                    if *selected == 14 {
                        self.text_cursor = draft.player.as_ref().map_or(0, String::len);
                    }
                    self.settings_number_input.clear();
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
                KeyCode::Esc => Some(false),
                _ => None,
            }
        }
    }
}

/// Write `text` to the system clipboard using the best available tool.
pub(super) fn write_clipboard(text: &str) -> anyhow::Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("pbcopy", &[] as &[&str])]
    } else {
        &[
            ("wl-copy", &[] as &[&str]),
            ("xclip", &["-selection", "clipboard"]),
        ]
    };

    for &(cmd, args) in candidates {
        let Ok(mut child) = Command::new(cmd).args(args).stdin(Stdio::piped()).spawn() else {
            continue;
        };
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(text.as_bytes());
        }
        child.wait()?;
        return Ok(());
    }

    Err(anyhow::anyhow!(
        "no clipboard tool found (pbcopy / wl-copy / xclip)"
    ))
}

#[cfg(test)]
mod mouse_list_tests {
    use super::mouse_list_index;
    use ratatui::layout::Rect;

    #[test]
    fn list_hit_testing_maps_visible_rows_through_scroll_offset() {
        let area = Rect::new(10, 5, 30, 12);
        assert_eq!(mouse_list_index(12, 7, area, 7, 4, 6), Some(4));
        assert_eq!(mouse_list_index(12, 10, area, 7, 4, 6), Some(7));
    }

    #[test]
    fn list_hit_testing_rejects_borders_and_rows_beyond_items() {
        let area = Rect::new(10, 5, 30, 12);
        assert_eq!(mouse_list_index(10, 7, area, 7, 0, 3), None);
        assert_eq!(mouse_list_index(9, 7, area, 7, 0, 3), None);
        assert_eq!(mouse_list_index(12, 6, area, 7, 0, 3), None);
        assert_eq!(mouse_list_index(12, 10, area, 7, 0, 3), None);
    }
}

#[cfg(test)]
mod browser_settings_tests {
    use super::*;
    use crate::config::{ColumnCount, InlineThumbnailSize, TuiConfig};
    use crate::pikpak::PikPak;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn numeric_settings_accept_multi_digit_values_and_reset_between_fields() {
        let mut app = App::new_login(PikPak::new().unwrap(), None, TuiConfig::default());
        app.input = InputMode::Settings {
            selected: 18,
            editing: false,
            draft: TuiConfig::default(),
            modified: false,
        };
        for code in [
            KeyCode::Enter,
            KeyCode::Char('1'),
            KeyCode::Char('3'),
            KeyCode::Backspace,
            KeyCode::Char('2'),
            KeyCode::Enter,
        ] {
            app.handle_key(code, KeyModifiers::NONE).unwrap();
        }
        assert!(
            matches!(&app.input, InputMode::Settings { draft, modified: true, .. } if draft.columns == ColumnCount::Fixed(12))
        );
        for code in [
            KeyCode::Down,
            KeyCode::Enter,
            KeyCode::Char('3'),
            KeyCode::Char('2'),
            KeyCode::Enter,
        ] {
            app.handle_key(code, KeyModifiers::NONE).unwrap();
        }
        assert!(
            matches!(&app.input, InputMode::Settings { draft, .. } if draft.column_min_width == 32 && draft.columns == ColumnCount::Fixed(12))
        );
        // Changes stay in the draft until the existing Save action is used.
        assert_eq!(app.config.columns, ColumnCount::Auto);
    }

    #[test]
    fn new_thumbnail_checkbox_uses_the_rendered_settings_hit_map() {
        let mut app = App::new_login(PikPak::new().unwrap(), None, TuiConfig::default());
        app.input = InputMode::Settings {
            selected: 20,
            editing: false,
            draft: TuiConfig::default(),
            modified: false,
        };
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let area = app.settings_area.get();
        let categories = App::settings_items(&app.config);
        let counts: Vec<_> = categories.iter().map(|(_, items)| items.len()).collect();
        let lines = widgets::settings_item_line_map(&counts);
        assert_eq!(lines.len() - 1, SETTINGS_LAST_INDEX);
        let (y, row) = terminal
            .backend()
            .buffer()
            .content
            .chunks(100)
            .enumerate()
            .find(|(_, row)| {
                row.iter()
                    .map(|c| c.symbol())
                    .collect::<String>()
                    .contains("Inline Thumbnails")
            })
            .unwrap();
        let x = row
            .windows(3)
            .position(|cells| {
                cells[0].symbol() == "[" && cells[1].symbol() == "✓" && cells[2].symbol() == "]"
            })
            .unwrap();
        assert!(area.contains((x as u16, y as u16).into()));
        app.handle_mouse_click(x as u16, y as u16, false);
        assert!(
            matches!(&app.input, InputMode::Settings { draft, modified: true, .. } if !draft.inline_thumbnails)
        );
        app.handle_key(KeyCode::Down, KeyModifiers::NONE).unwrap();
        app.handle_key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
        app.handle_key(KeyCode::Right, KeyModifiers::NONE).unwrap();
        assert!(
            matches!(&app.input, InputMode::Settings { draft, .. } if draft.inline_thumbnail_size == InlineThumbnailSize::Small)
        );
    }

    #[test]
    fn placeholder_and_source_size_edits_stay_in_draft_then_apply_to_shared_client() {
        let mut app = App::new_login(PikPak::new().unwrap(), None, TuiConfig::default());
        let worker_client = Arc::clone(&app.client);
        app.input = InputMode::Settings {
            selected: 22,
            editing: false,
            draft: TuiConfig::default(),
            modified: false,
        };
        for code in [
            KeyCode::Enter,
            KeyCode::Enter,
            KeyCode::Down,
            KeyCode::Enter,
            KeyCode::Right,
            KeyCode::Enter,
        ] {
            app.handle_key(code, KeyModifiers::NONE).unwrap();
        }
        let InputMode::Settings {
            draft, modified, ..
        } = &app.input
        else {
            panic!("settings closed");
        };
        assert!(*modified);
        assert!(!draft.thumbnail_placeholders);
        assert_eq!(draft.thumbnail_size, crate::config::ThumbnailSize::Large);
        assert!(app.config.thumbnail_placeholders);
        assert_eq!(worker_client.thumbnail_size(), "SIZE_MEDIUM");
        let saved = draft.clone();
        assert!(app.apply_settings(saved.clone()));
        assert!(!app.config.thumbnail_placeholders);
        assert_eq!(worker_client.thumbnail_size(), "SIZE_LARGE");
        assert!(!app.apply_settings(saved));
    }

    #[test]
    fn placeholder_checkbox_uses_scrolled_mouse_hit_map() {
        let mut app = App::new_login(PikPak::new().unwrap(), None, TuiConfig::default());
        app.input = InputMode::Settings {
            selected: 22,
            editing: false,
            draft: TuiConfig::default(),
            modified: false,
        };
        let mut terminal = Terminal::new(TestBackend::new(100, 32)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let (y, row) = terminal
            .backend()
            .buffer()
            .content
            .chunks(100)
            .enumerate()
            .find(|(_, row)| {
                row.iter()
                    .map(|c| c.symbol())
                    .collect::<String>()
                    .contains("Thumbnail Placeholders")
            })
            .unwrap();
        let x = row
            .windows(3)
            .position(|cells| {
                cells[0].symbol() == "[" && cells[1].symbol() == "✓" && cells[2].symbol() == "]"
            })
            .unwrap();
        app.handle_mouse_click(x as u16, y as u16, false);
        assert!(
            matches!(&app.input, InputMode::Settings { selected: 22, draft, modified: true, .. } if !draft.thumbnail_placeholders)
        );
    }
}
