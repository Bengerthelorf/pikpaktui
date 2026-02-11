use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_AUTH_BASE_URL: &str = "https://user.mypikpak.com";
const DEFAULT_CLIENT_ID: &str = "YNxT9w7GMdWvEOKa";
const DEFAULT_CLIENT_SECRET: &str = "dbw2OtmVEeuUvIptb1Coyg";
const USER_AGENT: &str = "ANDROID-com.pikcloud.pikpak/1.21.0";

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

pub struct NativeAuth {
    session_path: PathBuf,
    auth_base_url: String,
    client_id: String,
    client_secret: String,
    http: reqwest::blocking::Client,
}

pub struct AuthConfig {
    pub session_path: PathBuf,
    pub auth_base_url: String,
    pub client_id: String,
    pub client_secret: String,
}

impl NativeAuth {
    pub fn new() -> Result<Self> {
        let cfg = AuthConfig {
            session_path: default_session_path()?,
            auth_base_url: env::var("PIKPAK_AUTH_BASE_URL")
                .unwrap_or_else(|_| DEFAULT_AUTH_BASE_URL.to_string()),
            client_id: env::var("PIKPAK_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string()),
            client_secret: env::var("PIKPAK_CLIENT_SECRET")
                .unwrap_or_else(|_| DEFAULT_CLIENT_SECRET.to_string()),
        };
        Self::from_config(cfg)
    }

    pub fn from_config(cfg: AuthConfig) -> Result<Self> {
        Ok(Self {
            session_path: cfg.session_path,
            auth_base_url: cfg.auth_base_url,
            client_id: cfg.client_id,
            client_secret: cfg.client_secret,
            http: reqwest::blocking::Client::builder()
                .user_agent(USER_AGENT)
                .build()
                .context("failed to build http client")?,
        })
    }

    pub fn load_session(&self) -> Result<Option<SessionToken>> {
        if !self.session_path.exists() {
            return Ok(None);
        }

        let raw = fs::read_to_string(&self.session_path).with_context(|| {
            format!(
                "failed to read session file {}",
                self.session_path.display()
            )
        })?;
        let token: SessionToken =
            serde_json::from_str(&raw).context("failed to parse session json")?;
        Ok(Some(token))
    }

    pub fn save_session(&self, token: &SessionToken) -> Result<()> {
        if let Some(parent) = self.session_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create dir {}", parent.display()))?;
        }

        let raw = serde_json::to_string_pretty(token).context("failed to encode session json")?;
        fs::write(&self.session_path, raw).with_context(|| {
            format!(
                "failed to write session file {}",
                self.session_path.display()
            )
        })
    }

    pub fn clear_session(&self) -> Result<()> {
        if self.session_path.exists() {
            fs::remove_file(&self.session_path).with_context(|| {
                format!(
                    "failed to remove session file {}",
                    self.session_path.display()
                )
            })?;
        }
        Ok(())
    }

    pub fn login_with_password(&self, email: &str, password: &str) -> Result<SessionToken> {
        if email.trim().is_empty() {
            return Err(anyhow!("email is empty"));
        }
        if password.is_empty() {
            return Err(anyhow!("password is empty"));
        }

        let device_id = md5_hex(email);

        let captcha = self.init_captcha(email, &device_id)?;
        let captcha_token = captcha
            .captcha_token
            .clone()
            .or_else(|| env::var("PIKPAK_CAPTCHA_TOKEN").ok())
            .ok_or_else(|| {
                let hint = captcha
                    .url
                    .as_deref()
                    .unwrap_or("<no challenge url in response>");
                anyhow!(
                    "captcha token unavailable; complete challenge and set PIKPAK_CAPTCHA_TOKEN. challenge_url={}",
                    sanitize(hint)
                )
            })?;

        let url = format!("{}/v1/auth/signin", self.auth_base_url.trim_end_matches('/'));
        let payload = SigninRequest {
            username: email,
            password,
            client_id: &self.client_id,
            client_secret: &self.client_secret,
            captcha_token: &captcha_token,
            grant_type: "password",
        };

        let response = self
            .http
            .post(url)
            .header("x-device-id", &device_id)
            .json(&payload)
            .send()
            .context("signin request failed")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!("signin failed with status {}: {}", status, sanitize(&body)));
        }

        let signin: SigninResponse = response.json().context("invalid signin response json")?;
        let expires_in = i64::try_from(signin.expires_in).context("expires_in overflow")?;
        let now = now_unix()?;

        let token = SessionToken {
            access_token: signin.access_token,
            refresh_token: signin.refresh_token,
            expires_at_unix: now.saturating_add(expires_in),
        };

        self.save_session(&token)?;
        Ok(token)
    }

    fn init_captcha(&self, email: &str, device_id: &str) -> Result<CaptchaInitResponse> {
        let url = format!(
            "{}/v1/shield/captcha/init",
            self.auth_base_url.trim_end_matches('/')
        );

        let action = format!(
            "POST:{}/v1/auth/signin",
            self.auth_base_url.trim_end_matches('/')
        );

        let payload = CaptchaInitRequest {
            action: &action,
            client_id: &self.client_id,
            device_id,
            meta: CaptchaMeta {
                username: email,
            },
        };

        let response = self
            .http
            .post(url)
            .header("x-device-id", device_id)
            .json(&payload)
            .send()
            .context("captcha init request failed")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!(
                "captcha init failed with status {}: {}",
                status,
                sanitize(&body)
            ));
        }

        response
            .json::<CaptchaInitResponse>()
            .context("invalid captcha init response json")
    }

    pub fn session_path(&self) -> &PathBuf {
        &self.session_path
    }
}

#[derive(Serialize)]
struct CaptchaInitRequest<'a> {
    action: &'a str,
    client_id: &'a str,
    device_id: &'a str,
    meta: CaptchaMeta<'a>,
}

#[derive(Serialize)]
struct CaptchaMeta<'a> {
    username: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CaptchaInitResponse {
    #[serde(default)]
    pub captcha_token: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Serialize)]
struct SigninRequest<'a> {
    client_id: &'a str,
    client_secret: &'a str,
    grant_type: &'a str,
    username: &'a str,
    password: &'a str,
    captcha_token: &'a str,
}

#[derive(Debug, Deserialize)]
struct SigninResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

fn default_session_path() -> Result<PathBuf> {
    let base = dirs::config_dir().ok_or_else(|| anyhow!("unable to locate config dir"))?;
    Ok(base.join("pikpaktui").join("session.json"))
}

fn now_unix() -> Result<i64> {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("clock is before unix epoch")?;
    i64::try_from(d.as_secs()).context("unix timestamp overflow")
}

fn sanitize(s: &str) -> String {
    if s.len() > 240 {
        format!("{}...", &s[..240])
    } else {
        s.to_string()
    }
}

fn md5_hex(input: &str) -> String {
    // Simple MD5 implementation for device_id generation
    let digest = md5_compute(input.as_bytes());
    let mut hex = String::with_capacity(32);
    for b in &digest {
        write!(hex, "{:02x}", b).unwrap();
    }
    hex
}

fn md5_compute(input: &[u8]) -> [u8; 16] {
    // MD5 constants
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

    // Pad message
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
