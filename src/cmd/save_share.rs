use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!(
            "Usage: pikpaktui save-share [-n] <url_or_id> [--pass-code <code>] [--to <path>]"
        ));
    }

    let mut share_arg = args[0].as_str();
    let mut pass_code = "";
    let mut to_path: Option<&str> = None;
    let mut dry_run = false;
    let mut json = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-n" | "--dry-run" => dry_run = true,
            "-J" | "--json" => json = true,
            "--pass-code" | "-p" => {
                i += 1;
                if i >= args.len() {
                    return Err(anyhow!("--pass-code requires a value"));
                }
                pass_code = &args[i];
            }
            "--to" | "-t" => {
                i += 1;
                if i >= args.len() {
                    return Err(anyhow!("--to requires a path"));
                }
                to_path = Some(&args[i]);
            }
            _ => {
                return Err(anyhow!("unexpected argument: {}", args[i]));
            }
        }
        i += 1;
    }

    // Extract share_id from URL or use as-is
    let share_id = if share_arg.contains("mypikpak.com/s/") {
        share_arg = share_arg.trim_end_matches('/');
        share_arg.rsplit('/').next().unwrap_or(share_arg)
    } else {
        share_arg
    };

    let client = super::cli_client()?;

    // Resolve destination folder
    let to_parent_id = match to_path {
        Some(path) => client.resolve_path(path)?,
        None => String::new(), // root
    };

    let dest_display = match to_path {
        Some(p) => p.to_string(),
        None => "/".to_string(),
    };

    // Fetch share info
    if !json {
        println!("Fetching share info for '{}'...", share_id);
    }
    let info = client.share_info(share_id, pass_code)?;

    if info.files.is_empty() {
        return Err(anyhow!("share contains no files"));
    }

    if !json {
        println!("Found {} item(s):", info.files.len());
        for f in &info.files {
            println!("  {}", f.name);
        }
    }

    if dry_run {
        println!("[dry-run] Would save {} item(s) to '{}'", info.files.len(), dest_display);
        return Ok(());
    }

    let file_ids: Vec<&str> = info.files.iter().map(|f| f.id.as_str()).collect();
    if !json {
        println!("Saving to '{}'...", dest_display);
    }
    client.save_share(share_id, &info.pass_code_token, &file_ids, &to_parent_id)?;

    if json {
        let out = serde_json::json!({
            "saved": info.files.len(),
            "to": dest_display,
            "files": info.files.iter().map(|f| serde_json::json!({
                "id": f.id,
                "name": f.name,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("Saved {} item(s) to '{}'", info.files.len(), dest_display);
    }
    Ok(())
}
