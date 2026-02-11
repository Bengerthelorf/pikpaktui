mod config;
mod pikpak;
mod theme;
mod tui;

use crate::config::{AppConfig, TuiConfig};
use crate::pikpak::PikPak;
use anyhow::{Result, anyhow};
use std::env;
use std::process::exit;

fn main() {
    if let Err(e) = entry() {
        eprintln!("Error: {e:#}");
        exit(1);
    }
}

fn entry() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        return run_tui();
    }

    match args[0].as_str() {
        "ls" => cmd_ls(&args[1..]),
        "mv" => cmd_mv(&args[1..]),
        "cp" => cmd_cp(&args[1..]),
        "rename" => cmd_rename(&args[1..]),
        "rm" => cmd_rm(&args[1..]),
        "mkdir" => cmd_mkdir(&args[1..]),
        "download" => cmd_download(&args[1..]),
        "quota" => cmd_quota(),
        other => Err(anyhow!("unknown command: {}\nUsage: pikpaktui [ls|mv|cp|rename|rm|mkdir|download|quota]", other)),
    }
}

fn run_tui() -> Result<()> {
    let client = PikPak::new()?;
    let tui_config = TuiConfig::load();

    if client.has_valid_session() {
        return tui::run(client, tui_config);
    }

    // Check config.yaml for credentials
    let cfg = AppConfig::load()?;
    let credentials = match (cfg.username, cfg.password) {
        (Some(u), Some(p)) if !u.is_empty() && !p.is_empty() => Some((u, p)),
        _ => None,
    };

    tui::run_with_credentials(client, credentials, tui_config)
}

fn cli_client() -> Result<PikPak> {
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

// --- CLI commands ---

fn cmd_ls(args: &[String]) -> Result<()> {
    let path = args.first().map(|s| s.as_str()).unwrap_or("/");
    let client = cli_client()?;
    let parent_id = client.resolve_path(path)?;
    let entries = client.ls(&parent_id)?;

    for e in &entries {
        let kind = match e.kind {
            pikpak::EntryKind::Folder => "DIR ",
            pikpak::EntryKind::File => "FILE",
        };
        println!("{} {:>12}  {}", kind, format_size(e.size), e.name);
    }
    if entries.is_empty() {
        println!("(empty)");
    }
    Ok(())
}

fn cmd_mv(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(anyhow!("Usage: pikpaktui mv <source_path> <dest_folder_path>"));
    }
    let client = cli_client()?;
    let (src_parent, src_name) = split_parent_name(&args[0])?;
    let src_parent_id = client.resolve_path(&src_parent)?;
    let entry = find_entry(&client, &src_parent_id, &src_name)?;
    let dest_id = client.resolve_path(&args[1])?;
    client.mv(&[entry.id.as_str()], &dest_id)?;
    println!("Moved '{}' -> '{}'", args[0], args[1]);
    Ok(())
}

fn cmd_cp(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(anyhow!("Usage: pikpaktui cp <source_path> <dest_folder_path>"));
    }
    let client = cli_client()?;
    let (src_parent, src_name) = split_parent_name(&args[0])?;
    let src_parent_id = client.resolve_path(&src_parent)?;
    let entry = find_entry(&client, &src_parent_id, &src_name)?;
    let dest_id = client.resolve_path(&args[1])?;
    client.cp(&[entry.id.as_str()], &dest_id)?;
    println!("Copied '{}' -> '{}'", args[0], args[1]);
    Ok(())
}

fn cmd_rename(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(anyhow!("Usage: pikpaktui rename <file_path> <new_name>"));
    }
    let client = cli_client()?;
    let (parent, name) = split_parent_name(&args[0])?;
    let parent_id = client.resolve_path(&parent)?;
    let entry = find_entry(&client, &parent_id, &name)?;
    client.rename(&entry.id, &args[1])?;
    println!("Renamed '{}' -> '{}'", name, args[1]);
    Ok(())
}

fn cmd_rm(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!("Usage: pikpaktui rm <file_path>"));
    }
    let client = cli_client()?;
    let (parent, name) = split_parent_name(&args[0])?;
    let parent_id = client.resolve_path(&parent)?;
    let entry = find_entry(&client, &parent_id, &name)?;
    client.remove(&[entry.id.as_str()])?;
    println!("Removed '{}' (to trash)", args[0]);
    Ok(())
}

fn cmd_mkdir(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(anyhow!("Usage: pikpaktui mkdir <parent_path> <folder_name>"));
    }
    let client = cli_client()?;
    let parent_id = client.resolve_path(&args[0])?;
    let created = client.mkdir(&parent_id, &args[1])?;
    println!("Created folder '{}' (id={})", created.name, created.id);
    Ok(())
}

fn cmd_download(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!("Usage: pikpaktui download <file_path> [local_path]"));
    }
    let client = cli_client()?;
    let (parent, name) = split_parent_name(&args[0])?;
    let parent_id = client.resolve_path(&parent)?;
    let entry = find_entry(&client, &parent_id, &name)?;

    let dest = if args.len() > 1 {
        std::path::PathBuf::from(&args[1])
    } else {
        std::path::PathBuf::from(&name)
    };

    let total = client.download_to(&entry.id, &dest)?;
    println!("Downloaded '{}' -> '{}' ({})", name, dest.display(), format_size(total));
    Ok(())
}

fn cmd_quota() -> Result<()> {
    let client = cli_client()?;
    let quota = client.quota()?;

    if let Some(detail) = quota.quota {
        let limit = detail.limit.as_deref().unwrap_or("unknown");
        let usage = detail.usage.as_deref().unwrap_or("0");
        let trash = detail.usage_in_trash.as_deref().unwrap_or("0");

        let limit_n: u64 = limit.parse().unwrap_or(0);
        let usage_n: u64 = usage.parse().unwrap_or(0);

        println!("Quota:  {}", format_size(limit_n));
        println!("Used:   {}", format_size(usage_n));
        println!("Trash:  {}", format_size(trash.parse().unwrap_or(0)));
        if limit_n > 0 {
            println!("Free:   {}", format_size(limit_n.saturating_sub(usage_n)));
        }
    } else {
        println!("No quota info available");
    }
    Ok(())
}

// --- Helpers ---

fn split_parent_name(path: &str) -> Result<(String, String)> {
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

fn find_entry(client: &PikPak, parent_id: &str, name: &str) -> Result<pikpak::Entry> {
    let entries = client.ls(parent_id)?;
    entries
        .into_iter()
        .find(|e| e.name == name)
        .ok_or_else(|| anyhow!("'{}' not found", name))
}

fn format_size(bytes: u64) -> String {
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
