use super::{App, AsyncRequest, AsyncRequestKind, InputMode, OpResult, StatusKind};
use crate::pikpak::{Entry, OfflineListResponse, mutation_progress};
use anyhow::Result;
use crossterm::event::KeyCode;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

impl App {
    pub(super) fn open_offline_tasks_view(&mut self) {
        self.input = InputMode::InfoLoading;
        self.loading = true;
        self.loading_label = Some("Loading offline tasks...".into());
        self.fetch_offline_tasks();
    }

    fn fetch_offline_tasks(&mut self) {
        self.offline_refresh_at = None;
        let request = self.begin_modal_request(AsyncRequestKind::OfflineTasks, "offline-tasks");
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        std::thread::spawn(move || {
            let result = client.offline_list(u32::MAX, crate::pikpak::OFFLINE_TASK_PHASES);
            let _ = tx.send(OpResult::OfflineTasks(request, result));
        });
    }

    pub(super) fn schedule_offline_refresh(&mut self) {
        if !matches!(self.input, InputMode::OfflineTasksView { .. }) {
            self.offline_refresh_at = None;
            return;
        }
        if self.modal_request.is_none()
            && self.offline_refresh_at.is_some_and(|t| Instant::now() >= t)
        {
            self.fetch_offline_tasks();
        }
    }

    pub(super) fn handle_offline_tasks_key(
        &mut self,
        code: KeyCode,
        tasks: &mut Vec<crate::pikpak::OfflineTask>,
        selected: &mut usize,
    ) {
        match code {
            KeyCode::Esc => {
                self.offline_refresh_at = None;
                self.offline_selected_id = None;
                self.invalidate_modal_request();
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
                self.offline_selected_id = tasks.get(*selected).map(|t| t.id.clone());
                self.open_offline_tasks_view();
            }
            KeyCode::Char('R') => {
                if let Some(task) = tasks.get(*selected)
                    && task.phase == "PHASE_TYPE_ERROR"
                {
                    let client = Arc::clone(&self.client);
                    let tx = self.result_tx.clone();
                    let task_id = task.id.clone();
                    let task_name = task.name.clone();
                    self.offline_selected_id = Some(task_id.clone());
                    self.offline_refresh_at = None;
                    let request =
                        self.begin_modal_request(AsyncRequestKind::OfflineOp, task_id.clone());
                    self.input = InputMode::InfoLoading;
                    self.loading = true;
                    self.loading_label = Some("Retrying task...".into());
                    std::thread::spawn(move || {
                        let result = client
                            .offline_task_retry(&task_id)
                            .map(|()| format!("Retrying task: {}", task_name));
                        // OfflineOp reloads the task list, so the view returns
                        // here instead of falling back to the file browser.
                        let _ = tx.send(OpResult::OfflineOp(request, result));
                    });
                    return;
                }
                self.input = InputMode::OfflineTasksView {
                    tasks: std::mem::take(tasks),
                    selected: *selected,
                };
            }
            KeyCode::Char('x') => {
                if let Some(task) = tasks.get(*selected) {
                    let client = Arc::clone(&self.client);
                    let tx = self.result_tx.clone();
                    let task_id = task.id.clone();
                    let task_name = task.name.clone();
                    self.offline_selected_id = Some(task_id.clone());
                    self.offline_refresh_at = None;
                    let request =
                        self.begin_modal_request(AsyncRequestKind::OfflineOp, task_id.clone());
                    self.input = InputMode::InfoLoading;
                    self.loading = true;
                    self.loading_label = Some("Deleting task...".into());
                    std::thread::spawn(move || {
                        let result = client
                            .delete_tasks(&[task_id.as_str()], false)
                            .map(|()| format!("Deleted task: {}", task_name));
                        let _ = tx.send(OpResult::OfflineOp(request, result));
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

    pub(super) fn open_trash_view(&mut self) {
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
        let request = self.begin_modal_request(AsyncRequestKind::Trash, "trash");
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(OpResult::TrashList(request, client.ls_trash(u32::MAX)));
        });
    }

    pub(super) fn handle_trash_view_key(
        &mut self,
        code: KeyCode,
        entries: &mut Vec<Entry>,
        selected: &mut usize,
        expanded: bool,
    ) {
        if self.loading {
            if matches!(code, KeyCode::Esc) {
                self.invalidate_modal_request();
                self.trash_entries.clear();
                self.trash_selected = 0;
                self.trash_expanded = false;
                self.finish_loading();
                return;
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
                    self.invalidate_modal_request();
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
                    let request = self.begin_modal_request(AsyncRequestKind::TrashOp, eid.clone());
                    std::thread::spawn(move || {
                        let result = client
                            .untrash(&[eid.as_str()])
                            .map(|()| format!("Restored '{}'", name));
                        let _ = tx.send(OpResult::TrashOp(request, result));
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
                    let request = self.begin_modal_request(AsyncRequestKind::TrashOp, eid.clone());
                    std::thread::spawn(move || {
                        let result = client
                            .delete_permanent(&[eid.as_str()])
                            .map(|()| format!("Permanently deleted '{}'", name));
                        let _ = tx.send(OpResult::TrashOp(request, result));
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
                        size: if entry.size > 0 {
                            Some(entry.size.to_string())
                        } else {
                            None
                        },
                        hash: None,
                        mime_type: None,
                        created_time: if entry.created_time.is_empty() {
                            None
                        } else {
                            Some(entry.created_time)
                        },
                        modified_time: if entry.modified_time.is_empty() {
                            None
                        } else {
                            Some(entry.modified_time)
                        },
                        web_content_link: None,
                        thumbnail_link: entry.thumbnail_link,
                        phase: entry.phase,
                        audit: entry.audit,
                        links: None,
                        medias: None,
                    };
                    let thumb_url = info.thumbnail_link.clone().filter(|u| !u.is_empty());
                    let has_thumbnail = thumb_url.is_some();
                    let target_id = info.id.clone().unwrap_or_default();
                    let request =
                        self.begin_modal_request(AsyncRequestKind::Info, target_id.clone());
                    self.input = InputMode::InfoView {
                        request_id: request.id,
                        target_id,
                        info,
                        image: None,
                        has_thumbnail,
                    };
                    if let Some(url) = thumb_url {
                        self.spawn_thumbnail_fetch(url, move |result| {
                            super::OpResult::InfoThumbnail(request, result)
                        });
                    } else {
                        self.modal_request = None;
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
                self.open_trash_view_preserve();
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

    fn open_trash_view_preserve(&mut self) {
        self.input = InputMode::TrashView {
            entries: self.trash_entries.clone(),
            selected: self.trash_selected,
            expanded: self.trash_expanded,
        };
        self.loading = true;
        self.loading_label = Some("Loading trash...".into());
        let request = self.begin_modal_request(AsyncRequestKind::Trash, "trash");
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(OpResult::TrashList(request, client.ls_trash(u32::MAX)));
        });
    }

    pub(super) fn apply_offline_tasks(
        &mut self,
        request: AsyncRequest,
        result: Result<OfflineListResponse>,
    ) {
        match result {
            Ok(response) => {
                if !self.modal_request_matches(&request)
                    || !matches!(
                        self.input,
                        InputMode::InfoLoading | InputMode::OfflineTasksView { .. }
                    )
                {
                    return;
                }
                let old_selection = match &self.input {
                    InputMode::OfflineTasksView { tasks, selected } => {
                        tasks.get(*selected).map(|t| t.id.clone())
                    }
                    _ => self.offline_selected_id.take(),
                };
                self.modal_request = None;
                self.finish_loading();
                let tasks = response.tasks;
                let selected = old_selection
                    .and_then(|id| tasks.iter().position(|t| t.id == id))
                    .unwrap_or(0);
                self.offline_refresh_at = tasks
                    .iter()
                    .any(|t| {
                        matches!(
                            t.phase.as_str(),
                            "PHASE_TYPE_RUNNING" | "PHASE_TYPE_PENDING" | "PHASE_TYPE_UNKNOW"
                        )
                    })
                    .then(|| {
                        Instant::now()
                            .checked_add(Duration::from_secs(
                                response.expires_in.filter(|s| *s > 0).unwrap_or(5),
                            ))
                            .unwrap_or_else(|| Instant::now() + Duration::from_secs(5))
                    });
                self.input = InputMode::OfflineTasksView { tasks, selected };
            }
            Err(e) => {
                if !self.modal_request_matches(&request)
                    || !matches!(
                        self.input,
                        InputMode::InfoLoading | InputMode::OfflineTasksView { .. }
                    )
                {
                    return;
                }
                self.modal_request = None;
                self.finish_loading();
                if matches!(self.input, InputMode::OfflineTasksView { .. }) {
                    self.offline_refresh_at = Some(Instant::now() + Duration::from_secs(5));
                } else {
                    self.offline_refresh_at = None;
                    self.input = InputMode::Normal;
                }
                self.push_log(format!("Failed to load offline tasks: {e:#}"));
            }
        }
    }

    pub(super) fn apply_trash_list(&mut self, request: AsyncRequest, result: Result<Vec<Entry>>) {
        if !self.modal_request_matches(&request)
            || !matches!(self.input, InputMode::TrashView { .. })
        {
            return;
        }
        self.modal_request = None;
        self.finish_loading();
        match result {
            Ok(entries) => {
                let old_id = self.trash_entries.get(self.trash_selected).map(|e| &e.id);
                let selected = old_id
                    .and_then(|id| entries.iter().position(|e| &e.id == id))
                    .unwrap_or_else(|| self.trash_selected.min(entries.len().saturating_sub(1)));
                self.trash_entries = entries.clone();
                self.trash_selected = selected;
                self.input = InputMode::TrashView {
                    entries,
                    selected,
                    expanded: self.trash_expanded,
                };
            }
            Err(e) => self.push_log(format!("Failed to load trash: {e:#}")),
        }
    }

    pub(super) fn apply_task_operation(
        &mut self,
        request: AsyncRequest,
        result: Result<String>,
        trash: bool,
    ) {
        let active = self.modal_request_matches(&request)
            && if trash {
                matches!(self.input, InputMode::TrashView { .. })
            } else {
                matches!(self.input, InputMode::InfoLoading)
            };
        match result {
            Ok(msg) => self.push_status_log(msg, StatusKind::Info),
            Err(error) => {
                let pending = mutation_progress(&error).pending;
                self.push_status_log(
                    format!(
                        "{}: {error:#}",
                        if pending {
                            "Operation pending"
                        } else {
                            "Operation failed"
                        }
                    ),
                    if pending {
                        StatusKind::Warning
                    } else {
                        StatusKind::Error
                    },
                );
            }
        }
        // Completed operations still get logged after closing; they never
        // replace a different modal or release that modal's loading state.
        if active {
            self.modal_request = None;
            self.finish_loading();
            if trash {
                self.open_trash_view_preserve();
            } else {
                self.open_offline_tasks_view();
            }
        }
    }
}
