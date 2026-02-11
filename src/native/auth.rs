use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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
}

impl NativeAuth {
    pub fn new() -> Result<Self> {
        Ok(Self {
            session_path: default_session_path()?,
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

    pub fn login_with_password(&self, _email: &str, _password: &str) -> Result<SessionToken> {
        Err(anyhow!("rust-native auth is not implemented yet"))
    }

    pub fn session_path(&self) -> &PathBuf {
        &self.session_path
    }
}

fn default_session_path() -> Result<PathBuf> {
    let base = dirs::config_dir().ok_or_else(|| anyhow!("unable to locate config dir"))?;
    Ok(base.join("pikpaktui").join("session.json"))
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
