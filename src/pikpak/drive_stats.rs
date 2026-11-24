use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;

use anyhow::anyhow;
use serde::Deserialize;

use super::drive::{de_modified_time, de_opt_u64};
use super::folder_stats::{FolderTotals, check_cancelled};
use super::{Entry, EntryKind, FolderStats, FolderStatsError, PikPak};

/// Unlike a single-folder listing, the wildcard listing must identify the
/// parent and kind of every item before it can attribute its statistics.
#[derive(Deserialize)]
struct DriveRow {
    id: String,
    parent_id: String,
    kind: DriveKind,
    #[serde(default, deserialize_with = "de_opt_u64")]
    size: Option<u64>,
    #[serde(default, deserialize_with = "de_modified_time")]
    modified_time: Option<String>,
}

#[derive(Deserialize)]
enum DriveKind {
    #[serde(rename = "drive#folder")]
    Folder,
    #[serde(rename = "drive#file")]
    File,
}

#[derive(Default)]
struct IndexedFolder {
    parent: Option<String>,
    pending_children: usize,
    has_children: bool,
    totals: FolderTotals,
}

fn index_error(source: anyhow::Error) -> FolderStatsError {
    FolderStatsError::ReadFolder {
        folder_id: "*".into(),
        source,
    }
}

#[derive(Default)]
struct DriveIndex {
    seen: HashSet<String>,
    folders: HashMap<String, IndexedFolder>,
    root_file_count: usize,
}

/// Independent observations from a normal root listing. Only the directory
/// IDs and direct file statistics are needed, even for a very large root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RootListing {
    pub(crate) folders: HashSet<String>,
    pub(super) files: FolderStats,
    pub(super) file_count: usize,
}

impl RootListing {
    pub(crate) fn from_entries(entries: &[Entry]) -> Result<Self, FolderStatsError> {
        let mut seen = HashSet::new();
        let mut folders = HashSet::new();
        let mut totals = FolderTotals::default();
        let mut file_count = 0;
        for entry in entries {
            if entry.id.is_empty() || entry.id == "*" || !seen.insert(entry.id.as_str()) {
                return Err(FolderStatsError::ReadFolder {
                    folder_id: String::new(),
                    source: anyhow!("invalid or duplicate item ID in normal root listing"),
                });
            }
            match entry.kind {
                EntryKind::File => {
                    totals.add_file(entry.size, &entry.modified_time)?;
                    file_count += 1;
                }
                EntryKind::Folder => {
                    folders.insert(entry.id.clone());
                }
            }
        }
        Ok(Self {
            folders,
            files: totals.finish(entries.is_empty()),
            file_count,
        })
    }
}

impl DriveIndex {
    fn add(&mut self, row: DriveRow) -> Result<(), FolderStatsError> {
        if row.id.is_empty() || row.id == "*" || row.parent_id == "*" {
            return Err(index_error(anyhow!(
                "invalid item or parent ID in drive listing"
            )));
        }
        if !self.seen.insert(row.id.clone()) {
            return Err(index_error(anyhow!(
                "duplicate item ID across drive listing"
            )));
        }
        let parent = self.folders.entry(row.parent_id.clone()).or_default();
        parent.has_children = true;
        match row.kind {
            DriveKind::File => {
                if row.parent_id.is_empty() {
                    self.root_file_count += 1;
                }
                parent.totals.add_file(
                    row.size.unwrap_or(0),
                    row.modified_time.as_deref().unwrap_or_default(),
                )?;
            }
            DriveKind::Folder => {
                parent.pending_children += 1;
                self.folders.entry(row.id).or_default().parent = Some(row.parent_id);
            }
        }
        Ok(())
    }

    fn finish(
        mut self,
        cancelled: &AtomicBool,
        expected: Option<&RootListing>,
    ) -> Result<HashMap<String, FolderStats>, FolderStatsError> {
        drop(self.seen);
        let root = self.folders.entry(String::new()).or_default();
        let mismatch = || {
            index_error(anyhow!(
                "wildcard listing differs from the normal root listing"
            ))
        };
        if let Some(expected) = expected {
            let direct = std::mem::take(&mut root.totals).finish(!root.has_children);
            if self.root_file_count != expected.file_count
                || root.pending_children != expected.folders.len()
                || direct != expected.files
            {
                return Err(mismatch());
            }
            root.totals.add_child(&direct)?;
        }
        let mut ready = Vec::new();
        for (id, folder) in &self.folders {
            check_cancelled(cancelled)?;
            if folder.parent.as_deref() == Some("")
                && expected.is_some_and(|root| !root.folders.contains(id))
            {
                return Err(mismatch());
            }
            if !id.is_empty() && folder.parent.is_none() {
                return Err(index_error(anyhow!(
                    "drive listing names a missing or non-folder parent"
                )));
            }
            if folder.pending_children == 0 {
                ready.push(id.clone());
            }
        }
        // Propagate once per folder, after every child has contributed.
        // Cycles leave unprocessed folders instead of looping or publishing.
        let mut result = HashMap::with_capacity(self.folders.len());
        while let Some(id) = ready.pop() {
            check_cancelled(cancelled)?;
            let folder = self.folders.remove(&id).expect("ready folder is present");
            let stats = folder.totals.finish(!folder.has_children);
            if let Some(parent_id) = folder.parent {
                let parent = self
                    .folders
                    .get_mut(&parent_id)
                    .expect("parent awaits this child");
                parent.totals.add_child(&stats)?;
                parent.pending_children -= 1;
                if parent.pending_children == 0 {
                    ready.push(parent_id);
                }
            }
            result.insert(id, stats);
        }
        if !self.folders.is_empty() {
            return Err(index_error(anyhow!("cycle in drive listing")));
        }
        check_cancelled(cancelled)?;
        Ok(result)
    }
}

impl PikPak {
    /// Compute all visible folders' recursive statistics from one paginated
    /// full-drive listing. The map includes the drive root under the empty ID.
    /// Like `folder_stats`, call this on a worker with a per-operation cancel flag.
    /// An unreadable page or inconsistent parent graph invalidates the whole result.
    pub fn drive_folder_stats(
        &self,
        cancelled: &AtomicBool,
    ) -> Result<HashMap<String, FolderStats>, FolderStatsError> {
        self.drive_folder_stats_checked(cancelled, None)
    }

    pub(super) fn drive_folder_stats_checked(
        &self,
        cancelled: &AtomicBool,
        expected: Option<&RootListing>,
    ) -> Result<HashMap<String, FolderStats>, FolderStatsError> {
        self.drive_folder_stats_while(cancelled, expected, || true)
    }

    pub(super) fn drive_folder_stats_while(
        &self,
        cancelled: &AtomicBool,
        expected: Option<&RootListing>,
        still_current: impl Fn() -> bool,
    ) -> Result<HashMap<String, FolderStats>, FolderStatsError> {
        let check_page = || {
            check_cancelled(cancelled)?;
            if still_current() {
                Ok(())
            } else {
                Err(FolderStatsError::Cancelled)
            }
        };
        check_page()?;
        let pages = self.normal_list_pages::<DriveRow>("*");
        check_page()?;
        let mut pages = pages.map_err(index_error)?;
        let mut index = DriveIndex::default();
        loop {
            check_page()?;
            let page = pages.next();
            check_page()?;
            let Some(page) = page else { break };
            let rows = page
                .map_err(index_error)?
                .ok_or_else(|| index_error(anyhow!("drive listing is missing files")))?;
            for row in rows {
                check_cancelled(cancelled)?;
                index.add(row)?;
            }
        }
        index.finish(cancelled, expected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pikpak::parity_tests::scripted_server;
    use crate::pikpak::tests::{temp_test_dir, test_client};
    use std::fs;
    #[test]
    fn drive_folder_stats_matches_scoped_walk_across_out_of_order_pages() {
        // Wildcard pages need not group siblings or return parents first.
        let file = r#"{"id":"leaf","name":"leaf","kind":"drive#file","size":"12","modified_time":"2026-11-02T01:00:00+02:00","parent_id":"nested"}"#;
        let recent = r#"{"id":"recent","name":"recent","kind":"drive#file","size":3,"modified_time":"2026-11-01T23:00:00.0001Z","parent_id":"nested"}"#;
        let nested = r#"{"id":"nested","name":"nested","kind":"drive#folder","size":"999","modified_time":"2099-01-01T00:00:00Z","parent_id":"outer"}"#;
        let empty = r#"{"id":"empty","name":"empty","kind":"drive#folder","parent_id":""}"#;
        let outer = r#"{"id":"outer","name":"outer","kind":"drive#folder","parent_id":""}"#;
        let (base, requests, server) = scripted_server(vec![
            (
                200,
                format!(r#"{{"files":[{file},{empty}],"next_page_token":"second"}}"#),
            ),
            (200, format!(r#"{{"files":[{recent},{nested},{outer}]}}"#)),
            (200, format!(r#"{{"files":[{nested}]}}"#)),
            (200, format!(r#"{{"files":[{file},{recent}]}}"#)),
        ]);
        let dir = temp_test_dir("drive-stats-parity");
        let client = test_client(base, dir.join("session.json"));
        let cancel = AtomicBool::new(false);
        let index = client.drive_folder_stats(&cancel).unwrap();
        let scoped = client.folder_stats("outer", &cancel).unwrap();
        assert_eq!(index["outer"], scoped);
        assert_eq!(index[""], scoped);
        assert_eq!(index.len(), 4);
        assert_eq!(index["nested"].total_size_bytes, 15);
        assert_eq!(index["outer"], index["nested"]);
        assert_eq!(
            index["outer"].latest_modified_time.as_deref(),
            Some("2026-11-01T23:00:00.0001Z")
        );
        assert!(index["empty"].is_empty);
        server.join().unwrap();
        let requests = requests.lock().unwrap();
        for (i, request) in requests.iter().take(2).enumerate() {
            let path = request.split_whitespace().nth(1).unwrap();
            let url = reqwest::Url::parse(&format!("http://localhost{path}")).unwrap();
            let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(query["parent_id"], "*");
            assert_eq!(
                query.get("page_token").map(|s| s.as_ref()),
                (i == 1).then_some("second")
            );
            let filters: serde_json::Value = serde_json::from_str(&query["filters"]).unwrap();
            assert_eq!(
                filters,
                serde_json::json!({"trashed":{"eq":false},"phase":{"eq":"PHASE_TYPE_COMPLETE"}})
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn checked_drive_stats_rejects_root_scope_mismatches_before_returning_totals() {
        // The ID is opaque, and a missing zero-byte untimed file must still
        // invalidate the index even though SUM and MAX alone cannot detect it.
        let folder = r#"{"id":"folder:opaque","kind":"drive#folder","parent_id":""}"#;
        let zero = r#"{"id":"zero","kind":"drive#file","parent_id":"","size":0}"#;
        let file = r#"{"id":"file","kind":"drive#file","parent_id":"","size":"9","modified_time":"2026-11-01T00:00:00Z"}"#;
        let exact = format!(r#"{{"files":[{folder},{file},{zero}]}}"#);
        let (base, requests, server) = scripted_server(vec![
            (200, format!(r#"{{"files":[{file},{zero}]}}"#)),
            (200, exact.replace("folder:opaque", "unexpected")),
            (200, format!(r#"{{"files":[{folder},{file}]}}"#)),
            (200, exact.replace(r#""size":"9""#, r#""size":"8""#)),
            (200, exact.replace("2026-11-01", "2026-10-31")),
            (200, exact),
        ]);
        let mut root_entries: Vec<_> = [folder, file, zero]
            .iter()
            .map(|row| {
                let mut row: serde_json::Value = serde_json::from_str(row).unwrap();
                row["name"] = serde_json::json!("item");
                serde_json::from_value::<crate::pikpak::drive::DriveFile>(row)
                    .unwrap()
                    .into_entry()
            })
            .collect();
        let expected = RootListing::from_entries(&root_entries).unwrap();
        assert_eq!(expected.file_count, 2);
        assert_eq!(expected.files.total_size_bytes, 9);
        root_entries.push(root_entries[0].clone());
        assert!(RootListing::from_entries(&root_entries).is_err());
        let dir = temp_test_dir("drive-stats-root-scope");
        let client = test_client(base, dir.join("session.json"));
        let cancel = AtomicBool::new(false);
        for _ in 0..5 {
            let error = client
                .drive_folder_stats_checked(&cancel, Some(&expected))
                .unwrap_err();
            let FolderStatsError::ReadFolder { folder_id, source } = error else {
                panic!("unexpected error: {error:?}");
            };
            assert_eq!(folder_id, "*");
            assert!(source.to_string().contains("normal root listing"));
        }
        let results = client
            .drive_folder_stats_checked(&cancel, Some(&expected))
            .unwrap();
        assert_eq!(results[""], expected.files);
        assert!(results["folder:opaque"].is_empty);
        server.join().unwrap();
        assert_eq!(requests.lock().unwrap().len(), 6);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn drive_folder_stats_rejects_incomplete_graphs_and_failed_pages() {
        let (base, _, server) = scripted_server(vec![
            (200, r#"{}"#),
            (200, r#"{"files":[{"id":"file","kind":"drive#file"}]}"#),
            (
                200,
                r#"{"files":[{"parent_id":"missing","id":"file","kind":"drive#file"}]}"#,
            ),
            (
                200,
                r#"{"files":[{"parent_id":"","id":"a","kind":"drive#file"},{"parent_id":"a","id":"b","kind":"drive#file"}]}"#,
            ),
            (
                200,
                r#"{"files":[{"parent_id":"b","id":"a","kind":"drive#folder"},{"parent_id":"a","id":"b","kind":"drive#folder"}]}"#,
            ),
            (
                200,
                r#"{"files":[{"parent_id":"","id":"*","kind":"drive#folder"}]}"#,
            ),
            (
                200,
                r#"{"files":[{"parent_id":"","id":"a","kind":"unknown"}]}"#,
            ),
            (
                200,
                r#"{"files":[{"parent_id":"","id":"a","kind":"drive#folder"}],"next_page_token":"next"}"#,
            ),
            (
                200,
                r#"{"files":[{"parent_id":"","id":"a","kind":"drive#file"}]}"#,
            ),
            (200, r#"{"files":[],"next_page_token":"next"}"#),
            (200, r#"{"files":[],"next_page_token":"next"}"#),
            (200, r#"{"files":[],"next_page_token":"next"}"#),
            (500, r#"{"error":"page unavailable"}"#),
            (200, r#"{"files":[]}"#),
        ]);
        let dir = temp_test_dir("drive-stats-errors");
        let client = test_client(base, dir.join("session.json"));
        assert!(matches!(
            client.drive_folder_stats(&AtomicBool::new(true)),
            Err(FolderStatsError::Cancelled)
        ));
        let cancel = AtomicBool::new(false);
        for expected in [
            "missing files",
            "parent_id",
            "missing or non-folder",
            "missing or non-folder",
            "cycle",
            "invalid item",
            "unknown variant",
            "duplicate item",
            "repeated page token",
            "page unavailable",
        ] {
            let error = client.drive_folder_stats(&cancel).unwrap_err();
            let FolderStatsError::ReadFolder { folder_id, source } = error else {
                panic!("unexpected error: {error:?}");
            };
            assert_eq!(folder_id, "*");
            assert!(format!("{source:#}").contains(expected), "{source:#}");
        }
        let empty = client.drive_folder_stats(&cancel).unwrap();
        assert_eq!(empty.len(), 1);
        assert!(empty[""].is_empty);
        server.join().unwrap();
        fs::remove_dir_all(dir).unwrap();
    }
}
