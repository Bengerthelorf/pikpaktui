use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_AUTH_BASE_URL: &str = "https://user.mypikpak.com";
const DEFAULT_CLIENT_ID: &str = "YUMx5nI8dHdG8Aqv";
const DEFAULT_CLIENT_SECRET: &str = "2A8d6lyf2W1hweW";

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
                .user_agent("pikpaktui-native/0.1")
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

        let url = format!("{}/v1/auth/signin", self.auth_base_url.trim_end_matches('/'));
        let payload = SigninRequest {
            username: email,
            password,
            client_id: &self.client_id,
            client_secret: &self.client_secret,
        };

        let response = self
            .http
            .post(url)
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

    pub fn session_path(&self) -> &PathBuf {
        &self.session_path
    }
}

#[derive(Serialize)]
struct SigninRequest<'a> {
    username: &'a str,
    password: &'a str,
    client_id: &'a str,
    client_secret: &'a str,
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
}
