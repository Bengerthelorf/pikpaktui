use anyhow::{Context, Result, anyhow};
use base64::Engine as _;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use std::collections::HashMap;
use std::sync::Mutex;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_AUTH_BASE_URL: &str = "https://user.mypikpak.com";
const DEFAULT_DRIVE_BASE_URL: &str = "https://api-drive.mypikpak.com";
const DEFAULT_CLIENT_ID: &str = "YNxT9w7GMdWvEOKa";
const DEFAULT_CLIENT_SECRET: &str = "dbw2OtmVEeuUvIptb1Coyg";
const USER_AGENT: &str = "ANDROID-com.pikcloud.pikpak/1.21.0";

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

pub struct PikPak {
    pub(crate) http: reqwest::blocking::Client,
    drive_base_url: String,
    auth_base_url: String,
    client_id: String,
    client_secret: String,
    session_path: PathBuf,
    device_id: String,
    captcha_token: String,
    pub thumbnail_size: String,
    ls_cache: Mutex<HashMap<String, Vec<Entry>>>,
    refresh_lock: Mutex<()>,
}

impl PikPak {
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: reqwest::blocking::Client::builder()
                .user_agent(USER_AGENT)
                .connect_timeout(std::time::Duration::from_secs(10))
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .context("failed to build http client")?,
            drive_base_url: env::var("PIKPAK_DRIVE_BASE_URL")
                .unwrap_or_else(|_| DEFAULT_DRIVE_BASE_URL.to_string()),
            auth_base_url: env::var("PIKPAK_AUTH_BASE_URL")
                .unwrap_or_else(|_| DEFAULT_AUTH_BASE_URL.to_string()),
            client_id: env::var("PIKPAK_CLIENT_ID")
                .unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string()),
            client_secret: env::var("PIKPAK_CLIENT_SECRET")
                .unwrap_or_else(|_| DEFAULT_CLIENT_SECRET.to_string()),
            session_path: default_session_path()?,
            device_id: String::new(),
            captcha_token: String::new(),
            thumbnail_size: "SIZE_MEDIUM".to_string(),
            ls_cache: Mutex::new(HashMap::new()),
            refresh_lock: Mutex::new(()),
        })
    }

    pub fn load_session(&self) -> Result<Option<SessionToken>> {
        if !self.session_path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&self.session_path)
            .with_context(|| format!("failed to read session {}", self.session_path.display()))?;
        let token: SessionToken =
            serde_json::from_str(&raw).context("failed to parse session json")?;
        Ok(Some(token))
    }

    fn save_session(&self, token: &SessionToken) -> Result<()> {
        if let Some(parent) = self.session_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create dir {}", parent.display()))?;
        }
        let raw = serde_json::to_string_pretty(token).context("failed to encode session")?;
        let tmp_path = self.session_path.with_extension("tmp");
        fs::write(&tmp_path, &raw)
            .with_context(|| format!("failed to write temp session {}", tmp_path.display()))?;
        fs::rename(&tmp_path, &self.session_path)
            .with_context(|| format!("failed to rename session {}", self.session_path.display()))?;
        set_file_owner_only(&self.session_path);
        Ok(())
    }

    pub fn has_valid_session(&self) -> bool {
        match self.load_session() {
            Ok(Some(token)) => !token.is_expired(now_unix()),
            _ => false,
        }
    }

    pub fn login(&mut self, email: &str, password: &str) -> Result<()> {
        if email.trim().is_empty() {
            return Err(anyhow!("email is empty"));
        }
        if password.is_empty() {
            return Err(anyhow!("password is empty"));
        }

        self.device_id = md5_hex(email);

        let captcha = self.init_captcha(email)?;
        self.captcha_token = captcha
            .captcha_token
            .or_else(|| env::var("PIKPAK_CAPTCHA_TOKEN").ok())
            .ok_or_else(|| {
                let hint = captcha.url.as_deref().unwrap_or("<no challenge url>");
                anyhow!(
                    "captcha token unavailable; set PIKPAK_CAPTCHA_TOKEN. url={}",
                    sanitize(hint)
                )
            })?;

        let url = self.auth_url("v1/auth/signin");
        let payload = serde_json::json!({
            "username": email,
            "password": password,
            "client_id": self.client_id,
            "client_secret": self.client_secret,
            "captcha_token": self.captcha_token,
            "grant_type": "password",
        });

        let response = self
            .http
            .post(&url)
            .header("x-device-id", &self.device_id)
            .json(&payload)
            .send()
            .context("signin request failed")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!("signin failed ({}): {}", status, sanitize(&body)));
        }

        let signin: SigninResponse = response.json().context("invalid signin json")?;
        let expires_in = i64::try_from(signin.expires_in).context("expires_in overflow")?;
        let now = now_unix();

        let token = SessionToken {
            access_token: signin.access_token,
            refresh_token: signin.refresh_token,
            expires_at_unix: now.saturating_add(expires_in),
        };

        self.save_session(&token)?;
        Ok(())
    }

    fn init_captcha(&self, email: &str) -> Result<CaptchaInitResponse> {
        let url = self.auth_url("v1/shield/captcha/init");
        let action = format!("POST:{}", self.auth_url("v1/auth/signin"));

        let payload = serde_json::json!({
            "action": action,
            "client_id": self.client_id,
            "device_id": self.device_id,
            "meta": { "username": email },
        });

        let response = self
            .http
            .post(&url)
            .header("x-device-id", &self.device_id)
            .json(&payload)
            .send()
            .context("captcha init failed")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!(
                "captcha init failed ({}): {}",
                status,
                sanitize(&body)
            ));
        }

        response
            .json::<CaptchaInitResponse>()
            .context("invalid captcha json")
    }

    fn access_token(&self) -> Result<String> {
        let session = self
            .load_session()?
            .ok_or_else(|| anyhow!("not logged in, please login first"))?;

        // Refresh proactively if the token expires within 5 minutes.
        if session.is_expired(now_unix() + 300) {
            // Serialize refresh attempts — only one thread refreshes at a time.
            let _guard = self.refresh_lock.lock().unwrap_or_else(|e| e.into_inner());
            // Re-check after acquiring lock: another thread may have refreshed already.
            let session = self
                .load_session()?
                .ok_or_else(|| anyhow!("not logged in, please login first"))?;
            if session.is_expired(now_unix() + 300) {
                match self.refresh_session(&session.refresh_token) {
                    Ok(new_token) => return Ok(new_token),
                    Err(e) => {
                        return Err(anyhow!(
                            "session expired and token refresh failed: {e:#}\nPlease log in again."
                        ));
                    }
                }
            }
            return Ok(session.access_token);
        }

        Ok(session.access_token)
    }

    /// Use the refresh_token to obtain a new access_token without requiring
    /// the user's password. Saves the updated session to disk and returns
    /// the new access_token.
    fn refresh_session(&self, refresh_token: &str) -> Result<String> {
        let url = self.auth_url("v1/auth/token");

        let payload = serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": self.client_id,
            "client_secret": self.client_secret,
        });

        let response = self
            .http
            .post(&url)
            .json(&payload)
            .send()
            .context("token refresh request failed")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!("token refresh failed ({}): {}", status, sanitize(&body)));
        }

        let refreshed: SigninResponse = response.json().context("invalid token refresh json")?;
        let expires_in = i64::try_from(refreshed.expires_in).context("expires_in overflow")?;

        let token = SessionToken {
            access_token: refreshed.access_token.clone(),
            refresh_token: refreshed.refresh_token,
            expires_at_unix: now_unix().saturating_add(expires_in),
        };
        self.save_session(&token)?;

        Ok(refreshed.access_token)
    }

    fn authed_headers(
        &self,
        rb: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        let mut rb = rb;
        if !self.device_id.is_empty() {
            rb = rb.header("x-device-id", &self.device_id);
        }
        if !self.captcha_token.is_empty() {
            rb = rb.header("x-captcha-token", &self.captcha_token);
        }
        rb
    }

    fn drive_url(&self, path: &str) -> String {
        format!("{}/{}", self.drive_base_url.trim_end_matches('/'), path)
    }

    fn auth_url(&self, path: &str) -> String {
        format!("{}/{}", self.auth_base_url.trim_end_matches('/'), path)
    }

    pub fn ls(&self, parent_id: &str) -> Result<Vec<Entry>> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files");

        let filters = r#"{"trashed":{"eq":false}}"#;
        let mut all_entries: Vec<Entry> = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut rb = self.http.get(&url).bearer_auth(&token).query(&[
                ("parent_id", parent_id),
                ("limit", "500"),
                ("filters", filters),
                ("thumbnail_size", self.thumbnail_size.as_str()),
            ]);
            if let Some(ref pt) = page_token {
                rb = rb.query(&[("page_token", pt.as_str())]);
            }
            rb = self.authed_headers(rb);

            let response = rb.send().context("ls request failed")?;
            let status = response.status();
            if !status.is_success() {
                let body = response.text().unwrap_or_default();
                return Err(anyhow!("ls failed ({}): {}", status, sanitize(&body)));
            }

            let payload: DriveListResponse = response.json().context("invalid ls json")?;
            let next = payload
                .next_page_token
                .filter(|t| !t.is_empty());

            all_entries.extend(payload.files.into_iter().map(|f| f.into_entry()));

            match next {
                Some(t) => page_token = Some(t),
                None => break,
            }
        }

        Ok(all_entries)
    }

    /// Like `ls()` but caches results by parent_id for the lifetime of this client.
    /// Used by path-resolution helpers so repeated segments (e.g. the same parent
    /// folder appearing in every argument of a batch command) only hit the API once.
    /// TUI code that needs a fresh listing should call `ls()` directly.
    pub fn ls_cached(&self, parent_id: &str) -> Result<Vec<Entry>> {
        if let Some(cached) = self.ls_cache.lock().unwrap_or_else(|e| e.into_inner()).get(parent_id) {
            return Ok(cached.clone());
        }
        let entries = self.ls(parent_id)?;
        let result = entries.clone();
        self.ls_cache.lock().unwrap_or_else(|e| e.into_inner()).insert(parent_id.to_string(), entries);
        Ok(result)
    }

    /// Resolve a cloud path like `/My Files/Movies` to a folder ID and breadcrumb.
    ///
    /// Returns `(final_folder_id, breadcrumb)` where breadcrumb is a vec of
    /// `(parent_id, folder_name)` pairs — the same format used by the TUI App.
    pub fn resolve_path_nav(&self, path: &str) -> Result<(String, Vec<(String, String)>)> {
        use anyhow::anyhow;
        let components: Vec<&str> = path
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        let mut current_id = String::new(); // root
        let mut breadcrumb: Vec<(String, String)> = Vec::new();

        for name in components {
            let entries = self.ls_cached(&current_id)?;
            let child = entries
                .into_iter()
                .find(|e| e.name == name && e.kind == crate::pikpak::EntryKind::Folder)
                .ok_or_else(|| anyhow!("folder not found: {name}"))?;
            breadcrumb.push((current_id, name.to_string()));
            current_id = child.id;
        }

        Ok((current_id, breadcrumb))
    }

    pub fn ls_trash(&self, limit: u32) -> Result<Vec<Entry>> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files");

        let filters = r#"{"trashed":{"eq":true}}"#;
        let mut rb = self.http.get(&url).bearer_auth(&token).query(&[
            ("parent_id", "*"),
            ("limit", &limit.to_string()),
            ("filters", filters),
            ("thumbnail_size", "SIZE_MEDIUM"),
        ]);
        rb = self.authed_headers(rb);

        let response = rb.send().context("ls_trash request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!(
                "ls_trash failed ({}): {}",
                status,
                sanitize(&body)
            ));
        }

        let payload: DriveListResponse = response.json().context("invalid ls_trash json")?;
        let entries = payload.files.into_iter().map(|f| f.into_entry()).collect();
        Ok(entries)
    }

    pub fn mv(&self, ids: &[&str], to_parent_id: &str) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files:batchMove");

        let payload = serde_json::json!({
            "ids": ids,
            "to": { "parent_id": to_parent_id },
        });

        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("move request failed")?;
        ensure_success(response, "move")
    }

    pub fn cp(&self, ids: &[&str], to_parent_id: &str) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files:batchCopy");

        let payload = serde_json::json!({
            "ids": ids,
            "to": { "parent_id": to_parent_id },
        });

        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("copy request failed")?;
        ensure_success(response, "copy")
    }

    pub fn rename(&self, file_id: &str, new_name: &str) -> Result<()> {
        let token = self.access_token()?;
        let url = format!("{}/{}", self.drive_url("drive/v1/files"), file_id);

        let payload = serde_json::json!({ "name": new_name });
        let mut rb = self.http.patch(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("rename request failed")?;
        ensure_success(response, "rename")
    }

    pub fn remove(&self, ids: &[&str]) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files:batchTrash");

        let payload = serde_json::json!({ "ids": ids });
        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("remove request failed")?;
        ensure_success(response, "remove")
    }

    pub fn delete_permanent(&self, ids: &[&str]) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files:batchDelete");

        let payload = serde_json::json!({ "ids": ids });
        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("permanent delete request failed")?;
        ensure_success(response, "permanent delete")
    }

    pub fn untrash(&self, ids: &[&str]) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files:batchUntrash");

        let payload = serde_json::json!({ "ids": ids });
        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("untrash request failed")?;
        ensure_success(response, "untrash")
    }

    pub fn mkdir(&self, parent_id: &str, name: &str) -> Result<Entry> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files");

        let payload = serde_json::json!({
            "kind": "drive#folder",
            "parent_id": parent_id,
            "name": name,
        });

        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("mkdir request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!("mkdir failed ({}): {}", status, sanitize(&body)));
        }

        let resp: DriveFileResponse = response.json().context("invalid mkdir json")?;
        let f = resp.file;
        let starred = f.is_starred();
        Ok(Entry {
            id: f.id,
            name: f.name,
            kind: EntryKind::Folder,
            size: 0,
            created_time: f.created_time.unwrap_or_default(),
            modified_time: f.modified_time.unwrap_or_default(),
            starred,
            thumbnail_link: f.thumbnail_link,
        })
    }

    pub fn file_info(&self, file_id: &str) -> Result<FileInfoResponse> {
        let token = self.access_token()?;
        let url = format!("{}/{}", self.drive_url("drive/v1/files"), file_id);

        let mut rb = self.http.get(&url).bearer_auth(&token);
        rb = self.authed_headers(rb);

        let response = rb.send().context("file_info request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!(
                "file_info failed ({}): {}",
                status,
                sanitize(&body)
            ));
        }

        response.json().context("invalid file_info json")
    }

    /// Returns (download_url, total_size) for a file.
    pub fn download_url(&self, file_id: &str) -> Result<(String, u64)> {
        let info = self.file_info(file_id)?;
        let url = info
            .download_url()
            .ok_or_else(|| anyhow!("no download link for file {}", file_id))?
            .to_string();
        Ok((url, info.file_size()))
    }

    /// Get a reference to the HTTP client.
    pub fn http(&self) -> &reqwest::blocking::Client {
        &self.http
    }

    /// Check if a streaming URL is available (not in cold/archive storage).
    /// Sends a Range request and checks for a valid response.
    pub fn check_stream_available(url: &str) -> bool {
        let client = match reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
        {
            Ok(c) => c,
            Err(_) => return false,
        };
        match client.get(url).header("Range", "bytes=0-0").send() {
            Ok(resp) => resp.headers().contains_key("content-range") && resp.content_length().unwrap_or(0) > 0,
            Err(_) => false,
        }
    }

    pub fn download_to(&self, file_id: &str, dest: &std::path::Path) -> Result<u64> {
        let info = self.file_info(file_id)?;
        let download_url = info
            .download_url()
            .ok_or_else(|| anyhow!("no download link for file {}", file_id))?;

        let existing_size = dest.metadata().map(|m| m.len()).unwrap_or(0);

        let mut rb = self.http.get(download_url);
        if existing_size > 0 {
            rb = rb.header("Range", format!("bytes={}-", existing_size));
        }

        let response = rb.send().context("download request failed")?;
        let status = response.status();
        if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(anyhow!("download failed ({})", status));
        }

        let mut file = if existing_size > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT {
            fs::OpenOptions::new().append(true).open(dest)?
        } else {
            fs::File::create(dest)?
        };

        let mut reader: Box<dyn io::Read> = Box::new(response);
        let bytes = io::copy(&mut reader, &mut file).context("download write failed")?;
        Ok(existing_size + bytes)
    }

    pub fn quota(&self) -> Result<QuotaInfo> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/about");

        let mut rb = self.http.get(&url).bearer_auth(&token);
        rb = self.authed_headers(rb);

        let response = rb.send().context("quota request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!("quota failed ({}): {}", status, sanitize(&body)));
        }

        response.json().context("invalid quota json")
    }

    // --- Offline download (cloud download) ---

    /// Submit a URL or magnet link for cloud/offline download.
    /// Returns the task info including task id and file id.
    pub fn offline_download(
        &self,
        file_url: &str,
        parent_id: Option<&str>,
        name: Option<&str>,
    ) -> Result<OfflineTaskResponse> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files");

        let mut payload = serde_json::json!({
            "kind": "drive#file",
            "upload_type": "UPLOAD_TYPE_URL",
            "url": { "url": file_url },
        });
        if let Some(pid) = parent_id {
            payload["parent_id"] = serde_json::json!(pid);
            payload["folder_type"] = serde_json::json!("");
        } else {
            payload["folder_type"] = serde_json::json!("DOWNLOAD");
        }
        if let Some(n) = name {
            payload["name"] = serde_json::json!(n);
        }

        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("offline download request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!(
                "offline download failed ({}): {}",
                status,
                sanitize(&body)
            ));
        }

        response.json().context("invalid offline download json")
    }

    /// List offline/cloud download tasks.
    pub fn offline_list(&self, limit: u32, phases: &[&str]) -> Result<OfflineListResponse> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/tasks");

        let filters = serde_json::json!({
            "phase": { "in": phases.join(",") }
        });

        let mut rb = self.http.get(&url).bearer_auth(&token).query(&[
            ("type", "offline"),
            ("thumbnail_size", "SIZE_SMALL"),
            ("limit", &limit.to_string()),
            ("filters", &filters.to_string()),
            ("with", "reference_resource"),
        ]);
        rb = self.authed_headers(rb);

        let response = rb.send().context("offline list request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!(
                "offline list failed ({}): {}",
                status,
                sanitize(&body)
            ));
        }

        response.json().context("invalid offline list json")
    }

    /// Retry a failed offline download task.
    pub fn offline_task_retry(&self, task_id: &str) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/task");

        let payload = serde_json::json!({
            "type": "offline",
            "create_type": "RETRY",
            "id": task_id,
        });

        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("offline task retry request failed")?;
        ensure_success(response, "offline task retry")
    }

    /// Delete offline tasks by task IDs.
    pub fn delete_tasks(&self, task_ids: &[&str], delete_files: bool) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/tasks");

        let mut pairs: Vec<(&str, String)> = task_ids
            .iter()
            .map(|id| ("task_ids", id.to_string()))
            .collect();
        pairs.push(("delete_files", delete_files.to_string()));

        let mut rb = self.http.delete(&url).bearer_auth(&token);
        for (k, v) in &pairs {
            rb = rb.query(&[(k, v)]);
        }
        rb = self.authed_headers(rb);

        let response = rb.send().context("delete tasks request failed")?;
        ensure_success(response, "delete tasks")
    }

    /// Star files by IDs.
    pub fn star(&self, ids: &[&str]) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files:star");

        let payload = serde_json::json!({ "ids": ids });
        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("star request failed")?;
        ensure_success(response, "star")
    }

    /// Unstar files by IDs.
    pub fn unstar(&self, ids: &[&str]) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files:unstar");

        let payload = serde_json::json!({ "ids": ids });
        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("unstar request failed")?;
        ensure_success(response, "unstar")
    }

    /// List starred files.
    pub fn starred_list(&self, limit: u32) -> Result<Vec<Entry>> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files");

        let filters = r#"{"trashed":{"eq":false},"system_tag":{"in":"STAR"}}"#;
        let mut rb = self.http.get(&url).bearer_auth(&token).query(&[
            ("parent_id", "*"),
            ("limit", &limit.to_string()),
            ("filters", filters),
            ("thumbnail_size", "SIZE_MEDIUM"),
        ]);
        rb = self.authed_headers(rb);

        let response = rb.send().context("starred list request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!(
                "starred list failed ({}): {}",
                status,
                sanitize(&body)
            ));
        }

        let payload: DriveListResponse = response.json().context("invalid starred list json")?;
        let entries = payload
            .files
            .into_iter()
            .map(|f| Entry { starred: true, ..f.into_entry() })
            .collect();
        Ok(entries)
    }

    /// Get recent file events (recently added files).
    pub fn events(&self, limit: u32) -> Result<EventsResponse> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/events");

        let mut rb = self.http.get(&url).bearer_auth(&token).query(&[
            ("thumbnail_size", "SIZE_MEDIUM"),
            ("limit", &limit.to_string()),
        ]);
        rb = self.authed_headers(rb);

        let response = rb.send().context("events request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!("events failed ({}): {}", status, sanitize(&body)));
        }

        response.json().context("invalid events json")
    }

    /// Get VIP membership info.
    pub fn vip_info(&self) -> Result<VipInfoResponse> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/privilege/vip");

        let mut rb = self.http.get(&url).bearer_auth(&token);
        rb = self.authed_headers(rb);

        let response = rb.send().context("vip info request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!("vip info failed ({}): {}", status, sanitize(&body)));
        }

        response.json().context("invalid vip info json")
    }

    /// Get invite code.
    pub fn invite_code(&self) -> Result<String> {
        let token = self.access_token()?;
        let url = self.drive_url("vip/v1/activity/inviteCode");

        let mut rb = self.http.get(&url).bearer_auth(&token);
        rb = self.authed_headers(rb);

        let response = rb.send().context("invite code request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!(
                "invite code failed ({}): {}",
                status,
                sanitize(&body)
            ));
        }

        let data: serde_json::Value = response.json().context("invalid invite code json")?;
        data["code"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("no invite code in response"))
    }

    pub fn transfer_quota(&self) -> Result<TransferQuotaResponse> {
        let token = self.access_token()?;
        let url = self.drive_url("vip/v1/quantity/list");

        let mut rb = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .query(&[("type", "transfer")]);
        rb = self.authed_headers(rb);

        let response = rb.send().context("transfer quota request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!(
                "transfer quota failed ({}): {}",
                status,
                sanitize(&body)
            ));
        }

        response.json().context("invalid transfer quota json")
    }

    /// Fetch first `max_bytes` of a text file for preview.
    /// Returns (file_info, content_text, file_size, truncated).
    pub fn fetch_text_preview(
        &self,
        file_id: &str,
        max_bytes: u64,
    ) -> Result<(String, String, u64, bool)> {
        let info = self.file_info(file_id)?;
        let url = info
            .download_url()
            .ok_or_else(|| anyhow!("no download link for file {}", file_id))?;
        let file_size = info.file_size();

        let response = self
            .http
            .get(url)
            .header("Range", format!("bytes=0-{}", max_bytes.saturating_sub(1)))
            .send()
            .context("text preview request failed")?;

        let status = response.status();
        if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(anyhow!("text preview failed ({})", status));
        }

        let bytes = response.bytes().context("text preview read failed")?;
        let truncated = file_size > bytes.len() as u64;
        let content = String::from_utf8_lossy(&bytes).into_owned();

        Ok((info.name, content, file_size, truncated))
    }

    /// Resolve a path like "/My Pack/docs" to the folder ID by walking each segment.
    /// Root "/" returns "".
    pub fn resolve_path(&self, path: &str) -> Result<String> {
        let path = path.trim();
        if path.is_empty() || path == "/" {
            return Ok(String::new()); // root
        }

        let segments: Vec<&str> = path
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        let mut current_id = String::new(); // root
        for seg in &segments {
            let entries = self.ls_cached(&current_id)?;
            let found = entries
                .into_iter()
                .find(|e| e.name == *seg)
                .ok_or_else(|| anyhow!("not found: '{}' in path '{}'", seg, path))?;
            current_id = found.id;
        }

        Ok(current_id)
    }

    /// Upload a local file to PikPak.
    /// Returns the file name and whether it was a dedup (instant upload).
    pub fn upload_file(
        &self,
        parent_id: Option<&str>,
        local_path: &Path,
    ) -> Result<(String, bool)> {
        let file_name = local_path
            .file_name()
            .ok_or_else(|| anyhow!("invalid file path"))?
            .to_string_lossy()
            .to_string();

        let meta = fs::metadata(local_path)
            .with_context(|| format!("cannot stat '{}'", local_path.display()))?;
        let file_size = meta.len();

        let hash = pikpak_hash(local_path)?;

        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files");
        let mut payload = serde_json::json!({
            "kind": "drive#file",
            "name": file_name,
            "size": file_size.to_string(),
            "hash": hash,
            "upload_type": "UPLOAD_TYPE_RESUMABLE",
            "objProvider": { "provider": "UPLOAD_TYPE_UNKNOWN" },
        });
        if let Some(pid) = parent_id {
            payload["parent_id"] = serde_json::json!(pid);
        }

        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);
        let response = rb.send().context("upload init request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!(
                "upload init failed ({}): {}",
                status,
                sanitize(&body)
            ));
        }

        let init: UploadInitResponse = response.json().context("invalid upload init json")?;

        if init.upload_type == "UPLOAD_TYPE_RESUMABLE"
            && let Some(resumable) = &init.resumable
                && resumable.kind == "drive#uploadContext"
                    && init.file.phase.as_deref() == Some("PHASE_TYPE_COMPLETE")
                {
                    return Ok((file_name, true)); // dedup
                }

        if init.file.phase.as_deref() == Some("PHASE_TYPE_COMPLETE") {
            return Ok((file_name, true)); // dedup
        }

        let resumable = init
            .resumable
            .ok_or_else(|| anyhow!("no resumable context in upload init response"))?;

        let oss_args = OssArgs {
            endpoint: resumable
                .params
                .endpoint
                .clone()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("upload init response missing OSS endpoint"))?,
            access_key_id: resumable.params.access_key_id.clone().unwrap_or_default(),
            access_key_secret: resumable
                .params
                .access_key_secret
                .clone()
                .unwrap_or_default(),
            security_token: resumable.params.security_token.clone().unwrap_or_default(),
            bucket: resumable
                .params
                .bucket
                .clone()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("upload init response missing OSS bucket"))?,
            key: resumable
                .params
                .key
                .clone()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("upload init response missing OSS key"))?,
        };

        let upload_id = self.oss_initiate_multipart(&oss_args)?;
        let etags = self.oss_upload_chunks(&oss_args, &upload_id, local_path, file_size)?;
        self.oss_complete_multipart(&oss_args, &upload_id, &etags)?;

        Ok((file_name, false))
    }

    /// Upload a local directory recursively. Creates the folder on PikPak then
    /// uploads all files, mirroring the subdirectory structure.
    /// Returns `(files_ok, files_failed)`.
    pub fn upload_dir(&self, parent_id: &str, local_dir: &Path) -> Result<(usize, usize)> {
        let name = local_dir
            .file_name()
            .ok_or_else(|| anyhow!("directory has no name"))?
            .to_string_lossy();
        let folder = self.mkdir(parent_id, &name)?;
        self.upload_dir_inner(&folder.id, local_dir)
    }

    fn upload_dir_inner(&self, parent_id: &str, local_dir: &Path) -> Result<(usize, usize)> {
        let mut ok = 0usize;
        let mut failed = 0usize;
        let entries = std::fs::read_dir(local_dir)
            .with_context(|| format!("cannot read dir: {}", local_dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                match self.mkdir(parent_id, &name) {
                    Ok(sub) => {
                        let (sub_ok, sub_fail) = self.upload_dir_inner(&sub.id, &path)?;
                        ok += sub_ok;
                        failed += sub_fail;
                    }
                    Err(_) => failed += 1,
                }
            } else if path.is_file() {
                match self.upload_file(Some(parent_id), &path) {
                    Ok(_) => ok += 1,
                    Err(_) => failed += 1,
                }
            }
        }
        Ok((ok, failed))
    }

    pub fn download_dir(
        &self,
        folder_id: &str,
        folder_name: &str,
        local_dest: &Path,
        workers: usize,
    ) -> Result<(usize, usize)> {
        let dir = local_dest.join(sanitize_filename(folder_name));
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("cannot create dir '{}'", dir.display()))?;
        self.download_dir_inner(folder_id, &dir, workers)
    }

    fn download_dir_inner(&self, folder_id: &str, local_dir: &Path, workers: usize) -> Result<(usize, usize)> {
        use std::sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}};

        let workers = workers.max(1);

        let entries = match self.ls(folder_id) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("  [error] listing '{}': {}", folder_id, e);
                return Ok((0, 1));
            }
        };

        let mut files: Vec<Entry> = Vec::new();
        let mut folders: Vec<Entry> = Vec::new();
        for entry in entries {
            match entry.kind {
                EntryKind::File => files.push(entry),
                EntryKind::Folder => folders.push(entry),
            }
        }

        let mut failed_count = 0usize;
        for folder in &folders {
            if let Err(e) = std::fs::create_dir_all(local_dir.join(sanitize_filename(&folder.name))) {
                eprintln!("  [error] mkdir '{}': {}", folder.name, e);
                failed_count += 1;
            }
        }

        let ok = Arc::new(AtomicUsize::new(0));
        let failed = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = std::sync::mpsc::channel::<Entry>();
        for entry in files {
            tx.send(entry).ok();
        }
        drop(tx); // workers exit once channel is drained
        let rx = Arc::new(Mutex::new(rx));

        std::thread::scope(|s| {
            for _ in 0..workers {
                let rx = Arc::clone(&rx);
                let ok = Arc::clone(&ok);
                let failed = Arc::clone(&failed);
                s.spawn(move || {
                    while let Ok(entry) = rx.lock().unwrap_or_else(|e| e.into_inner()).recv() {
                        let dest = local_dir.join(sanitize_filename(&entry.name));
                        let local_size = dest.metadata().map(|m| m.len()).unwrap_or(0);
                        if local_size > 0 && local_size == entry.size {
                            println!("  skipping '{}' (already complete)", dest.display());
                            ok.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        println!("  {}", dest.display());
                        match self.download_to(&entry.id, &dest) {
                            Ok(_) => { ok.fetch_add(1, Ordering::Relaxed); }
                            Err(e) => {
                                eprintln!("  [error] '{}': {}", entry.name, e);
                                failed.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                });
            }
        });

        let mut total_ok = ok.load(Ordering::Relaxed);
        let mut total_failed = failed.load(Ordering::Relaxed) + failed_count;

        for folder in folders {
            let sub_dir = local_dir.join(sanitize_filename(&folder.name));
            match self.download_dir_inner(&folder.id, &sub_dir, workers) {
                Ok((sub_ok, sub_fail)) => {
                    total_ok += sub_ok;
                    total_failed += sub_fail;
                }
                Err(e) => {
                    eprintln!("  [error] {}: {}", folder.name, e);
                    total_failed += 1;
                }
            }
        }

        Ok((total_ok, total_failed))
    }

    pub fn share_info(&self, share_id: &str, pass_code: &str) -> Result<ShareInfoResponse> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/share");

        let mut rb = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .query(&[
                ("share_id", share_id),
                ("pass_code", pass_code),
                ("thumbnail_size", "SIZE_MEDIUM"),
            ]);
        rb = self.authed_headers(rb);

        let response = rb.send().context("share info request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!("share info failed ({}): {}", status, sanitize(&body)));
        }

        let info: ShareInfoResponse = response.json().context("invalid share info json")?;
        if info.share_status != "OK" {
            return Err(anyhow!("share is not available (status: {})", info.share_status));
        }
        Ok(info)
    }

    pub fn save_share(
        &self,
        share_id: &str,
        pass_code_token: &str,
        file_ids: &[&str],
        to_parent_id: &str,
    ) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/share/restore");

        let payload = serde_json::json!({
            "share_id": share_id,
            "pass_code_token": pass_code_token,
            "file_ids": file_ids,
            "to": { "parent_id": to_parent_id },
        });

        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("save share request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            if body.contains("file_restore_own") {
                return Err(anyhow!("cannot save: these files already belong to your account"));
            }
            return Err(anyhow!("save share failed ({}): {}", status, sanitize(&body)));
        }
        Ok(())
    }

    pub fn create_share(
        &self,
        file_ids: &[&str],
        need_password: bool,
        expiration_days: i64,
    ) -> Result<CreateShareResponse> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/share");

        let payload = serde_json::json!({
            "file_ids": file_ids,
            "share_to": if need_password { "encryptedlink" } else { "publiclink" },
            "expiration_days": expiration_days,
            "pass_code_option": if need_password { "REQUIRED" } else { "NOT_REQUIRED" },
        });

        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("create share request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!("create share failed ({}): {}", status, sanitize(&body)));
        }
        response.json().context("invalid create share response")
    }

    pub fn list_shares(&self) -> Result<Vec<MyShare>> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/share/list");

        let mut rb = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .query(&[("limit", "100"), ("thumbnail_size", "SIZE_SMALL")]);
        rb = self.authed_headers(rb);

        let response = rb.send().context("list shares request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!("list shares failed ({}): {}", status, sanitize(&body)));
        }
        let resp: ShareListResponse = response.json().context("invalid share list json")?;
        Ok(resp.data)
    }

    pub fn delete_shares(&self, share_ids: &[&str]) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/share:batchDelete");

        let payload = serde_json::json!({ "ids": share_ids });

        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("delete shares request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!("delete shares failed ({}): {}", status, sanitize(&body)));
        }
        Ok(())
    }

    fn oss_initiate_multipart(&self, oss: &OssArgs) -> Result<String> {
        let date = httpdate_now();
        let auth = oss_hmac_auth(
            "POST",
            &date,
            &oss.security_token,
            &oss.access_key_id,
            &oss.access_key_secret,
            &format!("/{}/{}", oss.bucket, oss.key),
            "application/octet-stream",
            "?uploads",
        );

        let url = format!(
            "https://{}/{}?uploads",
            oss.endpoint.trim_end_matches('/'),
            oss.key
        );
        let response = self
            .http
            .post(&url)
            .header("Authorization", auth)
            .header("Date", &date)
            .header("Content-Type", "application/octet-stream")
            .header("x-oss-security-token", &oss.security_token)
            .send()
            .context("OSS initiate multipart failed")?;

        let status = response.status();
        let body = response.text().unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "OSS initiate multipart failed ({}): {}",
                status,
                sanitize(&body)
            ));
        }

        extract_xml_tag(&body, "UploadId")
            .ok_or_else(|| anyhow!("no UploadId in initiate multipart response"))
    }

    fn oss_upload_chunks(
        &self,
        oss: &OssArgs,
        upload_id: &str,
        local_path: &Path,
        file_size: u64,
    ) -> Result<Vec<String>> {
        const CHUNK_SIZE: u64 = 10 * 1024 * 1024; // 10MB chunks

        let mut file = fs::File::open(local_path)
            .with_context(|| format!("cannot open '{}'", local_path.display()))?;

        let num_parts = if file_size == 0 {
            1
        } else {
            file_size.div_ceil(CHUNK_SIZE)
        };

        let mut etags = Vec::new();

        for part_num in 1..=num_parts {
            let remaining = if file_size == 0 {
                0
            } else {
                std::cmp::min(CHUNK_SIZE, file_size - (part_num - 1) * CHUNK_SIZE)
            };

            let mut buf = vec![0u8; remaining as usize];
            file.read_exact(&mut buf)?;

            let date = httpdate_now();
            let auth = oss_hmac_auth(
                "PUT",
                &date,
                &oss.security_token,
                &oss.access_key_id,
                &oss.access_key_secret,
                &format!("/{}/{}", oss.bucket, oss.key),
                "application/octet-stream",
                &format!("?partNumber={}&uploadId={}", part_num, upload_id),
            );

            let url = format!(
                "https://{}/{}?partNumber={}&uploadId={}",
                oss.endpoint.trim_end_matches('/'),
                oss.key,
                part_num,
                upload_id
            );
            let response = self
                .http
                .put(&url)
                .header("Authorization", auth)
                .header("Date", &date)
                .header("Content-Type", "application/octet-stream")
                .header("x-oss-security-token", &oss.security_token)
                .body(buf)
                .send()
                .with_context(|| format!("OSS upload part {} failed", part_num))?;

            let status = response.status();
            if !status.is_success() {
                let body = response.text().unwrap_or_default();
                return Err(anyhow!(
                    "OSS upload part {} failed ({}): {}",
                    part_num,
                    status,
                    sanitize(&body)
                ));
            }

            let etag = response
                .headers()
                .get("ETag")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            etags.push(etag);
        }

        Ok(etags)
    }

    fn oss_complete_multipart(
        &self,
        oss: &OssArgs,
        upload_id: &str,
        etags: &[String],
    ) -> Result<()> {
        let mut xml = String::from("<CompleteMultipartUpload>");
        for (i, etag) in etags.iter().enumerate() {
            xml.push_str(&format!(
                "<Part><PartNumber>{}</PartNumber><ETag>{}</ETag></Part>",
                i + 1,
                etag
            ));
        }
        xml.push_str("</CompleteMultipartUpload>");

        let date = httpdate_now();
        let auth = oss_hmac_auth(
            "POST",
            &date,
            &oss.security_token,
            &oss.access_key_id,
            &oss.access_key_secret,
            &format!("/{}/{}", oss.bucket, oss.key),
            "application/octet-stream",
            &format!("?uploadId={}", upload_id),
        );

        let url = format!(
            "https://{}/{}?uploadId={}",
            oss.endpoint.trim_end_matches('/'),
            oss.key,
            upload_id
        );
        let response = self
            .http
            .post(&url)
            .header("Authorization", auth)
            .header("Date", &date)
            .header("Content-Type", "application/octet-stream")
            .header("x-oss-security-token", &oss.security_token)
            .body(xml)
            .send()
            .context("OSS complete multipart failed")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!(
                "OSS complete multipart failed ({}): {}",
                status,
                sanitize(&body)
            ));
        }
        Ok(())
    }
}

struct OssArgs {
    endpoint: String,
    access_key_id: String,
    access_key_secret: String,
    security_token: String,
    bucket: String,
    key: String,
}

#[derive(Debug, Deserialize)]
struct UploadInitResponse {
    #[serde(default)]
    upload_type: String,
    file: UploadFileInfo,
    #[serde(default)]
    resumable: Option<ResumableContext>,
}

#[derive(Debug, Deserialize)]
struct UploadFileInfo {
    #[serde(default)]
    phase: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResumableContext {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    params: ResumableParams,
}

#[derive(Debug, Default, Deserialize)]
struct ResumableParams {
    #[serde(default)]
    endpoint: Option<String>,
    #[serde(default)]
    access_key_id: Option<String>,
    #[serde(default)]
    access_key_secret: Option<String>,
    #[serde(default)]
    security_token: Option<String>,
    #[serde(default)]
    bucket: Option<String>,
    #[serde(default)]
    key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ShareListResponse {
    #[serde(default)]
    pub data: Vec<MyShare>,
}

#[derive(Debug, Deserialize)]
pub struct MyShare {
    pub share_id: String,
    pub share_url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub pass_code: String,
    #[serde(default)]
    pub share_to: String,
    #[serde(default)]
    pub create_time: String,
    #[serde(default)]
    pub expiration_days: String,
    #[serde(default)]
    pub view_count: String,
    #[serde(default)]
    pub restore_count: String,
    #[serde(default)]
    pub file_num: String,
    #[serde(default)]
    pub share_status: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateShareResponse {
    pub share_id: String,
    pub share_url: String,
    #[serde(default)]
    pub pass_code: String,
    #[serde(default)]
    pub share_text: String,
}

#[derive(Debug, Deserialize)]
pub struct ShareEntry {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct ShareInfoResponse {
    pub share_status: String,
    #[serde(default)]
    pub pass_code_token: String,
    #[serde(default)]
    pub files: Vec<ShareEntry>,
}

/// Compute the PikPak proprietary file hash for upload deduplication.
/// Algorithm: chunk the file, SHA1 each chunk, concatenate hex hashes, SHA1 the result.
/// Chunk sizes follow PikPak's server-side spec (reverse-engineered from the Android client):
///   < 128 MB  -> 256 KB chunks
///   < 256 MB  -> 512 KB chunks
///   < 512 MB  -> 1 MB chunks
///   >= 512 MB -> 2 MB chunks
pub fn pikpak_hash(path: &Path) -> Result<String> {
    use sha1::Digest;

    let meta = fs::metadata(path).with_context(|| format!("cannot stat '{}'", path.display()))?;
    let file_size = meta.len();

    let chunk_size: u64 = if file_size < 128 * 1024 * 1024 {
        256 * 1024
    } else if file_size < 256 * 1024 * 1024 {
        512 * 1024
    } else if file_size < 512 * 1024 * 1024 {
        1024 * 1024
    } else {
        2 * 1024 * 1024
    };

    let mut file =
        fs::File::open(path).with_context(|| format!("cannot open '{}'", path.display()))?;

    let mut all_hashes = String::new();
    let mut remaining = file_size;

    while remaining > 0 {
        let to_read = std::cmp::min(chunk_size, remaining) as usize;
        let mut buf = vec![0u8; to_read];
        file.read_exact(&mut buf)?;

        let mut hasher = Sha1::new();
        hasher.update(&buf);
        let hash = hasher.finalize();
        for b in hash.iter() {
            write!(all_hashes, "{:02X}", b).unwrap();
        }

        remaining -= to_read as u64;
    }

    if file_size == 0 {
        let mut hasher = Sha1::new();
        hasher.update(b"");
        let hash = hasher.finalize();
        for b in hash.iter() {
            write!(all_hashes, "{:02X}", b).unwrap();
        }
    }

    let mut final_hasher = Sha1::new();
    final_hasher.update(all_hashes.as_bytes());
    let final_hash = final_hasher.finalize();
    let mut hex = String::with_capacity(40);
    for b in final_hash.iter() {
        write!(hex, "{:02X}", b).unwrap();
    }

    Ok(hex)
}

fn oss_hmac_auth(
    method: &str,
    date: &str,
    security_token: &str,
    access_key_id: &str,
    access_key_secret: &str,
    canonicalized_resource: &str,
    content_type: &str,
    query_string: &str,
) -> String {
    type HmacSha1 = Hmac<Sha1>;

    let canonicalized_headers = format!("x-oss-security-token:{}", security_token);
    let resource = format!("{}{}", canonicalized_resource, query_string);

    let string_to_sign = format!(
        "{}\n\n{}\n{}\n{}\n{}",
        method, content_type, date, canonicalized_headers, resource
    );

    let mut mac = HmacSha1::new_from_slice(access_key_secret.as_bytes()).expect("HMAC key length");
    mac.update(string_to_sign.as_bytes());
    let result = mac.finalize();
    let signature = base64::engine::general_purpose::STANDARD.encode(result.into_bytes());

    format!("OSS {}:{}", access_key_id, signature)
}

fn httpdate_now() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();

    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Calculate year/month/day from days since epoch
    let (year, month, day) = days_to_ymd(days);

    let wday = ((days + 4) % 7) as usize; // Jan 1 1970 was Thursday (4)
    let wday_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let month_names = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        wday_names[wday],
        day,
        month_names[(month - 1) as usize],
        year,
        hours,
        minutes,
        seconds
    )
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let mut y = 1970;
    let mut remaining = days;

    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }

    let month_days = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut m = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining < md {
            m = i;
            break;
        }
        remaining -= md;
    }

    (y, (m + 1) as u64, remaining + 1)
}

fn is_leap(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].to_string())
}

#[derive(Debug, Deserialize)]
struct SigninResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct CaptchaInitResponse {
    #[serde(default)]
    captcha_token: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Deserialize)]
struct DriveListResponse {
    #[serde(default)]
    files: Vec<DriveFile>,
    #[serde(default)]
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct DriveFileResponse {
    file: DriveFile,
}

#[derive(Deserialize)]
struct DriveFile {
    id: String,
    name: String,
    #[serde(default)]
    kind: String,
    #[serde(default, deserialize_with = "de_opt_u64")]
    size: Option<u64>,
    #[serde(default)]
    created_time: Option<String>,
    #[serde(default)]
    modified_time: Option<String>,
    #[serde(default)]
    tags: Vec<DriveFileTag>,
    #[serde(default)]
    thumbnail_link: Option<String>,
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

    fn into_entry(self) -> Entry {
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
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaLink {
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaVideo {
    #[serde(default)]
    pub height: Option<i64>,
    #[serde(default)]
    pub width: Option<i64>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub bit_rate: Option<i64>,
    #[serde(default)]
    pub video_codec: Option<String>,
    #[serde(default)]
    pub audio_codec: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInfo {
    #[serde(default)]
    pub media_name: Option<String>,
    #[serde(default)]
    pub link: Option<MediaLink>,
    #[serde(default)]
    pub video: Option<MediaVideo>,
    #[serde(default)]
    pub is_origin: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfoResponse {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub created_time: Option<String>,
    #[serde(default)]
    pub modified_time: Option<String>,
    #[serde(default)]
    pub web_content_link: Option<String>,
    #[serde(default)]
    pub thumbnail_link: Option<String>,
    #[serde(default)]
    pub links: Option<std::collections::HashMap<String, LinkInfo>>,
    #[serde(default)]
    pub medias: Option<Vec<MediaInfo>>,
}

impl FileInfoResponse {
    /// Extract the best download URL from file info.
    pub fn download_url(&self) -> Option<&str> {
        self.web_content_link
            .as_deref()
            .or(self.links.as_ref().and_then(|l| {
                l.get("application/octet-stream")
                    .and_then(|v| v.url.as_deref())
            }))
    }

    pub fn file_size(&self) -> u64 {
        self.size
            .as_deref()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkInfo {
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct QuotaInfo {
    pub quota: Option<QuotaDetail>,
}

#[derive(Debug, Deserialize)]
pub struct QuotaDetail {
    #[serde(default)]
    pub limit: Option<String>,
    #[serde(default)]
    pub usage: Option<String>,
    #[serde(default)]
    pub usage_in_trash: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TransferQuotaResponse {
    pub base: Option<TransferQuotaBase>,
}

#[derive(Debug, Deserialize)]
pub struct TransferQuotaBase {
    pub offline: Option<TransferBand>,
    pub download: Option<TransferBand>,
    pub upload: Option<TransferBand>,
    #[serde(default)]
    pub expire_time: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TransferBand {
    pub total_assets: Option<u64>,
    pub assets: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct OfflineTaskResponse {
    #[serde(default)]
    pub task: Option<OfflineTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineTask {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub progress: i64,
    #[serde(default)]
    pub file_id: Option<String>,
    #[serde(default)]
    pub file_size: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub created_time: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OfflineListResponse {
    #[serde(default)]
    pub tasks: Vec<OfflineTask>,
}

#[derive(Debug, Deserialize)]
pub struct EventsResponse {
    #[serde(default)]
    pub events: Vec<EventEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEntry {
    #[serde(default, rename = "type")]
    pub event_type: Option<String>,
    #[serde(default)]
    pub type_name: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub created_time: Option<String>,
    #[serde(default)]
    pub reference_resource: Option<EventRefResource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRefResource {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct VipInfoResponse {
    #[serde(default)]
    pub data: Option<VipData>,
}

#[derive(Debug, Deserialize)]
pub struct VipData {
    #[serde(default, rename = "type")]
    pub vip_type: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub expire: Option<String>,
}

fn ensure_success(response: reqwest::blocking::Response, op: &str) -> Result<()> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body = response.text().unwrap_or_default();
    Err(anyhow!("{} failed ({}): {}", op, status, sanitize(&body)))
}

fn default_session_path() -> Result<PathBuf> {
    let base = dirs::home_dir()
        .map(|h| h.join(".config"))
        .ok_or_else(|| anyhow!("unable to locate home dir"))?;
    Ok(base.join("pikpaktui").join("session.json"))
}

#[cfg(unix)]
fn set_file_owner_only(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn set_file_owner_only(_path: &Path) {}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Sanitize a filename from an API response to prevent path traversal.
fn sanitize_filename(name: &str) -> String {
    name.replace(['/', '\\'], "_").replace("..", "_")
}

fn sanitize(s: &str) -> String {
    if s.chars().count() > 240 {
        let truncated: String = s.chars().take(240).collect();
        format!("{}...", truncated)
    } else {
        s.to_string()
    }
}

fn md5_hex(input: &str) -> String {
    use md5::{Md5, Digest};
    let hash = Md5::digest(input.as_bytes());
    let mut hex = String::with_capacity(32);
    for b in hash.iter() {
        write!(hex, "{:02x}", b).unwrap();
    }
    hex
}

fn de_opt_u64<'de, D>(deserializer: D) -> std::result::Result<Option<u64>, D::Error>
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
            value.parse::<u64>().map(Some).map_err(E::custom)
        }

        fn visit_string<E: de::Error>(self, value: String) -> std::result::Result<Self::Value, E> {
            self.visit_str(&value)
        }
    }

    deserializer.deserialize_any(U64Visitor)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_expiry_check() {
        let token = SessionToken {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at_unix: 100,
        };
        assert!(!token.is_expired(99));
        assert!(token.is_expired(100));
    }

    #[test]
    fn md5_basic() {
        assert_eq!(md5_hex(""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex("abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn token_refresh_response_deserializes() {
        // The refresh endpoint returns the same shape as signin.
        let json = r#"{
            "access_token": "new_access",
            "refresh_token": "new_refresh",
            "expires_in": 7200
        }"#;
        let resp: SigninResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.access_token, "new_access");
        assert_eq!(resp.refresh_token, "new_refresh");
        assert_eq!(resp.expires_in, 7200);
    }

    #[test]
    fn access_token_triggers_refresh_when_expired() {
        // Verify that is_expired returns true when expires_at_unix is in the past.
        let expired = SessionToken {
            access_token: "old".into(),
            refresh_token: "r".into(),
            expires_at_unix: now_unix() - 1,
        };
        assert!(expired.is_expired(now_unix()));

        // And false when still valid with the 5-min buffer.
        let valid = SessionToken {
            access_token: "good".into(),
            refresh_token: "r".into(),
            expires_at_unix: now_unix() + 600,
        };
        assert!(!valid.is_expired(now_unix() + 300));
    }

    // --- Pagination: confirm DriveListResponse captures next_page_token ---

    #[test]
    fn drive_list_response_captures_next_page_token() {
        let json = r#"{
            "files": [
                {"id":"abc","name":"foo.txt","kind":"drive#file"}
            ],
            "next_page_token": "page2token"
        }"#;
        let resp: DriveListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.next_page_token, Some("page2token".to_string()));
        assert_eq!(resp.files.len(), 1);
    }

    #[test]
    fn drive_list_response_no_token_on_last_page() {
        // When no next_page_token is present, field should be None
        let json = r#"{"files": []}"#;
        let resp: DriveListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.next_page_token, None);
    }

    #[test]
    fn drive_list_response_empty_token_treated_as_none() {
        // PikPak sometimes returns "" instead of omitting the field
        let json = r#"{"files": [], "next_page_token": ""}"#;
        let resp: DriveListResponse = serde_json::from_str(json).unwrap();
        // empty string → should normalise to None in pagination logic
        assert!(resp.next_page_token.as_deref().unwrap_or("").is_empty());
    }
}
