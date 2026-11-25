//! A bounded page from the committed whole-drive index, ordered like Android's file list.
use std::{cmp::Ordering, collections::BinaryHeap};

use anyhow::{Result, ensure};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub(crate) enum MobileFileSort {
    #[default]
    Name,
    Created,
    Recent,
    ContentModified,
    Kind,
    Size,
}

#[derive(Deserialize)]
pub(crate) struct MobileSearchQuery {
    pub query: String,
    #[serde(default)]
    pub sort: MobileFileSort,
    #[serde(default)]
    pub descending: bool,
    #[serde(default)]
    pub folder_stats: bool,
    #[serde(default = "page_size")]
    pub limit: usize,
    pub after: Option<MobileSearchFile>,
}

fn page_size() -> usize {
    300
}

impl MobileSearchQuery {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            (1..=page_size()).contains(&self.limit),
            "search page limit must be between 1 and 300"
        );
        ensure!(!self.query.trim().is_empty(), "search query is empty");
        if let Some(cursor) = &self.after {
            ensure!(!cursor.id.is_empty(), "missing search cursor ID");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum MobileKind {
    File,
    Folder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MobileFolderStats {
    // Decimal strings preserve the API's full unsigned byte range on the JVM.
    bytes: String,
    modified_at: Option<String>,
    is_empty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MobileSearchFile {
    pub id: String,
    pub name: String,
    pub kind: MobileKind,
    #[serde(default)]
    pub parent_id: String,
    pub size: Option<String>,
    #[serde(default)]
    pub created_time: String,
    #[serde(default)]
    pub modified_time: String,
    #[serde(default)]
    pub starred: bool,
    pub thumbnail_link: Option<String>,
    pub folder_stats: Option<MobileFolderStats>,
}

#[derive(Serialize)]
pub(crate) struct MobileSearchPage {
    pub checked_at: i64,
    pub files: Vec<MobileSearchFile>,
    pub count: u64,
    pub has_next: bool,
}

fn category(file: &MobileSearchFile) -> u8 {
    if file.kind == MobileKind::Folder {
        return 0;
    }
    let ext = file
        .name
        .rsplit_once('.')
        .map_or("", |(_, ext)| ext)
        .to_ascii_lowercase();
    match ext.as_str() {
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "wmv" | "m4v" | "ts" | "rmvb" | "flv" => 1,
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "heic" | "avif" => 2,
        "mp3" | "flac" | "wav" | "ogg" | "m4a" | "aac" | "opus" => 3,
        "txt" | "md" | "csv" | "json" | "xml" | "yaml" | "yml" | "log" | "srt" | "ass" | "ini"
        | "toml" => 4,
        _ => 5,
    }
}

fn instant(stamp: &str) -> Option<i128> {
    OffsetDateTime::parse(stamp, &Rfc3339)
        .ok()
        .map(|t| t.unix_timestamp_nanos())
}

// Java/Kotlin's equalsIgnoreCase uses simple single-character case conversions.
// Unlike full string lowercasing, this matches final sigma and dotted/dotless I.
fn search_fold(text: &str) -> String {
    text.chars()
        .map(|ch| {
            let mut upper = ch.to_uppercase();
            let first = upper.next().unwrap_or(ch);
            let upper = if upper.next().is_none() { first } else { ch };
            upper.to_lowercase().next().unwrap_or(upper)
        })
        .collect()
}

struct Ranked {
    file: MobileSearchFile,
    sort: MobileFileSort,
    descending: bool,
    numeric: Option<i128>,
    // Java String.compareTo uses UTF-16 code units, including for astral characters.
    folded_name: Vec<u16>,
}

impl Ranked {
    fn new(file: MobileSearchFile, options: &MobileSearchQuery) -> Self {
        let folder = file.kind == MobileKind::Folder;
        let stats = file.folder_stats.as_ref().filter(|_| options.folder_stats);
        let numeric = match options.sort {
            MobileFileSort::Created => instant(&file.created_time),
            MobileFileSort::Recent => instant(&file.modified_time),
            MobileFileSort::ContentModified if folder => stats
                .and_then(|s| s.modified_at.as_deref())
                .and_then(instant),
            MobileFileSort::ContentModified => instant(&file.modified_time),
            MobileFileSort::Kind => Some(category(&file).into()),
            MobileFileSort::Size => {
                let size = if folder {
                    stats.map(|s| s.bytes.as_str())
                } else {
                    file.size.as_deref()
                };
                size.and_then(|s| s.parse::<u64>().ok()).map(i128::from)
            }
            MobileFileSort::Name => None,
        };
        let folded_name = file.name.to_lowercase().encode_utf16().collect();
        Self {
            file,
            sort: options.sort,
            descending: options.descending,
            numeric,
            folded_name,
        }
    }
}

impl Ord for Ranked {
    fn cmp(&self, other: &Self) -> Ordering {
        let direction = |order: Ordering| {
            if self.descending {
                order.reverse()
            } else {
                order
            }
        };
        let primary = if matches!(self.sort, MobileFileSort::Name) {
            (other.file.kind == MobileKind::Folder)
                .cmp(&(self.file.kind == MobileKind::Folder))
                .then_with(|| direction(self.folded_name.cmp(&other.folded_name)))
        } else {
            other
                .numeric
                .is_some()
                .cmp(&self.numeric.is_some())
                .then_with(|| direction(self.numeric.cmp(&other.numeric)))
                .then_with(|| {
                    if matches!(self.sort, MobileFileSort::ContentModified) {
                        let unknown = |file: &MobileSearchFile| {
                            file.kind == MobileKind::Folder && file.folder_stats.is_none()
                        };
                        unknown(&self.file).cmp(&unknown(&other.file))
                    } else {
                        Ordering::Equal
                    }
                })
        };
        primary
            .then_with(|| self.folded_name.cmp(&other.folded_name))
            .then_with(|| {
                self.file
                    .id
                    .encode_utf16()
                    .cmp(other.file.id.encode_utf16())
            })
    }
}
impl PartialOrd for Ranked {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Ranked {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for Ranked {}

pub(super) fn page(
    db: &Connection,
    query: &MobileSearchQuery,
    checked_at: i64,
) -> Result<MobileSearchPage> {
    // The rooted traversal excludes descendants of detached/trashed folders.
    let mut stmt = db.prepare(
        "WITH RECURSIVE visible(id) AS (
        SELECT id FROM items WHERE parent=''
        UNION ALL SELECT i.id FROM items i JOIN visible ON i.parent=visible.id
    ) SELECT i.id,i.parent,i.name,i.folder,i.size,i.created,i.modified,i.starred,i.thumbnail,
        i.total,i.latest,i.children=0 FROM visible JOIN items i ON i.id=visible.id",
    )?;
    let text = search_fold(query.query.trim());
    let mut cursor_file = query.after.clone();
    if !query.folder_stats
        && let Some(file) = &mut cursor_file
    {
        file.folder_stats = None;
    }
    let after = cursor_file.map(|c| Ranked::new(c, query));
    let mut kept = BinaryHeap::with_capacity(query.limit);
    let (mut count, mut remaining) = (0u64, 0u64);
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(2)?;
        if !search_fold(&name).contains(&text) {
            continue;
        }
        count += 1;
        let folder: bool = row.get(3)?;
        let folder_stats = if folder && query.folder_stats {
            Some(MobileFolderStats {
                bytes: row.get(9)?,
                modified_at: row.get(10)?,
                is_empty: row.get(11)?,
            })
        } else {
            None
        };
        let candidate = Ranked::new(
            MobileSearchFile {
                id: row.get(0)?,
                parent_id: row.get(1)?,
                name,
                kind: if folder {
                    MobileKind::Folder
                } else {
                    MobileKind::File
                },
                size: row.get(4)?,
                created_time: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                modified_time: row.get(6)?,
                starred: row.get(7)?,
                thumbnail_link: row.get(8)?,
                folder_stats,
            },
            query,
        );
        if after.as_ref().is_some_and(|c| candidate <= *c) {
            continue;
        }
        remaining += 1;
        if kept.len() < query.limit {
            kept.push(candidate);
        } else if let Some(mut worst) = kept.peek_mut()
            && candidate < *worst
        {
            *worst = candidate;
        }
    }
    let has_next = remaining > kept.len() as u64;
    let files = kept.into_sorted_vec().into_iter().map(|r| r.file).collect();
    Ok(MobileSearchPage {
        checked_at,
        files,
        count,
        has_next,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(
        id: &str,
        name: &str,
        kind: &str,
        size: &str,
        created: &str,
        modified: &str,
    ) -> MobileSearchFile {
        serde_json::from_value(json!({
            "id":id,"name":name,"kind":kind,"size":size,"created_time":created,"modified_time":modified
        })).unwrap()
    }

    #[test]
    fn mobile_sort_matches_folder_placement_missing_values_and_direction() {
        let earlier = "2026-11-02T01:00:00+01:00";
        let later = "2026-11-02T00:00:00.010Z";
        let mut folder = row("f", "zz", "Folder", "0", earlier, "missing");
        folder.folder_stats = Some(MobileFolderStats {
            bytes: "18446744073709551615".into(),
            modified_at: Some(later.into()),
            is_empty: false,
        });
        let files = [
            row("z", "A.mp4", "File", "12", later, earlier),
            row("a", "a.mp4", "File", "12", "", earlier),
            row("t", "note.TXT", "File", "0", earlier, later),
            folder,
        ];

        let cases = [
            ("Name", false, true, "fazt"),
            ("Name", true, true, "ftaz"),
            ("Size", true, true, "fazt"),
            ("Size", false, true, "tazf"),
            ("Size", true, false, "aztf"),
            ("Kind", true, true, "tazf"),
            ("Recent", false, true, "aztf"),
            ("Recent", true, true, "tazf"),
            ("Created", true, true, "ztfa"),
            ("Created", false, true, "tfza"),
            ("ContentModified", true, true, "tfaz"),
        ];
        for (sort, descending, folder_stats, expected) in cases {
            let options: MobileSearchQuery = serde_json::from_value(json!({"query":"x","sort":sort,"descending":descending,"folder_stats":folder_stats})).unwrap();
            let mut ranked: Vec<_> = files
                .iter()
                .cloned()
                .map(|file| Ranked::new(file, &options))
                .collect();
            ranked.sort_unstable();
            let ids: String = ranked.into_iter().map(|f| f.file.id).collect();
            assert_eq!(
                ids, expected,
                "{sort}, descending={descending}, stats={folder_stats}"
            );
        }
    }

    #[test]
    fn android_name_matching_and_sorting_handle_unicode() {
        assert_eq!(search_fold("İstanbul"), search_fold("ıSTANBUL"));
        assert_eq!(search_fold("Σςσ"), search_fold("σσσ"));
        assert_ne!(search_fold("ß"), search_fold("ss"));
        let options: MobileSearchQuery = serde_json::from_value(json!({"query":"a"})).unwrap();
        let utf16_first = Ranked::new(row("1", "😀", "File", "1", "", ""), &options);
        let utf16_second = Ranked::new(row("2", "\u{e000}", "File", "1", "", ""), &options);
        assert!(utf16_first < utf16_second);
    }
}
