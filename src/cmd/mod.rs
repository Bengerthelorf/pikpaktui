pub mod cat;
pub mod complete_path;
pub mod completions;
pub mod cp;
pub mod download;
pub mod events;
pub mod help;
pub mod info;
pub mod ls;
pub mod mkdir;
pub mod mv;
pub mod offline;
pub mod play;
pub mod quota;
pub mod rename;
pub mod rm;
pub mod share;
pub mod star;
pub mod starred;
pub mod tasks;
pub mod trash;
pub mod unstar;
pub mod untrash;
pub mod upload;
pub mod vip;

use crate::config::AppConfig;
use crate::pikpak::{self, PikPak};
use anyhow::{Result, anyhow};

pub fn cli_config() -> crate::config::TuiConfig {
    crate::config::TuiConfig::load()
}

pub fn cli_client() -> Result<PikPak> {
    let mut client = PikPak::new()?;
    client.thumbnail_size = cli_config().thumbnail_size.as_api_str().to_string();

    if client.has_valid_session() {
        return Ok(client);
    }

    // Try login.yaml
    let cfg = AppConfig::load()?;
    match (cfg.username, cfg.password) {
        (Some(u), Some(p)) if !u.is_empty() && !p.is_empty() => {
            client.login(&u, &p)?;
            Ok(client)
        }
        _ => Err(anyhow!(
            "not logged in. Run `pikpaktui` (TUI) to login first, or set credentials in login.yaml"
        )),
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

/// eza-style grid output (column-major) for a list of entries.
pub fn print_entries_short(entries: &[pikpak::Entry], nerd_font: bool) {
    use crate::theme;
    use unicode_width::UnicodeWidthStr;

    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);

    let display_widths: Vec<usize> = entries
        .iter()
        .map(|e| {
            let cat = theme::categorize(e);
            let icon = theme::cli_icon(cat, nerd_font);
            UnicodeWidthStr::width(icon) + UnicodeWidthStr::width(e.name.as_str())
        })
        .collect();

    let max_width = display_widths.iter().copied().max().unwrap_or(1);
    let col_width = max_width + 2;
    let num_cols = (term_width / col_width).max(1);
    let num_rows = (entries.len() + num_cols - 1) / num_cols;

    for row in 0..num_rows {
        for col in 0..num_cols {
            let idx = col * num_rows + row;
            if idx >= entries.len() {
                break;
            }
            let e = &entries[idx];
            let cat = theme::categorize(e);
            let icon = theme::cli_icon(cat, nerd_font);
            let display = format!("{}{}", icon, e.name);
            let colored = theme::cli_colored(&display, cat);
            let is_last_col = col + 1 == num_cols || (col + 1) * num_rows + row >= entries.len();
            if is_last_col {
                print!("{}", colored);
            } else {
                let padding = col_width.saturating_sub(display_widths[idx]);
                print!("{}{}", colored, " ".repeat(padding));
            }
        }
        println!();
    }
}

/// Returns the colored `id  size  date  ` prefix used in long-format output.
/// Shared between `print_entries_long` and tree long mode.
pub fn long_entry_prefix(e: &pikpak::Entry) -> String {
    let size_str = if e.kind == pikpak::EntryKind::Folder {
        format!("{:>9}", "-")
    } else {
        format!("{:>9}", format_size(e.size))
    };
    let date = format_date(&e.created_time);
    let colored_id = format!("\x1b[2m{}\x1b[0m", e.id);
    let colored_size = format!("\x1b[1;32m{}\x1b[0m", size_str);
    let colored_date = format!("\x1b[34m{:16}\x1b[0m", date);
    format!("{}  {}  {}  ", colored_id, colored_size, colored_date)
}

/// eza-style long format output: id, size, date, icon+name.
pub fn print_entries_long(entries: &[pikpak::Entry], nerd_font: bool) {
    use crate::theme;

    for e in entries {
        let cat = theme::categorize(e);
        let icon = theme::cli_icon(cat, nerd_font);
        let name_display = format!("{}{}", icon, e.name);
        let colored_name = theme::cli_colored(&name_display, cat);
        println!("{}{}", long_entry_prefix(e), colored_name);
    }
}

pub fn print_entries_json(entries: &[pikpak::Entry]) {
    let json = serde_json::to_string_pretty(entries).unwrap_or_else(|_| "[]".into());
    println!("{}", json);
}

pub fn format_date(iso: &str) -> String {
    if iso.len() >= 16 {
        let s = iso.replace('T', " ");
        s[..16].to_string()
    } else if iso.is_empty() {
        "-".to_string()
    } else {
        iso.to_string()
    }
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
