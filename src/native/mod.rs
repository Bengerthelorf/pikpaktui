pub mod auth;

use crate::backend::{Backend, Entry};
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
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

    fn access_token(&self) -> Result<String> {
        let session = self
            .auth
            .load_session()?
            .ok_or_else(|| anyhow!("native session not found, please login first"))?;
        Ok(session.access_token)
    }

    fn file_id_by_name(&self, parent_path: &str, name: &str) -> Result<String> {
        let list = self.list_raw(parent_path)?;
        list.files
            .into_iter()
            .find(|f| f.name == name)
            .map(|f| f.id)
            .ok_or_else(|| anyhow!("entry not found: '{}' in '{}'", name, parent_path))
    }

    fn list_raw(&self, path: &str) -> Result<DriveListResponse> {
        let token = self.access_token()?;
        let url = format!("{}/drive/v1/files", self.drive_base_url.trim_end_matches('/'));

        let response = self
            .http
            .get(url)
            .bearer_auth(&token)
            .query(&[("parent_path", path)])
            .send()
            .context("native ls request failed")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!("native ls failed with status {}: {}", status, body));
        }

        response.json().context("invalid native ls response")
    }

    fn move_file(&self, file_id: &str, to_path: &str) -> Result<()> {
        let token = self.access_token()?;
        let url = format!(
            "{}/drive/v1/files:batchMove",
            self.drive_base_url.trim_end_matches('/')
        );
        let payload = BatchMoveRequest {
            ids: vec![file_id.to_string()],
            to: MoveTarget {
                parent_path: to_path.to_string(),
            },
        };

        let response = self
            .http
            .post(url)
            .bearer_auth(token)
            .json(&payload)
            .send()
            .context("native move request failed")?;

        ensure_success(response, "native move")
    }

    fn copy_file(&self, file_id: &str, to_path: &str) -> Result<()> {
        let token = self.access_token()?;
        let url = format!(
            "{}/drive/v1/files:batchCopy",
            self.drive_base_url.trim_end_matches('/')
        );
        let payload = BatchMoveRequest {
            ids: vec![file_id.to_string()],
            to: MoveTarget {
                parent_path: to_path.to_string(),
            },
        };

        let response = self
            .http
            .post(url)
            .bearer_auth(token)
            .json(&payload)
            .send()
            .context("native copy request failed")?;

        ensure_success(response, "native copy")
    }

    fn rename_file(&self, file_id: &str, new_name: &str) -> Result<()> {
        let token = self.access_token()?;
        let url = format!(
            "{}/drive/v1/files/{}",
            self.drive_base_url.trim_end_matches('/'),
            file_id
        );
        let payload = RenameRequest {
            name: new_name.to_string(),
        };

        let response = self
            .http
            .patch(url)
            .bearer_auth(token)
            .json(&payload)
            .send()
            .context("native rename request failed")?;

        ensure_success(response, "native rename")
    }

    fn trash_file(&self, file_id: &str) -> Result<()> {
        let token = self.access_token()?;
        let url = format!(
            "{}/drive/v1/files:batchTrash",
            self.drive_base_url.trim_end_matches('/')
        );
        let payload = BatchTrashRequest {
            ids: vec![file_id.to_string()],
        };

        let response = self
            .http
            .post(url)
            .bearer_auth(token)
            .json(&payload)
            .send()
            .context("native remove request failed")?;

        ensure_success(response, "native remove")
    }
}

impl Backend for NativeBackend {
    fn name(&self) -> &'static str {
        "rust-native"
    }

    fn ls(&self, path: &str) -> Result<Vec<Entry>> {
        let payload = self.list_raw(path)?;
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

    fn mv(&self, current_path: &str, name: &str, target_path: &str) -> Result<String> {
        let file_id = self.file_id_by_name(current_path, name)?;
        self.move_file(&file_id, target_path)?;
        Ok(format!("moved '{}' -> '{}'", name, target_path))
    }

    fn cp(&self, current_path: &str, name: &str, target_path: &str) -> Result<String> {
        let file_id = self.file_id_by_name(current_path, name)?;
        self.copy_file(&file_id, target_path)?;
        Ok(format!("copied '{}' -> '{}'", name, target_path))
    }

    fn rename(&self, current_path: &str, old_name: &str, new_name: &str) -> Result<String> {
        let file_id = self.file_id_by_name(current_path, old_name)?;
        self.rename_file(&file_id, new_name)?;
        Ok(format!("renamed '{}' -> '{}'", old_name, new_name))
    }

    fn remove(&self, current_path: &str, name: &str) -> Result<String> {
        let file_id = self.file_id_by_name(current_path, name)?;
        self.trash_file(&file_id)?;
        Ok(format!("removed '{}'", name))
    }
}

fn ensure_success(response: reqwest::blocking::Response, op: &str) -> Result<()> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body = response.text().unwrap_or_default();
    Err(anyhow!("{} failed with status {}: {}", op, status, body))
}

#[derive(Deserialize)]
struct DriveListResponse {
    files: Vec<DriveFile>,
}

#[derive(Deserialize)]
struct DriveFile {
    id: String,
    name: String,
    kind: String,
    #[serde(default, deserialize_with = "de_opt_u64")]
    size: Option<u64>,
}

#[derive(Serialize)]
struct BatchMoveRequest {
    ids: Vec<String>,
    to: MoveTarget,
}

#[derive(Serialize)]
struct MoveTarget {
    parent_path: String,
}

#[derive(Serialize)]
struct RenameRequest {
    name: String,
}

#[derive(Serialize)]
struct BatchTrashRequest {
    ids: Vec<String>,
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
