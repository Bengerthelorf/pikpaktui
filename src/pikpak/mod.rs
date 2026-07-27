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
#[cfg(test)]
pub(crate) use download::part_path;
pub(crate) use download::{finish_partial_download, prepare_partial_download};
pub use file_info::FileInfoResponse;
pub use models::{Entry, EntryKind, SessionToken};
pub use responses::{
    CreateShareResponse, EventsResponse, MyShare, OfflineListResponse, OfflineTask,
    OfflineTaskResponse, QuotaInfo, ShareDetailResponse, ShareEntry, ShareInfoResponse,
    ShareListResponse, TransferBand, TransferQuotaResponse, VipInfoResponse,
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
// The version/package pair must match CLIENT_ID and CAPTCHA_SALTS: the
// captcha_sign seed concatenates all of them, and mixing sets breaks the sign.
const CLIENT_VERSION: &str = "1.53.2";
const PACKAGE_NAME: &str = "com.pikcloud.pikpak";
const USER_AGENT: &str = "ANDROID-com.pikcloud.pikpak/1.53.2";

/// Salt chain for captcha_sign, paired with the Android client 1.53.2
/// constants above (OpenList drivers/pikpak/util.go AndroidAlgorithms).
const CAPTCHA_SALTS: &[&str] = &[
    "SOP04dGzk0TNO7t7t9ekDbAmx+eq0OI1ovEx",
    "nVBjhYiND4hZ2NCGyV5beamIr7k6ifAsAbl",
    "Ddjpt5B/Cit6EDq2a6cXgxY9lkEIOw4yC1GDF28KrA",
    "VVCogcmSNIVvgV6U+AochorydiSymi68YVNGiz",
    "u5ujk5sM62gpJOsB/1Gu/zsfgfZO",
    "dXYIiBOAHZgzSruaQ2Nhrqc2im",
    "z5jUTBSIpBN9g4qSJGlidNAutX6",
    "KJE2oveZ34du/g1tiimm",
];

#[derive(Default)]
struct LsCache {
    generation: u64,
    entries: HashMap<String, Vec<Entry>>,
}

pub struct PikPak {
    pub(crate) http: reqwest::blocking::Client,
    drive_base_url: String,
    auth_base_url: String,
    client_id: String,
    client_secret: String,
    session_path: PathBuf,
    device_id: String,
    /// Refreshed lazily during `&self` drive calls, hence the Mutex.
    captcha_token: Mutex<String>,
    captcha_expires_at_unix: Mutex<i64>,
    /// Serializes action-captcha refreshes. Reactive retries keep this held
    /// through the retry so another action cannot replace the fresh token.
    captcha_refresh_lock: Mutex<()>,
    /// Action that produced the current in-memory captcha token. This lets
    /// concurrent failures for the same action reuse one refresh safely.
    captcha_action: Mutex<String>,
    user_id: String,
    pub thumbnail_size: String,
    ls_cache: Mutex<LsCache>,
    /// Serializes session-file writes and load/modify/save updates in this
    /// process. A disk lock below coordinates separate CLI/TUI processes.
    session_lock: Mutex<()>,
    refresh_lock: Mutex<()>,
}

impl PikPak {
    pub fn new() -> Result<Self> {
        let mut client = Self {
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
            captcha_token: Mutex::new(String::new()),
            captcha_expires_at_unix: Mutex::new(0),
            captcha_refresh_lock: Mutex::new(()),
            captcha_action: Mutex::new(String::new()),
            user_id: String::new(),
            thumbnail_size: "SIZE_MEDIUM".to_string(),
            ls_cache: Mutex::new(LsCache::default()),
            session_lock: Mutex::new(()),
            refresh_lock: Mutex::new(()),
        };
        // Re-adopt the device identity from the saved session: without it,
        // every run after the login one sent no x-device-id/x-captcha-token,
        // a known cause of intermittent 403/riskLimited responses.
        if let Ok(Some(session)) = client.load_session() {
            client.device_id = session.device_id;
            client.captcha_token = Mutex::new(session.captcha_token);
            client.captcha_expires_at_unix = Mutex::new(session.captcha_expires_at_unix);
            client.user_id = session.user_id;
        }
        Ok(client)
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
        let _guard = self.session_lock.lock().unwrap_or_else(|e| e.into_inner());
        let _file_guard = self.lock_session_file()?;
        self.save_session_unlocked(token)
    }

    fn lock_session_file(&self) -> Result<fs::File> {
        if let Some(parent) = self.session_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create dir {}", parent.display()))?;
        }
        let lock_path = self.session_path.with_extension("lock");
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .with_context(|| format!("failed to open session lock {}", lock_path.display()))?;
        set_file_owner_only(&lock_path);
        file.lock()
            .with_context(|| format!("failed to lock session {}", lock_path.display()))?;
        Ok(file)
    }

    fn save_session_unlocked(&self, token: &SessionToken) -> Result<()> {
        if let Some(parent) = self.session_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create dir {}", parent.display()))?;
        }
        let raw = serde_json::to_string_pretty(token).context("failed to encode session")?;
        let tmp_path = self.session_path.with_extension("tmp");
        write_owner_only(&tmp_path, raw.as_bytes())
            .with_context(|| format!("failed to write temp session {}", tmp_path.display()))?;
        fs::rename(&tmp_path, &self.session_path)
            .with_context(|| format!("failed to rename session {}", self.session_path.display()))?;
        set_file_owner_only(&self.session_path);
        Ok(())
    }

    fn update_session(&self, update: impl FnOnce(&mut SessionToken)) -> Result<()> {
        let _guard = self.session_lock.lock().unwrap_or_else(|e| e.into_inner());
        let _file_guard = self.lock_session_file()?;
        if let Some(mut session) = self.load_session()? {
            update(&mut session);
            self.save_session_unlocked(&session)?;
        }
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
        let captcha_expires_in =
            i64::try_from(captcha.expires_in).context("captcha expires_in overflow")?;
        let captcha_expires_at_unix = now_unix().saturating_add(captcha_expires_in);
        let login_captcha = captcha
            .captcha_token
            // An empty token would sail through and fail signin with an opaque
            // 4xx, and it would shadow the documented env escape hatch.
            .filter(|t| !t.is_empty())
            .or_else(|| {
                env::var("PIKPAK_CAPTCHA_TOKEN")
                    .ok()
                    .filter(|t| !t.is_empty())
            })
            .ok_or_else(|| {
                let hint = captcha.url.as_deref().unwrap_or("<no challenge url>");
                anyhow!(
                    "captcha token unavailable; set PIKPAK_CAPTCHA_TOKEN. url={}",
                    sanitize(hint)
                )
            })?;
        *self.captcha_token.lock().unwrap_or_else(|e| e.into_inner()) = login_captcha.clone();
        *self
            .captcha_expires_at_unix
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = captcha_expires_at_unix;

        let url = self.auth_url("v1/auth/signin");
        let payload = serde_json::json!({
            "username": email,
            "password": password,
            "client_id": self.client_id,
            "client_secret": self.client_secret,
            "captcha_token": login_captcha,
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
        self.user_id = signin.sub.clone();

        let token = SessionToken {
            access_token: signin.access_token,
            refresh_token: signin.refresh_token,
            expires_at_unix: now.saturating_add(expires_in),
            device_id: self.device_id.clone(),
            captcha_token: self
                .captcha_token
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
            captcha_expires_at_unix,
            user_id: signin.sub,
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
    fn refresh_session(&self, _refresh_token_hint: &str) -> Result<String> {
        // Hold both locks through the HTTP exchange. A second process then
        // reloads the newly rotated refresh token instead of submitting the
        // stale one it observed before waiting.
        let _guard = self.session_lock.lock().unwrap_or_else(|e| e.into_inner());
        let _file_guard = self.lock_session_file()?;
        let mut session = self
            .load_session()?
            .ok_or_else(|| anyhow!("not logged in, please login first"))?;
        if !session.is_expired(now_unix() + 300) {
            return Ok(session.access_token);
        }

        let url = self.auth_url("v1/auth/token");

        let payload = serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": session.refresh_token.clone(),
            "client_id": self.client_id,
            "client_secret": self.client_secret,
        });

        let mut rb = self.http.post(&url).json(&payload);
        if !self.device_id.is_empty() {
            rb = rb.header("x-device-id", &self.device_id);
        }
        let response = rb.send().context("token refresh request failed")?;

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
        let access_token = refreshed.access_token;
        let refresh_token = refreshed.refresh_token;
        let refreshed_user_id = refreshed.sub;

        // Only token fields change; captcha/device fields remain the latest
        // values loaded after acquiring the cross-process session lock.
        session.access_token = access_token.clone();
        session.refresh_token = refresh_token;
        session.expires_at_unix = now_unix().saturating_add(expires_in);
        if !self.device_id.is_empty() {
            session.device_id.clone_from(&self.device_id);
        }
        if !refreshed_user_id.is_empty() {
            session.user_id = refreshed_user_id;
        }
        self.save_session_unlocked(&session)?;

        Ok(access_token)
    }

    fn request_action(&self, rb: &reqwest::blocking::RequestBuilder) -> Result<String> {
        let request = rb
            .try_clone()
            .ok_or_else(|| anyhow!("cannot clone authenticated request"))?
            .build()
            .context("cannot inspect authenticated request")?;
        Ok(format!("{}:{}", request.method(), request.url().path()))
    }

    fn captcha_snapshot(&self) -> (String, i64) {
        let token = self
            .captcha_token
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let expires_at = *self
            .captcha_expires_at_unix
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        (token, expires_at)
    }

    fn captcha_needs_refresh(&self) -> bool {
        let (token, expires_at) = self.captcha_snapshot();
        if token.is_empty() {
            !self.device_id.is_empty()
        } else {
            expires_at <= now_unix().saturating_add(30)
        }
    }

    fn ensure_captcha_for_action(&self, action: &str) -> Result<()> {
        if !self.captcha_needs_refresh() {
            return Ok(());
        }
        let _guard = self
            .captcha_refresh_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Another request may have refreshed while this one waited.
        if self.captcha_needs_refresh() {
            self.refresh_captcha_for_action_locked(action)
                .with_context(|| format!("captcha refresh for {action} failed"))?;
        }
        Ok(())
    }

    fn attach_authed_headers(
        &self,
        mut rb: reqwest::blocking::RequestBuilder,
    ) -> (reqwest::blocking::RequestBuilder, String) {
        if !self.device_id.is_empty() {
            rb = rb.header("x-device-id", &self.device_id);
        }
        let captcha = self
            .captcha_token
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if !captcha.is_empty() {
            rb = rb.header("x-captcha-token", &captcha);
        }
        (rb, captcha)
    }

    fn send_authed(
        &self,
        op: &str,
        rb: reqwest::blocking::RequestBuilder,
    ) -> Result<reqwest::blocking::Response> {
        let action = self.request_action(&rb)?;
        let retry = rb
            .try_clone()
            .ok_or_else(|| anyhow!("cannot clone {op} request for captcha retry"))?;

        self.ensure_captcha_for_action(&action)?;
        let (first, used_captcha) = self.attach_authed_headers(rb);
        let response = first
            .send()
            .with_context(|| format!("{op} request failed"))?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let body = response.text().unwrap_or_default();
        if !api_error_requires_captcha_refresh(&body) {
            return Err(anyhow!("{} failed ({}): {}", op, status, sanitize(&body)));
        }

        let _guard = self
            .captcha_refresh_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let current_captcha = self
            .captcha_token
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let current_action = self
            .captcha_action
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let current_expiry = *self
            .captcha_expires_at_unix
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let same_action_was_refreshed = current_captcha != used_captcha
            && current_action == action
            && current_expiry > now_unix().saturating_add(30);
        if !same_action_was_refreshed {
            self.refresh_captcha_for_action_locked(&action)
                .with_context(|| format!("captcha refresh for {action} failed"))?;
        }

        let (retry, _) = self.attach_authed_headers(retry);
        let response = retry
            .send()
            .with_context(|| format!("{op} request failed"))?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let body = response.text().unwrap_or_default();
        Err(anyhow!("{} failed ({}): {}", op, status, sanitize(&body)))
    }

    /// The salted MD5 chain reference clients compute for captcha refresh:
    /// seed = client_id + client_version + package_name + device_id + timestamp,
    /// then re-hash appending each salt in order; the sign is "1." + digest.
    fn captcha_sign(&self, timestamp: &str) -> String {
        let mut s = format!(
            "{}{}{}{}{}",
            self.client_id, CLIENT_VERSION, PACKAGE_NAME, self.device_id, timestamp
        );
        for salt in CAPTCHA_SALTS {
            s = md5_hex(&format!("{s}{salt}"));
        }
        format!("1.{s}")
    }

    /// Refresh the captcha token for one drive action ("METHOD:/path").
    /// Reference clients do this reactively when the API answers error_code 9
    /// (riskLimited); the new token is kept for subsequent calls.
    fn refresh_captcha_for_action_locked(&self, action: &str) -> Result<()> {
        let token = self.access_token()?;
        let url = self.auth_url("v1/shield/captcha/init");
        let timestamp = (now_unix_millis()).to_string();
        let sign = self.captcha_sign(&timestamp);
        let previous = self
            .captcha_token
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        let mut meta = serde_json::json!({
            "client_version": CLIENT_VERSION,
            "package_name": PACKAGE_NAME,
            "timestamp": timestamp,
            "captcha_sign": sign,
        });
        if !self.user_id.is_empty() {
            meta["user_id"] = serde_json::json!(self.user_id);
        }
        let payload = serde_json::json!({
            "action": action,
            "captcha_token": previous,
            "client_id": self.client_id,
            "device_id": self.device_id,
            "meta": meta,
            "redirect_uri": "xlaccsdk01://xbase.cloud/callback?state=harbor",
        });

        let mut rb = self
            .http
            .post(&url)
            .query(&[("client_id", self.client_id.as_str())])
            .bearer_auth(&token)
            .json(&payload);
        if !self.device_id.is_empty() {
            rb = rb.header("x-device-id", &self.device_id);
        }
        let response = rb.send().context("captcha refresh failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!(
                "captcha refresh failed ({}): {}",
                status,
                sanitize(&body)
            ));
        }
        let captcha: CaptchaInitResponse = response.json().context("invalid captcha json")?;
        let expires_in =
            i64::try_from(captcha.expires_in).context("captcha expires_in overflow")?;
        let expires_at_unix = now_unix().saturating_add(expires_in);
        let new_token = captcha
            .captcha_token
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                let hint = captcha.url.as_deref().unwrap_or("<no challenge url>");
                anyhow!(
                    "captcha refresh needs human verification: {}",
                    sanitize(hint)
                )
            })?;

        *self.captcha_token.lock().unwrap_or_else(|e| e.into_inner()) = new_token.clone();
        *self
            .captcha_expires_at_unix
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = expires_at_unix;
        *self
            .captcha_action
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = action.to_string();
        // Persist with one load/modify/save lock so an overlapping access-token
        // refresh cannot be overwritten by this captcha update.
        self.update_session(|session| {
            session.captcha_token = new_token;
            session.captcha_expires_at_unix = expires_at_unix;
        })?;
        Ok(())
    }

    fn drive_url(&self, path: &str) -> String {
        format!("{}/{}", self.drive_base_url.trim_end_matches('/'), path)
    }

    fn auth_url(&self, path: &str) -> String {
        format!("{}/{}", self.auth_base_url.trim_end_matches('/'), path)
    }

    /// Drop the lifetime listing cache that backs `ls_cached` and path
    /// resolution. Mutations call this on success so later path lookups see the
    /// new tree instead of a stale snapshot.
    fn clear_ls_cache(&self) {
        let mut cache = self.ls_cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.generation = cache.generation.wrapping_add(1);
        cache.entries.clear();
    }

    pub fn http(&self) -> &reqwest::blocking::Client {
        &self.http
    }

    pub fn events(&self, limit: u32) -> Result<EventsResponse> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/events");

        let rb = self.http.get(&url).bearer_auth(&token).query(&[
            ("thumbnail_size", self.thumbnail_size.as_str()),
            ("limit", &limit.to_string()),
        ]);
        let response = self.send_authed("events", rb)?;
        json_or_api_error(response, "events")
    }
}

// These two helpers cover the common drive/auth API error shape: a non-success
// status carries a JSON/text body we surface (truncated by `sanitize`) in the
// error. Pick by what the *success* body is:
//   - `ensure_success`    — success body is ignored (batch mutations, retries)
//   - `json_or_api_error` — success body is decoded into `T`
// Endpoints that never read the error body (file downloads, text preview, range
// probes) build their own errors and use neither. A few calls with bespoke
// handling also stay hand-written on purpose — e.g. `save_share` maps a specific
// body marker, and `share_info` adds a post-decode status check.

/// Turn a non-success status into an error with the sanitized body, for
/// endpoints whose success response we don't need to decode.
fn ensure_success(response: reqwest::blocking::Response, op: &str) -> Result<()> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body = response.text().unwrap_or_default();
    Err(anyhow!("{} failed ({}): {}", op, status, sanitize(&body)))
}

/// Decode a JSON success body into `T`, or turn a non-success status into an
/// error carrying the sanitized response body. `op` names the operation for both
/// the failure message and the decode context (e.g. `"quota"` → `"invalid quota
/// json"`).
fn json_or_api_error<T: serde::de::DeserializeOwned>(
    response: reqwest::blocking::Response,
    op: &str,
) -> Result<T> {
    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(anyhow!("{} failed ({}): {}", op, status, sanitize(&body)));
    }
    response
        .json()
        .with_context(|| format!("invalid {op} json"))
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

/// Write `data` to `path`, creating the file 0600 on unix so the secret is
/// never world-readable — not even in the window before the post-rename chmod.
#[cfg(unix)]
fn write_owner_only(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(data)
}

#[cfg(not(unix))]
fn write_owner_only(path: &Path, data: &[u8]) -> std::io::Result<()> {
    fs::write(path, data)
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn now_unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// PikPak reuses numeric error code 9 for unrelated business failures. Only
/// the documented captcha/risk reasons may refresh and replay a request.
fn api_error_requires_captcha_refresh(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    if value.get("error_code").and_then(|code| code.as_i64()) != Some(9) {
        return false;
    }
    ["error", "reason", "error_description"]
        .into_iter()
        .filter_map(|field| value.get(field).and_then(|value| value.as_str()))
        .any(|reason| {
            reason.eq_ignore_ascii_case("captcha_invalid")
                || reason.eq_ignore_ascii_case("risklimited")
                || reason.eq_ignore_ascii_case("risk_limited")
        })
}

/// Sanitize an API-provided filename for a single portable path component.
pub(crate) fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.replace("..", "_");
    let cleaned = cleaned.trim_end_matches([' ', '.']);
    let mut cleaned = if cleaned.is_empty() {
        "_".to_string()
    } else {
        cleaned.to_string()
    };

    let stem = cleaned
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON" | "CONIN$" | "CONOUT$" | "PRN" | "AUX" | "NUL"
    ) || stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
        .is_some_and(|n| {
            matches!(
                n,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        });
    if reserved {
        cleaned.insert(0, '_');
    }
    cleaned
}

/// Reserve a name that is unique within `taken`, appending " (n)" before the
/// extension when needed. PikPak folders may hold duplicate names (and
/// sanitization can collapse distinct ones), but a local directory cannot —
/// without this, two same-named entries download into one interleaved file.
/// The ".part" sidecar of each reserved name is reserved too, so a cloud file
/// literally named "a.part" can't fight over the sidecar of "a".
pub(crate) fn unique_local_name(
    taken: &mut std::collections::HashSet<String>,
    name: &str,
) -> String {
    fn reserve(taken: &mut std::collections::HashSet<String>, cand: &str) -> bool {
        let sidecar = format!("{cand}.part");
        let identity = format!("{sidecar}.meta");
        if taken.contains(cand) || taken.contains(&sidecar) || taken.contains(&identity) {
            return false;
        }
        taken.insert(cand.to_string());
        taken.insert(sidecar);
        taken.insert(identity);
        true
    }

    if reserve(taken, name) {
        return name.to_string();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, Some(e)),
        _ => (name, None),
    };
    for n in 1u64.. {
        let candidate = match ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        };
        if reserve(taken, &candidate) {
            return candidate;
        }
    }
    unreachable!("u64 counter exhausted");
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
mod filename_safety_tests {
    use super::{sanitize_filename, unique_local_name};
    use std::collections::HashSet;

    #[test]
    fn sanitize_filename_replaces_windows_illegal_and_control_characters() {
        assert_eq!(
            sanitize_filename("a<b>c:d\"e/f\\g|h?i*j\u{1f}.txt"),
            "a_b_c_d_e_f_g_h_i_j_.txt"
        );
    }

    #[test]
    fn sanitize_filename_removes_windows_trailing_dots_and_spaces() {
        assert_eq!(sanitize_filename("report. "), "report");
        assert_eq!(sanitize_filename("..."), "_");
    }

    #[test]
    fn sanitize_filename_escapes_windows_reserved_device_stems() {
        for name in [
            "CON",
            "con.txt",
            "CONIN$",
            "conout$.log",
            "PRN",
            "AUX.log",
            "NUL",
            "COM1",
            "com¹.txt",
            "LPT²",
            "lpt9.bin",
        ] {
            assert!(
                sanitize_filename(name).starts_with('_'),
                "{name} remained a reserved device path"
            );
        }
        assert_eq!(sanitize_filename("com10.txt"), "com10.txt");
    }

    #[test]
    fn unique_name_reserves_partial_identity_sidecar() {
        let mut taken = HashSet::new();
        assert_eq!(unique_local_name(&mut taken, "movie.mkv"), "movie.mkv");
        assert_eq!(
            unique_local_name(&mut taken, "movie.mkv.part.meta"),
            "movie.mkv.part (1).meta"
        );
    }
}

#[cfg(test)]
mod unique_name_tests {
    use super::unique_local_name;
    use std::collections::HashSet;

    #[test]
    fn first_use_keeps_name() {
        let mut taken = HashSet::new();
        assert_eq!(unique_local_name(&mut taken, "a.txt"), "a.txt");
    }

    #[test]
    fn duplicates_get_numbered_before_extension() {
        let mut taken = HashSet::new();
        assert_eq!(unique_local_name(&mut taken, "a.txt"), "a.txt");
        assert_eq!(unique_local_name(&mut taken, "a.txt"), "a (1).txt");
        assert_eq!(unique_local_name(&mut taken, "a.txt"), "a (2).txt");
    }

    #[test]
    fn no_extension_appends_suffix() {
        let mut taken = HashSet::new();
        assert_eq!(unique_local_name(&mut taken, "folder"), "folder");
        assert_eq!(unique_local_name(&mut taken, "folder"), "folder (1)");
    }

    #[test]
    fn dotfile_is_not_split() {
        let mut taken = HashSet::new();
        assert_eq!(unique_local_name(&mut taken, ".gitignore"), ".gitignore");
        assert_eq!(
            unique_local_name(&mut taken, ".gitignore"),
            ".gitignore (1)"
        );
    }

    #[test]
    fn skips_names_already_reserved() {
        let mut taken: HashSet<String> = ["a (1).txt".to_string()].into();
        assert_eq!(unique_local_name(&mut taken, "a.txt"), "a.txt");
        assert_eq!(unique_local_name(&mut taken, "a.txt"), "a (2).txt");
    }
}

#[cfg(test)]
pub(crate) fn accept_test_connection(
    listener: &std::net::TcpListener,
) -> std::io::Result<std::net::TcpStream> {
    let (stream, _) = listener.accept()?;
    stream.set_nonblocking(false)?;
    Ok(stream)
}

#[cfg(test)]
pub(crate) fn read_test_http_request(reader: &mut impl std::io::Read) -> std::io::Result<String> {
    const MAX_REQUEST_SIZE: usize = 64 * 1024;

    let mut request = Vec::new();
    let expected_len = loop {
        if let Some(header_end) = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|position| position + 4)
        {
            let headers = std::str::from_utf8(&request[..header_end])
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
            let content_length = headers
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find_map(|(name, value)| {
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>())
                })
                .transpose()
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?
                .unwrap_or(0);
            break header_end.checked_add(content_length).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "HTTP request length overflow",
                )
            })?;
        }

        if request.len() >= MAX_REQUEST_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "HTTP test request headers are too large",
            ));
        }
        let mut buf = [0u8; 8192];
        let n = reader.read(&mut buf)?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "HTTP test request ended before its headers",
            ));
        }
        request.extend_from_slice(&buf[..n]);
    };

    if expected_len > MAX_REQUEST_SIZE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "HTTP test request is too large",
        ));
    }
    while request.len() < expected_len {
        let mut buf = [0u8; 8192];
        let remaining = expected_len - request.len();
        let read_len = remaining.min(buf.len());
        let n = reader.read(&mut buf[..read_len])?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "HTTP test request ended before its declared body",
            ));
        }
        request.extend_from_slice(&buf[..n]);
    }
    request.truncate(expected_len);
    Ok(String::from_utf8_lossy(&request).into_owned())
}

#[cfg(test)]
mod tests {
    use super::drive::DriveListResponse;
    use super::*;
    use std::io::Write as _;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    type PaginatedGetServer = (
        String,
        Arc<Mutex<Vec<Option<String>>>>,
        std::thread::JoinHandle<()>,
    );

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
            captcha_token: Mutex::new(String::new()),
            captcha_expires_at_unix: Mutex::new(0),
            captcha_refresh_lock: Mutex::new(()),
            captcha_action: Mutex::new(String::new()),
            user_id: String::new(),
            thumbnail_size: "SIZE_MEDIUM".to_string(),
            ls_cache: Mutex::new(LsCache::default()),
            session_lock: Mutex::new(()),
            refresh_lock: Mutex::new(()),
        };
        client
            .save_session(&SessionToken {
                access_token: "test-access".into(),
                refresh_token: "test-refresh".into(),
                expires_at_unix: now_unix() + 3600,
                ..Default::default()
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
                let request = read_test_http_request(&mut stream).unwrap_or_default();
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
                    let range_bounds = request.lines().find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("range: bytes=")
                            .and_then(|range| range.split_once('-'))
                            .and_then(|(start, end)| {
                                Some((
                                    start.parse::<usize>().ok()?,
                                    (!end.is_empty())
                                        .then(|| end.parse::<usize>().ok())
                                        .flatten(),
                                ))
                            })
                    });

                    if !ignore_range && let Some((start, requested_end)) = range_bounds {
                        let end = requested_end
                            .unwrap_or_else(|| content.len().saturating_sub(1))
                            .min(content.len().saturating_sub(1));
                        let content_range =
                            format!("Content-Range: bytes {start}-{end}/{}\r\n", content.len());
                        write_response_with_headers(
                            &mut stream,
                            206,
                            "Partial Content",
                            &content[start..=end],
                            &content_range,
                        );
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
        write_response_with_headers(stream, code, reason, body, "");
    }

    #[test]
    fn mock_http_reader_waits_for_the_declared_request_body() {
        struct ChunkedReader<R> {
            inner: R,
            max_chunk: usize,
        }

        impl<R: std::io::Read> std::io::Read for ChunkedReader<R> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let limit = buf.len().min(self.max_chunk);
                self.inner.read(&mut buf[..limit])
            }
        }

        let raw = concat!(
            "POST /v1/shield/captcha/init HTTP/1.1\r\n",
            "Host: 127.0.0.1\r\n",
            "Content-Length: 11\r\n",
            "\r\n",
            "hello-world"
        );
        let mut reader = ChunkedReader {
            inner: std::io::Cursor::new(raw.as_bytes()),
            max_chunk: 7,
        };

        let request = read_test_http_request(&mut reader).unwrap();

        assert_eq!(request, raw);
    }

    fn write_response_with_headers(
        stream: &mut std::net::TcpStream,
        code: u16,
        reason: &str,
        body: &[u8],
        extra_headers: &str,
    ) {
        let header = format!(
            "HTTP/1.1 {code} {reason}\r\nContent-Length: {}\r\n{extra_headers}Connection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(header.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
    }

    /// One-shot server that replies to a single request with a canned status and
    /// body, regardless of path. Used to exercise the shared API error handling.
    fn start_canned_server(
        status: u16,
        reason: &'static str,
        body: Vec<u8>,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming().take(1) {
                let Ok(mut stream) = stream else { continue };
                let _ = read_test_http_request(&mut stream);
                write_response(&mut stream, status, reason, &body);
            }
        });
        (base_url, handle)
    }

    fn start_paginated_get_server(responses: Vec<&'static str>) -> PaginatedGetServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let requested_tokens = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requested_tokens);
        let handle = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
            let mut served = 0usize;
            while served < responses.len() && std::time::Instant::now() < deadline {
                let mut stream = match accept_test_connection(&listener) {
                    Ok(stream) => stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        continue;
                    }
                    Err(e) => panic!("share detail server accept failed: {e}"),
                };
                let request = read_test_http_request(&mut stream).unwrap_or_default();
                let target = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or_default();
                let page_token = target.split_once('?').and_then(|(_, query)| {
                    query
                        .split('&')
                        .find_map(|pair| pair.strip_prefix("page_token=").map(str::to_string))
                });
                captured
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(page_token);
                write_response(&mut stream, 200, "OK", responses[served].as_bytes());
                served += 1;
            }
        });
        (base_url, requested_tokens, handle)
    }

    fn start_captcha_refresh_server()
    -> (String, Arc<Mutex<Vec<String>>>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let handle = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while std::time::Instant::now() < deadline {
                match accept_test_connection(&listener) {
                    Ok(mut stream) => {
                        let request = read_test_http_request(&mut stream).unwrap_or_default();
                        let first_line = request.lines().next().unwrap_or_default().to_string();
                        captured
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push(request);

                        if first_line.starts_with("POST /v1/shield/captcha/init") {
                            write_response(
                                &mut stream,
                                200,
                                "OK",
                                br#"{"captcha_token":"fresh-captcha","expires_in":300}"#,
                            );
                        } else if first_line.starts_with("GET /drive/v1/about") {
                            write_response(
                                &mut stream,
                                200,
                                "OK",
                                br#"{"quota":{"limit":"100","usage":"1"}}"#,
                            );
                        } else {
                            write_response(&mut stream, 404, "Not Found", b"not found");
                        }

                        if captured.lock().unwrap_or_else(|e| e.into_inner()).len() == 2 {
                            break;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(e) => panic!("captcha server accept failed: {e}"),
                }
            }
        });
        (base_url, requests, handle)
    }

    #[derive(Clone, Copy)]
    enum ReactiveCaptchaMode {
        RetrySucceeds,
        RefreshFails,
        RetryStillLimited,
    }

    fn start_reactive_captcha_server(
        mode: ReactiveCaptchaMode,
    ) -> (String, Arc<Mutex<Vec<String>>>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let handle = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            let mut about_attempts = 0usize;
            while std::time::Instant::now() < deadline {
                let mut stream = match accept_test_connection(&listener) {
                    Ok(stream) => stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        continue;
                    }
                    Err(e) => panic!("reactive captcha server accept failed: {e}"),
                };
                let request = read_test_http_request(&mut stream).unwrap_or_default();
                let first_line = request.lines().next().unwrap_or_default().to_string();
                captured
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(request);

                if first_line.starts_with("GET /drive/v1/about") {
                    about_attempts += 1;
                    if about_attempts == 1 || matches!(mode, ReactiveCaptchaMode::RetryStillLimited)
                    {
                        write_response(
                            &mut stream,
                            403,
                            "Forbidden",
                            br#"{"error_code":9,"error":"riskLimited"}"#,
                        );
                    } else {
                        write_response(
                            &mut stream,
                            200,
                            "OK",
                            br#"{"quota":{"limit":"100","usage":"1"}}"#,
                        );
                    }
                } else if first_line.starts_with("POST /v1/shield/captcha/init") {
                    assert!(
                        captured
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .last()
                            .unwrap()
                            .contains(r#""action":"GET:/drive/v1/about""#)
                    );
                    if matches!(mode, ReactiveCaptchaMode::RefreshFails) {
                        write_response(
                            &mut stream,
                            500,
                            "Internal Server Error",
                            br#"{"error":"refresh failed"}"#,
                        );
                        break;
                    }
                    write_response(
                        &mut stream,
                        200,
                        "OK",
                        br#"{"captcha_token":"fresh-captcha","expires_in":300}"#,
                    );
                } else {
                    write_response(&mut stream, 404, "Not Found", b"not found");
                }

                let request_count = captured.lock().unwrap_or_else(|e| e.into_inner()).len();
                let expected = match mode {
                    ReactiveCaptchaMode::RefreshFails => 2,
                    ReactiveCaptchaMode::RetrySucceeds | ReactiveCaptchaMode::RetryStillLimited => {
                        3
                    }
                };
                if request_count >= expected {
                    break;
                }
            }
        });
        (base_url, requests, handle)
    }

    fn start_concurrent_captcha_server(
        guarded_requests: usize,
    ) -> (String, Arc<AtomicUsize>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let refresh_count = Arc::new(AtomicUsize::new(0));
        let refreshes = Arc::clone(&refresh_count);
        let handle = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            let mut pending_refreshes: Vec<std::net::TcpStream> = Vec::new();
            let mut first_refresh_at: Option<std::time::Instant> = None;
            let mut about_count = 0usize;

            while std::time::Instant::now() < deadline {
                match accept_test_connection(&listener) {
                    Ok(mut stream) => {
                        let request = read_test_http_request(&mut stream).unwrap_or_default();
                        let first_line = request.lines().next().unwrap_or_default();
                        if first_line.starts_with("POST /v1/shield/captcha/init") {
                            refreshes.fetch_add(1, Ordering::SeqCst);
                            first_refresh_at.get_or_insert_with(std::time::Instant::now);
                            pending_refreshes.push(stream);
                        } else if first_line.starts_with("GET /drive/v1/about") {
                            about_count += 1;
                            write_response(
                                &mut stream,
                                200,
                                "OK",
                                br#"{"quota":{"limit":"100","usage":"1"}}"#,
                            );
                        } else {
                            write_response(&mut stream, 404, "Not Found", b"not found");
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                    Err(e) => panic!("concurrent captcha server accept failed: {e}"),
                }

                let held_long_enough = first_refresh_at.is_some_and(|started| {
                    started.elapsed() >= std::time::Duration::from_millis(250)
                });
                if !pending_refreshes.is_empty()
                    && (pending_refreshes.len() >= guarded_requests || held_long_enough)
                {
                    for mut stream in pending_refreshes.drain(..) {
                        write_response(
                            &mut stream,
                            200,
                            "OK",
                            br#"{"captcha_token":"fresh-captcha","expires_in":300}"#,
                        );
                    }
                }
                if about_count == guarded_requests && pending_refreshes.is_empty() {
                    break;
                }
            }
        });
        (base_url, refresh_count, handle)
    }

    /// Server that answers drive listings (counting how many it serves) and
    /// accepts any other request as a successful mutation. Used to prove the
    /// listing cache is dropped after a mutation.
    fn start_listing_server(
        max_requests: usize,
    ) -> (String, Arc<AtomicUsize>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let list_hits = Arc::new(AtomicUsize::new(0));
        let hits = Arc::clone(&list_hits);
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming().take(max_requests) {
                let Ok(mut stream) = stream else { continue };
                let request = read_test_http_request(&mut stream).unwrap_or_default();
                let first_line = request.lines().next().unwrap_or_default();
                if first_line.starts_with("GET /drive/v1/files") {
                    hits.fetch_add(1, Ordering::SeqCst);
                    let body = r#"{"files":[{"id":"id1","name":"A","kind":"drive#folder"}]}"#;
                    write_response(&mut stream, 200, "OK", body.as_bytes());
                } else {
                    write_response(&mut stream, 200, "OK", b"{}");
                }
            }
        });
        (base_url, list_hits, handle)
    }

    fn start_blocking_listing_server() -> (
        String,
        std::sync::mpsc::Receiver<()>,
        std::sync::mpsc::Sender<()>,
        Arc<AtomicUsize>,
        std::thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let list_hits = Arc::new(AtomicUsize::new(0));
        let hits = Arc::clone(&list_hits);
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();

        let handle = std::thread::spawn(move || {
            let (mut first, _) = listener.accept().unwrap();
            let _ = read_test_http_request(&mut first);
            hits.fetch_add(1, Ordering::SeqCst);
            started_tx.send(()).unwrap();
            release_rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap();
            write_response(
                &mut first,
                200,
                "OK",
                br#"{"files":[{"id":"old","name":"old","kind":"drive#folder"}]}"#,
            );

            listener.set_nonblocking(true).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
            while std::time::Instant::now() < deadline {
                match accept_test_connection(&listener) {
                    Ok(mut second) => {
                        let _ = read_test_http_request(&mut second);
                        hits.fetch_add(1, Ordering::SeqCst);
                        write_response(
                            &mut second,
                            200,
                            "OK",
                            br#"{"files":[{"id":"new","name":"new","kind":"drive#folder"}]}"#,
                        );
                        break;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(e) => panic!("listing server accept failed: {e}"),
                }
            }
        });

        (base_url, started_rx, release_tx, list_hits, handle)
    }

    #[test]
    fn token_expiry_check() {
        let token = SessionToken {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at_unix: 100,
            ..Default::default()
        };
        assert!(!token.is_expired(99));
        assert!(token.is_expired(100));
    }

    #[test]
    fn session_token_preserves_captcha_expiry() {
        let token: SessionToken = serde_json::from_str(
            r#"{
                "access_token":"a",
                "refresh_token":"r",
                "expires_at_unix":200,
                "captcha_token":"captcha",
                "captcha_expires_at_unix":123
            }"#,
        )
        .unwrap();

        let encoded = serde_json::to_value(token).unwrap();
        assert_eq!(encoded["captcha_expires_at_unix"], 123);
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
    fn captcha_response_exposes_its_expiry() {
        let response: CaptchaInitResponse =
            serde_json::from_str(r#"{"captcha_token":"captcha","expires_in":300}"#).unwrap();

        assert_eq!(response.expires_in, 300);
    }

    #[test]
    fn access_token_triggers_refresh_when_expired() {
        let expired = SessionToken {
            access_token: "old".into(),
            refresh_token: "r".into(),
            expires_at_unix: now_unix() - 1,
            ..Default::default()
        };
        assert!(expired.is_expired(now_unix()));

        let valid = SessionToken {
            access_token: "good".into(),
            refresh_token: "r".into(),
            expires_at_unix: now_unix() + 600,
            ..Default::default()
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
    fn share_detail_follows_token_across_empty_intermediate_page() {
        let (base_url, requested_tokens, handle) = start_paginated_get_server(vec![
            r#"{
                "files":[{"id":"first","name":"first","kind":"drive#file"}],
                "next_page_token":"empty-page"
            }"#,
            r#"{"files":[],"next_page_token":"last-page"}"#,
            r#"{"files":[{"id":"last","name":"last","kind":"drive#file"}]}"#,
        ]);
        let dir = temp_test_dir("share-detail-empty-page");
        let client = test_client(base_url, dir.join("session.json"));

        let entries = client.share_detail("share", "", "pass").unwrap();
        handle.join().unwrap();

        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "last"]
        );
        assert_eq!(
            *requested_tokens.lock().unwrap_or_else(|e| e.into_inner()),
            vec![
                None,
                Some("empty-page".to_string()),
                Some("last-page".to_string())
            ]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn share_detail_stops_before_reusing_any_seen_page_token() {
        let (base_url, requested_tokens, handle) = start_paginated_get_server(vec![
            r#"{
                "files":[{"id":"root","name":"root","kind":"drive#file"}],
                "next_page_token":"page-a"
            }"#,
            r#"{
                "files":[{"id":"a","name":"a","kind":"drive#file"}],
                "next_page_token":"page-b"
            }"#,
            r#"{
                "files":[{"id":"b","name":"b","kind":"drive#file"}],
                "next_page_token":"page-a"
            }"#,
        ]);
        let dir = temp_test_dir("share-detail-token-cycle");
        let client = test_client(base_url, dir.join("session.json"));

        let result = client.share_detail("share", "", "pass");
        handle.join().unwrap();

        let entries = result.unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["root", "a", "b"]
        );
        assert_eq!(
            *requested_tokens.lock().unwrap_or_else(|e| e.into_inner()),
            vec![None, Some("page-a".to_string()), Some("page-b".to_string())]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn list_shares_follows_token_across_empty_intermediate_page() {
        let (base_url, requested_tokens, handle) = start_paginated_get_server(vec![
            r#"{
                "data":[{"share_id":"first","share_url":"https://example/first"}],
                "next_page_token":"empty-page"
            }"#,
            r#"{"data":[],"next_page_token":"last-page"}"#,
            r#"{"data":[{"share_id":"last","share_url":"https://example/last"}]}"#,
        ]);
        let dir = temp_test_dir("list-shares-empty-page");
        let client = test_client(base_url, dir.join("session.json"));

        let shares = client.list_shares().unwrap();
        handle.join().unwrap();

        assert_eq!(
            shares
                .iter()
                .map(|share| share.share_id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "last"]
        );
        assert_eq!(
            *requested_tokens.lock().unwrap_or_else(|e| e.into_inner()),
            vec![
                None,
                Some("empty-page".to_string()),
                Some("last-page".to_string())
            ]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn list_shares_stops_before_reusing_any_seen_page_token() {
        let (base_url, requested_tokens, handle) = start_paginated_get_server(vec![
            r#"{
                "data":[{"share_id":"root","share_url":"https://example/root"}],
                "next_page_token":"page-a"
            }"#,
            r#"{
                "data":[{"share_id":"a","share_url":"https://example/a"}],
                "next_page_token":"page-b"
            }"#,
            r#"{
                "data":[{"share_id":"b","share_url":"https://example/b"}],
                "next_page_token":"page-a"
            }"#,
        ]);
        let dir = temp_test_dir("list-shares-token-cycle");
        let client = test_client(base_url, dir.join("session.json"));

        let result = client.list_shares();
        handle.join().unwrap();

        let shares = result.unwrap();
        assert_eq!(
            shares
                .iter()
                .map(|share| share.share_id.as_str())
                .collect::<Vec<_>>(),
            vec!["root", "a", "b"]
        );
        assert_eq!(
            *requested_tokens.lock().unwrap_or_else(|e| e.into_inner()),
            vec![None, Some("page-a".to_string()), Some("page-b".to_string())]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn drive_list_response_tolerates_non_numeric_size() {
        // A single entry with an empty/garbage size must not abort the whole
        // listing — it should fall back to size 0.
        let json = r#"{"files": [
            {"id":"f1","name":"folder","kind":"drive#folder","size":""},
            {"id":"f2","name":"a.bin","kind":"drive#file","size":"1234"},
            {"id":"f3","name":"b.bin","kind":"drive#file","size":"garbage"}
        ]}"#;
        let resp: DriveListResponse = serde_json::from_str(json).unwrap();
        let sizes: Vec<u64> = resp
            .files
            .into_iter()
            .map(|f| f.into_entry().size)
            .collect();
        assert_eq!(sizes, vec![0, 1234, 0]);
    }

    #[test]
    fn download_to_replaces_unverified_same_size_destination() {
        // Matching length alone cannot prove that an existing destination is
        // the requested cloud file.
        let server = start_mock_download_server(b"hello", false, 2);
        let dir = temp_test_dir("download-complete");
        let dest = dir.join("file.bin");
        std::fs::write(&dest, b"WRONG").unwrap();
        let client = test_client(server.base_url, dir.join("session.json"));

        let total = client.download_to("file", &dest).unwrap();

        assert_eq!(total, 5);
        assert_eq!(std::fs::read(&dest).unwrap(), b"hello");
        assert_eq!(server.download_hits.load(Ordering::SeqCst), 1);
        server.handle.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn download_to_reports_size_when_server_ignores_range() {
        let server = start_mock_download_server(b"hello", true, 3);
        let dir = temp_test_dir("download-range-ignored");
        let dest = dir.join("file.bin");
        std::fs::write(&dest, b"he").unwrap();
        let client = test_client(server.base_url, dir.join("session.json"));

        let total = client.download_to("file", &dest).unwrap();

        assert_eq!(total, 5);
        assert_eq!(std::fs::read(&dest).unwrap(), b"hello");
        assert_eq!(server.download_hits.load(Ordering::SeqCst), 2);
        server.handle.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn download_to_resumes_from_part_sidecar() {
        // file_info + ranged download = 2 requests
        let server = start_mock_download_server(b"hello", false, 2);
        let dir = temp_test_dir("download-part-resume");
        let dest = dir.join("file.bin");
        std::fs::write(part_path(&dest), b"he").unwrap();
        let mut identity_path = part_path(&dest).into_os_string();
        identity_path.push(".meta");
        let identity_path = std::path::PathBuf::from(identity_path);
        std::fs::write(&identity_path, r#"{"file_id":"file","expected_size":5}"#).unwrap();
        let client = test_client(server.base_url, dir.join("session.json"));

        let total = client.download_to("file", &dest).unwrap();

        assert_eq!(total, 5);
        assert_eq!(std::fs::read(&dest).unwrap(), b"hello");
        assert!(!part_path(&dest).exists(), "sidecar must be renamed away");
        assert!(
            !identity_path.exists(),
            "completed download must remove partial identity"
        );
        server.handle.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn download_to_fetches_bounded_ranges_and_commits_them_in_order() {
        let content: &'static [u8] = Box::leak(
            (0..3500)
                .map(|i| (i % 251) as u8)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        let server = start_mock_download_server(content, false, 5);
        let dir = temp_test_dir("download-parallel-ranges");
        let dest = dir.join("file.bin");
        let client = test_client(server.base_url, dir.join("session.json"));

        let total = client
            .download_to_with_connections_and_chunk_size("file", &dest, 4, 1024)
            .unwrap();

        assert_eq!(total, content.len() as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), content);
        assert_eq!(server.download_hits.load(Ordering::SeqCst), 4);
        server.handle.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn download_to_preserves_partial_owned_by_different_file() {
        // A prior download with the same destination must neither contribute
        // its prefix nor be silently deleted.
        let server = start_mock_download_server(b"hello", false, 1);
        let dir = temp_test_dir("download-part-identity");
        let dest = dir.join("file.bin");
        std::fs::write(part_path(&dest), b"XX").unwrap();
        let mut identity_path = part_path(&dest).into_os_string();
        identity_path.push(".meta");
        let identity_path = std::path::PathBuf::from(identity_path);
        let identity = r#"{"file_id":"other-file","expected_size":5}"#;
        std::fs::write(&identity_path, identity).unwrap();
        let client = test_client(server.base_url, dir.join("session.json"));

        let err = client.download_to("file", &dest).unwrap_err();

        assert!(
            format!("{err:#}").contains("belongs to remote file 'other-file'"),
            "got: {err:#}"
        );
        assert_eq!(std::fs::read(part_path(&dest)).unwrap(), b"XX");
        assert_eq!(std::fs::read_to_string(identity_path).unwrap(), identity);
        assert!(!dest.exists());
        assert_eq!(server.download_hits.load(Ordering::SeqCst), 0);
        server.handle.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn download_to_restarts_when_same_named_file_is_foreign() {
        // A same-named local file that is smaller than the remote is NOT
        // treated as a partial: the download runs fresh via the sidecar.
        let server = start_mock_download_server(b"hello", false, 2);
        let dir = temp_test_dir("download-foreign-file");
        let dest = dir.join("file.bin");
        std::fs::write(&dest, b"XX").unwrap();
        let client = test_client(server.base_url, dir.join("session.json"));

        let total = client.download_to("file", &dest).unwrap();

        assert_eq!(total, 5);
        assert_eq!(std::fs::read(&dest).unwrap(), b"hello");
        server.handle.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn quota_propagates_api_error_status_and_body() {
        let (base_url, handle) =
            start_canned_server(500, "Internal Server Error", b"quota blew up".to_vec());
        let dir = temp_test_dir("quota-api-error");
        let client = test_client(base_url, dir.join("session.json"));

        let err = client.quota().unwrap_err();
        let msg = format!("{err:#}");

        assert!(
            msg.contains("quota failed (500 Internal Server Error)"),
            "got: {msg}"
        );
        assert!(msg.contains("quota blew up"), "got: {msg}");

        handle.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn quota_reports_invalid_json() {
        let (base_url, handle) = start_canned_server(200, "OK", b"this is not json".to_vec());
        let dir = temp_test_dir("quota-bad-json");
        let client = test_client(base_url, dir.join("session.json"));

        let err = client.quota().unwrap_err();
        let msg = format!("{err:#}");

        assert!(msg.contains("invalid quota json"), "got: {msg}");

        handle.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn api_error_body_is_truncated_by_sanitize() {
        let long_body = "Z".repeat(300);
        let (base_url, handle) =
            start_canned_server(503, "Service Unavailable", long_body.into_bytes());
        let dir = temp_test_dir("quota-long-body");
        let client = test_client(base_url, dir.join("session.json"));

        let err = client.quota().unwrap_err();
        let msg = format!("{err:#}");

        // sanitize() keeps the first 240 chars and appends an ellipsis.
        assert!(msg.contains(&"Z".repeat(240)), "got: {msg}");
        assert!(msg.contains("..."), "got: {msg}");
        assert!(
            !msg.contains(&"Z".repeat(241)),
            "body should be truncated to 240 chars"
        );

        handle.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn file_info_propagates_api_error() {
        let (base_url, handle) = start_canned_server(403, "Forbidden", b"no access".to_vec());
        let dir = temp_test_dir("file-info-api-error");
        let client = test_client(base_url, dir.join("session.json"));

        let err = client.file_info("FID").unwrap_err();
        let msg = format!("{err:#}");

        assert!(
            msg.contains("file_info failed (403 Forbidden)"),
            "got: {msg}"
        );
        assert!(msg.contains("no access"), "got: {msg}");

        handle.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn file_info_surfaces_captcha_refresh_failure() {
        let body = br#"{"error_code":9,"error":"riskLimited"}"#.to_vec();
        let (base_url, handle) = start_canned_server(403, "Forbidden", body);
        let dir = temp_test_dir("file-info-captcha-refresh");
        let client = test_client(base_url, dir.join("session.json"));

        let err = client.file_info("FID").unwrap_err();
        let msg = format!("{err:#}");

        assert!(msg.contains("captcha refresh"), "got: {msg}");
        handle.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn business_error_code_nine_is_not_replayed_as_captcha() {
        let body =
            br#"{"error_code":9,"error":"file_move_or_copy_to_cur","error_description":"same parent"}"#
                .to_vec();
        let (base_url, handle) = start_canned_server(400, "Bad Request", body);
        let dir = temp_test_dir("business-code-nine");
        let client = test_client(base_url, dir.join("session.json"));

        let err = client.file_info("FID").unwrap_err();
        let msg = format!("{err:#}");

        assert!(msg.contains("file_move_or_copy_to_cur"), "got: {msg}");
        assert!(!msg.contains("captcha refresh"), "got: {msg}");
        handle.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn expired_captcha_is_refreshed_before_guarded_request() {
        let (base_url, requests, handle) = start_captcha_refresh_server();
        let dir = temp_test_dir("proactive-captcha-refresh");
        let session_path = dir.join("session.json");
        let mut client = test_client(base_url.clone(), session_path);
        client.auth_base_url = base_url;
        *client
            .captcha_token
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = "stale-captcha".to_string();
        client
            .save_session(&SessionToken {
                access_token: "test-access".into(),
                refresh_token: "test-refresh".into(),
                expires_at_unix: now_unix() + 3600,
                device_id: "device".into(),
                captcha_token: "stale-captcha".into(),
                captcha_expires_at_unix: now_unix() - 1,
                user_id: "user".into(),
            })
            .unwrap();

        client.quota().unwrap();
        handle.join().unwrap();
        let requests = requests.lock().unwrap_or_else(|e| e.into_inner());

        assert_eq!(requests.len(), 2, "{requests:#?}");
        assert!(
            requests[0].starts_with("POST /v1/shield/captcha/init"),
            "{requests:#?}"
        );
        assert!(
            requests[1].starts_with("GET /drive/v1/about"),
            "{requests:#?}"
        );
        assert!(
            requests[1]
                .to_ascii_lowercase()
                .contains("x-captcha-token: fresh-captcha"),
            "{requests:#?}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn captcha_test_client(
        base_url: String,
        session_path: std::path::PathBuf,
        captcha_expiry: i64,
    ) -> PikPak {
        let mut client = test_client(base_url.clone(), session_path);
        client.auth_base_url = base_url;
        client.device_id = "device".into();
        client.user_id = "user".into();
        *client
            .captcha_token
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = "stale-captcha".into();
        *client
            .captcha_expires_at_unix
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = captcha_expiry;
        client
            .save_session(&SessionToken {
                access_token: "test-access".into(),
                refresh_token: "test-refresh".into(),
                expires_at_unix: now_unix() + 3600,
                device_id: "device".into(),
                captcha_token: "stale-captcha".into(),
                captcha_expires_at_unix: captcha_expiry,
                user_id: "user".into(),
            })
            .unwrap();
        client
    }

    #[test]
    fn generic_authed_endpoint_refreshes_code_nine_and_retries_once() {
        let (base_url, requests, handle) =
            start_reactive_captcha_server(ReactiveCaptchaMode::RetrySucceeds);
        let dir = temp_test_dir("generic-reactive-captcha");
        let client = captcha_test_client(
            base_url,
            dir.join("session.json"),
            now_unix().saturating_add(300),
        );

        let result = client.quota();

        handle.join().unwrap();
        let requests = requests.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            result.is_ok(),
            "unexpected error: {:#}",
            result.unwrap_err()
        );
        assert_eq!(requests.len(), 3, "{requests:#?}");
        assert!(requests[0].starts_with("GET /drive/v1/about"));
        assert!(requests[1].starts_with("POST /v1/shield/captcha/init"));
        assert!(requests[2].starts_with("GET /drive/v1/about"));
        assert!(
            requests[2]
                .to_ascii_lowercase()
                .contains("x-captcha-token: fresh-captcha")
        );
        drop(requests);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn generic_authed_endpoint_surfaces_captcha_refresh_failure() {
        let (base_url, requests, handle) =
            start_reactive_captcha_server(ReactiveCaptchaMode::RefreshFails);
        let dir = temp_test_dir("generic-captcha-refresh-failure");
        let client = captcha_test_client(
            base_url,
            dir.join("session.json"),
            now_unix().saturating_add(300),
        );

        let result = client.quota();

        handle.join().unwrap();
        let err = result.unwrap_err();
        assert!(
            format!("{err:#}").contains("captcha refresh failed"),
            "unexpected error: {err:#}"
        );
        assert_eq!(requests.lock().unwrap_or_else(|e| e.into_inner()).len(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn generic_authed_endpoint_stops_after_second_code_nine() {
        let (base_url, requests, handle) =
            start_reactive_captcha_server(ReactiveCaptchaMode::RetryStillLimited);
        let dir = temp_test_dir("generic-second-captcha-code-nine");
        let client = captcha_test_client(
            base_url,
            dir.join("session.json"),
            now_unix().saturating_add(300),
        );

        let result = client.quota();

        handle.join().unwrap();
        let err = result.unwrap_err();
        let message = format!("{err:#}");
        assert!(
            message.contains("quota failed (403 Forbidden)"),
            "{message}"
        );
        assert!(message.contains("riskLimited"), "{message}");
        assert_eq!(requests.lock().unwrap_or_else(|e| e.into_inner()).len(), 3);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn concurrent_expired_requests_share_one_proactive_captcha_refresh() {
        const REQUESTS: usize = 4;
        let (base_url, refresh_count, handle) = start_concurrent_captcha_server(REQUESTS);
        let dir = temp_test_dir("concurrent-proactive-captcha");
        let client = Arc::new(captcha_test_client(
            base_url,
            dir.join("session.json"),
            now_unix().saturating_sub(1),
        ));
        let barrier = Arc::new(std::sync::Barrier::new(REQUESTS));
        let mut threads = Vec::new();
        for _ in 0..REQUESTS {
            let client = Arc::clone(&client);
            let barrier = Arc::clone(&barrier);
            threads.push(std::thread::spawn(move || {
                barrier.wait();
                client.quota()
            }));
        }

        let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        handle.join().unwrap();

        assert!(results.iter().all(Result::is_ok), "{results:#?}");
        assert_eq!(refresh_count.load(Ordering::SeqCst), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn empty_captcha_token_is_refreshed_before_authenticated_request() {
        let (base_url, requests, handle) = start_captcha_refresh_server();
        let dir = temp_test_dir("empty-proactive-captcha");
        let session_path = dir.join("session.json");
        let client = captcha_test_client(base_url, session_path, 0);
        *client
            .captcha_token
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = String::new();
        client
            .save_session(&SessionToken {
                access_token: "test-access".into(),
                refresh_token: "test-refresh".into(),
                expires_at_unix: now_unix() + 3600,
                device_id: "device".into(),
                captcha_token: String::new(),
                captcha_expires_at_unix: 0,
                user_id: "user".into(),
            })
            .unwrap();

        let result = client.quota();

        handle.join().unwrap();
        let requests = requests.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            result.is_ok(),
            "unexpected error: {:#}",
            result.unwrap_err()
        );
        assert_eq!(requests.len(), 2, "{requests:#?}");
        assert!(requests[0].starts_with("POST /v1/shield/captcha/init"));
        assert!(requests[1].starts_with("GET /drive/v1/about"));
        drop(requests);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn access_refresh_preserves_captcha_fields_under_session_lock() {
        let body = br#"{
            "access_token":"new-access",
            "refresh_token":"new-refresh",
            "expires_in":3600,
            "sub":"user"
        }"#
        .to_vec();
        let (base_url, handle) = start_canned_server(200, "OK", body);
        let dir = temp_test_dir("access-captcha-session-race");
        let session_path = dir.join("session.json");
        let mut client = test_client(base_url.clone(), session_path);
        client.auth_base_url = base_url;
        client.device_id = "device".into();
        client.user_id = "user".into();
        let fresh_expiry = now_unix() + 300;
        client
            .save_session(&SessionToken {
                access_token: "old-access".into(),
                refresh_token: "old-refresh".into(),
                expires_at_unix: now_unix() - 1,
                device_id: "device".into(),
                captcha_token: "new-captcha".into(),
                captcha_expires_at_unix: fresh_expiry,
                user_id: "user".into(),
            })
            .unwrap();

        assert_eq!(client.refresh_session("old-refresh").unwrap(), "new-access");
        handle.join().unwrap();
        let saved = client.load_session().unwrap().unwrap();
        assert_eq!(saved.access_token, "new-access");
        assert_eq!(saved.refresh_token, "new-refresh");
        assert_eq!(saved.captcha_token, "new-captcha");
        assert_eq!(saved.captcha_expires_at_unix, fresh_expiry);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn separate_clients_honor_the_same_session_file_lock() {
        let dir = temp_test_dir("cross-client-session-lock");
        let session_path = dir.join("session.json");
        let first = Arc::new(test_client(String::new(), session_path.clone()));
        let second = Arc::new(test_client(String::new(), session_path));
        first
            .save_session(&SessionToken {
                access_token: "before".into(),
                refresh_token: "refresh".into(),
                expires_at_unix: now_unix() + 3600,
                ..Default::default()
            })
            .unwrap();

        let file_guard = first.lock_session_file().unwrap();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            ready_tx.send(()).unwrap();
            let result = second.update_session(|session| {
                session.access_token = "after".into();
            });
            done_tx.send(result).unwrap();
        });

        ready_rx.recv().unwrap();
        assert!(
            done_rx
                .recv_timeout(std::time::Duration::from_millis(100))
                .is_err(),
            "a separate PikPak instance must wait for the disk lock"
        );
        drop(file_guard);
        done_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
            .unwrap();
        writer.join().unwrap();
        assert_eq!(first.load_session().unwrap().unwrap().access_token, "after");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn concurrent_session_writes_do_not_race_shared_temp_file() {
        const WRITERS: usize = 32;
        let dir = temp_test_dir("concurrent-session-writes");
        let client = Arc::new(test_client(String::new(), dir.join("shared-session.json")));
        let barrier = Arc::new(std::sync::Barrier::new(WRITERS));
        let mut threads = Vec::new();
        for writer in 0..WRITERS {
            let client = Arc::clone(&client);
            let barrier = Arc::clone(&barrier);
            threads.push(std::thread::spawn(move || {
                let token = SessionToken {
                    access_token: format!("access-{writer}"),
                    refresh_token: format!("refresh-{writer}"),
                    expires_at_unix: now_unix() + 3600,
                    ..Default::default()
                };
                barrier.wait();
                client.save_session(&token)
            }));
        }

        let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();

        assert!(
            results.iter().all(Result::is_ok),
            "concurrent save failed: {results:#?}"
        );
        let saved = client.load_session().unwrap().unwrap();
        assert!(saved.access_token.starts_with("access-"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn offline_list_reports_invalid_json() {
        let (base_url, handle) = start_canned_server(200, "OK", b"<html>nope</html>".to_vec());
        let dir = temp_test_dir("offline-list-bad-json");
        let client = test_client(base_url, dir.join("session.json"));

        let err = client
            .offline_list(50, &["PHASE_TYPE_RUNNING"])
            .unwrap_err();
        let msg = format!("{err:#}");

        assert!(msg.contains("invalid offline list json"), "got: {msg}");

        handle.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn events_propagates_api_error() {
        let (base_url, handle) =
            start_canned_server(500, "Internal Server Error", b"events boom".to_vec());
        let dir = temp_test_dir("events-api-error");
        let client = test_client(base_url, dir.join("session.json"));

        let err = client.events(20).unwrap_err();
        let msg = format!("{err:#}");

        assert!(
            msg.contains("events failed (500 Internal Server Error)"),
            "got: {msg}"
        );
        assert!(msg.contains("events boom"), "got: {msg}");

        handle.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn starred_list_propagates_api_error() {
        let (base_url, handle) =
            start_canned_server(429, "Too Many Requests", b"slow down".to_vec());
        let dir = temp_test_dir("starred-api-error");
        let client = test_client(base_url, dir.join("session.json"));

        let err = client.starred_list(100).unwrap_err();
        let msg = format!("{err:#}");

        assert!(
            msg.contains("starred list failed (429 Too Many Requests)"),
            "got: {msg}"
        );
        assert!(msg.contains("slow down"), "got: {msg}");

        handle.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ls_cached_is_invalidated_after_mutation() {
        // ls_cached #1 (GET), cached #2 (no request), rename (PATCH), ls_cached #3 (GET).
        let (base_url, list_hits, handle) = start_listing_server(3);
        let dir = temp_test_dir("ls-cache-invalidation");
        let client = test_client(base_url, dir.join("session.json"));

        let first = client.ls_cached("").unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(list_hits.load(Ordering::SeqCst), 1);

        // A second read is served from cache, not the network.
        let _ = client.ls_cached("").unwrap();
        assert_eq!(list_hits.load(Ordering::SeqCst), 1);

        // A successful mutation drops the cache...
        client.rename("id1", "A2").unwrap();

        // ...so the next read goes back to the server.
        let _ = client.ls_cached("").unwrap();
        assert_eq!(list_hits.load(Ordering::SeqCst), 2);

        handle.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalidation_during_inflight_listing_cannot_reinsert_stale_cache() {
        let (base_url, started, release, list_hits, handle) = start_blocking_listing_server();
        let dir = temp_test_dir("ls-cache-inflight-invalidation");
        let client = Arc::new(test_client(base_url, dir.join("session.json")));
        let requesting_client = Arc::clone(&client);

        let request = std::thread::spawn(move || requesting_client.ls_cached("").unwrap());
        started
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        client.clear_ls_cache();
        release.send(()).unwrap();
        let first = request.join().unwrap();
        assert_eq!(first[0].name, "old");

        let second = client.ls_cached("").unwrap();
        handle.join().unwrap();

        assert_eq!(second[0].name, "new");
        assert_eq!(list_hits.load(Ordering::SeqCst), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn save_session_writes_owner_only_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_test_dir("session-perms");
        let path = dir.join("session.json");
        // test_client() calls save_session() during construction.
        let _client = test_client("http://unused".to_string(), path.clone());
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "session file must be owner-only");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
