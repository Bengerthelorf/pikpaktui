use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum EntryKind {
    Folder,
    File,
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    pub created_time: String,
    pub modified_time: String,
    pub starred: bool,
    pub thumbnail_link: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionToken {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at_unix: i64,
}

impl SessionToken {
    pub fn is_expired(&self, now_unix: i64) -> bool {
        now_unix >= self.expires_at_unix
    }
}
