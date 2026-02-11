pub mod auth;

use crate::backend::{Backend, Entry};
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::env;

const DEFAULT_DRIVE_BASE_URL: &str = "https://api-drive.mypikpak.com";

pub struct NativeBackend {
    auth: auth::NativeAuth,
    drive_base_url: String,
    http: reqwest::blocking::Client,
}

pub struct NativeBackendConfig {
    pub auth: auth::NativeAuth,
    pub drive_base_url: String,
}

impl NativeBackend {
    pub fn new() -> Result<Self> {
        Self::from_config(NativeBackendConfig {
            auth: auth::NativeAuth::new()?,
            drive_base_url: env::var("PIKPAK_DRIVE_BASE_URL")
                .unwrap_or_else(|_| DEFAULT_DRIVE_BASE_URL.to_string()),
        })
    }

    pub fn from_config(cfg: NativeBackendConfig) -> Result<Self> {
        Ok(Self {
            auth: cfg.auth,
            drive_base_url: cfg.drive_base_url,
            http: reqwest::blocking::Client::builder()
                .user_agent("pikpaktui-native/0.1")
                .build()
                .context("failed to build drive http client")?,
        })
    }

    pub fn auth(&self) -> &auth::NativeAuth {
        &self.auth
    }
}

impl Backend for NativeBackend {
    fn name(&self) -> &'static str {
        "rust-native"
    }

    fn ls(&self, path: &str) -> Result<Vec<Entry>> {
        let session = self
            .auth
            .load_session()?
            .ok_or_else(|| anyhow!("native session not found, please login first"))?;

        let url = format!(
            "{}/drive/v1/files",
            self.drive_base_url.trim_end_matches('/')
        );

        let response = self
            .http
            .get(url)
            .bearer_auth(&session.access_token)
            .query(&[("parent_path", path)])
            .send()
            .context("native ls request failed")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!("native ls failed with status {}: {}", status, body));
        }

        let payload: DriveListResponse = response.json().context("invalid native ls response")?;
        let entries = payload
            .files
            .into_iter()
            .map(|f| Entry {
                name: f.name,
                size: if f.kind == "folder" { 0 } else { f.size.unwrap_or(0) },
            })
            .collect();

        Ok(entries)
    }

    fn mv(&self, _current_path: &str, _name: &str, _target_path: &str) -> Result<String> {
        Err(anyhow!("rust-native move not implemented yet"))
    }

    fn cp(&self, _current_path: &str, _name: &str, _target_path: &str) -> Result<String> {
        Err(anyhow!("rust-native copy not implemented yet"))
    }

    fn rename(&self, _current_path: &str, _old_name: &str, _new_name: &str) -> Result<String> {
        Err(anyhow!("rust-native rename not implemented yet"))
    }

    fn remove(&self, _current_path: &str, _name: &str) -> Result<String> {
        Err(anyhow!("rust-native remove not implemented yet"))
    }
}

#[derive(Deserialize)]
struct DriveListResponse {
    files: Vec<DriveFile>,
}

#[derive(Deserialize)]
struct DriveFile {
    name: String,
    kind: String,
    #[serde(default, deserialize_with = "de_opt_u64")]
    size: Option<u64>,
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

        fn visit_none<E>(self) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value))
        }

        fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            value.parse::<u64>().map(Some).map_err(E::custom)
        }

        fn visit_string<E>(self, value: String) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            self.visit_str(&value)
        }
    }

    deserializer.deserialize_any(U64Visitor)
}
