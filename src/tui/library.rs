use super::{App, AsyncRequestKind, InputMode, OpResult, handle_text_input};
use crate::pikpak::{Entry, EntryKind, PikPak, SearchQuery};
use anyhow::{Result, ensure};
use crossterm::event::{KeyCode, KeyModifiers};
use std::{collections::HashSet, sync::Arc};

#[derive(Default)]
pub(super) struct SearchForm {
    pub name: String,
    pub scoped: bool,
    pub kind: Option<EntryKind>,
    pub min_size: String,
    pub max_size: String,
    pub field: usize,
}

impl SearchForm {
    pub fn key(mut self, app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        match code {
            KeyCode::Tab => self.scoped = !self.scoped,
            KeyCode::Char('t') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.kind = match self.kind {
                    None => Some(EntryKind::File),
                    Some(EntryKind::File) => Some(EntryKind::Folder),
                    Some(EntryKind::Folder) => None,
                };
            }
            KeyCode::Down | KeyCode::Up => {
                self.field = (self.field + if code == KeyCode::Down { 1 } else { 2 }) % 3;
                app.text_cursor = usize::MAX;
            }
            _ => {
                let value = match self.field {
                    1 => &mut self.min_size,
                    2 => &mut self.max_size,
                    _ => &mut self.name,
                };
                match handle_text_input(value, &mut app.text_cursor, code, modifiers) {
                    Some(false) => return,
                    Some(true) => match self.query(&app.current_folder_id) {
                        Ok(query) => {
                            app.open_library(Source::Search(query));
                            return;
                        }
                        Err(e) => app.push_log(format!("{e:#}")),
                    },
                    None => {}
                }
            }
        }
        app.input = InputMode::SearchInput(self);
    }

    fn query(&self, current: &str) -> Result<SearchQuery> {
        let bytes = |s: &str| -> Result<Option<u64>> {
            (!s.trim().is_empty())
                .then(|| s.trim().parse::<u64>())
                .transpose()
                .map_err(|_| anyhow::anyhow!("Size must be a nonnegative whole number of bytes"))
        };
        let min_size = bytes(&self.min_size)?;
        let max_size = bytes(&self.max_size)?;
        ensure!(
            min_size.unwrap_or(0) <= max_size.unwrap_or(u64::MAX),
            "Minimum size exceeds maximum size"
        );
        ensure!(
            !self.name.trim().is_empty()
                || self.kind.is_some()
                || min_size.is_some()
                || max_size.is_some(),
            "Enter a filename or select a filter"
        );
        Ok(SearchQuery {
            name: self.name.trim().to_owned(),
            scope_id: if self.scoped {
                current.into()
            } else {
                String::new()
            },
            kind: self.kind.clone(),
            min_size,
            max_size,
            limit: usize::MAX,
            ..Default::default()
        })
    }
}

#[derive(Clone)]
pub(super) enum Source {
    Search(SearchQuery),
    Starred,
    Events { playback: bool },
    Statuses(String),
    Links,
    Receipts,
}
impl Source {
    pub fn title(&self) -> &str {
        match self {
            Self::Search(_) => "Search results",
            Self::Starred => "Starred",
            Self::Events { playback: true } => "Recently watched",
            Self::Events { playback: false } => "Recent activity",
            Self::Statuses(_) => "Task details",
            Self::Links => "Choose cloud downloads",
            Self::Receipts => "Cloud download results",
        }
    }
}

pub(super) struct Row {
    pub label: String,
    id: Option<String>,
    entry: Option<Entry>,
    url: Option<String>,
    pub checked: bool,
}
impl Row {
    fn label(text: String) -> Self {
        Self {
            label: text,
            id: None,
            entry: None,
            url: None,
            checked: false,
        }
    }

    fn drive(label: String, entry: Entry) -> Self {
        Self {
            id: Some(entry.id.clone()),
            entry: Some(entry),
            ..Self::label(label)
        }
    }
}

pub(super) struct Page {
    pub source: Source,
    pub rows: Vec<Row>,
    pub selected: usize,
    pub next: Option<String>,
    seen: HashSet<String>,
}

impl Page {
    pub fn in_cart(&self, i: usize, cart: &HashSet<String>) -> bool {
        self.rows
            .get(i)
            .and_then(|r| r.entry.as_ref())
            .is_some_and(|e| cart.contains(&e.id))
    }

    fn new(source: Source) -> Self {
        Self {
            source,
            rows: Vec::new(),
            selected: 0,
            next: None,
            seen: HashSet::new(),
        }
    }

    fn fetch(mut self, client: &PikPak) -> Result<Self> {
        let token = self.next.take();
        let (rows, next): (Vec<Row>, Option<String>) = match &self.source {
            Source::Search(query) => {
                let hits = client.search_drive(query, &false.into())?;
                (
                    hits.into_iter()
                        .map(|h| {
                            Row::drive(
                                h.path,
                                Entry {
                                    id: h.id,
                                    name: h.name,
                                    kind: h.kind,
                                    size: h.size,
                                    starred: false,
                                    thumbnail_link: None,
                                    phase: None,
                                    audit: None,
                                    created_time: String::new(),
                                    modified_time: String::new(),
                                },
                            )
                        })
                        .collect(),
                    None,
                )
            }
            Source::Starred => {
                let (items, next) = client.starred_page(100, token.as_deref())?;
                (
                    items
                        .into_iter()
                        .map(|e| Row::drive(e.name.clone(), e))
                        .collect(),
                    next,
                )
            }
            Source::Events { playback } => {
                let page = client.events_page(100, *playback, token.as_deref())?;
                let rows = page
                    .events
                    .into_iter()
                    .map(|e| {
                        let resource = e.reference_resource.as_ref();
                        let name = e.display_name();
                        let detail = e
                            .type_name
                            .as_deref()
                            .or(e.event_type.as_deref())
                            .unwrap_or("");
                        let time = e.created_time.as_deref().unwrap_or("");
                        let position = e
                            .playback_position()
                            .map(|v| format!("  [{v}]"))
                            .unwrap_or_default();
                        let label = format!("{name}{position}   {detail}   {time}");
                        Row {
                            id: e
                                .file_id
                                .or_else(|| resource?.id.clone())
                                .filter(|id| !id.is_empty()),
                            ..Row::label(label)
                        }
                    })
                    .collect();
                (rows, page.next_page_token.filter(|t| !t.is_empty()))
            }
            Source::Statuses(id) => {
                let (statuses, next) = client.task_status_page(id, token.as_deref())?;
                (
                    statuses
                        .into_iter()
                        .map(|s| Row {
                            id: s["file_id"].as_str().map(str::to_owned),
                            ..Row::label(format!(
                                "{}   {}",
                                s["name"]
                                    .as_str()
                                    .or(s["file_name"].as_str())
                                    .unwrap_or("?"),
                                s["phase"].as_str().unwrap_or("")
                            ))
                        })
                        .collect(),
                    next,
                )
            }
            _ => anyhow::bail!("no next page"),
        };
        if let Some(next) = &next {
            ensure!(self.seen.insert(next.clone()), "repeated page token");
        }
        if token.is_some() {
            self.selected = self.rows.len();
        }
        self.rows.extend(rows);
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
        self.next = next;
        Ok(self)
    }
}

pub(super) enum ResultView {
    Page(Page),
    Submitted(Page),
    Location(String, Vec<(String, String)>, String),
}

impl App {
    fn library_job(
        &mut self,
        label: &'static str,
        work: impl FnOnce(&PikPak) -> Result<ResultView> + Send + 'static,
    ) {
        let request = self.begin_modal_request(AsyncRequestKind::Library, label);
        self.loading = true;
        self.loading_label = Some(label.into());
        self.input = InputMode::LibraryPending;
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(OpResult::Library(request, work(&client)));
        });
    }

    pub(super) fn open_library(&mut self, source: Source) {
        self.load_library_page(Page::new(source));
    }

    fn load_library_page(&mut self, page: Page) {
        self.library_job("Loading list…", move |client| {
            Ok(ResultView::Page(page.fetch(client)?))
        });
    }

    pub(super) fn locate_library_file(&mut self, id: String) {
        self.library_job("Locating file…", move |client| {
            let (folder, crumbs) = client.locate_file(&id)?;
            Ok(ResultView::Location(folder, crumbs, id))
        });
    }

    pub(super) fn preview_offline_links(&mut self, input: String) {
        self.library_job("Previewing links…", move |client| {
            let mut page = Page::new(Source::Links);
            let mut seen = HashSet::new();
            let urls: Vec<_> = input
                .split_whitespace()
                .filter(|u| seen.insert(*u))
                .map(str::to_owned)
                .collect();
            for (url, parsed) in urls.iter().zip(client.preview_resources(&urls)) {
                let (detail, recognized) = match parsed {
                    Ok(parsed) => {
                        let first = parsed.get("list").unwrap_or(&parsed)["resources"].get(0);
                        match first {
                            Some(r) => {
                                let size = r["file_size"]
                                    .as_u64()
                                    .or_else(|| r["file_size"].as_str()?.parse().ok());
                                let name = r["name"].as_str().unwrap_or("Resource");
                                (
                                    format!(
                                        "{name}  {}",
                                        size.map(crate::cmd::format_size).unwrap_or_default()
                                    ),
                                    true,
                                )
                            }
                            None => ("Unrecognized".into(), false),
                        }
                    }
                    Err(e) => (format!("Preview failed: {e:#}"), false),
                };
                page.rows.push(Row {
                    url: Some(url.into()),
                    checked: recognized,
                    ..Row::label(format!("{detail}   {url}"))
                });
            }
            Ok(ResultView::Page(page))
        });
    }

    fn submit_library_links(&mut self, urls: Vec<String>) {
        let folder = (!self.current_folder_id.is_empty()).then(|| self.current_folder_id.clone());
        let guard = super::operations::WriteGuard::new(&self.background_writes);
        self.library_job("Submitting selected links…", move |client| {
            let _guard = guard;
            let mut page = Page::new(Source::Receipts);
            for url in urls {
                let status = match client.offline_download(&url, folder.as_deref(), None) {
                    Ok(result) => result
                        .task
                        .map(|t| format!("{}   {}", t.phase, t.id))
                        .unwrap_or_else(|| "Submitted".into()),
                    Err(e) => format!("Failed: {e:#}"),
                };
                page.rows.push(Row::label(format!("{status}   {url}")));
            }
            Ok(ResultView::Submitted(page))
        });
    }

    pub(super) fn handle_library_key(&mut self, mut page: Page, key: KeyCode) {
        match key {
            KeyCode::Esc => return,
            KeyCode::Down | KeyCode::Char('j') => {
                page.selected = (page.selected + 1).min(page.rows.len().saturating_sub(1))
            }
            KeyCode::Up | KeyCode::Char('k') => page.selected = page.selected.saturating_sub(1),
            KeyCode::PageDown => {
                page.selected = (page.selected + 12).min(page.rows.len().saturating_sub(1))
            }
            KeyCode::PageUp => page.selected = page.selected.saturating_sub(12),
            KeyCode::Char('n') if page.next.is_some() => {
                self.load_library_page(page);
                return;
            }
            KeyCode::Char(' ') if matches!(page.source, Source::Links) => {
                if let Some(row) = page.rows.get_mut(page.selected) {
                    row.checked = !row.checked;
                }
            }
            KeyCode::Enter if matches!(page.source, Source::Links) => {
                let urls: Vec<_> = page
                    .rows
                    .iter()
                    .filter(|r| r.checked)
                    .filter_map(|r| r.url.clone())
                    .collect();
                if !urls.is_empty() {
                    self.submit_library_links(urls);
                    return;
                }
                self.push_log("Select at least one link".into());
            }
            KeyCode::Enter => {
                if let Some(id) = page.rows.get(page.selected).and_then(|r| r.id.clone()) {
                    self.locate_library_file(id);
                    return;
                }
            }
            KeyCode::Char('a') => {
                if let Some(e) = page.rows.get(page.selected).and_then(|r| r.entry.as_ref()) {
                    self.toggle_cart_entry(e.clone());
                }
            }
            _ => {}
        }
        self.input = InputMode::Library(page);
    }
}
