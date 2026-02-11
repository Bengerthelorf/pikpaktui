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
    let base = home_config_dir()
        .ok_or_else(|| anyhow::anyhow!("unable to locate config dir"))?;
    Ok(base.join("pikpaktui").join("config.yaml"))
}

/// Returns ~/.config on all platforms instead of platform-specific config dirs.
fn home_config_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".config"))
}

#[derive(Debug, Clone, Deserialize)]
pub struct TuiConfig {
    #[serde(default)]
    pub nerd_font: bool,
    #[serde(default = "default_move_mode")]
    pub move_mode: String, // "picker" or "input"
    #[serde(default = "default_true")]
    pub show_help_bar: bool,
    #[serde(default)]
    pub cli_nerd_font: bool,
}

fn default_true() -> bool {
    true
}

fn default_move_mode() -> String {
    "picker".to_string()
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            nerd_font: false,
            move_mode: "picker".to_string(),
            show_help_bar: true,
            cli_nerd_font: false,
        }
    }
}

impl TuiConfig {
    pub fn use_picker(&self) -> bool {
        self.move_mode != "input"
    }
}

impl TuiConfig {
    pub fn load() -> Self {
        let path = match home_config_dir() {
            Some(base) => base.join("pikpaktui").join("config.toml"),
            None => return Self::default(),
        };
        if !path.exists() {
            return Self::default();
        }
        let raw = match fs::read_to_string(&path) {
            Ok(r) => r,
            Err(_) => return Self::default(),
        };
        toml::from_str(&raw).unwrap_or_default()
    }
}
