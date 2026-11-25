//! Account-scoped materialized statistics. Readers use the last committed SQLite
//! snapshot while the writer streams ordinary drive listing pages into a transaction.
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, LazyLock, Mutex, Weak};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::drive::{de_modified_time, de_opt_u64};
use super::drive_stats::RootListing;
use super::folder_stats::check_cancelled;
use super::index_store::{Checkpoint, IndexChange, IndexStore, NeedsFull};
use super::{EntryKind, FolderStats, FolderStatsError, PikPak, now_unix};

// Permanently removed items can disappear without a delta tombstone. The next
// sync after this interval reconciles the entire visible tree.
const RECONCILE_SECONDS: i64 = 72 * 60 * 60;

#[derive(Deserialize)]
struct IndexRow {
    #[serde(default)]
    name: String,
    id: String,
    parent_id: Option<String>,
    kind: Option<String>,
    phase: Option<String>,
    #[serde(default)]
    trashed: bool,
    #[serde(default, deserialize_with = "de_opt_u64")]
    size: Option<u64>,
    #[serde(default, deserialize_with = "de_modified_time")]
    modified_time: Option<String>,
}

impl IndexRow {
    fn change(self, full: bool) -> Result<IndexChange> {
        ensure!(
            !self.id.is_empty() && self.id != "*",
            "invalid index item ID"
        );
        if self.trashed {
            ensure!(!full, "active drive baseline contains a trashed item");
            return Ok(IndexChange::Remove { id: self.id });
        }
        ensure!(
            self.phase
                .as_deref()
                .is_none_or(|p| p == "PHASE_TYPE_COMPLETE"),
            "drive index listing contains an incomplete item"
        );
        let kind = match self.kind.as_deref() {
            Some("drive#file") => EntryKind::File,
            Some("drive#folder") => EntryKind::Folder,
            _ => bail!("unrecognized drive index item kind"),
        };
        let parent_id = self
            .parent_id
            .context("missing parent in drive index listing")?;
        ensure!(
            parent_id != "*" && parent_id != self.id,
            "invalid parent in drive index listing"
        );
        Ok(IndexChange::Upsert {
            id: self.id,
            name: self.name,
            parent_id,
            kind,
            size: self.size.unwrap_or(0),
            modified_time: self.modified_time.unwrap_or_default(),
        })
    }
}

/// Without a stable identity and a server watermark we cannot resume a
/// durable sync. The folder-by-ID API can still traverse ordinary listings.
#[derive(Clone, Copy, Debug, thiserror::Error)]
pub(crate) enum UnsupportedIndex {
    #[error("saved account has no stable ID for the drive index")]
    MissingAccountId,
    #[error("drive listing does not provide a valid server sync_time")]
    MissingWatermark,
}

#[derive(Debug, thiserror::Error)]
#[error("the drive sync cursor is no longer usable")]
pub(super) struct ExpiredCursor;

pub(super) fn rejects_cursor(status: reqwest::StatusCode, body: &str) -> bool {
    if status == reqwest::StatusCode::GONE {
        return true;
    }
    if status != reqwest::StatusCode::BAD_REQUEST && status != reqwest::StatusCode::NOT_FOUND {
        return false;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    ["error", "error_code", "reason"].into_iter().any(|key| {
        if status == reqwest::StatusCode::NOT_FOUND {
            return value[key].as_str() == Some("file_invalid_file_filters");
        }
        matches!(
            value[key].as_str(),
            Some(
                "invalid_sync_time"
                    | "sync_time_expired"
                    | "invalid_page_token"
                    | "page_token_expired"
                    | "cursor_expired"
            )
        )
    })
}

fn delta_lower_bound(cursor: &str) -> Result<String> {
    // PikPak uses an exclusive modified_time threshold. Repeat one second to
    // cover ties even when item timestamps are rounded down to whole seconds.
    let time = OffsetDateTime::parse(cursor, &Rfc3339).map_err(|_| ExpiredCursor)?;
    let prior = time
        .checked_sub(time::Duration::SECOND)
        .ok_or(ExpiredCursor)?
        .format(&Rfc3339)
        .map_err(|_| ExpiredCursor)?;
    // The server accepts its own offset and fractional precision in filters.
    // An equivalent UTC spelling can be rejected as file_invalid_file_filters.
    // Subtracting a whole second leaves both parts of the suffix unchanged.
    const WHOLE_SECONDS: usize = "2000-01-01T00:00:00".len();
    Ok(format!(
        "{}{}",
        &prior[..WHOLE_SECONDS],
        &cursor[WHOLE_SECONDS..]
    ))
}

enum Outcome {
    Complete { full: bool },
    Cancelled,
    Unsupported(UnsupportedIndex),
    SizeOverflow,
    Failed(String),
}

struct Flight {
    full: bool,
    root: Option<RootListing>,
    result: Mutex<Option<Outcome>>,
    ready: Condvar,
}

static FLIGHTS: LazyLock<Mutex<HashMap<PathBuf, Arc<Flight>>>> = LazyLock::new(Mutex::default);

struct Leader<'a> {
    path: &'a Path,
    flight: Arc<Flight>,
}

impl Drop for Leader<'_> {
    fn drop(&mut self) {
        let mut flights = FLIGHTS.lock().unwrap_or_else(|e| e.into_inner());
        flights.remove(self.path);
        let mut result = self.flight.result.lock().unwrap_or_else(|e| e.into_inner());
        result.get_or_insert_with(|| {
            Outcome::Failed("drive index synchronization interrupted".into())
        });
        self.flight.ready.notify_all();
    }
}

impl PikPak {
    pub fn search_drive(
        &self,
        query: &super::SearchQuery,
        cancelled: &AtomicBool,
    ) -> Result<Vec<super::SearchHit>> {
        let account = self.index_account()?;
        self.sync_folder_index(cancelled, false)?;
        self.search_account_snapshot(query, &account)
    }

    pub fn cached_search(&self, query: &super::SearchQuery) -> Result<Vec<super::SearchHit>> {
        self.search_account_snapshot(query, &self.index_account()?)
    }

    fn search_account_snapshot(
        &self,
        query: &super::SearchQuery,
        account: &str,
    ) -> Result<Vec<super::SearchHit>> {
        let path = self.index_path(account)?;
        ensure!(
            path.try_exists()?,
            "no cached filename index; run search online first"
        );
        let store = IndexStore::open_readonly(&path)?;
        let hits = store.search(query)?;
        ensure!(
            self.index_account()? == account,
            FolderStatsError::Cancelled
        );
        Ok(hits)
    }

    fn index_account(&self) -> Result<String> {
        let account = self
            .load_session()?
            .context("no saved account for the drive index")?
            .user_id;
        ensure!(
            !account.trim().is_empty(),
            UnsupportedIndex::MissingAccountId
        );
        ensure!(
            self.user_id.is_empty() || self.user_id == account,
            "saved account changed since the drive client was initialized"
        );
        Ok(account)
    }

    /// Whether the saved session identifies an account for persistent statistics.
    /// This only reads local session metadata.
    pub fn folder_index_available(&self) -> bool {
        self.index_account().is_ok()
    }

    fn index_path(&self, account: &str) -> Result<PathBuf> {
        let mut hash = Sha256::new();
        for part in [
            self.drive_base_url.trim_end_matches('/'),
            self.auth_base_url.trim_end_matches('/'),
            account,
        ] {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part.as_bytes());
        }
        let parent = self
            .session_path
            .parent()
            .context("missing session directory")?;
        Ok(parent
            .join("folder-index")
            .join(format!("{:x}.sqlite3", hash.finalize())))
    }

    /// Read a complete folder summary from the account's last committed snapshot.
    /// A cold database or unknown folder yields None. An empty folder yields Some.
    pub fn cached_folder_stats(&self, id: &str) -> Result<Option<FolderStats>> {
        let account = self.index_account()?;
        let path = self.index_path(&account)?;
        if !path.try_exists()? {
            return Ok(None);
        }
        let stats = IndexStore::open_readonly(&path)?.get(id)?;
        ensure!(
            self.index_account()? == account,
            FolderStatsError::Cancelled
        );
        Ok(stats)
    }

    /// Read several folders from a single local snapshot without contacting PikPak.
    pub fn cached_folder_stats_batch(
        &self,
        ids: &[String],
    ) -> Result<HashMap<String, FolderStats>> {
        let account = self.index_account()?;
        let path = self.index_path(&account)?;
        if !path.try_exists()? {
            return Ok(HashMap::new());
        }
        let stats = IndexStore::open_readonly(&path)?.get_many(ids)?;
        ensure!(
            self.index_account()? == account,
            FolderStatsError::Cancelled
        );
        Ok(stats)
    }

    /// Synchronize the account's persistent folder statistics. A cold account,
    /// explicit rebuild, or expired reconciliation interval fetches a baseline.
    /// Other calls check server changes since the last committed watermark.
    pub fn sync_folder_index(&self, cancelled: &AtomicBool, force_full: bool) -> Result<()> {
        self.sync_folder_index_impl(cancelled, force_full, None)
    }

    pub(crate) fn sync_folder_index_for_root(
        &self,
        cancelled: &AtomicBool,
        force_full: bool,
        expected: &RootListing,
    ) -> Result<()> {
        self.sync_folder_index_impl(cancelled, force_full, Some(expected))
    }

    fn sync_folder_index_impl(
        &self,
        cancelled: &AtomicBool,
        force_full: bool,
        expected: Option<&RootListing>,
    ) -> Result<()> {
        check_cancelled(cancelled)?;
        let account = self.index_account()?;
        let path = self.index_path(&account)?;
        let generation = {
            let cache = self.ls_cache.lock().unwrap_or_else(|e| e.into_inner());
            Arc::downgrade(&cache.generation)
        };
        loop {
            check_cancelled(cancelled)?;
            ensure!(generation.strong_count() > 0, FolderStatsError::Cancelled);
            let mut flights = FLIGHTS.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(active) = flights.get(&path).cloned() {
                drop(flights);
                let mut done = active.result.lock().unwrap_or_else(|e| e.into_inner());
                while done.is_none() {
                    check_cancelled(cancelled)?;
                    done = active
                        .ready
                        .wait_timeout(done, Duration::from_millis(100))
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }
                check_cancelled(cancelled)?;
                ensure!(
                    self.index_account()? == account && generation.strong_count() > 0,
                    FolderStatsError::Cancelled
                );
                if expected.is_some() && expected != active.root.as_ref() {
                    continue;
                }
                match done.as_ref().expect("completed flight") {
                    Outcome::Complete { full } if !force_full || *full => return Ok(()),
                    Outcome::Unsupported(reason) => return Err((*reason).into()),
                    Outcome::SizeOverflow => return Err(FolderStatsError::SizeOverflow.into()),
                    Outcome::Failed(msg) if !force_full || active.full => bail!("{msg}"),
                    _ => continue,
                }
            }
            let flight = Arc::new(Flight {
                full: force_full,
                root: expected.cloned(),
                result: Mutex::new(None),
                ready: Condvar::new(),
            });
            flights.insert(path.clone(), flight.clone());
            drop(flights);
            let leader = Leader {
                path: &path,
                flight,
            };
            let result = self.write_index(
                &path,
                &account,
                &generation,
                cancelled,
                force_full,
                expected,
            );
            let outcome = match &result {
                Ok(full) => Outcome::Complete { full: *full },
                Err(e) => {
                    if let Some(reason) = e.downcast_ref::<UnsupportedIndex>() {
                        Outcome::Unsupported(*reason)
                    } else {
                        match e.downcast_ref::<FolderStatsError>() {
                            Some(FolderStatsError::Cancelled) => Outcome::Cancelled,
                            Some(FolderStatsError::SizeOverflow) => Outcome::SizeOverflow,
                            _ => Outcome::Failed(format!("{e:#}")),
                        }
                    }
                }
            };
            *leader
                .flight
                .result
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(outcome);
            drop(leader);
            return result.map(|_| ());
        }
    }

    fn write_index(
        &self,
        path: &Path,
        account: &str,
        generation: &Weak<()>,
        cancelled: &AtomicBool,
        force_full: bool,
        expected_root: Option<&RootListing>,
    ) -> Result<bool> {
        let mut dir = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            dir.mode(0o700);
        }
        dir.recursive(true)
            .create(path.parent().context("missing drive index directory")?)?;
        let mut store = IndexStore::open(path)?;
        let previous = store.checkpoint()?;
        let now = now_unix();
        let mut full = force_full
            || previous.as_ref().is_none_or(|p| {
                now < p.full_at || now.saturating_sub(p.full_at) >= RECONCILE_SECONDS
            });
        let check = |committing: bool| {
            check_cancelled(cancelled)?;
            let account_lock = if committing {
                let file = self.lock_session_file()?;
                ensure!(
                    self.index_account()? == account,
                    FolderStatsError::Cancelled
                );
                Some(file)
            } else {
                None
            };
            let cache = self.ls_cache.lock().unwrap_or_else(|e| e.into_inner());
            ensure!(
                generation.ptr_eq(&Arc::downgrade(&cache.generation)),
                FolderStatsError::Cancelled
            );
            Ok((cache, account_lock))
        };
        loop {
            drop(check(false)?);
            let validate = || -> Result<()> {
                drop(check(false)?);
                ensure!(
                    self.index_account()? == account,
                    FolderStatsError::Cancelled
                );
                Ok(())
            };
            let old = previous.as_ref().filter(|_| !full);
            let result = store.apply(
                previous.as_ref(),
                full,
                expected_root,
                |sink| self.fetch_index_changes(old, cancelled, &validate, sink),
                |parent, sink| self.hydrate_index_folder(parent, cancelled, &validate, sink),
                check,
            );
            match result {
                Err(e) if !full && (e.is::<NeedsFull>() || e.is::<ExpiredCursor>()) => {
                    drop(check(false)?);
                    full = true;
                }
                other => return other.map(|()| full),
            }
        }
    }

    fn fetch_index_changes(
        &self,
        old: Option<&Checkpoint>,
        cancelled: &AtomicBool,
        validate: &dyn Fn() -> Result<()>,
        sink: &mut dyn FnMut(IndexChange) -> Result<()>,
    ) -> Result<Checkpoint> {
        let filters = match old {
            Some(p) => serde_json::json!({
                "phase":{"eq":"PHASE_TYPE_COMPLETE"},
                "modified_time":{"gt":delta_lower_bound(&p.cursor)?}
            }),
            None => {
                serde_json::json!({"phase":{"eq":"PHASE_TYPE_COMPLETE"},"trashed":{"eq":false}})
            }
        };
        validate()?;
        let mut pages =
            self.filtered_list_pages::<IndexRow>("*", filters.to_string(), "sync_folder_index")?;
        let mut last_page: Option<(OffsetDateTime, String)> = None;
        loop {
            validate()?;
            let Some(page) = pages.next() else { break };
            let page = page?;
            validate()?;
            let rows = page
                .files
                .context("drive index listing omitted its files array")?;
            let Some(serde_json::Value::String(cursor)) = page.sync_time else {
                return Err(UnsupportedIndex::MissingWatermark.into());
            };
            let time = OffsetDateTime::parse(&cursor, &Rfc3339)
                .map_err(|_| UnsupportedIndex::MissingWatermark)?;
            ensure!(
                last_page.as_ref().is_none_or(|(prior, _)| time >= *prior),
                "drive sync_time moved backwards across listing pages"
            );
            // sync_time tracks the progress of this listing, which may span
            // months of item changes. Publish only the final page's exact value.
            last_page = Some((time, cursor));
            for row in rows {
                check_cancelled(cancelled)?;
                sink(row.change(old.is_none())?)?;
            }
        }
        let (stamp, cursor) = last_page.context("drive index listing returned no pages")?;
        if let Some(previous) = old {
            let last =
                OffsetDateTime::parse(&previous.cursor, &Rfc3339).map_err(|_| ExpiredCursor)?;
            ensure!(stamp >= last, ExpiredCursor);
        }
        let checked_at = now_unix();
        Ok(Checkpoint {
            cursor,
            checked_at,
            full_at: old.map_or(checked_at, |p| p.full_at),
        })
    }

    fn hydrate_index_folder(
        &self,
        parent: &str,
        cancelled: &AtomicBool,
        validate: &dyn Fn() -> Result<()>,
        sink: &mut dyn FnMut(IndexChange) -> Result<()>,
    ) -> Result<()> {
        validate()?;
        let mut pages = self.normal_list_pages::<IndexRow>(parent)?;
        loop {
            validate()?;
            let Some(page) = pages.next() else {
                return Ok(());
            };
            let rows = page?.context("scoped index listing omitted its files array")?;
            validate()?;
            for mut row in rows {
                check_cancelled(cancelled)?;
                if let Some(returned) = &row.parent_id {
                    ensure!(
                        returned == parent,
                        "scoped index listing returned an unexpected parent"
                    );
                } else {
                    row.parent_id = Some(parent.to_string());
                }
                sink(row.change(true)?)?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pikpak::parity_tests::scripted_server;
    use crate::pikpak::tests::{temp_test_dir, test_client};
    use serde_json::{Value, json};

    fn account(client: &PikPak, id: &str) {
        client.update_session(|s| s.user_id = id.into()).unwrap();
    }

    fn page(files: Value, cursor: &str, next: &str) -> String {
        json!({"files":files, "sync_time":cursor, "next_page_token":next}).to_string()
    }

    fn query(request: &str) -> HashMap<String, String> {
        let target = request
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap();
        reqwest::Url::parse(&format!("http://example.test{target}"))
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect()
    }

    #[test]
    fn index_sync_commits_final_page_progress_and_replays_equal_second() {
        let t1 = "2025-12-06T00:41:59.954+08:00";
        let t2 = "2026-11-24T16:24:46.970+08:00";
        let t3 = "2026-11-24T18:07:37.414+08:00";
        let dir = temp_test_dir("persistent-index-delta");
        let folder = json!({"id":"a","parent_id":"","kind":"drive#folder"});
        let file =
            json!({"id":"f","parent_id":"a","kind":"drive#file","size":"18","modified_time":t1});
        let update =
            json!({"id":"f","parent_id":"","kind":"drive#file","size":"21","modified_time":t2});
        let (base, requests, server) = scripted_server(vec![
            (200, page(json!([file]), t1, "next")),
            (200, page(json!([folder]), t2, "")),
            (200, page(json!([update.clone(), update]), t3, "")),
            (200, page(json!([{"id":"f","trashed":true}]), t3, "")),
        ]);
        let client = test_client(base, dir.join("session.json"));
        assert!(!client.folder_index_available());
        account(&client, "one");
        let running = AtomicBool::new(false);
        assert!(client.cached_folder_stats("").unwrap().is_none());
        assert!(!dir.join("folder-index").exists());
        client.sync_folder_index(&running, false).unwrap();
        let checkpoint = IndexStore::open_readonly(&client.index_path("one").unwrap())
            .unwrap()
            .checkpoint()
            .unwrap()
            .unwrap();
        assert_eq!(checkpoint.cursor, t2);
        assert_eq!(
            client
                .cached_folder_stats("a")
                .unwrap()
                .unwrap()
                .total_size_bytes,
            18
        );
        client.sync_folder_index(&running, false).unwrap();
        let stats = client
            .cached_folder_stats_batch(&["".into(), "a".into(), "unknown".into()])
            .unwrap();
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[""].total_size_bytes, 21);
        assert!(stats["a"].is_empty);
        client.sync_folder_index(&running, false).unwrap();
        assert_eq!(
            client
                .cached_folder_stats("")
                .unwrap()
                .unwrap()
                .total_size_bytes,
            0
        );
        account(&client, "two");
        assert!(client.cached_folder_stats("").unwrap().is_none());
        account(&client, "one");
        assert_eq!(
            client
                .cached_folder_stats("")
                .unwrap()
                .unwrap()
                .latest_modified_time,
            None
        );
        server.join().unwrap();
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 4);
        let qs: Vec<_> = requests.iter().map(|r| query(r)).collect();
        assert!(
            qs.iter()
                .all(|q| q["parent_id"] == "*" && q["limit"] == "500")
        );
        assert_eq!(qs[1]["page_token"], "next");
        let filter = |i: usize| serde_json::from_str::<Value>(&qs[i]["filters"]).unwrap();
        assert_eq!(
            filter(0),
            json!({"trashed":{"eq":false},"phase":{"eq":"PHASE_TYPE_COMPLETE"}})
        );
        assert_eq!(
            filter(2),
            json!({"modified_time":{"gt":"2026-11-24T16:24:45.970+08:00"},"phase":{"eq":"PHASE_TYPE_COMPLETE"}})
        );
        assert_eq!(
            filter(3)["modified_time"]["gt"],
            "2026-11-24T18:07:36.414+08:00"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn delta_overlap_keeps_offset_precision_and_crosses_calendar_boundaries() {
        for (cursor, expected) in [
            (
                "2026-01-01T00:00:00.000+00:00",
                "2025-12-31T23:59:59.000+00:00",
            ),
            (
                "2024-03-01T00:00:00.010000-04:30",
                "2024-02-29T23:59:59.010000-04:30",
            ),
            ("2026-11-24T03:00:00Z", "2026-11-24T02:59:59Z"),
        ] {
            assert_eq!(delta_lower_bound(cursor).unwrap(), expected);
        }
    }

    #[test]
    fn regressing_page_progress_rolls_back_without_replacing_the_checkpoint() {
        let first = "2025-12-06T00:41:59.954+08:00";
        let latest = "2026-11-24T16:24:46.970+08:00";
        let row = json!({"id":"file","parent_id":"","kind":"drive#file","size":7});
        let (base, _, server) = scripted_server(vec![
            (200, page(json!([row]), first, "")),
            (200, page(json!([]), latest, "more")),
            (200, page(json!([]), first, "")),
        ]);
        let dir = temp_test_dir("index-regressing-cursor");
        let client = test_client(base, dir.join("session.json"));
        account(&client, "user");
        let running = AtomicBool::new(false);
        client.sync_folder_index(&running, false).unwrap();
        let err = client.sync_folder_index(&running, true).unwrap_err();
        assert!(format!("{err:#}").contains("backwards"));
        assert_eq!(
            client
                .cached_folder_stats("")
                .unwrap()
                .unwrap()
                .total_size_bytes,
            7
        );
        assert_eq!(
            IndexStore::open_readonly(&client.index_path("user").unwrap())
                .unwrap()
                .checkpoint()
                .unwrap()
                .unwrap()
                .cursor,
            first
        );
        server.join().unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn index_sync_preserves_snapshot_across_broken_page_cancel_and_expired_cursor() {
        let time = "2026-11-24T03:00:00Z";
        let original = json!({"id":"f","parent_id":"","kind":"drive#file","size":"11"});
        let changed = json!({"id":"f","parent_id":"","kind":"drive#file","size":"90"});
        let (base, requests, server) = scripted_server(vec![
            (200, page(json!([original]), time, "")),
            (200, page(json!([changed.clone()]), time, "token")),
            (200, page(json!([]), time, "token")),
            (404, r#"{"error":"file_not_found"}"#.to_string()),
            (404, r#"{"error":"file_invalid_file_filters"}"#.to_string()),
            (200, page(json!([changed]), time, "")),
        ]);
        let dir = temp_test_dir("persistent-index-rollback");
        let client = test_client(base, dir.join("session.json"));
        account(&client, "id");
        let running = AtomicBool::new(false);
        client.sync_folder_index(&running, false).unwrap();
        assert!(client.sync_folder_index(&running, false).is_err());
        assert_eq!(
            client
                .cached_folder_stats("")
                .unwrap()
                .unwrap()
                .total_size_bytes,
            11
        );
        let err = client
            .sync_folder_index(&AtomicBool::new(true), true)
            .unwrap_err();
        assert!(matches!(
            err.downcast_ref(),
            Some(FolderStatsError::Cancelled)
        ));
        // Other 404 responses remain ordinary errors, so they cannot silently
        // trigger full-drive requests. Only the explicit invalid-filter code can.
        let err = client.sync_folder_index(&running, false).unwrap_err();
        assert!(format!("{err:#}").contains("file_not_found"));
        client.sync_folder_index(&running, false).unwrap();
        assert_eq!(
            client
                .cached_folder_stats("")
                .unwrap()
                .unwrap()
                .total_size_bytes,
            90
        );
        server.join().unwrap();
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 6);
        let last = query(&requests[5]);
        assert_eq!(
            serde_json::from_str::<Value>(&last["filters"]).unwrap()["trashed"],
            json!({"eq":false})
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn index_sync_hydrates_only_the_new_folder_and_its_descendants() {
        let time = "2026-11-24T03:00:00Z";
        let old = json!({"id":"old","parent_id":"","kind":"drive#folder"});
        let new = json!({"id":"new","parent_id":"","kind":"drive#folder"});
        let sub = json!({"id":"sub","kind":"drive#folder"});
        let file = json!({"id":"f","kind":"drive#file","size":19,"modified_time":time});
        let (base, requests, server) = scripted_server(vec![
            (200, page(json!([old]), time, "")),
            (200, page(json!([new]), time, "")),
            (200, json!({"files":[sub]}).to_string()),
            (200, json!({"files":[file]}).to_string()),
        ]);
        let dir = temp_test_dir("persistent-index-hydration");
        let client = test_client(base, dir.join("session.json"));
        account(&client, "user");
        let running = AtomicBool::new(false);
        client.sync_folder_index(&running, false).unwrap();
        client.sync_folder_index(&running, false).unwrap();
        assert_eq!(
            client
                .cached_folder_stats("new")
                .unwrap()
                .unwrap()
                .total_size_bytes,
            19
        );
        server.join().unwrap();
        let all = requests.lock().unwrap();
        let parents: Vec<_> = all
            .iter()
            .map(|request| query(request)["parent_id"].clone())
            .collect();
        assert_eq!(parents, ["*", "*", "new", "sub"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn disk_account_switch_disables_the_old_client_before_any_network() {
        let dir = temp_test_dir("persistent-index-switch");
        let mut client = test_client("http://invalid.invalid".into(), dir.join("session.json"));
        client.user_id = "first".into();
        account(&client, "first");
        assert!(client.folder_index_available());
        account(&client, "second");
        assert!(!client.folder_index_available());
        assert!(client.cached_folder_stats("").is_err());
        let err = client
            .sync_folder_index(&AtomicBool::new(false), false)
            .unwrap_err();
        assert!(err.to_string().contains("saved account changed"));
        assert!(!dir.join("folder-index").exists());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn periodic_reconciliation_removes_a_permanently_deleted_file() {
        let stamp = "2026-11-24T03:00:00Z";
        let file = json!({"id":"f","parent_id":"","kind":"drive#file","size":"11"});
        let (base, requests, server) = scripted_server(vec![
            (200, page(json!([file]), stamp, "")),
            (200, page(json!([]), stamp, "")),
            (200, page(json!([]), stamp, "")),
        ]);
        let dir = temp_test_dir("persistent-index-hard-delete");
        let client = test_client(base, dir.join("session.json"));
        account(&client, "user");
        let running = AtomicBool::new(false);
        client.sync_folder_index(&running, false).unwrap();
        client.sync_folder_index(&running, false).unwrap();
        assert_eq!(
            client
                .cached_folder_stats("")
                .unwrap()
                .unwrap()
                .total_size_bytes,
            11
        );
        let db = rusqlite::Connection::open(client.index_path("user").unwrap()).unwrap();
        db.execute(
            "UPDATE checkpoint SET full_at=?1",
            [now_unix() - RECONCILE_SECONDS],
        )
        .unwrap();
        drop(db);
        client.sync_folder_index(&running, false).unwrap();
        assert!(client.cached_folder_stats("").unwrap().unwrap().is_empty);
        server.join().unwrap();
        let all = requests.lock().unwrap();
        let filters: Vec<Value> = all
            .iter()
            .map(|r| serde_json::from_str(&query(r)["filters"]).unwrap())
            .collect();
        assert!(filters[1].get("modified_time").is_some());
        assert_eq!(filters[2]["trashed"]["eq"], false);
        fs::remove_dir_all(dir).unwrap();
    }

    fn wait_for_follower(client: &PikPak) {
        let path = client.index_path("user").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !FLIGHTS
            .lock()
            .unwrap()
            .get(&path)
            .is_some_and(|f| Arc::strong_count(f) >= 3)
        {
            assert!(
                std::time::Instant::now() < deadline,
                "follower never joined the active sync"
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn simultaneous_checks_coalesce_and_local_mutation_aborts_publication() {
        use crate::pikpak::{read_test_http_request, tests::write_response};
        use std::net::TcpListener;
        use std::sync::mpsc;
        use std::thread;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (started, waiting) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let server = thread::spawn(move || {
            for bytes in [11, 20, 30] {
                let (mut stream, _) = listener.accept().unwrap();
                read_test_http_request(&mut stream).unwrap();
                if bytes != 11 {
                    started.send(()).unwrap();
                    resume.recv_timeout(Duration::from_secs(6)).unwrap();
                }
                let entries = json!([{"id":"f","parent_id":"","kind":"drive#file","size":bytes}]);
                write_response(
                    &mut stream,
                    200,
                    "OK",
                    page(entries, "2026-11-24T03:00:00Z", "").as_bytes(),
                );
            }
        });
        let dir = temp_test_dir("persistent-index-coalesce");
        let client = Arc::new(test_client(base, dir.join("session.json")));
        account(&client, "user");
        client
            .sync_folder_index(&AtomicBool::new(false), false)
            .unwrap();
        let check = || {
            let client = client.clone();
            thread::spawn(move || client.sync_folder_index(&AtomicBool::new(false), false))
        };
        let leader = check();
        waiting.recv_timeout(Duration::from_secs(6)).unwrap();
        let follower = check();
        wait_for_follower(&client);
        assert_eq!(
            client
                .cached_folder_stats("")
                .unwrap()
                .unwrap()
                .total_size_bytes,
            11
        );
        release.send(()).unwrap();
        leader.join().unwrap().unwrap();
        follower.join().unwrap().unwrap();
        assert_eq!(
            client
                .cached_folder_stats("")
                .unwrap()
                .unwrap()
                .total_size_bytes,
            20
        );
        let worker = check();
        waiting.recv_timeout(Duration::from_secs(6)).unwrap();
        client.clear_ls_cache();
        release.send(()).unwrap();
        let err = worker.join().unwrap().unwrap_err();
        assert!(matches!(
            err.downcast_ref(),
            Some(FolderStatsError::Cancelled)
        ));
        assert_eq!(
            client
                .cached_folder_stats("")
                .unwrap()
                .unwrap()
                .total_size_bytes,
            20
        );
        server.join().unwrap();
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn legacy_watermark_uses_scoped_stats_but_explicit_sync_and_real_errors_stay_incomplete() {
        let normal =
            json!({"files":[{"id":"f","kind":"drive#file","name":"f","size":12}]}).to_string();
        let missing = json!({"files":[]}).to_string();
        let malformed = json!({"files":[],"sync_time":"unavailable"}).to_string();
        let (base, requests, server) = scripted_server(vec![
            (200, missing),
            (200, normal),
            (200, malformed),
            (503, "unavailable".into()),
            (200, "{}".into()),
        ]);
        let dir = temp_test_dir("index-watermark-compatibility");
        let client = test_client(base, dir.join("session.json"));
        account(&client, "user");
        let running = AtomicBool::new(false);
        let stats = client.folder_stats("a", &running).unwrap();
        assert_eq!(stats.total_size_bytes, 12);
        let explicit = client.sync_folder_index(&running, false).unwrap_err();
        assert!(matches!(
            explicit.downcast_ref(),
            Some(UnsupportedIndex::MissingWatermark)
        ));
        assert!(client.cached_folder_stats("").unwrap().is_none());
        for expected in ["503", "files array"] {
            let Err(FolderStatsError::ReadFolder { source, .. }) =
                client.folder_stats("a", &running)
            else {
                panic!("folder lookup must report real request or decoding failure");
            };
            assert!(format!("{source:#}").contains(expected), "{source:#}");
        }
        server.join().unwrap();
        let parents: Vec<_> = requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| query(r)["parent_id"].clone())
            .collect();
        assert_eq!(parents, ["*", "a", "*", "*", "*"]);
        fs::write(client.index_path("user").unwrap(), b"invalid database").unwrap();
        let Err(FolderStatsError::ReadFolder { source, .. }) = client.folder_stats("a", &running)
        else {
            panic!("expected the database error");
        };
        assert!(source.is::<rusqlite::Error>(), "{source:#}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn coalesced_sync_retains_unsupported_watermark_and_overflow_types() {
        use crate::pikpak::{read_test_http_request, tests::write_response};
        use std::{net::TcpListener, sync::mpsc, thread};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let dir = temp_test_dir("index-error-types");
        let client = Arc::new(test_client(
            format!("http://{}", listener.local_addr().unwrap()),
            dir.join("session.json"),
        ));
        account(&client, "user");
        let (started, waiting) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let entries = json!([{"id":"a","parent_id":"","kind":"drive#file","size":u64::MAX},
                           {"id":"b","parent_id":"","kind":"drive#file","size":1}]);
        let overflow = page(entries, "2026-11-24T03:00:00Z", "");
        let final_response = overflow.clone();
        let server = thread::spawn(move || {
            for body in [json!({"files":[]}).to_string(), overflow] {
                let (mut stream, _) = listener.accept().unwrap();
                read_test_http_request(&mut stream).unwrap();
                started.send(()).unwrap();
                resume.recv_timeout(Duration::from_secs(6)).unwrap();
                write_response(&mut stream, 200, "OK", body.as_bytes());
            }
            let (mut stream, _) = listener.accept().unwrap();
            read_test_http_request(&mut stream).unwrap();
            write_response(&mut stream, 200, "OK", final_response.as_bytes());
        });
        let run = || {
            let client = client.clone();
            thread::spawn(move || client.sync_folder_index(&AtomicBool::new(false), false))
        };
        for missing in [true, false] {
            let first = run();
            waiting.recv_timeout(Duration::from_secs(6)).unwrap();
            let second = run();
            wait_for_follower(&client);
            release.send(()).unwrap();
            for worker in [first, second] {
                let err = worker.join().unwrap().unwrap_err();
                if missing {
                    assert!(matches!(
                        err.downcast_ref(),
                        Some(UnsupportedIndex::MissingWatermark)
                    ));
                } else {
                    assert!(matches!(
                        err.downcast_ref(),
                        Some(FolderStatsError::SizeOverflow)
                    ));
                }
            }
            assert!(client.cached_folder_stats("").unwrap().is_none());
        }
        assert!(matches!(
            client.folder_stats("", &AtomicBool::new(false)),
            Err(FolderStatsError::SizeOverflow)
        ));
        server.join().unwrap();
        fs::remove_dir_all(dir).unwrap();
    }
}
