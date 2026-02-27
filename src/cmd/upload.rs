use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!(
            "Usage: pikpaktui upload [-n] <local> [remote]\n       pikpaktui upload [-n] -t <remote> <local...>"
        ));
    }

    let mut target: Option<&str> = None;
    let mut dry_run = false;
    let mut paths: Vec<&str> = Vec::new();
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-n" | "--dry-run" => dry_run = true,
            "-t" => {
                target = Some(
                    iter.next()
                        .ok_or_else(|| anyhow!("-t requires a remote path"))?
                        .as_str(),
                );
            }
            _ => paths.push(arg),
        }
    }

    if paths.is_empty() {
        return Err(anyhow!("no file specified"));
    }

    let client = super::cli_client()?;

    if let Some(dst) = target {
        let parent_id = client.resolve_path(dst)?;
        for path in &paths {
            let local_path = std::path::PathBuf::from(path);
            if !local_path.exists() {
                return Err(anyhow!("local file '{}' does not exist", local_path.display()));
            }
            if !local_path.is_file() {
                return Err(anyhow!("'{}' is not a file", local_path.display()));
            }

            let file_size = std::fs::metadata(&local_path)?.len();
            let file_name = local_path.file_name().unwrap_or_default().to_string_lossy();

            if dry_run {
                println!(
                    "[dry-run] Would upload '{}' ({}) -> '{}' (parent id: {})",
                    file_name,
                    super::format_size(file_size),
                    dst,
                    parent_id
                );
                continue;
            }

            eprintln!("{} ({}) uploading...", file_name, super::format_size(file_size));
            let (name, dedup) = client.upload_file(Some(&parent_id), &local_path)?;
            if dedup {
                println!("{} - complete (dedup)", name);
            } else {
                println!("{} - done", name);
            }
        }
    } else {
        let local_path = std::path::PathBuf::from(paths[0]);
        if !local_path.exists() {
            return Err(anyhow!("local file '{}' does not exist", local_path.display()));
        }
        if !local_path.is_file() {
            return Err(anyhow!("'{}' is not a file", local_path.display()));
        }

        let remote_dest = paths.get(1).copied();
        let parent_id = if let Some(dst) = remote_dest {
            Some(client.resolve_path(dst)?)
        } else {
            None
        };

        let file_size = std::fs::metadata(&local_path)?.len();
        let file_name = local_path.file_name().unwrap_or_default().to_string_lossy();

        if dry_run {
            let dest_display = remote_dest.unwrap_or("/");
            println!(
                "[dry-run] Would upload '{}' ({}) -> '{}'{}",
                file_name,
                super::format_size(file_size),
                dest_display,
                parent_id.as_deref().map(|id| format!(" (parent id: {id})")).unwrap_or_default()
            );
            return Ok(());
        }

        eprintln!("{} ({}) uploading...", file_name, super::format_size(file_size));
        let (name, dedup) = client.upload_file(parent_id.as_deref(), &local_path)?;
        if dedup {
            println!("{} - complete (dedup)", name);
        } else {
            println!("{} - done", name);
        }
    }
    Ok(())
}
