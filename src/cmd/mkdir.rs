use anyhow::{Result, anyhow};

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
        if rest.is_empty() {
            return Err(anyhow!("Usage: pikpaktui mkdir [-n] -p <full_path>"));
        }
        let full_path = rest.join(" ");
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
            println!("[dry-run] Would create folder(s) at '/{}':", segments.join("/"));
            let mut accumulated = String::new();
            for (i, seg) in segments.iter().enumerate() {
                if !accumulated.is_empty() { accumulated.push('/'); }
                accumulated.push_str(seg);
                let entries = client.ls(&current_id)?;
                if let Some(existing) = entries.into_iter().find(|e| e.name == *seg) {
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

        for seg in &segments {
            let entries = client.ls(&current_id)?;
            if let Some(existing) = entries.into_iter().find(|e| e.name == *seg) {
                current_id = existing.id;
            } else {
                let entry = client.mkdir(&current_id, seg)?;
                current_id = entry.id;
                created_count += 1;
            }
        }
        println!("Created {} folder(s) at '/{}'", created_count, segments.join("/"));
    } else {
        if rest.len() < 2 {
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
