use serde::Deserialize;

use super::models::{Entry, EntryKind};

#[derive(Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub(super) struct NormalListPage<T> {
    #[serde(default = "Option::default", deserialize_with = "present_files")]
    pub(super) files: Option<Vec<T>>,
    pub(super) next_page_token: Option<String>,
}

// A missing list is accepted by legacy per-folder callers. A present null
// or non-array is a malformed response, even on those older endpoints.
fn present_files<'de, D, T>(de: D) -> Result<Option<Vec<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Vec::deserialize(de).map(Some)
}

#[derive(Deserialize)]
pub(super) struct DriveListResponse {
    #[serde(default)]
    pub(super) files: Vec<DriveFile>,
    #[serde(default)]
    pub(super) next_page_token: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct DriveFileResponse {
    pub(super) file: DriveFile,
}

#[derive(Deserialize)]
pub(super) struct DriveFile {
    id: String,
    name: String,
    #[serde(default)]
    kind: String,
    #[serde(default, deserialize_with = "de_opt_u64")]
    size: Option<u64>,
    #[serde(default)]
    created_time: Option<String>,
    #[serde(default, deserialize_with = "de_modified_time")]
    modified_time: Option<String>,
    #[serde(default)]
    tags: Vec<DriveFileTag>,
    #[serde(default)]
    thumbnail_link: Option<String>,
    #[serde(default)]
    phase: Option<String>,
    #[serde(default)]
    audit: Option<serde_json::Value>,
}

// A malformed optional timestamp must not hide the rest of a listed file.
pub(super) fn de_modified_time<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Ok(match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(time) => Some(time),
        _ => None,
    })
}

#[derive(Deserialize)]
struct DriveFileTag {
    #[serde(default)]
    name: String,
}

impl DriveFile {
    fn is_starred(&self) -> bool {
        self.tags.iter().any(|t| t.name == "STAR")
    }

    pub(super) fn into_entry(self) -> Entry {
        let starred = self.is_starred();
        Entry {
            kind: if self.kind.contains("folder") {
                EntryKind::Folder
            } else {
                EntryKind::File
            },
            id: self.id,
            name: self.name,
            size: self.size.unwrap_or(0),
            created_time: self.created_time.unwrap_or_default(),
            modified_time: self.modified_time.unwrap_or_default(),
            starred,
            thumbnail_link: self.thumbnail_link,
            phase: self.phase,
            audit: self.audit,
        }
    }

    pub(super) fn into_folder_entry(self) -> Entry {
        let starred = self.is_starred();
        Entry {
            kind: EntryKind::Folder,
            id: self.id,
            name: self.name,
            size: 0,
            created_time: self.created_time.unwrap_or_default(),
            modified_time: self.modified_time.unwrap_or_default(),
            starred,
            thumbnail_link: self.thumbnail_link,
            phase: self.phase,
            audit: self.audit,
        }
    }
}

pub(super) fn de_opt_u64<'de, D>(deserializer: D) -> std::result::Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct U64Visitor;
    impl<'de> Visitor<'de> for U64Visitor {
        type Value = Option<u64>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("u64 or stringified u64 or null")
        }

        fn visit_none<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_u64<E: de::Error>(self, value: u64) -> std::result::Result<Self::Value, E> {
            Ok(Some(value))
        }

        fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Self::Value, E> {
            // PikPak sometimes returns size as "" (and could send other
            // non-numeric values). Treat anything unparseable as absent rather
            // than failing — otherwise one bad entry aborts the whole listing.
            Ok(value.parse::<u64>().ok())
        }

        fn visit_string<E: de::Error>(self, value: String) -> std::result::Result<Self::Value, E> {
            self.visit_str(&value)
        }
    }

    deserializer.deserialize_any(U64Visitor)
}
