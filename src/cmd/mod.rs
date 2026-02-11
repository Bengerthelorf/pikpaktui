pub mod cp;
pub mod download;
pub mod help;
pub mod ls;
pub mod mkdir;
pub mod mv;
pub mod quota;
pub mod rename;
pub mod rm;
pub mod share;
pub mod upload;

use crate::config::AppConfig;
use crate::pikpak::{self, PikPak};
use anyhow::{Result, anyhow};

pub fn cli_config() -> crate::config::TuiConfig {
    crate::config::TuiConfig::load()
}

pub fn cli_client() -> Result<PikPak> {
    let mut client = PikPak::new()?;

    if client.has_valid_session() {
        return Ok(client);
    }

    // Try config.yaml
    let cfg = AppConfig::load()?;
    match (cfg.username, cfg.password) {
        (Some(u), Some(p)) if !u.is_empty() && !p.is_empty() => {
            client.login(&u, &p)?;
            Ok(client)
        }
        _ => Err(anyhow!("not logged in. Run `pikpaktui` (TUI) to login first, or set credentials in config.yaml")),
    }
}

pub fn split_parent_name(path: &str) -> Result<(String, String)> {
    let path = path.trim().trim_end_matches('/');
    if path.is_empty() || path == "/" {
        return Err(anyhow!("invalid path: cannot operate on root"));
    }
    match path.rsplit_once('/') {
        Some(("", name)) => Ok(("/".to_string(), name.to_string())),
        Some((parent, name)) => Ok((parent.to_string(), name.to_string())),
        None => Ok(("/".to_string(), path.to_string())),
    }
}

pub fn find_entry(client: &PikPak, parent_id: &str, name: &str) -> Result<pikpak::Entry> {
    let entries = client.ls(parent_id)?;
    entries
        .into_iter()
        .find(|e| e.name == name)
        .ok_or_else(|| anyhow!("'{}' not found", name))
}

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    const TB: u64 = 1024 * GB;

    if bytes >= TB {
        format!("{:.1} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
