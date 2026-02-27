use anyhow::{Result, anyhow};
use crate::pikpak::EntryKind;

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!("Usage: pikpaktui rm [-n] [-r] [-f] <path...>"));
    }

    let mut force = false;
    let mut recursive = false;
    let mut dry_run = false;
    let mut paths: Vec<&str> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-f" => force = true,
            "-r" => recursive = true,
            "-rf" | "-fr" => { recursive = true; force = true; }
            "-n" | "--dry-run" => dry_run = true,
            _ => paths.push(arg),
        }
    }

    if paths.is_empty() {
        return Err(anyhow!("Usage: pikpaktui rm [-n] [-r] [-f] <path...>"));
    }

    let client = super::cli_client()?;

    struct Resolved<'a> {
        path: &'a str,
        id: String,
        kind: EntryKind,
        size: u64,
    }

    let mut resolved: Vec<Resolved> = Vec::new();
    for path in &paths {
        let (parent, name) = super::split_parent_name(path)?;
        let parent_id = client.resolve_path(&parent)?;
        let entry = super::find_entry(&client, &parent_id, &name)?;

        if entry.kind == EntryKind::Folder && !recursive {
            return Err(anyhow!(
                "'{}' is a folder. Use -r to remove folders.",
                path
            ));
        }
        resolved.push(Resolved { path, id: entry.id, kind: entry.kind, size: entry.size });
    }

    if dry_run {
        let action = if force { "permanently delete" } else { "trash" };
        println!("[dry-run] Would {} {} item(s):", action, resolved.len());
        for r in &resolved {
            let kind_tag = if r.kind == EntryKind::Folder { "folder" } else { &super::format_size(r.size) };
            println!("  {} (id: {}, {})", r.path, r.id, kind_tag);
        }
        return Ok(());
    }

    let ids: Vec<&str> = resolved.iter().map(|r| r.id.as_str()).collect();
    if force {
        client.delete_permanent(&ids)?;
        println!("Permanently deleted {} item(s)", paths.len());
    } else {
        client.remove(&ids)?;
        println!("Removed {} item(s) (to trash)", paths.len());
    }
    Ok(())
}
