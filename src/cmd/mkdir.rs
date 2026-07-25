use anyhow::{Result, anyhow};

fn unique_existing_folder(
    entries: Vec<crate::pikpak::Entry>,
    name: &str,
    full_path: &str,
) -> Result<Option<crate::pikpak::Entry>> {
    let mut matches: Vec<_> = entries
        .into_iter()
        .filter(|entry| entry.name == name && entry.kind == crate::pikpak::EntryKind::Folder)
        .collect();
    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.pop()),
        count => {
            let ids = matches
                .iter()
                .map(|entry| format!("  id: {}", entry.id))
                .collect::<Vec<_>>()
                .join("\n");
            Err(anyhow!(
                "folder '{}' in path '{}' is ambiguous: {} folders share this name:\n{}\nrename one first (or operate on it in the TUI)",
                name,
                full_path,
                count,
                ids
            ))
        }
    }
}

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!(
            "Usage: pikpaktui mkdir [-n] [-p] <parent_path> <folder_name>\n       pikpaktui mkdir [-n] -p <full_path>"
        ));
    }

    let mut dry_run = false;
    let mut recursive = false;
    let mut rest: Vec<&str> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-n" | "--dry-run" => dry_run = true,
            "-p" => recursive = true,
            _ => rest.push(arg),
        }
    }

    let client = super::cli_client()?;

    if recursive {
        if rest.len() != 1 {
            return Err(anyhow!("Usage: pikpaktui mkdir [-n] -p <full_path>"));
        }
        let full_path = rest[0];
        let segments: Vec<&str> = full_path
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        if segments.is_empty() {
            return Err(anyhow!("invalid path"));
        }

        let mut current_id = String::new();
        let mut created_count = 0u32;

        if dry_run {
            println!(
                "[dry-run] Would create folder(s) at '/{}':",
                segments.join("/")
            );
            let mut accumulated = String::new();
            for (i, seg) in segments.iter().enumerate() {
                if !accumulated.is_empty() {
                    accumulated.push('/');
                }
                accumulated.push_str(seg);
                let entries = client.ls(&current_id)?;
                let display_path = format!("/{accumulated}");
                if let Some(existing) = unique_existing_folder(entries, seg, &display_path)? {
                    println!("  /{} (exists, id: {})", accumulated, existing.id);
                    current_id = existing.id;
                } else {
                    println!("  /{} (would create)", accumulated);
                    for seg in &segments[i + 1..] {
                        accumulated.push('/');
                        accumulated.push_str(seg);
                        println!("  /{} (would create)", accumulated);
                    }
                    break;
                }
            }
            return Ok(());
        }

        let mut accumulated = String::new();
        for seg in &segments {
            if !accumulated.is_empty() {
                accumulated.push('/');
            }
            accumulated.push_str(seg);
            let entries = client.ls(&current_id)?;
            // Only a folder counts as "exists": matching a same-named file
            // would make it the parent id for the next mkdir call.
            let display_path = format!("/{accumulated}");
            if let Some(existing) = unique_existing_folder(entries, seg, &display_path)? {
                current_id = existing.id;
            } else {
                let entry = client.mkdir(&current_id, seg)?;
                current_id = entry.id;
                created_count += 1;
            }
        }
        println!(
            "Created {} folder(s) at '/{}'",
            created_count,
            segments.join("/")
        );
    } else {
        if rest.len() != 2 {
            return Err(anyhow!(
                "Usage: pikpaktui mkdir [-n] <parent_path> <folder_name>"
            ));
        }
        let parent_id = client.resolve_path(rest[0])?;

        if dry_run {
            println!(
                "[dry-run] Would create folder '{}' in '{}' (parent id: {})",
                rest[1], rest[0], parent_id
            );
            return Ok(());
        }

        let created = client.mkdir(&parent_id, rest[1])?;
        println!("Created folder '{}' (id={})", created.name, created.id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pikpak::{Entry, EntryKind};

    fn folder(id: &str, name: &str) -> Entry {
        Entry {
            id: id.to_string(),
            name: name.to_string(),
            kind: EntryKind::Folder,
            size: 0,
            created_time: String::new(),
            modified_time: String::new(),
            starred: false,
            thumbnail_link: None,
        }
    }

    #[test]
    fn recursive_mkdir_rejects_duplicate_existing_folders() {
        let entries = vec![folder("first", "docs"), folder("second", "docs")];

        let err = unique_existing_folder(entries, "docs", "/docs").unwrap_err();

        assert!(format!("{err:#}").contains("ambiguous"));
    }
}
