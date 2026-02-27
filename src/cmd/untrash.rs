use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!("usage: pikpaktui untrash [-n] <name...>"));
    }

    let mut dry_run = false;
    let mut names: Vec<&str> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-n" | "--dry-run" => dry_run = true,
            _ => names.push(arg),
        }
    }

    if names.is_empty() {
        return Err(anyhow!("usage: pikpaktui untrash [-n] <name...>"));
    }

    let client = super::cli_client()?;
    let trash_entries = client.ls_trash(500)?;

    let mut ids = Vec::new();
    for name in &names {
        let entry = trash_entries
            .iter()
            .find(|e| e.name == *name)
            .ok_or_else(|| anyhow!("'{}' not found in trash", name))?;
        ids.push(entry.id.clone());
    }

    if dry_run {
        println!("[dry-run] Would restore {} item(s) from trash:", ids.len());
        for (name, id) in names.iter().zip(ids.iter()) {
            println!("  {} (id: {})", name, id);
        }
        return Ok(());
    }

    let id_refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
    client.untrash(&id_refs)?;
    println!("Restored {} item(s) from trash", ids.len());

    Ok(())
}
