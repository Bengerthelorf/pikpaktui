mod account;
mod auth;
mod download;
mod drive;
mod file_info;
mod files;
mod models;
mod offline;
mod responses;
mod share;
mod upload;

use auth::{CaptchaInitResponse, SigninResponse};
pub use file_info::FileInfoResponse;
pub use models::{Entry, EntryKind, SessionToken};
pub use responses::{
    CreateShareResponse, EventsResponse, MyShare, OfflineListResponse, OfflineTask,
    OfflineTaskResponse, QuotaInfo, ShareInfoResponse, ShareListResponse, TransferBand,
    TransferQuotaResponse, VipInfoResponse,
};

use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_AUTH_BASE_URL: &str = "https://user.mypikpak.com";
const DEFAULT_DRIVE_BASE_URL: &str = "https://api-drive.mypikpak.com";
const DEFAULT_CLIENT_ID: &str = "YNxT9w7GMdWvEOKa";
const DEFAULT_CLIENT_SECRET: &str = "dbw2OtmVEeuUvIptb1Coyg";
const USER_AGENT: &str = "ANDROID-com.pikcloud.pikpak/1.21.0";

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
            return Err(anyhow!(
                "token refresh failed ({}): {}",
                status,
                sanitize(&body)
            ));
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

    pub fn http(&self) -> &reqwest::blocking::Client {
        &self.http
    }

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
    use md5::{Digest, Md5};
    let hash = Md5::digest(input.as_bytes());
    let mut hex = String::with_capacity(32);
    for b in hash.iter() {
        write!(hex, "{:02x}", b).unwrap();
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::drive::DriveListResponse;
    use super::*;
    use std::collections::HashMap;
    use std::io::Write as _;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    struct MockDownloadServer {
        base_url: String,
        download_hits: Arc<AtomicUsize>,
        handle: std::thread::JoinHandle<()>,
    }

    fn test_client(base_url: String, session_path: std::path::PathBuf) -> PikPak {
        let client = PikPak {
            http: reqwest::blocking::Client::builder().build().unwrap(),
            drive_base_url: base_url,
            auth_base_url: String::new(),
            client_id: String::new(),
            client_secret: String::new(),
            session_path,
            device_id: String::new(),
            captcha_token: String::new(),
            thumbnail_size: "SIZE_MEDIUM".to_string(),
            ls_cache: Mutex::new(HashMap::new()),
            refresh_lock: Mutex::new(()),
        };
        client
            .save_session(&SessionToken {
                access_token: "test-access".into(),
                refresh_token: "test-refresh".into(),
                expires_at_unix: now_unix() + 3600,
            })
            .unwrap();
        client
    }

    fn temp_test_dir(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("pikpaktui-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn start_mock_download_server(
        content: &'static [u8],
        ignore_range: bool,
        max_requests: usize,
    ) -> MockDownloadServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let download_hits = Arc::new(AtomicUsize::new(0));
        let hits = Arc::clone(&download_hits);
        let server_base_url = base_url.clone();

        let handle = std::thread::spawn(move || {
            for stream in listener.incoming().take(max_requests) {
                let Ok(mut stream) = stream else { continue };
                let mut request = [0u8; 4096];
                let n = std::io::Read::read(&mut stream, &mut request).unwrap_or(0);
                let request = String::from_utf8_lossy(&request[..n]);
                let first_line = request.lines().next().unwrap_or_default();

                if first_line.starts_with("GET /drive/v1/files/file") {
                    let body = format!(
                        r#"{{"name":"file.bin","size":"{}","web_content_link":"{}/download"}}"#,
                        content.len(),
                        server_base_url
                    );
                    write_response(&mut stream, 200, "OK", body.as_bytes());
                } else if first_line.starts_with("GET /download") {
                    hits.fetch_add(1, Ordering::SeqCst);
                    let range_start = request.lines().find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("range: bytes=")
                            .and_then(|range| range.strip_suffix('-'))
                            .and_then(|start| start.parse::<usize>().ok())
                    });

                    if !ignore_range && let Some(start) = range_start {
                        write_response(&mut stream, 206, "Partial Content", &content[start..]);
                    } else {
                        write_response(&mut stream, 200, "OK", content);
                    }
                } else {
                    write_response(&mut stream, 404, "Not Found", b"not found");
                }
            }
        });

        MockDownloadServer {
            base_url,
            download_hits,
            handle,
        }
    }

    fn write_response(stream: &mut std::net::TcpStream, code: u16, reason: &str, body: &[u8]) {
        let header = format!(
            "HTTP/1.1 {code} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(header.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
    }

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
        let expired = SessionToken {
            access_token: "old".into(),
            refresh_token: "r".into(),
            expires_at_unix: now_unix() - 1,
        };
        assert!(expired.is_expired(now_unix()));

        let valid = SessionToken {
            access_token: "good".into(),
            refresh_token: "r".into(),
            expires_at_unix: now_unix() + 600,
        };
        assert!(!valid.is_expired(now_unix() + 300));
    }

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
        let json = r#"{"files": []}"#;
        let resp: DriveListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.next_page_token, None);
    }

    #[test]
    fn drive_list_response_empty_token_treated_as_none() {
        let json = r#"{"files": [], "next_page_token": ""}"#;
        let resp: DriveListResponse = serde_json::from_str(json).unwrap();
        assert!(resp.next_page_token.as_deref().unwrap_or("").is_empty());
    }

    #[test]
    fn download_to_skips_already_complete_file() {
        let server = start_mock_download_server(b"hello", false, 1);
        let dir = temp_test_dir("download-complete");
        let dest = dir.join("file.bin");
        std::fs::write(&dest, b"hello").unwrap();
        let client = test_client(server.base_url, dir.join("session.json"));

        let total = client.download_to("file", &dest).unwrap();

        assert_eq!(total, 5);
        assert_eq!(std::fs::read(&dest).unwrap(), b"hello");
        assert_eq!(server.download_hits.load(Ordering::SeqCst), 0);
        server.handle.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn download_to_reports_size_when_server_ignores_range() {
        let server = start_mock_download_server(b"hello", true, 2);
        let dir = temp_test_dir("download-range-ignored");
        let dest = dir.join("file.bin");
        std::fs::write(&dest, b"he").unwrap();
        let client = test_client(server.base_url, dir.join("session.json"));

        let total = client.download_to("file", &dest).unwrap();

        assert_eq!(total, 5);
        assert_eq!(std::fs::read(&dest).unwrap(), b"hello");
        assert_eq!(server.download_hits.load(Ordering::SeqCst), 1);
        server.handle.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
