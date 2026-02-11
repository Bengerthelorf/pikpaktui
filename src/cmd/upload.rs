use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!("Usage: pikpaktui upload <local_path> [remote_path]"));
    }

    let local_path = std::path::PathBuf::from(&args[0]);
    if !local_path.exists() {
        return Err(anyhow!("local file '{}' does not exist", local_path.display()));
    }
    if !local_path.is_file() {
        return Err(anyhow!("'{}' is not a file", local_path.display()));
    }

    let remote_path = if args.len() > 1 { &args[1] } else { "/" };

    let client = super::cli_client()?;
    let parent_id = client.resolve_path(remote_path)?;

    let file_size = std::fs::metadata(&local_path)?.len();
    let file_name = local_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();

    eprintln!("{} ({}) uploading...", file_name, super::format_size(file_size));

    let (name, dedup) = client.upload_file(&parent_id, &local_path)?;

    if dedup {
        println!("{} - complete (dedup)", name);
    } else {
        println!("{} - done", name);
    }

    Ok(())
}
