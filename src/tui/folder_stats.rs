use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::config::SortField;
use crate::pikpak::{
    Entry, EntryKind, FolderStats, FolderStatsError, FolderStatsSession, RootListing,
};

use super::{App, InputMode, OpResult, browser::PaneKind};

pub(super) type ScanResult = Result<FolderStats, FolderStatsError>;
pub(super) type DriveScanResult = Result<HashMap<String, FolderStats>, FolderStatsError>;
pub(super) enum IndexUpdate {
    Cached(HashMap<String, FolderStats>),
    Synced(HashMap<String, FolderStats>),
    Failed(String),
    Unavailable,
}

/// Reorder at most four times a second while individual folders finish.
const RESORT_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Default)]
pub(super) struct FolderStatsState {
    session: Arc<FolderStatsSession>,
    queue: Option<VecDeque<String>>,
    active: Option<Arc<AtomicBool>>,
    drive_scan_attempted: bool,
    drive_scan_active: bool,
    index_attempted: bool,
    cached_ids: HashSet<String>,
    results: HashMap<String, ScanResult>,
    sort_dirty_since: Option<Instant>,
}

impl FolderStatsState {
    pub(super) fn completed(&self, id: &str) -> Option<&FolderStats> {
        self.results.get(id)?.as_ref().ok()
    }

    pub(super) fn failed(&self, id: &str) -> bool {
        self.results.get(id).is_some_and(Result::is_err)
    }

    pub(super) fn is_cached(&self, id: &str) -> bool {
        self.cached_ids.contains(id)
    }

    pub(super) fn running(&self) -> bool {
        self.active.is_some()
    }

    pub(super) fn cancel_work(&mut self) {
        if let Some(active) = &self.active {
            active.store(true, Ordering::Relaxed);
        }
        self.queue = None;
    }

    /// Leaving a view cancels its outstanding work but preserves reusable cloud data.
    pub(super) fn leave_listing(&mut self) {
        self.cancel_work();
        self.results.clear();
        self.cached_ids.clear();
        self.index_attempted = false;
        self.sort_dirty_since = None;
    }

    pub(super) fn invalidate(&mut self) {
        self.leave_listing();
        self.drive_scan_attempted = false;
        self.session.invalidate();
    }

    pub(super) fn change_account(&mut self) {
        self.invalidate();
        // A worker launched against the previous client must never publish into this session.
        self.session = Arc::default();
    }

    pub(super) fn restore(&mut self, entries: &[Entry]) {
        for entry in entries
            .iter()
            .filter(|entry| entry.kind == EntryKind::Folder)
        {
            if let Some(stats) = self.session.completed(&entry.id) {
                self.record(entry.id.clone(), Ok(stats));
            }
        }
    }

    fn begin(&mut self) -> Arc<AtomicBool> {
        let cancelled = Arc::new(AtomicBool::new(false));
        self.active = Some(Arc::clone(&cancelled));
        cancelled
    }

    pub(super) fn begin_drive(
        &mut self,
        entries: &[Entry],
    ) -> Option<(Arc<AtomicBool>, RootListing)> {
        if self.drive_scan_attempted || self.running() {
            return None;
        }
        self.drive_scan_attempted = true;
        let mut cold = 0;
        let mut folders = 0;
        for entry in entries.iter().filter(|e| e.kind == EntryKind::Folder) {
            folders += 1;
            cold += usize::from(!self.results.contains_key(&entry.id));
        }
        if !FolderStatsSession::prefer_drive_scan(folders, cold) {
            return None;
        }
        // The displayed root is authoritative even if its cached listing was evicted.
        // If its direct entries cannot be summarized, use ordinary per-folder scans.
        let expected = RootListing::from_entries(entries).ok()?;
        self.drive_scan_active = true;
        Some((self.begin(), expected))
    }

    pub(super) fn next_folder(
        &mut self,
        entries: &[Entry],
        priority: impl Iterator<Item = usize>,
        all: bool,
    ) -> Option<(String, Arc<AtomicBool>)> {
        if self.running() {
            return None;
        }
        let mut priority = priority
            .filter_map(|index| entries.get(index))
            .filter(|entry| {
                entry.kind == EntryKind::Folder && !self.results.contains_key(&entry.id)
            });
        let id = if let Some(entry) = priority.next() {
            entry.id.clone()
        } else if all {
            let queue = self.queue.get_or_insert_with(|| {
                entries
                    .iter()
                    .filter(|e| e.kind == EntryKind::Folder && !self.results.contains_key(&e.id))
                    .map(|e| e.id.clone())
                    .collect()
            });
            loop {
                let id = queue.pop_front()?;
                if !self.results.contains_key(&id) {
                    break id;
                }
            }
        } else {
            return None;
        };
        Some((id, self.begin()))
    }

    pub(super) fn record(&mut self, id: String, result: ScanResult) {
        self.cached_ids.remove(&id);
        self.results.insert(id, result);
    }

    fn accept(&mut self) -> bool {
        self.drive_scan_active = false;
        self.active
            .take()
            .is_some_and(|flag| !flag.load(Ordering::Relaxed))
    }

    pub(super) fn finish(&mut self, id: String, result: ScanResult) -> bool {
        if !self.accept() {
            return false;
        }
        let complete = result.is_ok();
        self.record(id, result);
        complete
    }

    pub(super) fn finish_drive(&mut self, result: DriveScanResult, entries: &[Entry]) -> bool {
        if !self.accept() {
            return false;
        }
        let Ok(mut all) = result else {
            return false;
        };
        let folders = || entries.iter().filter(|e| e.kind == EntryKind::Folder);
        if !all.contains_key("") || folders().any(|e| !all.contains_key(&e.id)) {
            // The broad response disagrees with the displayed normal root listing.
            self.results.clear();
            self.session.invalidate();
            return false;
        }
        for entry in folders() {
            if let Some(stats) = all.remove(&entry.id) {
                self.record(entry.id.clone(), Ok(stats));
            }
        }
        true
    }

    pub(super) fn mark_sort_dirty(&mut self) {
        self.sort_dirty_since.get_or_insert_with(Instant::now);
    }

    pub(super) fn take_due_sort(&mut self) -> bool {
        if self
            .sort_dirty_since
            .is_some_and(|since| since.elapsed() >= RESORT_INTERVAL)
        {
            self.sort_dirty_since = None;
            true
        } else {
            false
        }
    }
}

impl Drop for FolderStatsState {
    fn drop(&mut self) {
        self.cancel_work();
    }
}

impl App {
    fn start_folder_index(&mut self) -> bool {
        if self.folder_stats.index_attempted {
            return false;
        }
        self.folder_stats.index_attempted = true;
        let ids: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| entry.kind == EntryKind::Folder)
            .map(|entry| entry.id.clone())
            .collect();
        if ids.is_empty() {
            return false;
        }
        let expected = if self.current_folder_id.is_empty() {
            match RootListing::from_entries(&self.entries) {
                Ok(root) => Some(root),
                Err(_) => return false,
            }
        } else {
            None
        };
        let active = self.folder_stats.begin();
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        std::thread::spawn(move || {
            let send = |update| {
                tx.send(OpResult::FolderIndex(Arc::clone(&active), update))
                    .is_ok()
            };
            if !client.folder_index_available() {
                let _ = send(IndexUpdate::Unavailable);
                return;
            }
            if let Ok(rows) = client.cached_folder_stats_batch(&ids)
                && !rows.is_empty()
                && !send(IndexUpdate::Cached(rows))
            {
                return;
            }
            let sync = match expected.as_ref() {
                Some(root) => client.sync_folder_index_for_root(&active, false, root),
                None => client.sync_folder_index(&active, false),
            };
            let update = match sync.and_then(|()| client.cached_folder_stats_batch(&ids)) {
                Ok(rows) => IndexUpdate::Synced(rows),
                Err(error) => IndexUpdate::Failed(format!("{error:#}")),
            };
            let _ = send(update);
        });
        true
    }

    pub(super) fn apply_folder_index(&mut self, token: &Arc<AtomicBool>, update: IndexUpdate) {
        let state = &mut self.folder_stats;
        if !state
            .active
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(active, token))
        {
            return;
        }
        if matches!(update, IndexUpdate::Cached(_)) {
            if token.load(Ordering::Relaxed) {
                return;
            }
        } else if !state.accept() {
            // A sort change can cancel the index without leaving the view.
            state.index_attempted = false;
            return;
        }
        match update {
            IndexUpdate::Cached(rows) => {
                for (id, stats) in rows {
                    state.cached_ids.insert(id.clone());
                    state.results.insert(id, Ok(stats));
                }
            }
            IndexUpdate::Synced(rows) => {
                state.results.clear();
                state.cached_ids.clear();
                state.drive_scan_attempted = true;
                for (id, stats) in rows {
                    state.record(id, Ok(stats));
                }
            }
            IndexUpdate::Failed(error) => {
                state.drive_scan_attempted = true;
                self.push_log(format!("Folder index sync failed: {error}"));
                return;
            }
            IndexUpdate::Unavailable => return,
        }
        self.resort_folder_stats();
    }

    /// Each consumer keeps its own deadline while sharing the same normal-list data.
    pub(super) fn folder_lister(
        &self,
    ) -> impl Fn(&str) -> anyhow::Result<Vec<Entry>> + Send + 'static + use<> {
        let client = Arc::clone(&self.client);
        let session = self
            .config
            .folder_stats
            .then(|| Arc::clone(&self.folder_stats.session));
        move |id| match &session {
            Some(session) => session.list(&client, id),
            None => client.ls(id),
        }
    }

    pub(super) fn schedule_folder_stats(&mut self) {
        if !self.config.folder_stats
            || self.main_listing_loading
            || self.main_listing_error.is_some()
            || self.show_help_sheet
        {
            return;
        }
        let info = matches!(self.input, InputMode::InfoFolderView { .. });
        if !info && !matches!(self.input, InputMode::Normal) {
            return;
        }
        let all = !info
            && matches!(
                self.config.sort_field,
                SortField::Size | SortField::ContentModified
            );
        if !info && self.config.sort_field == SortField::Modified {
            if self.folder_stats.running() {
                self.folder_stats.cancel_work();
            }
            return;
        }
        if self.folder_stats.running() {
            if !all && self.folder_stats.drive_scan_active {
                self.folder_stats.cancel_work();
            }
            return;
        }
        if self.start_folder_index() {
            return;
        }
        let session = Arc::clone(&self.folder_stats.session);
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        if all
            && self.current_folder_id.is_empty()
            && let Some((token, expected)) = self.folder_stats.begin_drive(&self.entries)
        {
            std::thread::spawn(move || {
                let result = session.drive_stats_for_root(&client, &token, &expected);
                let _ = tx.send(OpResult::DriveStats(result));
            });
            return;
        }
        let hits = self.browser.hits.borrow();
        let visible = hits
            .iter()
            .filter(|pane| !info && pane.kind == PaneKind::Active)
            .flat_map(|pane| pane.rows.iter().map(|row| row.index));
        let priority = std::iter::once(self.selected).chain(visible);
        let Some((id, cancelled)) = self.folder_stats.next_folder(&self.entries, priority, all)
        else {
            return;
        };
        std::thread::spawn(move || {
            let result = session.stats(&client, &id, &cancelled);
            let _ = tx.send(OpResult::FolderStats(id, result));
        });
    }
}
