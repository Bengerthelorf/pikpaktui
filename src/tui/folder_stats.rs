use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::pikpak::{Entry, EntryKind, FolderStats, FolderStatsError};

use super::{App, OpResult};

pub(super) type ScanResult = Result<FolderStats, FolderStatsError>;

/// Statistics belong to one listing. A cancelled scan keeps its worker slot
/// until its blocking request finishes, even across repeated navigation.
#[derive(Default)]
pub(super) struct FolderStatsState {
    queue: Option<VecDeque<String>>,
    active: Option<Arc<AtomicBool>>,
    results: HashMap<String, ScanResult>,
}

impl FolderStatsState {
    pub(super) fn completed(&self, id: &str) -> Option<&FolderStats> {
        self.results.get(id)?.as_ref().ok()
    }

    pub(super) fn failed(&self, id: &str) -> bool {
        self.results.get(id).is_some_and(Result::is_err)
    }

    pub(super) fn running(&self) -> bool {
        self.active.is_some()
    }

    pub(super) fn invalidate(&mut self) {
        if let Some(active) = &self.active {
            active.store(true, Ordering::Relaxed);
        }
        self.queue = None;
        self.results.clear();
    }

    pub(super) fn next_folder(&mut self, entries: &[Entry]) -> Option<(String, Arc<AtomicBool>)> {
        if self.running() {
            return None;
        }
        let queue = self.queue.get_or_insert_with(|| {
            entries
                .iter()
                .filter(|e| e.kind == EntryKind::Folder)
                .map(|e| e.id.clone())
                .collect()
        });
        let id = queue.pop_front()?;
        let cancelled = Arc::new(AtomicBool::new(false));
        self.active = Some(Arc::clone(&cancelled));
        Some((id, cancelled))
    }

    pub(super) fn record(&mut self, id: String, result: ScanResult) {
        self.results.insert(id, result);
    }

    pub(super) fn finish(&mut self, id: String, result: ScanResult) -> bool {
        let Some(cancelled) = self.active.take() else {
            return false;
        };
        if cancelled.load(Ordering::Relaxed) {
            return false;
        }
        self.record(id, result);
        true
    }
}

impl Drop for FolderStatsState {
    fn drop(&mut self) {
        if let Some(active) = &self.active {
            active.store(true, Ordering::Relaxed);
        }
    }
}

impl App {
    pub(super) fn schedule_folder_stats(&mut self) {
        if !self.config.folder_stats
            || self.main_listing_loading
            || self.main_listing_error.is_some()
        {
            return;
        }
        let Some((id, cancelled)) = self.folder_stats.next_folder(&self.entries) else {
            return;
        };
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        std::thread::spawn(move || {
            let result = client.folder_stats(&id, &cancelled);
            let _ = tx.send(OpResult::FolderStats(id, result));
        });
    }
}
