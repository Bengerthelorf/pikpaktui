use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!(
            "Usage: pikpaktui mkdir [-p] <parent_path> <folder_name>\n       pikpaktui mkdir -p <full_path>"
        ));
    }

    let recursive = args.first().map(|a| a.as_str()) == Some("-p");
    let rest = if recursive { &args[1..] } else { &args[..] };

    let client = super::cli_client()?;

    if recursive {
        if rest.is_empty() {
            return Err(anyhow!("Usage: pikpaktui mkdir -p <full_path>"));
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
                "Usage: pikpaktui mkdir <parent_path> <folder_name>"
            ));
        }
        let parent_id = client.resolve_path(&rest[0])?;
        let created = client.mkdir(&parent_id, &rest[1])?;
        println!("Created folder '{}' (id={})", created.name, created.id);
    }
    Ok(())
}
