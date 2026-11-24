use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, Weak};
use std::time::{Duration, Instant};

use super::drive_stats::RootListing;
use super::folder_stats::{FolderTotals, check_cancelled};
use super::{Entry, EntryKind, FolderStats, FolderStatsError, PikPak, interactive};

const TTL: Duration = Duration::from_secs(60);
const MAX_LISTINGS: usize = 64;
const MAX_ENTRIES_PER_LISTING: usize = 500;
const MAX_TOTALS: usize = 2048;
const MAX_LIST_READERS: usize = 2;
const WAIT_POLL: Duration = Duration::from_millis(25);

#[derive(Clone)]
struct Observed<T> {
    since: Instant,
    value: T,
}

/// FIFO admission with a fixed capacity. Hits never renew an observation.
struct Cache<T> {
    values: HashMap<String, Observed<T>>,
    order: VecDeque<String>,
}

impl<T> Default for Cache<T> {
    fn default() -> Self {
        Self {
            values: HashMap::new(),
            order: VecDeque::new(),
        }
    }
}

impl<T: Clone> Cache<T> {
    fn get(&self, id: &str, at: Instant) -> Option<Observed<T>> {
        self.values
            .get(id)
            .filter(|v| at.saturating_duration_since(v.since) < TTL)
            .cloned()
    }

    fn insert(&mut self, id: &str, value: Observed<T>, limit: usize) {
        if let Some(existing) = self.values.get_mut(id) {
            if existing.since <= value.since {
                *existing = value;
            }
            return;
        }
        if self.order.len() == limit
            && let Some(oldest) = self.order.pop_front()
        {
            self.values.remove(&oldest);
        }
        self.order.push_back(id.to_owned());
        self.values.insert(id.to_owned(), value);
    }
}

struct Direct {
    files: FolderStats,
    file_count: usize,
    children: Vec<String>,
}

impl Direct {
    fn root_listing(&self) -> RootListing {
        RootListing {
            folders: self.children.iter().cloned().collect(),
            files: self.files.clone(),
            file_count: self.file_count,
        }
    }
}

#[derive(Clone, Copy)]
enum InvalidFacts {
    SizeOverflow,
    InvalidFolderId,
}

#[derive(Clone)]
struct Directory {
    facts: Result<Arc<Direct>, InvalidFacts>,
    entries: Option<Arc<Vec<Entry>>>,
}

type Listing = Observed<Directory>;

#[derive(Clone)]
enum ListingResult {
    Complete(Listing),
    Failed(String),
    Abandoned,
}

struct Flight {
    epoch: u64,
    waiters: AtomicUsize,
    result: OnceLock<ListingResult>,
}

#[derive(Default)]
struct State {
    // Only the owning client's current LsCache holds a strong reference.
    // Local mutations, thumbnail changes and sign-in replace that marker.
    client: Weak<()>,
    epoch: u64,
    lists: Cache<Directory>,
    totals: Cache<FolderStats>,
    flights: HashMap<String, Arc<Flight>>,
    full_readers: HashMap<String, usize>,
}

impl State {
    fn reset(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.lists = Cache::default();
        self.totals = Cache::default();
        self.full_readers.clear();
        // In-flight blocking reads continue to occupy their slots until they return.
    }

    fn check(&self, epoch: u64) -> Result<(), FolderStatsError> {
        if self.epoch != epoch || self.client.strong_count() == 0 {
            Err(FolderStatsError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Bounded reuse of completed normal listings and recursive statistics. Share
/// one session between related background workers. Switching clients, a local
/// mutation, or explicit invalidation fences off previously started work.
#[derive(Default)]
pub struct FolderStatsSession {
    state: Mutex<State>,
    changed: Condvar,
}

struct Frame {
    id: String,
    direct: Arc<Direct>,
    next_child: usize,
    totals: FolderTotals,
    since: Instant,
}

impl Frame {
    fn new(id: String, listing: Listing) -> Result<Self, FolderStatsError> {
        let mut totals = FolderTotals::default();
        let direct = listing.value.facts.map_err(|error| match error {
            InvalidFacts::SizeOverflow => FolderStatsError::SizeOverflow,
            InvalidFacts::InvalidFolderId => read_error(&id, "subfolder has an invalid ID"),
        })?;
        totals.add_child(&direct.files)?;
        Ok(Self {
            id,
            direct,
            next_child: 0,
            totals,
            since: listing.since,
        })
    }

    fn add(&mut self, child: &Observed<FolderStats>) -> Result<(), FolderStatsError> {
        self.since = self.since.min(child.since);
        self.totals.add_child(&child.value)
    }
}

fn read_error(folder_id: &str, message: &str) -> FolderStatsError {
    FolderStatsError::ReadFolder {
        folder_id: folder_id.to_owned(),
        source: anyhow::anyhow!("{message}"),
    }
}

impl FolderStatsSession {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn begin(&self, client: &PikPak) -> u64 {
        let cache = client.ls_cache.lock().unwrap_or_else(|e| e.into_inner());
        let marker = Arc::downgrade(&cache.generation);
        let mut state = self.lock();
        if !state.client.ptr_eq(&marker) {
            state.reset();
            state.client = marker;
            self.changed.notify_all();
        }
        state.epoch
    }

    /// Discard cached results and cancel work belonging to the previous epoch.
    pub fn invalidate(&self) {
        self.lock().reset();
        self.changed.notify_all();
    }

    /// A fresh completed subtree, or None on expiry, failure or a cache miss.
    pub fn completed(&self, id: &str) -> Option<FolderStats> {
        let state = self.lock();
        state.check(state.epoch).ok()?;
        state.totals.get(id, Instant::now()).map(|v| v.value)
    }

    /// The normal paginated listing, shared with subtree scans in this session.
    pub fn list(&self, client: &PikPak, id: &str) -> anyhow::Result<Vec<Entry>> {
        let epoch = self.begin(client);
        {
            let mut state = self.lock();
            state.check(epoch)?;
            *state.full_readers.entry(id.to_owned()).or_default() += 1;
        }
        // Register across retries: if a stats-only owner already discarded a
        // prefix, the next flight retains every row from its very first page.
        let result = self.listing(
            client,
            id,
            &AtomicBool::new(false),
            epoch,
            Instant::now(),
            true,
        );
        let mut state = self.lock();
        if state.epoch == epoch
            && let Some(count) = state.full_readers.get_mut(id)
        {
            *count -= 1;
            if *count == 0 {
                state.full_readers.remove(id);
            }
        }
        let listing = result?;
        state.check(epoch)?;
        drop(state);
        Ok(Arc::unwrap_or_clone(
            listing.value.entries.expect("full listing requested"),
        ))
    }

    /// Compute a subtree on the calling worker. Successful child summaries
    /// remain reusable even if a later sibling fails. Their oldest input
    /// observation sets the parent's freshness, including for long scans.
    pub fn stats(
        &self,
        client: &PikPak,
        id: &str,
        cancelled: &AtomicBool,
    ) -> Result<FolderStats, FolderStatsError> {
        check_cancelled(cancelled)?;
        if id == "*" {
            return Err(read_error(id, "expected a folder ID, not the wildcard"));
        }
        let epoch = self.begin(client);
        let started = Instant::now();
        if let Some(complete) = self.cached_total(id, epoch, started)? {
            return Ok(complete.value);
        }
        let root = self.listing(client, id, cancelled, epoch, started, false)?;
        let root = Frame::new(id.to_owned(), root)?;
        if id.is_empty() && root.direct.children.len() >= 2 {
            let state = self.lock();
            state.check(epoch)?;
            let count = root.direct.children.len();
            let cold = root
                .direct
                .children
                .iter()
                .filter(|child| state.totals.get(child, started).is_none())
                .count();
            drop(state);
            if Self::prefer_drive_scan(count, cold) {
                let expected = root.direct.root_listing();
                match self.drive_in_epoch(client, cancelled, epoch, Some(&expected)) {
                    Ok(mut map) => return Ok(map.remove("").expect("drive scan includes root")),
                    Err(FolderStatsError::ReadFolder { .. }) => {}
                    Err(error) => return Err(error),
                }
            }
        }
        let mut frames = vec![root];
        let mut visited = HashSet::from([id.to_owned()]);
        loop {
            check_cancelled(cancelled)?;
            let frame = frames.last_mut().expect("active scan has a folder");
            if let Some(child) = frame.direct.children.get(frame.next_child).cloned() {
                frame.next_child += 1;
                if !visited.insert(child.clone()) {
                    return Err(read_error(&frame.id, "repeated directory in folder tree"));
                }
                if let Some(done) = self.cached_total(&child, epoch, started)? {
                    frame.add(&done)?;
                } else {
                    let list = self.listing(client, &child, cancelled, epoch, started, false)?;
                    frames.push(Frame::new(child, list)?);
                }
                continue;
            }
            let frame = frames.pop().unwrap();
            let done = Observed {
                since: frame.since,
                value: frame.totals.finish(frame.direct.files.is_empty),
            };
            {
                let mut state = self.lock();
                state.check(epoch)?;
                check_cancelled(cancelled)?;
                state.totals.insert(&frame.id, done.clone(), MAX_TOTALS);
            }
            match frames.last_mut() {
                Some(parent) => parent.add(&done)?,
                None => return Ok(done.value),
            }
        }
    }

    fn cached_total(
        &self,
        id: &str,
        epoch: u64,
        at: Instant,
    ) -> Result<Option<Observed<FolderStats>>, FolderStatsError> {
        let state = self.lock();
        state.check(epoch)?;
        Ok(state.totals.get(id, at))
    }

    /// Run the explicit whole-drive scan and warm the same bounded cache.
    /// The caller receives every completed folder even when not all fit.
    pub fn drive_stats(
        &self,
        client: &PikPak,
        cancelled: &AtomicBool,
    ) -> Result<HashMap<String, FolderStats>, FolderStatsError> {
        check_cancelled(cancelled)?;
        let epoch = self.begin(client);
        let expected = self
            .lock()
            .lists
            .get("", Instant::now())
            .and_then(|listing| listing.value.facts.ok())
            .map(|direct| direct.root_listing());
        self.drive_in_epoch(client, cancelled, epoch, expected.as_ref())
    }

    pub(crate) fn prefer_drive_scan(folders: usize, cold: usize) -> bool {
        cold >= 2 && cold >= folders - folders / 4
    }

    /// Use the caller's visible root even if its listing no longer fits in the cache.
    pub(crate) fn drive_stats_for_root(
        &self,
        client: &PikPak,
        cancelled: &AtomicBool,
        expected: &RootListing,
    ) -> Result<HashMap<String, FolderStats>, FolderStatsError> {
        check_cancelled(cancelled)?;
        self.drive_in_epoch(client, cancelled, self.begin(client), Some(expected))
    }

    fn drive_in_epoch(
        &self,
        client: &PikPak,
        cancelled: &AtomicBool,
        epoch: u64,
        expected: Option<&RootListing>,
    ) -> Result<HashMap<String, FolderStats>, FolderStatsError> {
        self.lock().check(epoch)?;
        let since = Instant::now();
        let results = client
            .drive_folder_stats_while(cancelled, expected, || self.lock().check(epoch).is_ok());
        let mut state = self.lock();
        state.check(epoch)?;
        check_cancelled(cancelled)?;
        let results = results?;
        for (id, stats) in results
            .iter()
            .filter(|(id, _)| !id.is_empty())
            .take(MAX_TOTALS - 1)
        {
            state.totals.insert(
                id,
                Observed {
                    since,
                    value: stats.clone(),
                },
                MAX_TOTALS,
            );
        }
        if let Some(root) = results.get("") {
            state.totals.insert(
                "",
                Observed {
                    since,
                    value: root.clone(),
                },
                MAX_TOTALS,
            );
        }
        Ok(results)
    }

    fn listing(
        &self,
        client: &PikPak,
        id: &str,
        cancelled: &AtomicBool,
        epoch: u64,
        at: Instant,
        full: bool,
    ) -> Result<Listing, FolderStatsError> {
        let flight = loop {
            let mut state = self.lock();
            state.check(epoch)?;
            check_cancelled(cancelled)?;
            if let Some(listing) = state.lists.get(id, at)
                && (!full || listing.value.entries.is_some())
            {
                return Ok(listing);
            }
            if let Some(current) = state.flights.get(id).cloned() {
                if current.epoch == epoch {
                    current.waiters.fetch_add(1, Ordering::Relaxed);
                    let result = self.wait_for_flight(state, &current, id, cancelled);
                    current.waiters.fetch_sub(1, Ordering::Relaxed);
                    if let Some(listing) = result?
                        && (!full || listing.value.entries.is_some())
                    {
                        return Ok(listing);
                    }
                } else {
                    drop(self.wait(state, epoch, id, cancelled)?);
                }
                continue;
            }
            if state.flights.len() >= MAX_LIST_READERS {
                drop(self.wait(state, epoch, id, cancelled)?);
                continue;
            }
            let flight = Arc::new(Flight {
                epoch,
                waiters: AtomicUsize::new(0),
                result: OnceLock::new(),
            });
            state.flights.insert(id.to_owned(), Arc::clone(&flight));
            break flight;
        };

        let since = Instant::now();
        let read = || -> anyhow::Result<Option<Listing>> {
            let mut pages = client.ls_pages(id)?;
            let mut entries = Some(Vec::new());
            let mut totals = FolderTotals::default();
            let mut file_count = 0;
            let mut children = Vec::new();
            let mut invalid = None;
            let mut is_empty = true;
            loop {
                {
                    let state = self.lock();
                    if state.check(epoch).is_err()
                        || (cancelled.load(Ordering::Relaxed)
                            && flight.waiters.load(Ordering::Relaxed) == 0)
                    {
                        return Ok(None);
                    }
                }
                let Some(page) = pages.next() else { break };
                let page = page?;
                is_empty &= page.is_empty();
                for entry in &page {
                    match entry.kind {
                        EntryKind::Folder if entry.id.is_empty() || entry.id == "*" => {
                            invalid = Some(InvalidFacts::InvalidFolderId)
                        }
                        EntryKind::Folder => children.push(entry.id.clone()),
                        EntryKind::File => {
                            file_count += 1;
                            if totals.add_file(entry.size, &entry.modified_time).is_err() {
                                invalid = Some(InvalidFacts::SizeOverflow);
                            }
                        }
                    }
                }
                if let Some(rows) = &mut entries {
                    if rows.len() + page.len() <= MAX_ENTRIES_PER_LISTING
                        || self.lock().full_readers.contains_key(id)
                    {
                        rows.extend(page);
                    } else {
                        entries = None;
                    }
                }
            }
            let facts = match invalid {
                Some(error) => Err(error),
                None => Ok(Arc::new(Direct {
                    files: totals.finish(is_empty),
                    file_count,
                    children,
                })),
            };
            Ok(Some(Observed {
                since,
                value: Directory {
                    facts,
                    entries: entries.map(Arc::new),
                },
            }))
        };
        let outcome = match read() {
            Ok(Some(list)) => ListingResult::Complete(list),
            Ok(None) => ListingResult::Abandoned,
            Err(error) => ListingResult::Failed(format!("{error:#}")),
        };
        let mut state = self.lock();
        state.flights.remove(id);
        if state.check(epoch).is_ok()
            && let ListingResult::Complete(list) = &outcome
            && list
                .value
                .facts
                .as_ref()
                .ok()
                .is_none_or(|d| d.children.len() <= MAX_ENTRIES_PER_LISTING)
        {
            let mut cached = list.clone();
            if cached
                .value
                .entries
                .as_ref()
                .is_some_and(|rows| rows.len() > MAX_ENTRIES_PER_LISTING)
            {
                cached.value.entries = None;
            }
            state.lists.insert(id, cached, MAX_LISTINGS);
        }
        let _ = flight.result.set(outcome.clone());
        self.changed.notify_all();
        state.check(epoch)?;
        check_cancelled(cancelled)?;
        Self::outcome(&outcome, id).unwrap_or(Err(FolderStatsError::Cancelled))
    }

    fn outcome(result: &ListingResult, id: &str) -> Option<Result<Listing, FolderStatsError>> {
        match result {
            ListingResult::Complete(list) => Some(Ok(list.clone())),
            ListingResult::Failed(error) => Some(Err(read_error(id, error))),
            ListingResult::Abandoned => None,
        }
    }

    fn wait_for_flight(
        &self,
        mut state: MutexGuard<'_, State>,
        flight: &Flight,
        id: &str,
        cancelled: &AtomicBool,
    ) -> Result<Option<Listing>, FolderStatsError> {
        loop {
            state.check(flight.epoch)?;
            check_cancelled(cancelled)?;
            if let Some(result) = flight.result.get() {
                return Self::outcome(result, id).transpose();
            }
            state = self.wait(state, flight.epoch, id, cancelled)?;
        }
    }

    fn wait<'a>(
        &self,
        state: MutexGuard<'a, State>,
        epoch: u64,
        id: &str,
        cancelled: &AtomicBool,
    ) -> Result<MutexGuard<'a, State>, FolderStatsError> {
        let timeout =
            interactive::remaining().map_or(WAIT_POLL, |remaining| remaining.min(WAIT_POLL));
        if timeout <= Duration::from_millis(1) {
            return Err(read_error(id, "timed out waiting for folder listing"));
        }
        let (state, _) = self
            .changed
            .wait_timeout(state, timeout)
            .unwrap_or_else(|e| e.into_inner());
        state.check(epoch)?;
        check_cancelled(cancelled)?;
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::thread;

    use super::*;
    use crate::pikpak::tests::{temp_test_dir, test_client, write_response};
    use crate::pikpak::{accept_test_connection, read_test_http_request};

    fn fixture() -> (PikPak, PathBuf, TcpListener) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let dir = temp_test_dir("stats-session");
        let client = test_client(
            format!("http://{}", listener.local_addr().unwrap()),
            dir.join("session.json"),
        );
        (client, dir, listener)
    }

    fn request(listener: &TcpListener, expected_query: &str) -> TcpStream {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match accept_test_connection(listener) {
                Ok(stream) => break stream,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(2))
                }
                Err(e) => panic!("expected request {expected_query}: {e}"),
            }
        };
        let req = read_test_http_request(&mut stream).unwrap();
        assert!(
            req.lines().next().unwrap().contains(expected_query),
            "{req}"
        );
        stream
    }

    fn reply(listener: &TcpListener, query: &str, body: &str) {
        write_response(&mut request(listener, query), 200, "OK", body.as_bytes());
    }

    #[test]
    fn normal_listing_and_postorder_results_reuse_all_three_directories() {
        let (client, dir, listener) = fixture();
        let server = thread::spawn(move || {
            reply(
                &listener,
                "parent_id=root",
                r#"{"files":[{"id":"child","kind":"drive#folder","name":"child"},{"id":"a","name":"a","size":1}]}"#,
            );
            reply(
                &listener,
                "parent_id=child",
                r#"{"files":[{"id":"deep","kind":"drive#folder","name":"deep"}]}"#,
            );
            reply(
                &listener,
                "parent_id=deep",
                r#"{"files":[{"id":"b","name":"b","size":8,"modified_time":"2026-01-02T00:00:00Z"}]}"#,
            );
        });
        let session = FolderStatsSession::default();
        let cancel = AtomicBool::new(false);
        assert_eq!(session.list(&client, "root").unwrap().len(), 2);
        let stats = session.stats(&client, "root", &cancel).unwrap();
        assert_eq!(stats.total_size_bytes, 9);
        assert_eq!(
            stats.latest_modified_time.as_deref(),
            Some("2026-01-02T00:00:00Z")
        );
        for id in ["child", "deep"] {
            assert_eq!(session.completed(id).unwrap().total_size_bytes, 8);
            assert_eq!(
                session
                    .stats(&client, id, &cancel)
                    .unwrap()
                    .total_size_bytes,
                8
            );
            assert_eq!(session.list(&client, id).unwrap().len(), 1);
        }
        server.join().unwrap();
        client.clear_ls_cache();
        assert!(session.completed("root").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn oldest_child_sets_expiry_and_another_client_cannot_reuse_it() {
        let session = FolderStatsSession::default();
        let (a, dir_a, listener) = fixture();
        let epoch = session.begin(&a);
        let oldest = Instant::now() - TTL + Duration::from_secs(5);
        session.lock().totals.insert(
            "child",
            Observed {
                since: oldest,
                value: FolderTotals::default().finish(true),
            },
            MAX_TOTALS,
        );
        let server = thread::spawn(move || {
            reply(
                &listener,
                "parent_id=root",
                r#"{"files":[{"id":"child","kind":"drive#folder","name":"child"}]}"#,
            )
        });
        let result = session.stats(&a, "root", &AtomicBool::new(false)).unwrap();
        assert!(!result.is_empty);
        assert!(
            session
                .cached_total("root", epoch, oldest + TTL)
                .unwrap()
                .is_none()
        );
        server.join().unwrap();

        let (b, dir_b, listener) = fixture();
        let server = thread::spawn(move || {
            reply(
                &listener,
                "parent_id=root",
                r#"{"files":[{"id":"file","name":"new","size":19}]}"#,
            )
        });
        assert_eq!(
            session
                .stats(&b, "root", &AtomicBool::new(false))
                .unwrap()
                .total_size_bytes,
            19
        );
        assert!(session.completed("child").is_none());
        server.join().unwrap();
        std::fs::remove_dir_all(dir_a).unwrap();
        std::fs::remove_dir_all(dir_b).unwrap();
    }

    #[test]
    fn cancelling_stats_owner_keeps_paginated_preview_reader_alive() {
        let (client, dir, listener) = fixture();
        let session = FolderStatsSession::default();
        let cancelled = AtomicBool::new(false);
        thread::scope(|scope| {
            let scan = scope.spawn(|| session.stats(&client, "folder", &cancelled));
            let mut first = request(&listener, "parent_id=folder");
            let preview = scope.spawn(|| session.list(&client, "folder"));
            let deadline = Instant::now() + Duration::from_secs(5);
            while session.lock().flights["folder"]
                .waiters
                .load(Ordering::Relaxed)
                == 0
            {
                assert!(
                    Instant::now() < deadline,
                    "preview did not join in-flight listing"
                );
                thread::yield_now();
            }
            cancelled.store(true, Ordering::Relaxed);
            write_response(
                &mut first,
                200,
                "OK",
                br#"{"files":[{"id":"a","name":"a","size":1}],"next_page_token":"next"}"#,
            );
            drop(first);
            reply(
                &listener,
                "page_token=next",
                r#"{"files":[{"id":"b","name":"b","size":2}]}"#,
            );
            drop(listener);
            assert!(matches!(
                scan.join().unwrap(),
                Err(FolderStatsError::Cancelled)
            ));
            assert_eq!(preview.join().unwrap().unwrap().len(), 2);
        });
        assert_eq!(
            session
                .stats(&client, "folder", &AtomicBool::new(false))
                .unwrap()
                .total_size_bytes,
            3
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn oversized_stats_only_listing_keeps_facts_without_full_entries() {
        let (client, dir, listener) = fixture();
        let session = FolderStatsSession::default();
        let server = thread::spawn(move || {
            let files: Vec<_> = (0..MAX_ENTRIES_PER_LISTING)
                .map(|i| serde_json::json!({"id":i.to_string(),"name":"f","size":1}))
                .collect();
            reply(
                &listener,
                "parent_id=dir",
                &serde_json::json!({"files":files,"next_page_token":"n"}).to_string(),
            );
            reply(
                &listener,
                "page_token=n",
                r#"{"files":[{"id":"last","name":"last","size":9}]}"#,
            );
        });
        let first = session
            .listing(
                &client,
                "dir",
                &AtomicBool::new(false),
                session.begin(&client),
                Instant::now(),
                false,
            )
            .unwrap();
        assert!(first.value.entries.is_none());
        let result = session
            .stats(&client, "dir", &AtomicBool::new(false))
            .unwrap();
        assert_eq!(result.total_size_bytes, MAX_ENTRIES_PER_LISTING as u64 + 9);
        let stored = session.lock().lists.get("dir", Instant::now()).unwrap();
        assert!(stored.value.entries.is_none());
        assert_eq!(stored.value.facts.ok().unwrap().files, result);
        server.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn regular_listing_is_available_even_when_stats_overflow_or_a_folder_id_is_invalid() {
        let (client, dir, listener) = fixture();
        let session = FolderStatsSession::default();
        let server = thread::spawn(move || {
            reply(
                &listener,
                "parent_id=overflow",
                r#"{"files":[{"id":"max","name":"max","size":"18446744073709551615"},{"id":"one","name":"one","size":1}]}"#,
            );
            reply(
                &listener,
                "parent_id=invalid",
                r#"{"files":[{"id":"","name":"missing id","kind":"drive#folder"}]}"#,
            );
        });
        assert_eq!(session.list(&client, "overflow").unwrap().len(), 2);
        assert!(matches!(
            session.stats(&client, "overflow", &AtomicBool::new(false)),
            Err(FolderStatsError::SizeOverflow)
        ));
        assert_eq!(session.list(&client, "invalid").unwrap().len(), 1);
        assert!(matches!(
            session.stats(&client, "invalid", &AtomicBool::new(false)),
            Err(FolderStatsError::ReadFolder { .. })
        ));
        assert!(session.completed("invalid").is_none());
        server.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalidating_an_in_flight_scan_does_not_publish_its_results() {
        let (client, dir, listener) = fixture();
        let session = FolderStatsSession::default();
        thread::scope(|scope| {
            let scan = scope.spawn(|| session.stats(&client, "dir", &AtomicBool::new(false)));
            let mut stream = request(&listener, "parent_id=dir");
            session.invalidate();
            write_response(&mut stream, 200, "OK", br#"{"files":[]}"#);
            assert!(matches!(
                scan.join().unwrap(),
                Err(FolderStatsError::Cancelled)
            ));
        });
        assert!(session.completed("dir").is_none());
        assert!(session.lock().lists.values.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
