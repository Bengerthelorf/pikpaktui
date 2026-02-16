use anyhow::{Context, Result, anyhow};
use base64::Engine as _;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    Folder,
    File,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    pub created_time: String,
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
}

impl PikPak {
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: reqwest::blocking::Client::builder()
                .user_agent(USER_AGENT)
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
        })
    }

    // --- Session management ---

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
        fs::write(&self.session_path, raw)
            .with_context(|| format!("failed to write session {}", self.session_path.display()))
    }

    pub fn has_valid_session(&self) -> bool {
        match self.load_session() {
            Ok(Some(token)) => !token.is_expired(now_unix()),
            _ => false,
        }
    }

    // --- Auth ---

    pub fn login(&mut self, email: &str, password: &str) -> Result<()> {
        if email.trim().is_empty() {
            return Err(anyhow!("email is empty"));
        }
        if password.is_empty() {
            return Err(anyhow!("password is empty"));
        }

        self.device_id = md5_hex(email);

        // captcha init
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

        // signin
        let url = format!(
            "{}/v1/auth/signin",
            self.auth_base_url.trim_end_matches('/')
        );
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
        let url = format!(
            "{}/v1/shield/captcha/init",
            self.auth_base_url.trim_end_matches('/')
        );
        let action = format!(
            "POST:{}/v1/auth/signin",
            self.auth_base_url.trim_end_matches('/')
        );

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
        Ok(session.access_token)
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

    // --- Drive API ---

    pub fn ls(&self, parent_id: &str) -> Result<Vec<Entry>> {
        let token = self.access_token()?;
        let url = format!(
            "{}/drive/v1/files",
            self.drive_base_url.trim_end_matches('/')
        );

        let filters = r#"{"trashed":{"eq":false}}"#;
        let mut rb = self.http.get(&url).bearer_auth(&token).query(&[
            ("parent_id", parent_id),
            ("limit", "500"),
            ("filters", filters),
            ("thumbnail_size", "SIZE_MEDIUM"),
        ]);
        rb = self.authed_headers(rb);

        let response = rb.send().context("ls request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!("ls failed ({}): {}", status, sanitize(&body)));
        }

        let payload: DriveListResponse = response.json().context("invalid ls json")?;
        let entries = payload
            .files
            .into_iter()
            .map(|f| {
                let starred = f.is_starred();
                Entry {
                    id: f.id,
                    name: f.name,
                    kind: if f.kind.contains("folder") {
                        EntryKind::Folder
                    } else {
                        EntryKind::File
                    },
                    size: f.size.unwrap_or(0),
                    created_time: f.created_time.unwrap_or_default(),
                    starred,
                    thumbnail_link: f.thumbnail_link,
                }
            })
            .collect();
        Ok(entries)
    }

    pub fn ls_trash(&self, limit: u32) -> Result<Vec<Entry>> {
        let token = self.access_token()?;
        let url = format!(
            "{}/drive/v1/files",
            self.drive_base_url.trim_end_matches('/')
        );

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
        let entries = payload
            .files
            .into_iter()
            .map(|f| {
                let starred = f.is_starred();
                Entry {
                    id: f.id,
                    name: f.name,
                    kind: if f.kind.contains("folder") {
                        EntryKind::Folder
                    } else {
                        EntryKind::File
                    },
                    size: f.size.unwrap_or(0),
                    created_time: f.created_time.unwrap_or_default(),
                    starred,
                    thumbnail_link: f.thumbnail_link,
                }
            })
            .collect();
        Ok(entries)
    }

    pub fn mv(&self, ids: &[&str], to_parent_id: &str) -> Result<()> {
        let token = self.access_token()?;
        let url = format!(
            "{}/drive/v1/files:batchMove",
            self.drive_base_url.trim_end_matches('/')
        );

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
        let url = format!(
            "{}/drive/v1/files:batchCopy",
            self.drive_base_url.trim_end_matches('/')
        );

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
        let url = format!(
            "{}/drive/v1/files/{}",
            self.drive_base_url.trim_end_matches('/'),
            file_id
        );

        let payload = serde_json::json!({ "name": new_name });
        let mut rb = self.http.patch(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("rename request failed")?;
        ensure_success(response, "rename")
    }

    pub fn remove(&self, ids: &[&str]) -> Result<()> {
        let token = self.access_token()?;
        let url = format!(
            "{}/drive/v1/files:batchTrash",
            self.drive_base_url.trim_end_matches('/')
        );

        let payload = serde_json::json!({ "ids": ids });
        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("remove request failed")?;
        ensure_success(response, "remove")
    }

    pub fn delete_permanent(&self, ids: &[&str]) -> Result<()> {
        let token = self.access_token()?;
        let url = format!(
            "{}/drive/v1/files:batchDelete",
            self.drive_base_url.trim_end_matches('/')
        );

        let payload = serde_json::json!({ "ids": ids });
        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("permanent delete request failed")?;
        ensure_success(response, "permanent delete")
    }

    pub fn untrash(&self, ids: &[&str]) -> Result<()> {
        let token = self.access_token()?;
        let url = format!(
            "{}/drive/v1/files:batchUntrash",
            self.drive_base_url.trim_end_matches('/')
        );

        let payload = serde_json::json!({ "ids": ids });
        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("untrash request failed")?;
        ensure_success(response, "untrash")
    }

    pub fn mkdir(&self, parent_id: &str, name: &str) -> Result<Entry> {
        let token = self.access_token()?;
        let url = format!(
            "{}/drive/v1/files",
            self.drive_base_url.trim_end_matches('/')
        );

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
            starred,
            thumbnail_link: f.thumbnail_link,
        })
    }

    pub fn file_info(&self, file_id: &str) -> Result<FileInfoResponse> {
        let token = self.access_token()?;
        let url = format!(
            "{}/drive/v1/files/{}",
            self.drive_base_url.trim_end_matches('/'),
            file_id
        );

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
            .web_content_link
            .as_deref()
            .or(info.links.as_ref().and_then(|l| {
                l.get("application/octet-stream")
                    .and_then(|v| v.url.as_deref())
            }))
            .ok_or_else(|| anyhow!("no download link for file {}", file_id))?
            .to_string();

        let total_size = info
            .size
            .as_deref()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        Ok((url, total_size))
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
            .web_content_link
            .as_deref()
            .or(info.links.as_ref().and_then(|l| {
                l.get("application/octet-stream")
                    .and_then(|v| v.url.as_deref())
            }))
            .ok_or_else(|| anyhow!("no download link for file {}", file_id))?;

        // Check existing file size for resume
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
        let url = format!(
            "{}/drive/v1/about",
            self.drive_base_url.trim_end_matches('/')
        );

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
        let url = format!(
            "{}/drive/v1/files",
            self.drive_base_url.trim_end_matches('/')
        );

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
        let url = format!(
            "{}/drive/v1/tasks",
            self.drive_base_url.trim_end_matches('/')
        );

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
        let url = format!(
            "{}/drive/v1/task",
            self.drive_base_url.trim_end_matches('/')
        );

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
        let url = format!(
            "{}/drive/v1/tasks",
            self.drive_base_url.trim_end_matches('/')
        );

        // Build query params: task_ids=a&task_ids=b&delete_files=true
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

    // --- Star ---

    /// Star files by IDs.
    pub fn star(&self, ids: &[&str]) -> Result<()> {
        let token = self.access_token()?;
        let url = format!(
            "{}/drive/v1/files:star",
            self.drive_base_url.trim_end_matches('/')
        );

        let payload = serde_json::json!({ "ids": ids });
        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("star request failed")?;
        ensure_success(response, "star")
    }

    /// Unstar files by IDs.
    pub fn unstar(&self, ids: &[&str]) -> Result<()> {
        let token = self.access_token()?;
        let url = format!(
            "{}/drive/v1/files:unstar",
            self.drive_base_url.trim_end_matches('/')
        );

        let payload = serde_json::json!({ "ids": ids });
        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("unstar request failed")?;
        ensure_success(response, "unstar")
    }

    /// List starred files.
    pub fn starred_list(&self, limit: u32) -> Result<Vec<Entry>> {
        let token = self.access_token()?;
        let url = format!(
            "{}/drive/v1/files",
            self.drive_base_url.trim_end_matches('/')
        );

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
            .map(|f| Entry {
                id: f.id,
                name: f.name,
                kind: if f.kind.contains("folder") {
                    EntryKind::Folder
                } else {
                    EntryKind::File
                },
                size: f.size.unwrap_or(0),
                created_time: f.created_time.unwrap_or_default(),
                starred: true, // starred_list only returns starred items
                thumbnail_link: f.thumbnail_link,
            })
            .collect();
        Ok(entries)
    }

    // --- Events ---

    /// Get recent file events (recently added files).
    pub fn events(&self, limit: u32) -> Result<EventsResponse> {
        let token = self.access_token()?;
        let url = format!(
            "{}/drive/v1/events",
            self.drive_base_url.trim_end_matches('/')
        );

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

    // --- VIP / Account info ---

    /// Get VIP membership info.
    pub fn vip_info(&self) -> Result<VipInfoResponse> {
        let token = self.access_token()?;
        let url = format!(
            "{}/drive/v1/privilege/vip",
            self.drive_base_url.trim_end_matches('/')
        );

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
        let url = format!(
            "{}/vip/v1/activity/inviteCode",
            self.drive_base_url.trim_end_matches('/')
        );

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

    /// Get transfer quota info.
    pub fn transfer_quota(&self) -> Result<serde_json::Value> {
        let token = self.access_token()?;
        let url = format!(
            "{}/vip/v1/quantity/list",
            self.drive_base_url.trim_end_matches('/')
        );

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
            .web_content_link
            .as_deref()
            .or(info.links.as_ref().and_then(|l| {
                l.get("application/octet-stream")
                    .and_then(|v| v.url.as_deref())
            }))
            .ok_or_else(|| anyhow!("no download link for file {}", file_id))?;

        let file_size = info
            .size
            .as_deref()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

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
            let entries = self.ls(&current_id)?;
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

        // Step 1: init upload
        let token = self.access_token()?;
        let url = format!(
            "{}/drive/v1/files",
            self.drive_base_url.trim_end_matches('/')
        );
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

        // Step 2: check phase
        if init.upload_type == "UPLOAD_TYPE_RESUMABLE" {
            if let Some(resumable) = &init.resumable {
                if resumable.kind == "drive#uploadContext"
                    && init.file.phase.as_deref() == Some("PHASE_TYPE_COMPLETE")
                {
                    return Ok((file_name, true)); // dedup
                }
            }
        }

        if init.file.phase.as_deref() == Some("PHASE_TYPE_COMPLETE") {
            return Ok((file_name, true)); // dedup
        }

        // Need to actually upload
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

        // Step 3: initiate multipart upload
        let upload_id = self.oss_initiate_multipart(&oss_args)?;

        // Step 4: upload chunks
        let etags = self.oss_upload_chunks(&oss_args, &upload_id, local_path, file_size)?;

        // Step 5: complete multipart upload
        self.oss_complete_multipart(&oss_args, &upload_id, &etags)?;

        Ok((file_name, false))
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

        // Parse UploadId from XML
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
            (file_size + CHUNK_SIZE - 1) / CHUNK_SIZE
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
        // Build XML body
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

/// Compute the PikPak hash of a file.
/// Algorithm: chunk the file, SHA1 each chunk, concatenate hex hashes, SHA1 the result.
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

    // If file is empty, hash empty string
    if file_size == 0 {
        let mut hasher = Sha1::new();
        hasher.update(b"");
        let hash = hasher.finalize();
        for b in hash.iter() {
            write!(all_hashes, "{:02X}", b).unwrap();
        }
    }

    // Final SHA1 of concatenated hashes
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

    // Format as HTTP date: "Thu, 01 Jan 1970 00:00:00 GMT"
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
    // Simplified Gregorian calendar calculation
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
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].to_string())
}

// --- Response types ---

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
}

#[derive(Debug, Clone, Deserialize)]
pub struct MediaLink {
    #[serde(default)]
    pub url: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
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

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct MediaInfo {
    #[serde(default)]
    pub media_name: Option<String>,
    #[serde(default)]
    pub link: Option<MediaLink>,
    #[serde(default)]
    pub video: Option<MediaVideo>,
    #[serde(default)]
    pub is_default: Option<bool>,
    #[serde(default)]
    pub is_origin: Option<bool>,
    #[serde(default)]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileInfoResponse {
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
    pub web_content_link: Option<String>,
    #[serde(default)]
    pub links: Option<std::collections::HashMap<String, LinkInfo>>,
    #[serde(default)]
    pub medias: Option<Vec<MediaInfo>>,
}

#[derive(Debug, Clone, Deserialize)]
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

// --- Offline / Tasks response types ---

#[derive(Debug, Deserialize)]
pub struct OfflineTaskResponse {
    #[serde(default)]
    pub task: Option<OfflineTask>,
}

#[derive(Debug, Clone, Deserialize)]
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

// --- Events response types ---

#[derive(Debug, Deserialize)]
pub struct EventsResponse {
    #[serde(default)]
    pub events: Vec<EventEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventEntry {
    #[serde(default)]
    pub event: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub file_kind: Option<String>,
    #[serde(default)]
    pub created_time: Option<String>,
}

// --- VIP response types ---

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

// --- Helpers ---

fn ensure_success(response: reqwest::blocking::Response, op: &str) -> Result<()> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body = response.text().unwrap_or_default();
    Err(anyhow!("{} failed ({}): {}", op, status, sanitize(&body)))
}

fn default_session_path() -> Result<PathBuf> {
    let base = dirs::config_dir().ok_or_else(|| anyhow!("unable to locate config dir"))?;
    Ok(base.join("pikpaktui").join("session.json"))
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn sanitize(s: &str) -> String {
    if s.len() > 240 {
        format!("{}...", &s[..240])
    } else {
        s.to_string()
    }
}

fn md5_hex(input: &str) -> String {
    let digest = md5_compute(input.as_bytes());
    let mut hex = String::with_capacity(32);
    for b in &digest {
        write!(hex, "{:02x}", b).unwrap();
    }
    hex
}

fn md5_compute(input: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];

    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    let orig_len_bits = (input.len() as u64).wrapping_mul(8);
    let mut msg = input.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&orig_len_bits.to_le_bytes());

    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    for chunk in msg.chunks_exact(64) {
        let mut m = [0u32; 16];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            m[i] = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        }

        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);

        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };

            let temp = d;
            d = c;
            c = b;
            b = b.wrapping_add(
                (a.wrapping_add(f).wrapping_add(K[i]).wrapping_add(m[g])).rotate_left(S[i]),
            );
            a = temp;
        }

        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut result = [0u8; 16];
    result[0..4].copy_from_slice(&a0.to_le_bytes());
    result[4..8].copy_from_slice(&b0.to_le_bytes());
    result[8..12].copy_from_slice(&c0.to_le_bytes());
    result[12..16].copy_from_slice(&d0.to_le_bytes());
    result
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
}
