use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        let cfg: AppConfig =
            serde_yaml::from_str(&raw).with_context(|| "failed to parse config.yaml")?;
        Ok(cfg)
    }

    pub fn save_credentials(username: &str, password: &str) -> Result<()> {
        let path = config_path()?;
        let mut cfg = if path.exists() {
            let raw = fs::read_to_string(&path).unwrap_or_default();
            serde_yaml::from_str(&raw).unwrap_or_default()
        } else {
            AppConfig::default()
        };

        cfg.username = Some(username.to_string());
        cfg.password = Some(password.to_string());

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create dir {}", parent.display()))?;
        }

        let raw = serde_yaml::to_string(&cfg).context("failed to serialize config")?;
        fs::write(&path, raw)
            .with_context(|| format!("failed to write config {}", path.display()))?;
        Ok(())
    }
}

pub fn config_path() -> Result<PathBuf> {
    let base = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("unable to locate config dir"))?;
    Ok(base.join("pikpaktui").join("config.yaml"))
}
