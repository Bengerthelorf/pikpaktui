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
    pub(super) fn handle_share_prompt_key(
        &mut self,
        code: KeyCode,
        mut grouped: bool,
        mut days: String,
    ) {
        match code {
            KeyCode::Esc => {
                self.input = InputMode::CartView;
                return;
            }
            KeyCode::Char('g') => grouped = !grouped,
            KeyCode::Char('p' | 'P') => {
                let expiry = if days.is_empty() {
                    -1
                } else if let Ok(n) = days.parse::<i64>() {
                    n
                } else {
                    self.push_status_log("Enter a valid number of days".into(), StatusKind::Error);
                    self.input = InputMode::SharePrompt { grouped, days };
                    return;
                };
                self.spawn_create_shares(code == KeyCode::Char('P'), grouped, expiry);
                return;
            }
            KeyCode::Char('i') => {
                self.open_share_page(SharePage::Status {
                    from_cart: true,
                    files: self
                        .cart
                        .iter()
                        .map(|e| (e.id.clone(), e.name.clone()))
                        .collect(),
                });
                return;
            }
            KeyCode::Char(c) if c.is_ascii_digit() => days.push(c),
            KeyCode::Backspace => {
                days.pop();
            }
            _ => {}
        }
        self.input = InputMode::SharePrompt { grouped, days };
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
                    self.copy_to_clipboard(url.clone());
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

    pub(super) fn spawn_create_shares(&mut self, need_password: bool, grouped: bool, days: i64) {
        if self.cart.is_empty() {
            self.input = InputMode::CartView;
            return;
        }
        self.input = InputMode::ShareCreatedView { shares: vec![] };
        self.loading = true;
        self.loading_label = Some("Creating shares...".into());
        let request = self.begin_modal_request(AsyncRequestKind::ShareCreate, "create-shares");
        self.share_creations_pending = if grouped { 1 } else { self.cart.len() };
        // One worker queues the batch instead of starting a thread per item.
        let jobs: Vec<_> = self
            .cart
            .iter()
            .map(|entry| (entry.id.clone(), entry.name.clone()))
            .collect();
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        let guard = super::operations::WriteGuard::new(&self.background_writes);
        std::thread::spawn(move || {
            let _guard = guard;
            let batch = if grouped { jobs.len() } else { 1 };
            for group in jobs.chunks(batch) {
                let ids: Vec<_> = group.iter().map(|(id, _)| id.as_str()).collect();
                let title = if group.len() == 1 {
                    group[0].1.clone()
                } else {
                    format!("{} cart items", group.len())
                };
                let result = client.create_share(&ids, need_password, days);
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
                    let guard = super::operations::WriteGuard::new(&self.background_writes);
                    std::thread::spawn(move || {
                        let _guard = guard;
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

        if !self.loading {
            match code {
                KeyCode::Char('f') => {
                    self.open_share_page(SharePage::Following);
                    return;
                }
                KeyCode::Char('v') => {
                    self.open_share_page(SharePage::Updates(None));
                    return;
                }
                KeyCode::Char('c') => {
                    self.share_field(ShareField::Code, String::new());
                    return;
                }
                _ => {}
            }
            if let Some(share) = shares.get(*selected) {
                let id = share.share_id.clone();
                match code {
                    KeyCode::Char('b') => {
                        self.share_field(ShareField::FollowSetting(id), String::new());
                        return;
                    }
                    KeyCode::Char('e') => {
                        self.share_field(ShareField::Expiry(id), String::new());
                        return;
                    }
                    KeyCode::Char('p') => {
                        self.share_field(ShareField::Password(id), String::new());
                        return;
                    }
                    KeyCode::Char('Y') => {
                        self.share_field(
                            ShareField::Link(share.share_url.clone()),
                            "list name_asc".into(),
                        );
                        return;
                    }
                    KeyCode::Char('C') => {
                        self.open_share_page(SharePage::CodeInfo(id));
                        return;
                    }
                    _ => {}
                }
            }
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
                    self.copy_to_clipboard(url);
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
        let edited_id = request.target.strip_prefix("edit-share:");
        if !self.modal_request_matches(&request)
            || !matches!(self.input, InputMode::MySharesView { .. })
        {
            if edited_id.is_some() {
                match result {
                    Ok(_) => self.push_status_log("Share updated".to_owned(), StatusKind::Info),
                    Err(error) => {
                        self.report_mutation_error("Edit share", &error);
                    }
                }
            }
            return;
        }
        self.modal_request = None;
        self.finish_loading();
        match result {
            Ok(shares) => {
                if edited_id.is_some() {
                    self.push_status_log("Share updated".to_owned(), StatusKind::Info);
                }
                let (old_index, old_id) = match &self.input {
                    InputMode::MySharesView {
                        shares, selected, ..
                    } => (*selected, shares.get(*selected).map(|s| s.share_id.clone())),
                    _ => unreachable!(),
                };
                let selected = edited_id
                    .or(old_id.as_deref())
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

use crate::pikpak::share_manage::{ShareFeed, share_expiry, share_link};
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) enum ShareField {
    Expiry(String),
    FollowSetting(String),
    Password(String),
    Link(String),
    Code,
    SetCode {
        id: String,
        old: Option<String>,
        remove: bool,
    },
    Unfollow(String),
    Follow,
    Save {
        share: String,
        record: u64,
        ids: Vec<String>,
        destination: Option<String>,
    },
}

impl ShareField {
    pub(super) fn title(&self) -> &'static str {
        match self {
            Self::Expiry(_) => "Expiration (RFC 3339 timestamp or -1 for permanent)",
            Self::Password(_) => "New share password (enter - to remove it)",
            Self::Link(_) => "Copy link: view and order (e.g. grid mod_desc)",
            Self::FollowSetting(_) => "Can recipients follow this share? Enter yes or no",
            Self::Code => "Resolve share code",
            Self::SetCode { remove: true, .. } => "Type DELETE to remove this share code",
            Self::SetCode { .. } => "Enter a new share code",
            Self::Unfollow(_) => "Type UNFOLLOW to stop following this share",
            Self::Follow => "Follow share URL (include pass_code for protected shares)",
            Self::Save {
                destination: None, ..
            } => "Destination folder in your drive",
            Self::Save { .. } => {
                "Enter share passcode if needed, then Enter to save selected files"
            }
        }
    }
}

#[derive(Clone)]
pub(super) enum SharePage {
    Following,
    Updates(Option<String>),
    Files(String, u64),
    Status {
        files: Vec<(String, String)>,
        from_cart: bool,
    },
    Codes(String),
    CodeInfo(String),
}

impl SharePage {
    pub(super) fn title(&self) -> &'static str {
        match self {
            Self::Following => "Followed shares",
            Self::Updates(_) => "Share updates",
            Self::Files(..) => "Files in selected update",
            Self::Status { .. } => "Sharing status",
            Self::Codes(_) => "Share code results",
            Self::CodeInfo(_) => "Share code details",
        }
    }
    fn load(&self, client: &crate::pikpak::PikPak) -> Result<Vec<Value>> {
        Ok(match self {
            Self::Following => client.share_feed(ShareFeed::Following)?,
            Self::Updates(id) => client.share_feed(ShareFeed::Updates(id.as_deref()))?,
            Self::Files(_, record) => client
                .update_file_ids(*record)?
                .into_iter()
                .map(Value::String)
                .collect(),
            Self::Codes(phrase) => client
                .resolve_share_code(phrase)?
                .into_iter()
                .map(Value::String)
                .collect(),
            Self::Status { files, .. } => {
                let ids = files.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>();
                let names: std::collections::HashMap<_, _> = files
                    .iter()
                    .map(|(id, name)| (id.as_str(), name.as_str()))
                    .collect();
                let mut rows: Vec<Value> = serde_json::from_value(client.share_file_status(&ids)?)?;
                for row in &mut rows {
                    if let Some(name) = row["file_id"].as_str().and_then(|id| names.get(id)) {
                        row["name"] = Value::String((*name).to_owned());
                    }
                }
                rows
            }
            Self::CodeInfo(id) => {
                let info = client.share_code_info(id)?;
                if info["data"].is_null() {
                    return Ok(vec![]);
                }
                let codes = info["data"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("missing share code list"))?;
                codes
                    .iter()
                    .map(|r| {
                        r["phrase"]
                            .as_str()
                            .map(|s| Value::String(s.to_owned()))
                            .ok_or_else(|| anyhow::anyhow!("invalid share code"))
                    })
                    .collect::<Result<_>>()?
            }
        })
    }
    pub(super) fn label(&self, row: &Value) -> String {
        let text = |key: &str| row[key].as_str().unwrap_or("");
        match self {
            Self::Status { .. } => format!(
                "{}  {}  {}",
                row["name"].as_str().unwrap_or_else(|| text("file_id")),
                text("share_status"),
                text("share_id")
            ),
            Self::Following => format!(
                "{}  {}  {}",
                text("share_name"),
                text("share_id"),
                text("share_status")
            ),
            Self::Updates(_) => {
                let folder = if text("folder_path").is_empty() {
                    text("folder_name")
                } else {
                    text("folder_path")
                };
                let count = row
                    .get("file_count")
                    .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
                    .map(|count| format!("  {count} files"))
                    .unwrap_or_default();
                format!(
                    "{}  {}{}  {}  #{}",
                    text("share_title"),
                    folder,
                    count,
                    text("updated_at"),
                    row["auto_id"]
                )
            }
            _ => row
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| row.to_string()),
        }
    }
}

impl App {
    fn share_field(&mut self, field: ShareField, value: String) {
        self.text_cursor = value.len();
        self.input = InputMode::ShareEdit { field, value };
    }

    pub(super) fn handle_share_edit(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        field: ShareField,
        mut value: String,
    ) {
        match super::handle_text_input(&mut value, &mut self.text_cursor, code, modifiers) {
            None => {
                self.input = InputMode::ShareEdit { field, value };
                return;
            }
            Some(false) => {
                match field {
                    ShareField::Save {
                        share,
                        record,
                        ids,
                        destination: Some(path),
                    } => self.share_field(
                        ShareField::Save {
                            share,
                            record,
                            ids,
                            destination: None,
                        },
                        path,
                    ),
                    ShareField::Save { share, record, .. } => {
                        self.open_share_page(SharePage::Files(share, record))
                    }
                    ShareField::SetCode { id, .. } => self.open_share_page(SharePage::CodeInfo(id)),
                    ShareField::Follow | ShareField::Unfollow(_) => {
                        self.open_share_page(SharePage::Following)
                    }
                    _ => self.open_my_shares_view(),
                }
                return;
            }
            Some(true) => {}
        }
        if let ShareField::Save {
            share,
            record,
            ids,
            destination: None,
        } = field
        {
            let path = if value.trim().is_empty() {
                "/".to_owned()
            } else {
                value
            };
            self.share_field(
                ShareField::Save {
                    share,
                    record,
                    ids,
                    destination: Some(path),
                },
                String::new(),
            );
            return;
        }
        match &field {
            ShareField::Link(url) => {
                let fields: Vec<_> = value.split_whitespace().collect();
                let result = match &fields[..] {
                    [view, order] => share_link(url, Some(view), Some(order)),
                    [view] => share_link(url, Some(view), None),
                    _ => Err(anyhow::anyhow!(
                        "Enter a view (list/grid/flow) and optional order"
                    )),
                };
                match result {
                    Ok(link) => {
                        self.copy_to_clipboard(link);
                        self.open_my_shares_view();
                    }
                    Err(error) => {
                        self.push_status_log(error.to_string(), StatusKind::Error);
                        self.share_field(field, value);
                    }
                }
                return;
            }
            ShareField::Code => {
                self.open_share_page(SharePage::Codes(value));
                return;
            }
            ShareField::Expiry(_) => {
                if let Err(error) = share_expiry(&value) {
                    self.push_status_log(error.to_string(), StatusKind::Error);
                    self.share_field(field, value);
                    return;
                }
            }
            ShareField::SetCode { remove: true, .. } if value != "DELETE" => {
                self.push_status_log("Type DELETE to confirm".into(), StatusKind::Error);
                self.share_field(field, value);
                return;
            }
            ShareField::SetCode { remove: false, .. } if value.trim().is_empty() => {
                self.push_status_log("Enter a nonempty code".into(), StatusKind::Error);
                self.share_field(field, value);
                return;
            }
            ShareField::Unfollow(_) if value != "UNFOLLOW" => {
                self.push_status_log("Type UNFOLLOW to confirm".into(), StatusKind::Error);
                self.share_field(field, value);
                return;
            }
            ShareField::FollowSetting(_) if !matches!(value.as_str(), "yes" | "no") => {
                self.push_status_log("Enter yes or no".into(), StatusKind::Error);
                self.share_field(field, value);
                return;
            }
            ShareField::Password(_) if value.is_empty() => {
                self.push_status_log(
                    "Enter a password or - to remove it".into(),
                    StatusKind::Error,
                );
                self.share_field(field, value);
                return;
            }
            _ => {}
        }
        let page = match &field {
            ShareField::Follow | ShareField::Unfollow(_) => SharePage::Following,
            ShareField::SetCode { id, .. } => SharePage::CodeInfo(id.clone()),
            ShareField::Save { share, record, .. } => SharePage::Files(share.clone(), *record),
            _ => {
                self.input = InputMode::MySharesView {
                    shares: vec![],
                    selected: 0,
                    confirm_delete: None,
                };
                self.loading = true;
                self.loading_label = Some("Editing share...".into());
                let id = match &field {
                    ShareField::FollowSetting(id)
                    | ShareField::Expiry(id)
                    | ShareField::Password(id) => id,
                    _ => unreachable!(),
                };
                let request = self
                    .begin_modal_request(AsyncRequestKind::MyShares, format!("edit-share:{id}"));
                let client = Arc::clone(&self.client);
                let tx = self.result_tx.clone();
                let guard = super::operations::WriteGuard::new(&self.background_writes);
                std::thread::spawn(move || {
                    let _guard = guard;
                    let result = match field {
                        ShareField::FollowSetting(id) => {
                            client.edit_share(&id, None, None, Some(value == "no"))
                        }
                        ShareField::Expiry(id) => client.edit_share(&id, Some(&value), None, None),
                        ShareField::Password(id) => client.edit_share(
                            &id,
                            None,
                            Some(if value == "-" { "" } else { &value }),
                            None,
                        ),
                        _ => unreachable!(),
                    }
                    .and_then(|()| {
                        client.list_shares().map_err(|e| {
                            e.context("Share updated, but the list could not be refreshed")
                        })
                    });
                    let _ = tx.send(OpResult::MyShares(request, result));
                });
                return;
            }
        };
        let notice = match &field {
            ShareField::Save { .. } => "Saved selected files to destination folder",
            ShareField::SetCode { .. } => "Share code updated",
            ShareField::Unfollow(_) => "Unfollowed share",
            _ => "Followed share",
        };
        self.run_share_page(
            page,
            move |client| match field {
                ShareField::SetCode { id, old, remove } => {
                    client.set_share_code(&id, old.as_deref(), if remove { "" } else { &value })
                }
                ShareField::Unfollow(id) => client.set_share_followed(&id, "", false),
                ShareField::Follow => {
                    let location = crate::pikpak::ShareLocation::parse(&value)?;
                    client.set_share_followed(
                        &location.id,
                        location.pass_code.as_deref().unwrap_or(""),
                        true,
                    )
                }
                ShareField::Save {
                    share,
                    record,
                    ids,
                    destination: Some(path),
                } => {
                    let parent = client.resolve_folder(&path)?;
                    client.save_update_selection(
                        &share,
                        record,
                        &ids.iter().map(String::as_str).collect::<Vec<_>>(),
                        &value,
                        &parent,
                        false,
                    )
                }
                _ => unreachable!(),
            },
            Some(notice),
        );
    }

    pub(super) fn show_selected_share_status(&mut self) {
        if let Some(entry) = self.current_entry() {
            let files = vec![(entry.id.clone(), entry.name.clone())];
            self.open_share_page(SharePage::Status {
                files,
                from_cart: false,
            });
        }
    }

    fn open_share_page(&mut self, page: SharePage) {
        self.run_share_page(page, |_| Ok(()), None);
    }
    fn run_share_page(
        &mut self,
        page: SharePage,
        action: impl FnOnce(&crate::pikpak::PikPak) -> Result<()> + Send + 'static,
        notice: Option<&'static str>,
    ) {
        self.loading = true;
        self.loading_label = Some(page.title().into());
        let request = self.begin_modal_request(AsyncRequestKind::SharePage, page.title());
        self.input = InputMode::SharePage {
            page: page.clone(),
            rows: vec![],
            selected: 0,
            marked: BTreeSet::new(),
        };
        let tx = self.result_tx.clone();
        let client = Arc::clone(&self.client);
        let guard = notice.map(|_| super::operations::WriteGuard::new(&self.background_writes));
        std::thread::spawn(move || {
            let _guard = guard;
            let result = action(&client).and_then(|()| {
                let result = page.load(&client);
                match notice {
                    Some(done) => result.map_err(|e| {
                        e.context(format!("{done}, but the list could not be refreshed"))
                    }),
                    None => result,
                }
            });
            let _ = tx.send(OpResult::SharePage(request, notice, result));
        });
    }
    pub(super) fn apply_share_page(
        &mut self,
        request: AsyncRequest,
        notice: Option<&str>,
        result: Result<Vec<Value>>,
    ) {
        if !self.modal_request_matches(&request) {
            if let Some(done) = notice {
                match result {
                    Ok(_) => self.push_status_log(done.to_owned(), StatusKind::Info),
                    Err(error) => {
                        self.report_mutation_error("Share", &error);
                    }
                }
            }
            return;
        }
        self.modal_request = None;
        self.finish_loading();
        match result {
            Ok(items) => {
                if let Some(done) = notice {
                    self.push_status_log(done.to_owned(), StatusKind::Info);
                }
                if let InputMode::SharePage { rows, .. } = &mut self.input {
                    *rows = items;
                }
            }
            Err(e) => {
                self.report_mutation_error("Share", &e);
            }
        }
    }

    pub(super) fn handle_share_page(
        &mut self,
        code: KeyCode,
        page: SharePage,
        rows: Vec<Value>,
        mut selected: usize,
        mut marked: BTreeSet<usize>,
    ) {
        let current = rows.get(selected);
        match code {
            KeyCode::Esc => {
                self.invalidate_modal_request();
                self.finish_loading();
                match &page {
                    SharePage::Files(id, _) => {
                        self.open_share_page(SharePage::Updates(Some(id.clone())))
                    }
                    SharePage::Updates(Some(_)) => self.open_share_page(SharePage::Following),
                    SharePage::Status { from_cart, .. } => {
                        self.input = if *from_cart {
                            InputMode::CartView
                        } else {
                            InputMode::Normal
                        }
                    }
                    _ => self.open_my_shares_view(),
                }
                return;
            }
            _ if self.loading => {}
            KeyCode::Down | KeyCode::Char('j') => {
                selected = (selected + 1).min(rows.len().saturating_sub(1))
            }
            KeyCode::Up | KeyCode::Char('k') => selected = selected.saturating_sub(1),
            KeyCode::Char('r') => {
                self.open_share_page(page);
                return;
            }
            KeyCode::Char('a' | 'e' | 'd') if matches!(page, SharePage::CodeInfo(_)) => {
                if let SharePage::CodeInfo(id) = &page {
                    let old = if code == KeyCode::Char('a') {
                        None
                    } else {
                        current.and_then(Value::as_str).map(str::to_owned)
                    };
                    if code == KeyCode::Char('a') || old.is_some() {
                        self.share_field(
                            ShareField::SetCode {
                                id: id.clone(),
                                old,
                                remove: code == KeyCode::Char('d'),
                            },
                            String::new(),
                        );
                        return;
                    }
                }
            }
            KeyCode::Char('d') if matches!(page, SharePage::Following) => {
                if let Some(id) = current.and_then(|row| row["share_id"].as_str()) {
                    self.share_field(ShareField::Unfollow(id.into()), String::new());
                    return;
                }
            }
            KeyCode::Char('a') if matches!(page, SharePage::Following) => {
                self.share_field(ShareField::Follow, String::new());
                return;
            }
            KeyCode::Char(' ') if matches!(page, SharePage::Files(..)) && current.is_some() => {
                if !marked.remove(&selected) {
                    marked.insert(selected);
                }
            }
            KeyCode::Char('s') if !marked.is_empty() => {
                if let SharePage::Files(share, record) = &page {
                    let ids = marked
                        .iter()
                        .filter_map(|i| rows[*i].as_str().map(str::to_owned))
                        .collect();
                    self.share_field(
                        ShareField::Save {
                            share: share.clone(),
                            record: *record,
                            ids,
                            destination: None,
                        },
                        "/".into(),
                    );
                    return;
                }
            }
            KeyCode::Char('o')
                if matches!(
                    page,
                    SharePage::Following | SharePage::Updates(_) | SharePage::Codes(_)
                ) =>
            {
                if let Some(row) = current
                    && let Some(id) = row["share_id"].as_str().or(row.as_str())
                {
                    self.open_incoming_share(id);
                    return;
                }
            }
            KeyCode::Enter => {
                if let Some(row) = current {
                    match &page {
                        SharePage::Following => {
                            if let Some(id) = row["share_id"].as_str() {
                                self.open_share_page(SharePage::Updates(Some(id.to_owned())));
                                return;
                            }
                        }
                        SharePage::Updates(_) => {
                            if let (Some(id), Some(record)) = (
                                row["share_id"].as_str(),
                                row["auto_id"]
                                    .as_u64()
                                    .or_else(|| row["auto_id"].as_str()?.parse().ok()),
                            ) {
                                self.open_share_page(SharePage::Files(id.into(), record));
                                return;
                            }
                        }
                        SharePage::Codes(_) => {
                            if let Some(link) = row.as_str() {
                                self.open_incoming_share(link);
                                return;
                            }
                        }
                        _ => {}
                    }
                }
            }
            KeyCode::Char('y') => {
                if let Some(row) = current {
                    let text = row
                        .as_str()
                        .map(str::to_owned)
                        .or_else(|| {
                            row["share_id"]
                                .as_str()
                                .map(|id| format!("https://mypikpak.com/s/{id}"))
                        })
                        .unwrap_or_else(|| row.to_string());
                    self.copy_to_clipboard(text);
                }
            }
            _ => {}
        }
        self.input = InputMode::SharePage {
            page,
            rows,
            selected,
            marked,
        };
    }
}
