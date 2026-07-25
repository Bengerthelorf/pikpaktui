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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionToken {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at_unix: i64,
    /// Device identity established at login. Persisted because reference
    /// clients (alist, PikPakAPI) send x-device-id on every call; omitting it
    /// after a restart is a known cause of intermittent 403/riskLimited on
    /// download-link fetches. Defaults keep pre-existing session files valid.
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub captcha_token: String,
    /// Expiry of the action captcha token. Older session files omit this and
    /// deserialize to zero, which intentionally forces a refresh before reuse.
    #[serde(default)]
    pub captcha_expires_at_unix: i64,
    #[serde(default)]
    pub user_id: String,
}

impl SessionToken {
    pub fn is_expired(&self, now_unix: i64) -> bool {
        now_unix >= self.expires_at_unix
    }
}
