use anyhow::{Context, Result, anyhow};
use std::io::Write as _;

use crate::pikpak;

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!("Usage: pikpaktui share <path> [-o <output_file>]"));
    }

    // Parse -o flag
    let path = &args[0];
    let mut output_file: Option<&str> = None;
    let mut i = 1;
    while i < args.len() {
        if args[i] == "-o" && i + 1 < args.len() {
            output_file = Some(&args[i + 1]);
            i += 2;
        } else {
            i += 1;
        }
    }

    let client = super::cli_client()?;

    // Try to resolve as a path - determine if it's a file or folder
    let (parent_path, name) = super::split_parent_name(path)?;
    let parent_id = client.resolve_path(&parent_path)?;
    let entry = super::find_entry(&client, &parent_id, &name)?;

    let mut lines = Vec::new();

    match entry.kind {
        pikpak::EntryKind::File => {
            let info = client.file_info(&entry.id)?;
            let size = info.size.as_deref().unwrap_or("0");
            let hash = info.hash.as_deref().unwrap_or("");
            lines.push(format!("PikPak://{}|{}|{}", info.name, size, hash));
        }
        pikpak::EntryKind::Folder => {
            let entries = client.ls(&entry.id)?;
            for e in &entries {
                if e.kind == pikpak::EntryKind::File {
                    let info = client.file_info(&e.id)?;
                    let size = info.size.as_deref().unwrap_or("0");
                    let hash = info.hash.as_deref().unwrap_or("");
                    lines.push(format!("PikPak://{}|{}|{}", info.name, size, hash));
                }
            }
        }
    }

    if let Some(out_path) = output_file {
        let mut f = std::fs::File::create(out_path)
            .with_context(|| format!("cannot create output file '{}'", out_path))?;
        for line in &lines {
            writeln!(f, "{}", line)?;
        }
        println!("Wrote {} share link(s) to '{}'", lines.len(), out_path);
    } else {
        for line in &lines {
            println!("{}", line);
        }
    }

    Ok(())
}
