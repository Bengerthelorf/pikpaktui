use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Serialize, Serializer};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{FolderStatsSession, PikPak};

/// A completed scan of a folder in the normal (non-trash) drive listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FolderStats {
    /// Sum of the sizes of all descendant files, in bytes. JSON uses a
    /// decimal string so JNI clients can read the full unsigned range.
    #[serde(serialize_with = "serialize_byte_count")]
    pub total_size_bytes: u64,
    /// Original RFC 3339 timestamp of the most recently modified descendant
    /// file, compared by instant. None if no file had a valid timestamp.
    pub latest_modified_time: Option<String>,
    /// True when the queried folder had no visible children of either kind.
    pub is_empty: bool,
}

fn serialize_byte_count<S: Serializer>(bytes: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(bytes)
}

/// An incomplete scan never publishes partial totals.
#[derive(Debug, thiserror::Error)]
pub enum FolderStatsError {
    #[error("folder statistics cancelled")]
    Cancelled,
    #[error("could not list folder {folder_id:?}")]
    ReadFolder {
        folder_id: String,
        source: anyhow::Error,
    },
    #[error("folder size exceeds the u64 byte range")]
    SizeOverflow,
}

/// All listing strategies use the same byte arithmetic and timestamp ordering.
#[derive(Clone, Debug, Default)]
pub(super) struct FolderTotals {
    bytes: u64,
    latest: Option<(OffsetDateTime, String)>,
}

impl FolderTotals {
    pub(super) fn add_file(&mut self, bytes: u64, modified: &str) -> Result<(), FolderStatsError> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or(FolderStatsError::SizeOverflow)?;
        if let Ok(time) = OffsetDateTime::parse(modified, &Rfc3339)
            && self.latest.as_ref().is_none_or(|(current, original)| {
                // Equal instants can have different RFC 3339 representations.
                // Pick the same original string regardless of enumeration order.
                (time, modified) > (*current, original.as_str())
            })
        {
            self.latest = Some((time, modified.to_owned()));
        }
        Ok(())
    }

    pub(super) fn add_child(&mut self, child: &FolderStats) -> Result<(), FolderStatsError> {
        self.add_file(
            child.total_size_bytes,
            child.latest_modified_time.as_deref().unwrap_or_default(),
        )
    }

    pub(super) fn finish(self, is_empty: bool) -> FolderStats {
        FolderStats {
            total_size_bytes: self.bytes,
            latest_modified_time: self.latest.map(|(_, text)| text),
            is_empty,
        }
    }
}

pub(super) fn check_cancelled(cancelled: &AtomicBool) -> Result<(), FolderStatsError> {
    if cancelled.load(Ordering::Relaxed) {
        Err(FolderStatsError::Cancelled)
    } else {
        Ok(())
    }
}

impl PikPak {
    /// Scan by folder ID (the empty string selects the drive root).
    ///
    /// A new session reads fresh data. An entire-drive query can use a
    /// wildcard listing when multiple root folders need statistics. Share a
    /// `FolderStatsSession` between calls to reuse completed observations.
    /// Run this blocking operation on a worker and set its flag to cancel.
    /// An in-flight request must return or time out before its worker exits.
    pub fn folder_stats(
        &self,
        folder_id: &str,
        cancelled: &AtomicBool,
    ) -> Result<FolderStats, FolderStatsError> {
        FolderStatsSession::default().stats(self, folder_id, cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_root_query_uses_bulk_and_recovers_from_a_missing_root_file() {
        use crate::pikpak::parity_tests::scripted_server;
        use crate::pikpak::tests::{temp_test_dir, test_client};

        let a = serde_json::json!({"id":"a","parent_id":"","kind":"drive#folder","name":"a"});
        let b = serde_json::json!({"id":"b","parent_id":"","kind":"drive#folder","name":"b"});
        let f =
            serde_json::json!({"id":"zero","parent_id":"","kind":"drive#file","name":"z","size":0});
        let leaf = serde_json::json!({"id":"leaf","parent_id":"a","kind":"drive#file","name":"l",
            "size":17,"modified_time":"2026-01-01T00:00:00Z"});
        let page =
            |entries: &[&serde_json::Value]| serde_json::json!({"files":entries}).to_string();
        for missing in [false, true] {
            let mut responses = vec![(200, page(&[&a, &b, &f]))];
            if missing {
                responses.extend([
                    (200, page(&[&leaf, &b, &a])),
                    (200, page(&[&leaf])),
                    (200, page(&[])),
                ]);
            } else {
                responses.push((200, page(&[&leaf, &f, &b, &a])));
            }
            let (base, requests, server) = scripted_server(responses);
            let dir = temp_test_dir("adaptive-root-stats");
            let client = test_client(base, dir.join("session.json"));
            let stats = client.folder_stats("", &AtomicBool::new(false)).unwrap();
            assert_eq!(stats.total_size_bytes, 17);
            assert_eq!(
                stats.latest_modified_time.as_deref(),
                Some("2026-01-01T00:00:00Z")
            );
            assert!(!stats.is_empty);
            server.join().unwrap();
            let requests = requests.lock().unwrap();
            let parents: Vec<_> = requests
                .iter()
                .map(|request| {
                    let path = request.split_whitespace().nth(1).unwrap();
                    reqwest::Url::parse(&format!("http://localhost{path}"))
                        .unwrap()
                        .query_pairs()
                        .find(|(key, _)| key == "parent_id")
                        .unwrap()
                        .1
                        .into_owned()
                })
                .collect();
            if missing {
                assert_eq!(parents, ["", "*", "a", "b"]);
            } else {
                assert_eq!(parents, ["", "*"]);
            }
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn equal_instants_keep_the_same_original_text_across_listing_orders() {
        let utc = "2026-02-01T00:00:00Z";
        let offset = "2026-02-01T08:00:00+08:00";
        let mut first = FolderTotals::default();
        first.add_file(3, utc).unwrap();
        first.add_file(5, offset).unwrap();

        let mut child = FolderTotals::default();
        child.add_file(3, utc).unwrap();
        let mut other = FolderTotals::default();
        other.add_file(5, offset).unwrap();
        other.add_child(&child.finish(false)).unwrap();
        let stats = first.finish(false);
        assert_eq!(stats, other.finish(false));
        assert_eq!(stats.total_size_bytes, 8);
        assert_eq!(stats.latest_modified_time.as_deref(), Some(offset));
    }

    #[test]
    fn folder_stats_json_preserves_unsigned_bytes_and_nullable_time() {
        let stats = FolderStats {
            total_size_bytes: u64::MAX,
            latest_modified_time: None,
            is_empty: false,
        };
        assert_eq!(
            serde_json::to_value(stats).unwrap(),
            serde_json::json!({
                "total_size_bytes": "18446744073709551615",
                "latest_modified_time": null,
                "is_empty": false
            })
        );
    }
}
