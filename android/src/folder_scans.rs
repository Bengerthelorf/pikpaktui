//! JNI scan registrations. Network work runs outside the registry lock.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock, Weak};

use std::time::Instant;

use anyhow::{Result, ensure};

use crate::pikpak::{
    Entry, FolderStats, FolderStatsError, FolderStatsSession, PikPak, RootListing,
};

pub(super) static SCANS: LazyLock<Mutex<FolderScans>> = LazyLock::new(Mutex::default);

#[derive(Default)]
pub(super) struct FolderScans {
    client: Weak<PikPak>,
    active: HashMap<String, Arc<Scan>>,
    session: Arc<FolderStatsSession>,
    root: Option<(Weak<()>, Arc<RootListing>)>,
    root_request: u64,
    listing_root: bool,
}

struct Batch {
    started: Instant,
    root: Arc<RootListing>,
    totals: HashMap<String, FolderStats>,
}

pub(super) struct Scan {
    pub(super) cancelled: AtomicBool,
    generation: Option<Weak<()>>,
    root: bool,
    batch: OnceLock<Option<Batch>>,
    index: Mutex<Option<IndexSync>>,
}

#[derive(Clone, Copy)]
enum IndexSync {
    Unavailable,
    Failed,
    Ready,
}

impl Scan {
    fn new(client: Option<&PikPak>, root: bool) -> Self {
        Self {
            cancelled: AtomicBool::default(),
            generation: client.map(PikPak::listing_generation),
            root,
            batch: OnceLock::new(),
            index: Mutex::new(None),
        }
    }

    fn check(&self) -> Result<(), FolderStatsError> {
        if self.cancelled.load(Ordering::Acquire)
            || self
                .generation
                .as_ref()
                .is_some_and(|g| g.strong_count() == 0)
        {
            return Err(FolderStatsError::Cancelled);
        }
        Ok(())
    }
}

impl FolderScans {
    fn reset_session(&mut self) {
        self.session.invalidate();
        self.session = Arc::default();
    }

    fn cancel_all(&mut self) {
        for (_, scan) in self.active.drain() {
            scan.cancelled.store(true, Ordering::Release);
        }
        self.reset_session();
    }

    pub(super) fn change_account(&mut self, client: Option<&Arc<PikPak>>) {
        self.client = client.map(Arc::downgrade).unwrap_or_default();
        self.cancel_all();
        self.root = None;
        self.root_request = self.root_request.wrapping_add(1);
        self.listing_root = false;
    }

    pub(super) fn begin(&mut self, key: &str, root: bool, fresh: bool) -> Result<()> {
        ensure!(
            !self.active.contains_key(key),
            "folder scan is already registered"
        );
        if fresh {
            // Keep the normal root listing the GUI just displayed for batch validation.
            self.cancel_all();
        }
        self.active.insert(
            key.to_owned(),
            Arc::new(Scan::new(self.client.upgrade().as_deref(), root)),
        );
        Ok(())
    }

    pub(super) fn cancel(&mut self, key: &str) {
        if let Some(scan) = self.active.remove(key) {
            scan.cancelled.store(true, Ordering::Release);
        }
    }

    pub(super) fn registered(&self, key: &str) -> Result<Arc<Scan>, FolderStatsError> {
        self.active
            .get(key)
            .cloned()
            .ok_or(FolderStatsError::Cancelled)
    }

    fn current_root(&self) -> Option<Arc<RootListing>> {
        self.root
            .as_ref()
            .filter(|(generation, _)| !self.listing_root && generation.strong_count() != 0)
            .map(|(_, listing)| Arc::clone(listing))
    }

    fn prepare_index(
        registry: &Mutex<Self>,
        client: &PikPak,
        scan: &Scan,
        explicit: bool,
        force_full: bool,
    ) -> Result<IndexSync> {
        // Only workers for this view share the lock. Cancellation and account
        // changes can proceed during the entire network operation.
        let mut done = scan.index.lock().unwrap_or_else(|e| e.into_inner());
        scan.check()?;
        if let Some(previous) = *done
            && !force_full
            && (!explicit || matches!(previous, IndexSync::Ready))
        {
            return Ok(previous);
        }
        if !explicit && !client.folder_index_available() {
            *done = Some(IndexSync::Unavailable);
            return Ok(IndexSync::Unavailable);
        }
        let root = registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .current_root();
        let result = match root.filter(|_| scan.root) {
            Some(root) => client.sync_folder_index_for_root(&scan.cancelled, force_full, &root),
            None => client.sync_folder_index(&scan.cancelled, force_full),
        };
        scan.check()?;
        match result {
            Ok(()) => {
                *done = Some(IndexSync::Ready);
                Ok(IndexSync::Ready)
            }
            Err(error) => {
                *done = Some(IndexSync::Failed);
                if matches!(error.downcast_ref(), Some(FolderStatsError::Cancelled)) {
                    scan.cancelled.store(true, Ordering::Release);
                    return Err(error);
                }
                if explicit {
                    Err(error)
                } else {
                    Ok(IndexSync::Failed)
                }
            }
        }
    }

    pub(super) fn sync_index(
        registry: &Mutex<Self>,
        client: &PikPak,
        scan: &Scan,
        force_full: bool,
    ) -> Result<()> {
        Self::prepare_index(registry, client, scan, true, force_full).map(|_| ())
    }

    pub(super) fn files(
        registry: &Mutex<Self>,
        client: &PikPak,
        parent: &str,
    ) -> Result<Vec<Entry>> {
        if !parent.is_empty() {
            return client.ls(parent);
        }
        let generation = client.listing_generation();
        let ticket = {
            let mut state = registry.lock().unwrap_or_else(|e| e.into_inner());
            state.root_request = state.root_request.wrapping_add(1);
            state.listing_root = true;
            state.root_request
        };
        // Always run ordinary ls, including its authentication and pagination.
        let entries = client.ls(parent);
        let root = entries
            .as_ref()
            .ok()
            .and_then(|rows| RootListing::from_entries(rows).ok());
        let mut state = registry.lock().unwrap_or_else(|e| e.into_inner());
        if ticket == state.root_request {
            state.listing_root = false;
            let same = state.root.as_ref().is_some_and(|(old, listed)| {
                old.ptr_eq(&generation) && root.as_ref() == Some(listed.as_ref())
            });
            if !same {
                if state.root.is_some() {
                    state.reset_session();
                }
                state.root = root.map(|list| (generation, Arc::new(list)));
            }
        }
        entries
    }

    pub(super) fn stats(
        registry: &Mutex<Self>,
        client: &PikPak,
        id: &str,
        registered: Option<&Scan>,
    ) -> Result<FolderStats, FolderStatsError> {
        let anonymous;
        let scan = match registered {
            Some(scan) => scan,
            None => {
                anonymous = Scan::new(Some(client), false);
                &anonymous
            }
        };
        let (session, root) = {
            let state = registry.lock().unwrap_or_else(|e| e.into_inner());
            scan.check()?;
            if !std::ptr::eq(state.client.as_ptr(), client) {
                return Err(FolderStatsError::Cancelled);
            }
            (Arc::clone(&state.session), state.current_root())
        };
        let expected = root.filter(|r| scan.root && (id.is_empty() || r.folders.contains(id)));
        // Bare API v1 calls retain their folder scope. A registered view syncs once,
        // even when its workers query many siblings or the first view is nested.
        let index = if registered.is_some() {
            Self::prepare_index(registry, client, scan, false, false)
                .map_err(|_| FolderStatsError::Cancelled)?
        } else {
            IndexSync::Unavailable
        };
        scan.check()?;
        let index_hit = if matches!(index, IndexSync::Ready) {
            client.cached_folder_stats(id).ok().flatten()
        } else {
            None
        };
        let result = if index_hit.is_some() {
            index_hit
        } else if let Some(expected) = expected.filter(|_| matches!(index, IndexSync::Unavailable))
        {
            // Only peer workers on this registration may wait for the wildcard.
            // begin/cancel, ordinary files and other registrations stay independent.
            let batch = scan.batch.get_or_init(|| {
                let count = expected.folders.len();
                let cold = expected
                    .folders
                    .iter()
                    .filter(|f| session.completed(f).is_none())
                    .count();
                if !FolderStatsSession::prefer_drive_scan(count, cold) {
                    return None;
                }
                let started = Instant::now();
                let mut totals = session
                    .drive_stats_for_root(client, &scan.cancelled, &expected)
                    .ok()?;
                totals.retain(|key, _| key.is_empty() || expected.folders.contains(key));
                Some(Batch {
                    started,
                    root: Arc::clone(&expected),
                    totals,
                })
            });
            scan.check()?;
            batch
                .as_ref()
                .filter(|b| {
                    Arc::ptr_eq(&expected, &b.root)
                        && b.started.elapsed() < FolderStatsSession::MAX_AGE
                })
                .and_then(|b| b.totals.get(id))
                .cloned()
        } else {
            None
        };
        let result = match result {
            Some(stats) => Ok(stats),
            // After an index attempt, recover through the single requested
            // subtree without retrying the same whole-drive scan.
            None if scan.root || !matches!(index, IndexSync::Unavailable) => {
                session.subtree_stats(client, id, &scan.cancelled)
            }
            None => session.stats(client, id, &scan.cancelled),
        };
        let state = registry.lock().unwrap_or_else(|e| e.into_inner());
        scan.check()?;
        if !Arc::ptr_eq(&session, &state.session) {
            return Err(FolderStatsError::Cancelled);
        }
        result
    }
}
