//! Pane planning, directory history and bounded loading for the file browser.
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

use crate::config::{ColumnCount, SortField, TuiConfig, sort_entries};
use crate::pikpak::{Entry, EntryKind};
use crate::theme::{self, FileCategory};

use super::{
    App, InputMode, OpResult, PreviewState, fetch_and_render_thumbnail, highlight_content,
};

const MAX_LOADS: usize = 3;
const MAX_DIRECTORIES: usize = 64;
const MAX_CACHED_ENTRIES: usize = 20_000;
const MAX_INLINE_IMAGES: usize = 256;
const MAX_FILE_PREVIEWS: usize = 24;
const CURSOR_SETTLE: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PaneKind {
    Active,
    Ancestor(usize),
    Preview(usize),
    Descendant(usize),
    Empty,
}

#[derive(Clone)]
pub(super) struct PreviewTarget {
    pub entry: Entry,
    /// Folders between the active directory and this entry's parent.
    pub path: Vec<Entry>,
}

#[derive(Clone, Copy)]
struct AutoLayout {
    width: u16,
    min_width: u16,
    count: usize,
    ready: bool,
}

#[derive(Clone, Debug)]
pub(super) struct RowHit {
    pub top: u16,
    pub height: u16,
    pub index: usize,
}

#[derive(Clone, Debug)]
pub(super) struct PaneHit {
    pub area: Rect,
    pub kind: PaneKind,
    pub rows: Vec<RowHit>,
}

impl PaneHit {
    pub fn row_at(&self, y: u16) -> Option<usize> {
        self.rows
            .iter()
            .find(|r| y >= r.top && y < r.top.saturating_add(r.height))
            .map(|r| r.index)
    }
}

pub(super) struct PanePlan {
    pub panes: Vec<PaneKind>,
    pub active: usize,
    pub requested: Option<usize>,
}

#[cfg(test)]
pub(super) fn pane_plan(
    config: &TuiConfig,
    width: u16,
    depth: usize,
    entries: &[Entry],
    selected: usize,
) -> PanePlan {
    let name_width = if config.columns == ColumnCount::Auto {
        directory_name_width(entries)
    } else {
        0
    };
    pane_plan_with_name_width(config, width, depth, entries.len(), selected, name_width)
}

pub(super) fn directory_name_width(entries: &[Entry]) -> usize {
    let mut widths: Vec<_> = entries.iter().map(|e| e.name.width()).collect();
    if widths.is_empty() {
        return 0;
    }
    let index = widths.len().saturating_sub(1) * 4 / 5;
    *widths.select_nth_unstable(index).1
}

pub(super) fn pane_plan_with_name_width(
    config: &TuiConfig,
    width: u16,
    depth: usize,
    entry_count: usize,
    selected: usize,
    name_width: usize,
) -> PanePlan {
    // Terminal layout uses character cells, not pixels. Unicode display width
    // includes CJK and emoji; a percentile prevents one huge filename from
    // collapsing an otherwise readable layout. Use the whole directory so
    // moving the cursor never changes the number of panes.
    let min_width = config.column_min_width.max(16) as usize;
    let fitting = (width as usize / min_width).max(1);
    let (count, requested) = match config.columns {
        ColumnCount::Fixed(n) => (n.max(1).min(fitting), Some(n.max(1))),
        ColumnCount::Auto => {
            let thumb_width = if config.inline_thumbnails {
                config.inline_thumbnail_size.dimensions().0 as usize + 1
            } else {
                0
            };
            let preferred = (name_width + 12 + thumb_width).clamp(min_width, min_width.max(56));
            ((width as usize / preferred).max(1).min(fitting), None)
        }
    };
    let preview_slots = usize::from(config.show_preview && count > 1);
    let active = depth.min(count - 1 - preview_slots);
    let start_depth = depth - active;
    let mut panes: Vec<_> = (start_depth..depth).map(PaneKind::Ancestor).collect();
    panes.push(PaneKind::Active);
    for offset in 0..count - active - 1 {
        let index = selected.saturating_add(offset);
        panes.push(if config.show_preview && index < entry_count {
            PaneKind::Preview(index)
        } else {
            PaneKind::Empty
        });
    }
    PanePlan {
        panes,
        active,
        requested,
    }
}

#[derive(Clone)]
pub(super) struct DirectorySnapshot {
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub scroll: Cell<usize>,
    pub name_width: usize,
    generation: u64,
    touched: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum LoadKey {
    Directory(String),
    InlineImage(String),
    File(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BrowserRequest {
    pub id: u64,
    pub generation: u64,
    pub key: LoadKey,
    name: String,
}

pub(super) enum BrowserPayload {
    Directory(Vec<Entry>),
    InlineImage(image::DynamicImage),
    File(Box<PreviewState>),
}

pub(super) struct Cached<T> {
    pub value: T,
    pub touched: Instant,
}

#[derive(Default)]
pub(super) struct BrowserState {
    pub directories: HashMap<String, DirectorySnapshot>,
    pub inline_images: HashMap<String, Cached<image::DynamicImage>>,
    pub inline_protocols: RefCell<HashMap<String, super::image_render::InlineImageProtocol>>,
    pub preview_protocols: RefCell<HashMap<String, super::image_render::InlineImageProtocol>>,
    pub file_previews: HashMap<String, Cached<PreviewState>>,
    pub thumbnail_previews: HashMap<String, Cached<PreviewState>>,
    pub preview_scrolls: HashMap<String, usize>,
    pub hits: RefCell<Vec<PaneHit>>,
    pub visible_images: RefCell<Vec<(String, String, Rect)>>,
    pub visible_directories: RefCell<HashSet<String>>,
    pub visible_files: RefCell<HashSet<String>>,
    pub visible_file_images: RefCell<HashSet<String>>,
    pub thumbnail_entries: RefCell<HashMap<String, Entry>>,
    preview_targets: RefCell<Vec<PreviewTarget>>,
    auto_layout: Cell<Option<AutoLayout>>,
    restore_selection: Option<(String, String)>,
    pub failed: HashSet<LoadKey>,
    last_clicked_entry: Option<String>,
    generation: u64,
    next_request: u64,
    in_flight: HashMap<u64, BrowserRequest>,
    obsolete: HashSet<u64>,
}

impl BrowserState {
    pub fn cache_directory(&mut self, id: String, entries: Vec<Entry>) {
        let old = self.directories.remove(&id);
        let selected = old
            .as_ref()
            .and_then(|s| s.entries.get(s.selected))
            .and_then(|selected| entries.iter().position(|e| e.id == selected.id))
            .unwrap_or(0);
        let scroll = old.map(|s| s.scroll.get()).unwrap_or(0);
        let name_width = directory_name_width(&entries);
        self.directories.insert(
            id,
            DirectorySnapshot {
                entries,
                selected,
                scroll: Cell::new(scroll),
                name_width,
                generation: self.generation,
                touched: Instant::now(),
            },
        );
        self.trim();
    }

    fn trim(&mut self) {
        while self.directories.len() > 1
            && (self.directories.len() > MAX_DIRECTORIES
                || self
                    .directories
                    .values()
                    .map(|d| d.entries.len())
                    .sum::<usize>()
                    > MAX_CACHED_ENTRIES)
        {
            let oldest = self
                .directories
                .iter()
                .filter(|(id, _)| !self.visible_directories.borrow().contains(*id))
                .min_by_key(|(_, d)| d.touched)
                .map(|(id, _)| id.clone());
            let Some(oldest) = oldest else {
                break;
            };
            self.directories.remove(&oldest);
        }
        // The visible working set must survive eviction. Otherwise several
        // large sibling folders (or many visible images) would reload forever.
        let images = self
            .visible_images
            .borrow()
            .iter()
            .map(|(url, _, _)| url.clone())
            .collect();
        trim_cache(&mut self.inline_images, MAX_INLINE_IMAGES, &images);
        self.inline_protocols
            .borrow_mut()
            .retain(|url, _| self.inline_images.contains_key(url));
        self.preview_protocols
            .borrow_mut()
            .retain(|id, _| self.visible_files.borrow().contains(id));
        trim_cache(
            &mut self.file_previews,
            MAX_FILE_PREVIEWS,
            &self.visible_files.borrow(),
        );
        trim_cache(
            &mut self.thumbnail_previews,
            MAX_FILE_PREVIEWS,
            &self.visible_file_images.borrow(),
        );
        self.preview_scrolls.retain(|id, _| {
            self.file_previews.contains_key(id) || self.directories.contains_key(id)
        });
    }

    pub fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.file_previews.clear();
        self.thumbnail_previews.clear();
        self.inline_images.clear();
        self.inline_protocols.borrow_mut().clear();
        self.preview_protocols.borrow_mut().clear();
        self.failed.clear();
        // Keep running workers counted until they finish. Clearing them here
        // would let repeated refreshes exceed the concurrency limit.
    }

    pub fn cancel_directory_loads(&mut self, id: &str) {
        self.obsolete.extend(
            self.in_flight
                .values()
                .filter(|request| request.key == LoadKey::Directory(id.to_owned()))
                .map(|request| request.id),
        );
    }

    pub fn take_restored_selection(&mut self, folder_id: &str) -> Option<String> {
        self.restore_selection
            .take()
            .filter(|(id, _)| id == folder_id)
            .map(|(_, entry)| entry)
    }

    pub fn resort(&mut self, field: SortField, reverse: bool) {
        for snapshot in self.directories.values_mut() {
            let selected_id = snapshot
                .entries
                .get(snapshot.selected)
                .map(|e| e.id.clone());
            sort_entries(&mut snapshot.entries, field, reverse);
            snapshot.selected = selected_id
                .and_then(|id| snapshot.entries.iter().position(|e| e.id == id))
                .unwrap_or(0);
        }
    }

    fn needs_load(&self, key: &LoadKey) -> bool {
        if self.failed.contains(key) || self.in_flight.values().any(|r| &r.key == key) {
            return false;
        }
        match key {
            LoadKey::Directory(id) => !self
                .directories
                .get(id)
                .is_some_and(|s| s.generation == self.generation),
            LoadKey::InlineImage(url) => {
                !self.inline_images.contains_key(url)
                    || (self.visible_file_images.borrow().contains(url)
                        && !self.thumbnail_previews.contains_key(url))
            }
            LoadKey::File(id) => !self.file_previews.contains_key(id),
        }
    }

    fn begin_load(&mut self, key: LoadKey, name: String) -> Option<BrowserRequest> {
        if self.in_flight.len() >= MAX_LOADS || !self.needs_load(&key) {
            return None;
        }
        self.next_request = self.next_request.wrapping_add(1);
        let request = BrowserRequest {
            id: self.next_request,
            generation: self.generation,
            key,
            name,
        };
        self.in_flight.insert(request.id, request.clone());
        Some(request)
    }
}

fn trim_cache<T>(cache: &mut HashMap<String, Cached<T>>, limit: usize, visible: &HashSet<String>) {
    while cache.len() > limit {
        let oldest = cache
            .iter()
            .filter(|(id, _)| !visible.contains(*id))
            .min_by_key(|(_, d)| d.touched)
            .map(|(id, _)| id.clone());
        let Some(oldest) = oldest else {
            break;
        };
        cache.remove(&oldest);
    }
}

pub(super) fn inline_thumbnail_url(entry: &Entry) -> Option<&str> {
    (entry.kind == EntryKind::File)
        .then(|| {
            entry
                .thumbnail_link
                .as_deref()
                .filter(|url| !url.is_empty())
        })
        .flatten()
}

impl App {
    pub(super) fn browser_pane_plan(&self, width: u16) -> PanePlan {
        let name_width = self
            .browser
            .directories
            .get(&self.current_folder_id)
            .map(|s| s.name_width)
            .unwrap_or_else(|| directory_name_width(&self.entries));
        let mut config = self.config.clone();
        if config.columns == ColumnCount::Auto {
            let ready = !self.loading && !self.entries.is_empty();
            let cached = self.browser.auto_layout.get().filter(|layout| {
                layout.width == width
                    && layout.min_width == config.column_min_width
                    && (layout.ready || !ready)
            });
            let layout = cached.unwrap_or_else(|| AutoLayout {
                width,
                min_width: config.column_min_width,
                count: pane_plan_with_name_width(
                    &config,
                    width,
                    self.breadcrumb.len(),
                    self.entries.len(),
                    self.selected,
                    name_width,
                )
                .panes
                .len(),
                ready,
            });
            self.browser.auto_layout.set(Some(layout));
            config.columns = ColumnCount::Fixed(layout.count);
        } else {
            self.browser.auto_layout.set(None);
        }
        let mut plan = pane_plan_with_name_width(
            &config,
            width,
            self.breadcrumb.len(),
            self.entries.len(),
            self.selected,
            name_width,
        );
        if self.config.columns == ColumnCount::Auto {
            plan.requested = None;
        }
        self.browser.preview_targets.borrow_mut().clear();
        if !self.config.show_preview {
            return plan;
        }
        let mut target = self
            .entries
            .get(self.selected)
            .cloned()
            .map(|entry| PreviewTarget {
                entry,
                path: Vec::new(),
            });
        for pane in plan.panes.iter_mut().skip(plan.active + 1) {
            if let Some(current) = target.take() {
                *pane = if current.path.is_empty() {
                    PaneKind::Preview(
                        self.entries
                            .iter()
                            .position(|e| e.id == current.entry.id)
                            .unwrap(),
                    )
                } else {
                    let mut sources = self.browser.preview_targets.borrow_mut();
                    let index = sources.len();
                    sources.push(current.clone());
                    PaneKind::Descendant(index)
                };
                target = self.next_preview_target(current);
            } else {
                *pane = PaneKind::Empty;
            }
        }
        plan
    }

    fn next_preview_target(&self, mut target: PreviewTarget) -> Option<PreviewTarget> {
        // Continue the selected path first. Once it reaches a file (or an
        // empty directory), use following siblings to fill remaining panes.
        if target.entry.kind == EntryKind::Folder
            && target.entry.id != self.current_folder_id
            && !target
                .path
                .iter()
                .any(|parent| parent.id == target.entry.id)
            && let Some(snapshot) = self.browser.directories.get(&target.entry.id)
            && let Some(child) = snapshot.entries.get(snapshot.selected)
        {
            target.path.push(target.entry);
            return Some(PreviewTarget {
                entry: child.clone(),
                path: target.path,
            });
        }
        loop {
            let entries = if let Some(parent) = target.path.last() {
                &self.browser.directories.get(&parent.id)?.entries
            } else {
                &self.entries
            };
            if let Some(index) = entries.iter().position(|e| e.id == target.entry.id)
                && let Some(next) = entries.get(index + 1)
            {
                return Some(PreviewTarget {
                    entry: next.clone(),
                    path: target.path,
                });
            }
            target.entry = target.path.pop()?;
        }
    }

    pub(super) fn preview_target(&self, kind: PaneKind) -> Option<PreviewTarget> {
        match kind {
            PaneKind::Preview(index) => {
                self.entries.get(index).cloned().map(|entry| PreviewTarget {
                    entry,
                    path: Vec::new(),
                })
            }
            PaneKind::Descendant(index) => {
                self.browser.preview_targets.borrow().get(index).cloned()
            }
            _ => None,
        }
    }

    pub(super) fn thumbnail_placeholder_state(
        &self,
        entry: &Entry,
    ) -> super::thumbnail_placeholder::ThumbnailPlaceholder {
        let url = inline_thumbnail_url(entry);
        let file_key = LoadKey::File(entry.id.clone());
        let image_key = url.map(|url| LoadKey::InlineImage(url.to_owned()));
        let loading = self
            .browser
            .in_flight
            .values()
            .any(|request| request.key == file_key || image_key.as_ref() == Some(&request.key))
            || (self.preview_target_id.as_deref() == Some(entry.id.as_str())
                && matches!(self.preview_state, PreviewState::Loading));
        if loading {
            super::thumbnail_placeholder::ThumbnailPlaceholder::Loading
        } else if url.is_none()
            || self.browser.failed.contains(&file_key)
            || image_key
                .as_ref()
                .is_some_and(|key| self.browser.failed.contains(key))
        {
            super::thumbnail_placeholder::ThumbnailPlaceholder::Unavailable
        } else {
            super::thumbnail_placeholder::ThumbnailPlaceholder::Pending
        }
    }

    #[cfg(test)]
    pub(super) fn file_preview_state(&self, index: usize) -> &PreviewState {
        let Some(entry) = self.entries.get(index) else {
            return &PreviewState::FileBasicInfo;
        };
        self.file_preview_state_for_entry(entry)
    }

    pub(super) fn file_preview_state_for_entry(&self, entry: &Entry) -> &PreviewState {
        if self
            .current_entry()
            .is_some_and(|selected| selected.id == entry.id)
            && self.preview_target_id.as_deref() == Some(entry.id.as_str())
            && !matches!(
                self.preview_state,
                PreviewState::Empty | PreviewState::FileBasicInfo
            )
        {
            &self.preview_state
        } else {
            let cached = inline_thumbnail_url(entry)
                .and_then(|url| self.browser.thumbnail_previews.get(url))
                .or_else(|| self.browser.file_previews.get(&entry.id))
                .map(|p| &p.value);
            if let Some(state) = cached {
                state
            } else if self.browser.in_flight.values().any(|request| {
                request.key == LoadKey::File(entry.id.clone())
                    || inline_thumbnail_url(entry)
                        .is_some_and(|url| request.key == LoadKey::InlineImage(url.to_owned()))
            }) {
                &PreviewState::Loading
            } else {
                &PreviewState::FileBasicInfo
            }
        }
    }

    pub(super) fn cache_current_directory(&mut self) {
        self.snapshot_current_directory(false);
    }

    pub(super) fn cache_main_directory(&mut self) {
        self.snapshot_current_directory(true);
    }

    fn snapshot_current_directory(&mut self, fresh: bool) {
        if self.loading && self.entries.is_empty() {
            return;
        }
        let id = self.current_folder_id.clone();
        let generation = if fresh {
            self.browser.generation
        } else {
            self.browser
                .directories
                .get(&id)
                .map(|s| s.generation)
                .unwrap_or(self.browser.generation)
        };
        self.browser
            .cache_directory(id.clone(), self.entries.clone());
        if let Some(snapshot) = self.browser.directories.get_mut(&id) {
            snapshot.generation = generation;
            snapshot.selected = self.selected;
            snapshot.scroll.set(self.scroll_offset.get());
        }
    }

    pub(super) fn cache_parent_directory(&mut self) {
        if let Some((id, _)) = self.breadcrumb.last() {
            let id = id.clone();
            self.browser
                .cache_directory(id.clone(), self.parent_entries.clone());
            if let Some(snapshot) = self.browser.directories.get_mut(&id) {
                snapshot.selected = self.parent_selected;
                snapshot.scroll.set(self.parent_scroll_offset.get());
            }
        }
    }

    pub(super) fn ancestor_id(&self, depth: usize) -> Option<&str> {
        self.breadcrumb.get(depth).map(|(id, _)| id.as_str())
    }

    pub(super) fn ancestor_path(&self, depth: usize) -> String {
        format!(
            "/{}",
            self.breadcrumb
                .iter()
                .take(depth)
                .map(|(_, name)| name.as_str())
                .collect::<Vec<_>>()
                .join("/")
        )
    }

    fn restore_directory(&mut self, preferred: Option<String>) {
        self.invalidate_main_listing();
        self.finish_loading();
        self.clear_preview();
        self.browser.hits.borrow_mut().clear();
        let snapshot = self
            .browser
            .directories
            .get(&self.current_folder_id)
            .cloned();
        self.entries = snapshot
            .as_ref()
            .map(|s| s.entries.clone())
            .unwrap_or_default();
        self.selected = preferred
            .as_ref()
            .and_then(|id| self.entries.iter().position(|e| &e.id == id))
            .unwrap_or_else(|| {
                snapshot
                    .as_ref()
                    .map(|s| s.selected.min(s.entries.len().saturating_sub(1)))
                    .unwrap_or(0)
            });
        self.scroll_offset
            .set(snapshot.as_ref().map(|s| s.scroll.get()).unwrap_or(0));
        self.parent_entries = self
            .breadcrumb
            .last()
            .and_then(|(id, _)| self.browser.directories.get(id))
            .map(|s| s.entries.clone())
            .unwrap_or_default();
        self.parent_selected = self
            .parent_entries
            .iter()
            .position(|e| e.id == self.current_folder_id)
            .unwrap_or(0);
        self.parent_listing_request = None;
        self.on_cursor_move();
        if !snapshot.is_some_and(|s| s.generation == self.browser.generation) {
            self.browser.restore_selection =
                preferred.map(|entry| (self.current_folder_id.clone(), entry));
            self.loading = true;
            self.request_main_listing(self.current_folder_id.clone());
        } else {
            self.browser.restore_selection = None;
        }
    }

    pub(super) fn enter_directory(&mut self, entry: Entry) {
        if entry.kind != EntryKind::Folder {
            return;
        }
        self.cache_current_directory();
        if self.preview_target_id.as_deref() == Some(entry.id.as_str())
            && let PreviewState::FolderListing(children) =
                std::mem::replace(&mut self.preview_state, PreviewState::Empty)
        {
            self.browser.cache_directory(entry.id.clone(), children);
        }
        let old_id = std::mem::replace(&mut self.current_folder_id, entry.id);
        self.breadcrumb.push((old_id, entry.name));
        self.restore_directory(None);
    }

    pub(super) fn leave_directory(&mut self) {
        if self.breadcrumb.is_empty() {
            return;
        }
        self.cache_current_directory();
        // Also accept the legacy parent cache (including an asynchronously
        // loaded empty directory) when it is the only available snapshot.
        if let Some((id, _)) = self.breadcrumb.last()
            && !self.browser.directories.contains_key(id)
            && !self.parent_entries.is_empty()
        {
            self.cache_parent_directory();
        }
        if let Some((id, _)) = self.breadcrumb.pop() {
            let child_id = self.current_folder_id.clone();
            self.current_folder_id = id;
            self.restore_directory(Some(child_id));
        }
    }

    fn activate_ancestor(&mut self, depth: usize) {
        let Some(id) = self.ancestor_id(depth).map(str::to_owned) else {
            return;
        };
        self.cache_current_directory();
        let child_id = if depth + 1 == self.breadcrumb.len() {
            self.current_folder_id.clone()
        } else {
            self.breadcrumb[depth + 1].0.clone()
        };
        self.breadcrumb.truncate(depth);
        self.current_folder_id = id;
        self.restore_directory(Some(child_id));
    }

    fn browser_load_candidates(&self) -> Vec<(LoadKey, String, Option<Entry>)> {
        if !matches!(self.input, InputMode::Normal) || self.show_help_sheet {
            return Vec::new();
        }
        let hits = self.browser.hits.borrow().clone();
        let settled = self.last_cursor_move.elapsed() >= CURSOR_SETTLE;
        let mut work: Vec<(LoadKey, String, Option<Entry>)> = Vec::new();
        for pane in hits {
            if self.show_logs_overlay && pane.area.intersects(self.logs_overlay_area.get()) {
                continue;
            }
            match pane.kind {
                PaneKind::Ancestor(depth) => {
                    if let Some(id) = self.ancestor_id(depth) {
                        let immediate = depth + 1 == self.breadcrumb.len();
                        if !immediate || self.parent_listing_request.is_none() {
                            work.push((
                                LoadKey::Directory(id.to_owned()),
                                self.ancestor_path(depth),
                                None,
                            ));
                        }
                    }
                }
                PaneKind::Preview(_) | PaneKind::Descendant(_)
                    if settled && self.config.show_preview =>
                {
                    if let Some(target) = self.preview_target(pane.kind) {
                        let entry = target.entry;
                        let primary = target.path.is_empty()
                            && self.current_entry().is_some_and(|e| e.id == entry.id);
                        if entry.kind == EntryKind::Folder {
                            if !self
                                .preview_request
                                .as_ref()
                                .is_some_and(|r| r.target == entry.id)
                            {
                                work.push((
                                    LoadKey::Directory(entry.id.clone()),
                                    entry.name.clone(),
                                    None,
                                ));
                            }
                        } else if !(primary && self.preview_request.is_some())
                            && !(primary
                                && self.preview_target_id.as_deref() == Some(entry.id.as_str())
                                && !matches!(
                                    self.preview_state,
                                    PreviewState::Empty | PreviewState::FileBasicInfo
                                ))
                        {
                            if let Some(url) = inline_thumbnail_url(&entry) {
                                // One request supplies both the row thumbnail and the full preview.
                                work.push((
                                    LoadKey::InlineImage(url.to_owned()),
                                    entry.name.clone(),
                                    Some(entry),
                                ));
                            } else if (self.config.lazy_preview
                                || (theme::categorize(&entry) == FileCategory::Image
                                    && entry.size <= 16 * 1024 * 1024))
                                && !(theme::is_text_previewable(&entry)
                                    && entry.size > self.config.preview_max_size)
                            {
                                work.push((
                                    LoadKey::File(entry.id.clone()),
                                    entry.name.clone(),
                                    Some(entry),
                                ));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if settled && self.config.inline_thumbnails {
            for (url, name, area) in self.browser.visible_images.borrow().iter() {
                if self.show_logs_overlay && area.intersects(self.logs_overlay_area.get()) {
                    continue;
                }
                if self.preview_request.as_ref().is_some_and(|request| {
                    self.browser
                        .thumbnail_entries
                        .borrow()
                        .get(url)
                        .is_some_and(|entry| entry.id == request.target)
                }) {
                    continue;
                }
                work.push((
                    LoadKey::InlineImage(url.clone()),
                    name.clone(),
                    self.browser.thumbnail_entries.borrow().get(url).cloned(),
                ));
            }
        }
        work
    }

    pub(super) fn schedule_browser_loads(&mut self) {
        self.browser.trim();
        for (key, name, entry) in self.browser_load_candidates() {
            let Some(request) = self.browser.begin_load(key, name) else {
                continue;
            };
            let client = Arc::clone(&self.client);
            let tx = self.result_tx.clone();
            let max_size = self.config.preview_max_size;
            std::thread::spawn(move || {
                let result = match &request.key {
                    LoadKey::Directory(id) => client.ls(id).map(BrowserPayload::Directory),
                    LoadKey::InlineImage(url) => {
                        let image = match entry.as_ref() {
                            Some(entry) => super::fetch_entry_thumbnail(entry, &client),
                            None => fetch_and_render_thumbnail(url, &client),
                        };
                        image.map(|image| BrowserPayload::InlineImage(image.thumbnail(512, 512)))
                    }
                    LoadKey::File(id) => {
                        let entry = entry.expect("file load has an entry");
                        if inline_thumbnail_url(&entry).is_some()
                            || theme::categorize(&entry) == FileCategory::Image
                        {
                            super::fetch_entry_thumbnail(&entry, &client).map(|image| {
                                BrowserPayload::File(Box::new(PreviewState::ThumbnailImage {
                                    image: image.thumbnail(512, 512),
                                }))
                            })
                        } else if theme::is_text_previewable(&entry) {
                            client.fetch_text_preview(id, max_size).map(
                                |(name, content, size, truncated)| {
                                    let lines = highlight_content(&name, &content);
                                    BrowserPayload::File(Box::new(PreviewState::FileTextPreview {
                                        name,
                                        lines,
                                        size,
                                        truncated,
                                    }))
                                },
                            )
                        } else {
                            client.file_info(id).map(|info| {
                                BrowserPayload::File(Box::new(PreviewState::FileDetailedInfo(info)))
                            })
                        }
                    }
                };
                let _ = tx.send(OpResult::Browser(request, result));
            });
        }
    }

    pub(super) fn apply_browser_result(
        &mut self,
        request: BrowserRequest,
        result: Result<BrowserPayload>,
    ) {
        let obsolete = self.browser.obsolete.remove(&request.id);
        if self.browser.in_flight.remove(&request.id).as_ref() != Some(&request)
            || request.generation != self.browser.generation
            || obsolete
        {
            return;
        }
        match result {
            Ok(BrowserPayload::Directory(mut entries)) => {
                sort_entries(
                    &mut entries,
                    self.config.sort_field,
                    self.config.sort_reverse,
                );
                if let LoadKey::Directory(id) = request.key {
                    if self
                        .breadcrumb
                        .last()
                        .is_some_and(|(parent, _)| parent == &id)
                    {
                        self.parent_entries = entries.clone();
                        self.parent_selected = self
                            .parent_entries
                            .iter()
                            .position(|e| e.id == self.current_folder_id)
                            .unwrap_or(0);
                    }
                    self.browser.cache_directory(id, entries);
                }
            }
            Ok(BrowserPayload::InlineImage(image)) => {
                if let LoadKey::InlineImage(url) = request.key {
                    if let Some(entry) = self.browser.thumbnail_entries.borrow().get(&url) {
                        self.browser
                            .preview_protocols
                            .borrow_mut()
                            .remove(&entry.id);
                    }
                    self.browser.thumbnail_previews.insert(
                        url.clone(),
                        Cached {
                            value: PreviewState::ThumbnailImage {
                                image: image.clone(),
                            },
                            touched: Instant::now(),
                        },
                    );
                    self.browser.inline_images.insert(
                        url,
                        Cached {
                            value: image.thumbnail(96, 96),
                            touched: Instant::now(),
                        },
                    );
                }
            }
            Ok(BrowserPayload::File(state)) => {
                if let LoadKey::File(id) = request.key {
                    self.browser.preview_protocols.borrow_mut().remove(&id);
                    self.browser.file_previews.insert(
                        id,
                        Cached {
                            value: *state,
                            touched: Instant::now(),
                        },
                    );
                }
            }
            Err(error) => {
                self.browser.failed.insert(request.key);
                // Background media errors belong in the log. A large error
                // toast must not cover the browser for every failed row image.
                self.logs
                    .push_back(format!("Preview failed for {}: {error:#}", request.name));
                if self.logs.len() > 500 {
                    self.logs.pop_front();
                }
            }
        }
        self.browser.trim();
    }

    pub(super) fn handle_browser_click(&mut self, col: u16, row: u16, double: bool) {
        let pane = self
            .browser
            .hits
            .borrow()
            .iter()
            .find(|p| p.area.contains((col, row).into()))
            .cloned();
        let Some(pane) = pane else {
            return;
        };
        let clicked = if col > pane.area.x && col < pane.area.right().saturating_sub(1) {
            pane.row_at(row)
        } else {
            None
        };
        let target = match pane.kind {
            PaneKind::Active => clicked
                .and_then(|index| self.entries.get(index))
                .map(|e| e.id.clone()),
            PaneKind::Ancestor(depth) => self.ancestor_id(depth).and_then(|id| {
                clicked
                    .and_then(|index| {
                        self.browser
                            .directories
                            .get(id)?
                            .entries
                            .get(index)
                            .map(|e| e.id.clone())
                    })
                    .or_else(|| Some(id.to_owned()))
            }),
            PaneKind::Preview(_) | PaneKind::Descendant(_) => {
                self.preview_target(pane.kind).map(|target| {
                    clicked
                        .and_then(|row| {
                            self.browser
                                .directories
                                .get(&target.entry.id)?
                                .entries
                                .get(row)
                                .map(|e| e.id.clone())
                        })
                        .unwrap_or(target.entry.id)
                })
            }
            PaneKind::Empty => None,
        };
        // Focusing a preview can move the pane under the pointer. A second
        // click at the same coordinates must never open a different item.
        if double
            && self.browser.last_clicked_entry.is_some()
            && self.browser.last_clicked_entry != target
        {
            return;
        }
        self.browser.last_clicked_entry = target;
        match pane.kind {
            PaneKind::Active => {
                if let Some(index) = clicked {
                    self.selected = index;
                    self.on_cursor_move();
                    if double {
                        let _ = self.handle_key(
                            crossterm::event::KeyCode::Enter,
                            crossterm::event::KeyModifiers::NONE,
                        );
                    }
                }
            }
            PaneKind::Ancestor(depth) => {
                self.activate_ancestor(depth);
                if let Some(index) = clicked {
                    self.selected = index.min(self.entries.len().saturating_sub(1));
                    self.on_cursor_move();
                }
                if double && clicked.is_some() {
                    let _ = self.handle_key(
                        crossterm::event::KeyCode::Enter,
                        crossterm::event::KeyModifiers::NONE,
                    );
                }
            }
            PaneKind::Preview(_) | PaneKind::Descendant(_) => {
                let Some(target) = self.preview_target(pane.kind) else {
                    return;
                };
                self.focus_preview_parent(&target);
                if target.entry.kind == EntryKind::Folder {
                    self.enter_directory(target.entry);
                    if let Some(index) = clicked {
                        self.selected = index.min(self.entries.len().saturating_sub(1));
                        self.on_cursor_move();
                    }
                    if double && clicked.is_some() {
                        let _ = self.handle_key(
                            crossterm::event::KeyCode::Enter,
                            crossterm::event::KeyModifiers::NONE,
                        );
                    }
                } else {
                    self.on_cursor_move();
                    if double {
                        let _ = self.handle_key(
                            crossterm::event::KeyCode::Char(' '),
                            crossterm::event::KeyModifiers::NONE,
                        );
                    }
                }
            }
            PaneKind::Empty => {}
        }
    }

    fn focus_preview_parent(&mut self, target: &PreviewTarget) {
        for folder in &target.path {
            if let Some(index) = self.entries.iter().position(|e| e.id == folder.id) {
                self.selected = index;
            }
            self.enter_directory(folder.clone());
        }
        if let Some(index) = self.entries.iter().position(|e| e.id == target.entry.id) {
            self.selected = index;
        }
    }

    pub(super) fn handle_browser_scroll(&mut self, col: u16, row: u16, up: bool) {
        let pane = self
            .browser
            .hits
            .borrow()
            .iter()
            .find(|p| p.area.contains((col, row).into()))
            .cloned();
        let Some(pane) = pane else {
            return;
        };
        let step = |current: usize, max: usize| {
            if up {
                current.saturating_sub(3).min(max)
            } else {
                current.saturating_add(3).min(max)
            }
        };
        match pane.kind {
            PaneKind::Active => {
                let selected = step(self.selected, self.entries.len().saturating_sub(1));
                if selected != self.selected {
                    self.selected = selected;
                    self.on_cursor_move();
                }
            }
            PaneKind::Ancestor(depth) => {
                // Scrolling an ancestor focuses it, so subsequent commands
                // operate on the list the user is actually interacting with.
                self.activate_ancestor(depth);
                self.selected = step(self.selected, self.entries.len().saturating_sub(1));
                self.on_cursor_move();
            }
            PaneKind::Preview(_) | PaneKind::Descendant(_) => {
                if let Some(target) = self.preview_target(pane.kind) {
                    let entry = target.entry;
                    if entry.kind == EntryKind::Folder {
                        if let Some(snapshot) = self.browser.directories.get_mut(&entry.id) {
                            snapshot.selected =
                                step(snapshot.selected, snapshot.entries.len().saturating_sub(1));
                            snapshot.scroll.set(step(
                                snapshot.scroll.get(),
                                snapshot
                                    .entries
                                    .len()
                                    .saturating_sub(pane.rows.len().max(1)),
                            ));
                            self.last_cursor_move = Instant::now();
                        }
                    } else {
                        let max = match self.file_preview_state_for_entry(&entry) {
                            PreviewState::FileTextPreview {
                                lines, truncated, ..
                            } => lines.len().saturating_sub(
                                (pane.area.height.saturating_sub(2) as usize)
                                    .saturating_sub(usize::from(*truncated)),
                            ),
                            _ => 0,
                        };
                        let primary = target.path.is_empty()
                            && self.current_entry().is_some_and(|e| e.id == entry.id);
                        let current = if primary {
                            self.preview_scroll
                        } else {
                            self.browser
                                .preview_scrolls
                                .get(&entry.id)
                                .copied()
                                .unwrap_or(0)
                        };
                        let scroll = step(current, max);
                        self.browser
                            .preview_scrolls
                            .insert(entry.id.clone(), scroll);
                        if primary {
                            self.preview_scroll = scroll;
                        }
                    }
                }
            }
            PaneKind::Empty => {}
        }
    }

    pub(super) fn browser_page_size(&self) -> usize {
        self.browser
            .hits
            .borrow()
            .iter()
            .find(|p| p.kind == PaneKind::Active)
            .map(|p| p.rows.len().max(1))
            .unwrap_or_else(|| self.list_area_height.get().max(1) as usize)
    }

    pub(super) fn save_browser_shortcut(&mut self) {
        match self.config.save() {
            Ok(()) => self.push_log(format!(
                "Inline thumbnails: {} · {}",
                if self.config.inline_thumbnails {
                    "on"
                } else {
                    "off"
                },
                self.config.inline_thumbnail_size.label()
            )),
            Err(error) => self.push_log(format!("Failed to save thumbnail setting: {error:#}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pikpak::PikPak;

    fn folder(id: &str) -> Entry {
        Entry {
            id: id.into(),
            name: id.into(),
            kind: EntryKind::Folder,
            size: 0,
            created_time: String::new(),
            modified_time: String::new(),
            starred: false,
            thumbnail_link: None,
            phase: None,
            audit: None,
        }
    }

    fn app() -> App {
        let mut app = App::new_login(PikPak::new().unwrap(), None, TuiConfig::default());
        app.input = InputMode::Normal;
        app.config.columns = ColumnCount::Fixed(4);
        app.config.show_help_bar = false;
        app
    }

    #[test]
    fn four_panes_advance_active_directory_and_preview_siblings() {
        let config: TuiConfig = toml::from_str("columns = 4").unwrap();
        let entries: Vec<_> = (0..10).map(|i| folder(&i.to_string())).collect();
        assert_eq!(
            pane_plan(&config, 120, 0, &entries, 5).panes,
            vec![
                PaneKind::Active,
                PaneKind::Preview(5),
                PaneKind::Preview(6),
                PaneKind::Preview(7)
            ]
        );
        assert_eq!(
            pane_plan(&config, 120, 1, &entries, 5).panes,
            vec![
                PaneKind::Ancestor(0),
                PaneKind::Active,
                PaneKind::Preview(5),
                PaneKind::Preview(6)
            ]
        );
        let plan = pane_plan(&config, 120, 8, &entries, 5);
        assert_eq!(
            plan.panes,
            vec![
                PaneKind::Ancestor(6),
                PaneKind::Ancestor(7),
                PaneKind::Active,
                PaneKind::Preview(5)
            ]
        );
        assert_eq!(plan.active, 2);
        assert_eq!(
            pane_plan(&config, 120, 1, &entries, 9).panes[3],
            PaneKind::Empty
        );
    }

    #[test]
    fn narrow_terminals_retain_fixed_preference_and_always_have_an_active_pane() {
        let mut config: TuiConfig = toml::from_str("columns = 6").unwrap();
        for width in [0, 1, 15, 40, 80, 180, 400] {
            let plan = pane_plan(&config, width, 20, &[], usize::MAX);
            assert_eq!(plan.requested, Some(6));
            assert_eq!(plan.panes[plan.active], PaneKind::Active);
            assert!(plan.panes.len() <= 6);
        }
        assert_eq!(pane_plan(&config, 80, 3, &[], 0).panes.len(), 2);
        assert_eq!(config.columns, ColumnCount::Fixed(6));
        config.show_preview = false;
        let plan = pane_plan(&config, 180, 8, &[], 0);
        assert_eq!(plan.active, 5);
        assert!(
            !plan
                .panes
                .iter()
                .any(|kind| matches!(kind, PaneKind::Preview(_)))
        );
        config.columns = ColumnCount::Fixed(usize::MAX);
        assert_eq!(pane_plan(&config, 80, 3, &[], 0).panes.len(), 2);
    }

    #[test]
    fn auto_width_uses_unicode_and_ignores_a_single_outlier_and_cursor_moves() {
        let config = TuiConfig::default();
        let mut short: Vec<_> = (0..20).map(|i| folder(&format!("item{i}"))).collect();
        short.push(folder(&"x".repeat(2000)));
        let count = pane_plan(&config, 160, 1, &short, 0).panes.len();
        assert_eq!(count, 5);
        assert_eq!(pane_plan(&config, 160, 1, &short, 20).panes.len(), count);
        let wide: Vec<_> = (0..20)
            .map(|_| folder("目录名称很长需要更多宽度"))
            .collect();
        assert!(pane_plan(&config, 160, 1, &wide, 0).panes.len() < count);
    }

    #[test]
    fn deep_navigation_restores_each_directory_selection_and_scroll() {
        let mut app = app();
        app.entries = (0..60).map(|i| folder(&format!("root{i}"))).collect();
        app.selected = 37;
        app.scroll_offset.set(30);
        let child = app.entries[37].clone();
        let child_entries: Vec<_> = (0..20).map(|i| folder(&format!("child{i}"))).collect();
        let grandchild = child_entries[7].clone();
        app.browser.cache_directory(child.id.clone(), child_entries);
        app.browser
            .cache_directory(grandchild.id.clone(), Vec::new());
        app.enter_directory(child.clone());
        app.selected = 7;
        app.scroll_offset.set(4);
        app.enter_directory(grandchild);
        app.leave_directory();
        assert_eq!(app.current_folder_id, child.id);
        assert_eq!(app.selected, 7);
        assert_eq!(app.scroll_offset.get(), 4);
        app.leave_directory();
        assert_eq!(app.current_folder_id, "");
        assert_eq!(app.entries[app.selected].id, "root37");
        assert_eq!(app.scroll_offset.get(), 30);
        assert!(!app.loading);
    }

    #[test]
    fn refresh_rejects_old_results_without_releasing_running_worker_slots() {
        let mut app = app();
        let mut requests = Vec::new();
        for i in 0..MAX_LOADS {
            requests.push(
                app.browser
                    .begin_load(LoadKey::Directory(i.to_string()), i.to_string())
                    .unwrap(),
            );
        }
        app.browser.invalidate();
        assert!(
            app.browser
                .begin_load(LoadKey::Directory("new".into()), "new".into())
                .is_none()
        );
        let first = requests.remove(0);
        app.apply_browser_result(first, Ok(BrowserPayload::Directory(vec![folder("stale")])));
        assert!(app.browser.directories.is_empty());
        let next = app
            .browser
            .begin_load(LoadKey::Directory("new".into()), "new".into())
            .unwrap();
        app.apply_browser_result(next, Ok(BrowserPayload::Directory(vec![folder("fresh")])));
        assert_eq!(app.browser.directories["new"].entries[0].id, "fresh");
        assert!(app.entries.is_empty());
    }

    #[test]
    fn failed_previews_stop_retrying_until_refresh() {
        let mut app = app();
        let key = LoadKey::InlineImage("https://example.invalid/image".into());
        let request = app
            .browser
            .begin_load(key.clone(), "photo.jpg".into())
            .unwrap();
        app.apply_browser_result(request, Err(anyhow::anyhow!("unavailable")));
        assert!(
            app.browser
                .begin_load(key.clone(), "photo.jpg".into())
                .is_none()
        );
        app.browser.invalidate();
        assert!(app.browser.begin_load(key, "photo.jpg".into()).is_some());
    }

    #[test]
    fn media_previews_load_automatically_and_share_the_inline_image_request() {
        use ratatui::{Terminal, backend::TestBackend};
        let mut app = app();
        app.config.columns = ColumnCount::Fixed(2);
        app.config.lazy_preview = false;
        let mut video = folder("video");
        video.kind = EntryKind::File;
        video.name = "clip.mp4".into();
        video.thumbnail_link = Some("https://example.invalid/thumbnail".into());
        app.entries = vec![video];
        app.last_cursor_move = Instant::now() - Duration::from_secs(1);
        Terminal::new(TestBackend::new(64, 15))
            .unwrap()
            .draw(|frame| app.draw(frame))
            .unwrap();
        let candidates = app.browser_load_candidates();
        assert_eq!(
            candidates.len(),
            2,
            "the row and the right pane both need this image"
        );
        for (key, name, _) in candidates {
            app.browser.begin_load(key, name);
        }
        assert_eq!(
            app.browser.in_flight.len(),
            1,
            "one download must satisfy both views"
        );
        assert!(matches!(app.file_preview_state(0), PreviewState::Loading));
        let request = app.browser.in_flight.values().next().unwrap().clone();
        app.apply_browser_result(
            request,
            Ok(BrowserPayload::InlineImage(image::DynamicImage::new_rgb8(
                512, 256,
            ))),
        );
        assert!(matches!(
            app.file_preview_state(0),
            PreviewState::ThumbnailImage { .. }
        ));
        assert_eq!(
            app.browser.inline_images["https://example.invalid/thumbnail"]
                .value
                .width(),
            96
        );
        assert!(!app.browser.needs_load(&LoadKey::InlineImage(
            "https://example.invalid/thumbnail".into()
        )));
    }

    #[test]
    fn manual_preview_success_restores_a_failed_inline_thumbnail() {
        let mut app = app();
        let mut entry = folder("image");
        entry.kind = EntryKind::File;
        entry.name = "photo.jpg".into();
        entry.thumbnail_link = Some("https://example.invalid/image".into());
        app.entries = vec![entry];
        app.on_cursor_move();
        let key = LoadKey::InlineImage("https://example.invalid/image".into());
        app.browser.failed.insert(key.clone());
        let request =
            app.begin_preview_request(super::super::AsyncRequestKind::FilePreview, "image");
        app.result_tx
            .send(OpResult::PreviewThumbnail(
                request,
                Ok(image::DynamicImage::new_rgb8(300, 150)),
            ))
            .unwrap();
        app.poll_results();
        assert!(!app.browser.failed.contains(&key));
        assert!(
            app.browser
                .inline_images
                .contains_key("https://example.invalid/image")
        );
        assert!(
            app.browser
                .thumbnail_previews
                .contains_key("https://example.invalid/image")
        );
        assert!(app.browser.file_previews.contains_key("image"));
    }

    #[test]
    fn load_candidates_follow_visible_panes_and_rows_after_debounce() {
        let mut app = app();
        app.entries = vec![folder("selected"), folder("hidden"), folder("visible")];
        app.browser.hits.borrow_mut().push(PaneHit {
            area: Rect::new(60, 0, 30, 20),
            kind: PaneKind::Preview(2),
            rows: Vec::new(),
        });
        app.browser.visible_images.borrow_mut().push((
            "https://example.invalid/visible-image".into(),
            "photo.jpg".into(),
            Rect::new(3, 1, 2, 1),
        ));
        assert!(app.browser_load_candidates().is_empty());
        app.last_cursor_move = Instant::now() - Duration::from_secs(1);
        let keys: Vec<_> = app
            .browser_load_candidates()
            .into_iter()
            .map(|(key, _, _)| key)
            .collect();
        assert_eq!(
            keys,
            vec![
                LoadKey::Directory("visible".into()),
                LoadKey::InlineImage("https://example.invalid/visible-image".into())
            ]
        );
        app.config.inline_thumbnails = false;
        assert_eq!(app.browser_load_candidates().len(), 1);
        app.config.show_preview = false;
        assert!(app.browser_load_candidates().is_empty());
    }

    #[test]
    fn caches_remain_bounded_during_long_browsing_sessions() {
        let mut browser = BrowserState::default();
        for i in 0..MAX_DIRECTORIES + 10 {
            browser.cache_directory(i.to_string(), vec![folder("child")]);
        }
        assert_eq!(browser.directories.len(), MAX_DIRECTORIES);
        browser.cache_directory(
            "huge".into(),
            (0..MAX_CACHED_ENTRIES + 1)
                .map(|i| folder(&i.to_string()))
                .collect(),
        );
        assert_eq!(browser.directories.len(), 1);
        assert!(browser.directories.contains_key("huge"));
    }

    #[test]
    fn cached_primary_text_preview_can_scroll_without_a_manual_preview_request() {
        let mut app = app();
        let mut entry = folder("file");
        entry.kind = EntryKind::File;
        entry.name = "readme.txt".into();
        app.entries.push(entry);
        app.on_cursor_move();
        app.browser.file_previews.insert(
            "file".into(),
            Cached {
                value: PreviewState::FileTextPreview {
                    name: "readme.txt".into(),
                    lines: (0..40)
                        .map(|i| ratatui::text::Line::from(i.to_string()))
                        .collect(),
                    size: 80,
                    truncated: false,
                },
                touched: Instant::now(),
            },
        );
        app.browser.hits.borrow_mut().push(PaneHit {
            area: Rect::new(30, 0, 30, 12),
            kind: PaneKind::Preview(0),
            rows: Vec::new(),
        });
        app.handle_browser_scroll(35, 5, false);
        assert_eq!(app.preview_scroll, 3);
        assert!(matches!(
            app.file_preview_state(0),
            PreviewState::FileTextPreview { .. }
        ));
    }

    #[test]
    fn main_or_manual_listing_supersedes_an_older_background_preview() {
        let mut app = app();
        let request = app
            .browser
            .begin_load(LoadKey::Directory("folder".into()), "folder".into())
            .unwrap();
        app.browser.cancel_directory_loads("folder");
        app.browser
            .cache_directory("folder".into(), vec![folder("fresh")]);
        app.apply_browser_result(request, Ok(BrowserPayload::Directory(vec![folder("old")])));
        assert_eq!(app.browser.directories["folder"].entries[0].id, "fresh");
        assert!(app.browser.in_flight.is_empty());
    }

    #[test]
    fn visible_large_directories_survive_the_history_cache_budget() {
        let mut browser = BrowserState::default();
        let visible: HashSet<_> = ["a", "b", "c"].into_iter().map(str::to_owned).collect();
        *browser.visible_directories.borrow_mut() = visible;
        let entries: Vec<_> = (0..8000).map(|i| folder(&i.to_string())).collect();
        for id in ["a", "b", "c"] {
            browser.cache_directory(id.into(), entries.clone());
        }
        assert_eq!(browser.directories.len(), 3);
        browser.visible_directories.borrow_mut().remove("a");
        browser.trim();
        assert!(!browser.directories.contains_key("a"));
        assert!(browser.directories.contains_key("b") && browser.directories.contains_key("c"));
    }

    #[test]
    fn returning_after_history_eviction_selects_the_child_by_id_when_listing_arrives() {
        let mut app = app();
        app.current_folder_id = "parent".into();
        app.main_listing_request_id = 12;
        app.browser.restore_selection = Some(("parent".into(), "child".into()));
        app.result_tx
            .send(OpResult::Ls(
                12,
                "parent".into(),
                Ok(vec![folder("other"), folder("child")]),
            ))
            .unwrap();
        app.poll_results();
        assert_eq!(app.entries[app.selected].id, "child");
        assert!(app.browser.restore_selection.is_none());
    }
}
