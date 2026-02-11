use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::PathBuf;
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
    http: reqwest::blocking::Client,
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
                let hint = captcha
                    .url
                    .as_deref()
                    .unwrap_or("<no challenge url>");
                anyhow!(
                    "captcha token unavailable; set PIKPAK_CAPTCHA_TOKEN. url={}",
                    sanitize(hint)
                )
            })?;

        // signin
        let url = format!("{}/v1/auth/signin", self.auth_base_url.trim_end_matches('/'));
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
            return Err(anyhow!("captcha init failed ({}): {}", status, sanitize(&body)));
        }

        response.json::<CaptchaInitResponse>().context("invalid captcha json")
    }

    fn access_token(&self) -> Result<String> {
        let session = self
            .load_session()?
            .ok_or_else(|| anyhow!("not logged in, please login first"))?;
        Ok(session.access_token)
    }

    fn authed_headers(&self, rb: reqwest::blocking::RequestBuilder) -> reqwest::blocking::RequestBuilder {
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
        let url = format!("{}/drive/v1/files", self.drive_base_url.trim_end_matches('/'));

        let filters = r#"{"trashed":{"eq":false}}"#;
        let mut rb = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .query(&[
                ("parent_id", parent_id),
                ("limit", "500"),
                ("filters", filters),
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
            .map(|f| Entry {
                id: f.id,
                name: f.name,
                kind: if f.kind.contains("folder") {
                    EntryKind::Folder
                } else {
                    EntryKind::File
                },
                size: f.size.unwrap_or(0),
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

    pub fn mkdir(&self, parent_id: &str, name: &str) -> Result<Entry> {
        let token = self.access_token()?;
        let url = format!("{}/drive/v1/files", self.drive_base_url.trim_end_matches('/'));

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

        let f: DriveFile = response.json().context("invalid mkdir json")?;
        Ok(Entry {
            id: f.id,
            name: f.name,
            kind: EntryKind::Folder,
            size: 0,
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
            return Err(anyhow!("file_info failed ({}): {}", status, sanitize(&body)));
        }

        response.json().context("invalid file_info json")
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
        let url = format!("{}/drive/v1/about", self.drive_base_url.trim_end_matches('/'));

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
struct DriveFile {
    id: String,
    name: String,
    #[serde(default)]
    kind: String,
    #[serde(default, deserialize_with = "de_opt_u64")]
    size: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct FileInfoResponse {
    #[allow(dead_code)]
    pub name: String,
    #[serde(default)]
    pub web_content_link: Option<String>,
    #[serde(default)]
    pub links: Option<std::collections::HashMap<String, LinkInfo>>,
}

#[derive(Debug, Deserialize)]
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
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22,
        5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
        4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
        6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];

    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee,
        0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
        0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be,
        0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
        0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa,
        0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
        0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
        0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c,
        0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
        0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05,
        0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
        0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039,
        0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1,
        0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
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
                (a.wrapping_add(f).wrapping_add(K[i]).wrapping_add(m[g]))
                    .rotate_left(S[i]),
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
