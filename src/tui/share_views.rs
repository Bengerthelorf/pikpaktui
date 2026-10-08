use super::handler::write_clipboard;
use super::{App, AsyncRequest, AsyncRequestKind, InputMode, OpResult, StatusKind};
use crate::pikpak::{CreateShareResponse, MyShare};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use std::sync::Arc;

impl App {
    pub(super) fn open_my_shares_view(&mut self) {
        self.input = InputMode::MySharesView {
            shares: Vec::new(),
            selected: 0,
            confirm_delete: None,
        };
        self.loading = true;
        self.loading_label = Some("Loading shares...".into());
        let request = self.begin_modal_request(AsyncRequestKind::MyShares, "my-shares");
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(OpResult::MyShares(request, client.list_shares()));
        });
    }
    pub(super) fn handle_share_prompt_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('p') => {
                self.spawn_create_shares(false);
            }
            KeyCode::Char('P') => {
                self.spawn_create_shares(true);
            }
            _ => {
                self.input = InputMode::CartView;
            }
        }
    }

    pub(super) fn handle_share_created_view_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        shares: &mut Vec<(String, String, String)>,
    ) {
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        match code {
            KeyCode::Esc if ctrl => {
                shares.clear();
                self.invalidate_modal_request();
                self.input = InputMode::CartView;
            }
            KeyCode::Esc => {
                shares.pop();
                if shares.is_empty() {
                    self.invalidate_modal_request();
                    self.input = InputMode::CartView;
                } else {
                    let owned = std::mem::take(shares);
                    self.input = InputMode::ShareCreatedView { shares: owned };
                }
            }
            KeyCode::Char('y') => {
                if let Some((_, url, _)) = shares.last() {
                    match write_clipboard(url) {
                        Ok(()) => self.push_log(format!("Copied URL: {url}")),
                        Err(e) => self.push_log(format!("Clipboard failed: {e:#}")),
                    }
                }
                let owned = std::mem::take(shares);
                self.input = InputMode::ShareCreatedView { shares: owned };
            }
            _ => {
                let owned = std::mem::take(shares);
                self.input = InputMode::ShareCreatedView { shares: owned };
            }
        }
    }

    pub(super) fn spawn_create_shares(&mut self, need_password: bool) {
        if self.cart.is_empty() {
            self.input = InputMode::CartView;
            return;
        }
        self.input = InputMode::ShareCreatedView { shares: vec![] };
        self.loading = true;
        self.loading_label = Some("Creating shares...".into());
        let request = self.begin_modal_request(AsyncRequestKind::ShareCreate, "create-shares");
        self.share_creations_pending = self.cart.len();
        // One worker queues the batch instead of starting a thread per item.
        let jobs: Vec<_> = self
            .cart
            .iter()
            .map(|entry| (entry.id.clone(), entry.name.clone()))
            .collect();
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        std::thread::spawn(move || {
            for (file_id, title) in jobs {
                let result = client.create_share(&[file_id.as_str()], need_password, 0);
                let _ = tx.send(OpResult::ShareCreated(request.clone(), title, result));
            }
        });
    }

    pub(super) fn handle_my_shares_key(
        &mut self,
        code: KeyCode,
        shares: &mut Vec<crate::pikpak::MyShare>,
        selected: &mut usize,
        confirm_delete: &mut Option<String>,
    ) {
        if self.loading
            && (matches!(code, KeyCode::Char('d' | 'x' | 'r'))
                || (confirm_delete.is_some()
                    && matches!(code, KeyCode::Enter | KeyCode::Char('y'))))
        {
            self.input = InputMode::MySharesView {
                shares: std::mem::take(shares),
                selected: *selected,
                confirm_delete: confirm_delete.take(),
            };
            return;
        }
        if confirm_delete.is_some() {
            match code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    let Some(share_id) = confirm_delete.take() else {
                        return;
                    };
                    let client = Arc::clone(&self.client);
                    let tx = self.result_tx.clone();
                    self.loading = true;
                    // Restore mode before spawning so the view stays visible during load
                    let owned_shares = std::mem::take(shares);
                    let sel = *selected;
                    self.input = InputMode::MySharesView {
                        shares: owned_shares,
                        selected: sel,
                        confirm_delete: None,
                    };
                    let request = self.begin_modal_request(AsyncRequestKind::MyShares, "my-shares");
                    std::thread::spawn(move || {
                        let result = client
                            .delete_shares(&[share_id.as_str()])
                            .and_then(|()| client.list_shares());
                        let _ = tx.send(OpResult::MyShares(request, result));
                    });
                }
                _ => {
                    *confirm_delete = None;
                    let owned_shares = std::mem::take(shares);
                    let sel = *selected;
                    self.input = InputMode::MySharesView {
                        shares: owned_shares,
                        selected: sel,
                        confirm_delete: None,
                    };
                }
            }
            return;
        }

        match code {
            KeyCode::Esc => {
                self.invalidate_modal_request();
                self.finish_loading();
                self.input = InputMode::Normal;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !shares.is_empty() {
                    *selected = (*selected + 1).min(shares.len() - 1);
                }
                let owned = std::mem::take(shares);
                let sel = *selected;
                self.input = InputMode::MySharesView {
                    shares: owned,
                    selected: sel,
                    confirm_delete: None,
                };
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if *selected > 0 {
                    *selected -= 1;
                }
                let owned = std::mem::take(shares);
                let sel = *selected;
                self.input = InputMode::MySharesView {
                    shares: owned,
                    selected: sel,
                    confirm_delete: None,
                };
            }
            KeyCode::Char('y') => {
                if let Some(share) = shares.get(*selected) {
                    let url = share.share_url.clone();
                    match write_clipboard(&url) {
                        Ok(()) => {
                            self.push_log(format!("Copied: {url}"));
                            self.show_logs_overlay = true;
                        }
                        Err(e) => {
                            self.push_log(format!("Clipboard failed: {e:#}"));
                            self.show_logs_overlay = true;
                        }
                    }
                }
                let owned = std::mem::take(shares);
                let sel = *selected;
                self.input = InputMode::MySharesView {
                    shares: owned,
                    selected: sel,
                    confirm_delete: None,
                };
            }
            KeyCode::Char('l') => {
                self.show_logs_overlay = !self.show_logs_overlay;
                let owned = std::mem::take(shares);
                let sel = *selected;
                self.input = InputMode::MySharesView {
                    shares: owned,
                    selected: sel,
                    confirm_delete: None,
                };
            }
            KeyCode::Char('d') | KeyCode::Char('x') => {
                if let Some(share) = shares.get(*selected) {
                    let id = share.share_id.clone();
                    let owned = std::mem::take(shares);
                    let sel = *selected;
                    self.input = InputMode::MySharesView {
                        shares: owned,
                        selected: sel,
                        confirm_delete: Some(id),
                    };
                } else {
                    let owned = std::mem::take(shares);
                    let sel = *selected;
                    self.input = InputMode::MySharesView {
                        shares: owned,
                        selected: sel,
                        confirm_delete: None,
                    };
                }
            }
            KeyCode::Char('r') => {
                self.loading = true;
                self.loading_label = Some("Loading shares...".into());
                let client = Arc::clone(&self.client);
                let tx = self.result_tx.clone();
                let sel = *selected;
                self.input = InputMode::MySharesView {
                    shares: std::mem::take(shares),
                    selected: sel,
                    confirm_delete: None,
                };
                let request = self.begin_modal_request(AsyncRequestKind::MyShares, "my-shares");
                std::thread::spawn(move || {
                    let _ = tx.send(OpResult::MyShares(request, client.list_shares()));
                });
            }
            _ => {
                let owned = std::mem::take(shares);
                let sel = *selected;
                self.input = InputMode::MySharesView {
                    shares: owned,
                    selected: sel,
                    confirm_delete: None,
                };
            }
        }
    }

    /// Log every result, but return a clipboard URL only while this request owns the view.
    pub(super) fn record_share_created(
        &mut self,
        request: AsyncRequest,
        title: String,
        result: Result<CreateShareResponse>,
    ) -> Option<String> {
        let active = self.modal_request_matches(&request)
            && matches!(self.input, InputMode::ShareCreatedView { .. });
        let url = match result {
            Ok(response) => {
                self.push_status_log(
                    format!("Share created: {}", response.share_url),
                    StatusKind::Info,
                );
                let url = response.share_url;
                if active && let InputMode::ShareCreatedView { shares } = &mut self.input {
                    shares.push((title, url.clone(), response.pass_code));
                }
                active.then_some(url)
            }
            Err(error) => {
                self.push_status_log(
                    format!("Share failed for '{title}': {error:#}"),
                    StatusKind::Error,
                );
                None
            }
        };
        if active {
            self.share_creations_pending = self.share_creations_pending.saturating_sub(1);
            if self.share_creations_pending == 0 {
                self.modal_request = None;
                self.finish_loading();
            }
        }
        url
    }

    pub(super) fn apply_my_shares(&mut self, request: AsyncRequest, result: Result<Vec<MyShare>>) {
        if !self.modal_request_matches(&request)
            || !matches!(self.input, InputMode::MySharesView { .. })
        {
            return;
        }
        self.modal_request = None;
        self.finish_loading();
        match result {
            Ok(shares) => {
                let (old_index, old_id) = match &self.input {
                    InputMode::MySharesView {
                        shares, selected, ..
                    } => (*selected, shares.get(*selected).map(|s| s.share_id.clone())),
                    _ => unreachable!(),
                };
                let selected = old_id
                    .and_then(|id| shares.iter().position(|s| s.share_id == id))
                    .unwrap_or_else(|| old_index.min(shares.len().saturating_sub(1)));
                self.input = InputMode::MySharesView {
                    shares,
                    selected,
                    confirm_delete: None,
                };
            }
            Err(error) => {
                self.report_mutation_error("Shares", &error);
            }
        }
    }
}
