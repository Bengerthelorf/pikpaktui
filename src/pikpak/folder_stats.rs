use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Serialize, Serializer};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{EntryKind, PikPak};

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

impl PikPak {
    /// Scan by folder ID (the empty string selects the drive root).
    ///
    /// Call this blocking operation on a worker. Each scan reads one page at
    /// a time. Set its flag from another thread to cancel; an in-flight
    /// blocking request returns or times out before cancellation is observed.
    pub fn folder_stats(
        &self,
        folder_id: &str,
        cancelled: &AtomicBool,
    ) -> Result<FolderStats, FolderStatsError> {
        let check_cancelled = || {
            if cancelled.load(Ordering::Relaxed) {
                Err(FolderStatsError::Cancelled)
            } else {
                Ok(())
            }
        };
        let mut pending = vec![folder_id.to_owned()];
        let mut visited = HashSet::new();
        let mut total_size_bytes = 0u64;
        let mut latest: Option<(OffsetDateTime, String)> = None;
        let mut is_empty = true;

        while let Some(id) = pending.pop() {
            check_cancelled()?;
            if !visited.insert(id.clone()) {
                continue;
            }
            let read_error = |source| FolderStatsError::ReadFolder {
                folder_id: id.clone(),
                source,
            };
            let pages = self.ls_pages(&id);
            check_cancelled()?;
            for page in pages.map_err(read_error)? {
                check_cancelled()?;
                let entries = page.map_err(read_error)?;
                is_empty &= entries.is_empty();
                for entry in entries {
                    match entry.kind {
                        EntryKind::Folder => {
                            if entry.id.is_empty() {
                                return Err(read_error(anyhow::anyhow!("subfolder has no ID")));
                            }
                            pending.push(entry.id);
                        }
                        EntryKind::File => {
                            total_size_bytes = total_size_bytes
                                .checked_add(entry.size)
                                .ok_or(FolderStatsError::SizeOverflow)?;
                            if let Ok(time) = OffsetDateTime::parse(&entry.modified_time, &Rfc3339)
                                && latest.as_ref().is_none_or(|(current, _)| time > *current)
                            {
                                latest = Some((time, entry.modified_time));
                            }
                        }
                    }
                }
                check_cancelled()?;
            }
        }
        check_cancelled()?;
        Ok(FolderStats {
            total_size_bytes,
            latest_modified_time: latest.map(|(_, timestamp)| timestamp),
            is_empty,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
