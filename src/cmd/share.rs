use anyhow::{Result, anyhow};
use std::io::Write as _;

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!(
            "Usage:\n  pikpaktui share [-p] [-d <days>] [-J] [-o <file>] <path...>\n  pikpaktui share -S [-n] [-p <code>] [-t <path>] [-J] <url>"
        ));
    }

    // Detect mode from first flag scan
    let save_mode = args.iter().any(|a| a == "-S" || a == "--save");

    if save_mode {
        run_save(&args)
    } else {
        run_create(&args)
    }
}

fn run_create(args: &[String]) -> Result<()> {
    let mut paths: Vec<&str> = Vec::new();
    let mut need_password = false;
    let mut expiration_days: i64 = -1;
    let mut output_file: Option<&str> = None;
    let mut json = false;
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-p" | "--password" => need_password = true,
            "-J" | "--json" => json = true,
            "-d" | "--days" => {
                let val = iter.next().ok_or_else(|| anyhow!("-d requires a number"))?;
                expiration_days = val
                    .parse::<i64>()
                    .map_err(|_| anyhow!("-d requires an integer"))?;
            }
            "-o" => {
                output_file = Some(
                    iter.next()
                        .ok_or_else(|| anyhow!("-o requires a file path"))?
                        .as_str(),
                );
            }
            _ => paths.push(arg),
        }
    }

    if paths.is_empty() {
        return Err(anyhow!("no path specified"));
    }

    let client = super::cli_client()?;

    let mut file_ids: Vec<String> = Vec::new();
    for path in &paths {
        let (parent, name) = super::split_parent_name(path)?;
        let parent_id = client.resolve_path(&parent)?;
        let entry = super::find_entry(&client, &parent_id, &name)?;
        file_ids.push(entry.id);
    }

    let id_refs: Vec<&str> = file_ids.iter().map(|s| s.as_str()).collect();
    let result = client.create_share(&id_refs, need_password, expiration_days)?;

    if json {
        let out = serde_json::json!({
            "share_id": result.share_id,
            "share_url": result.share_url,
            "pass_code": if result.pass_code.is_empty() { None } else { Some(&result.pass_code) },
            "share_text": if result.share_text.is_empty() { None } else { Some(&result.share_text) },
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("{}", result.share_url);
        if !result.pass_code.is_empty() {
            println!("Password: {}", result.pass_code);
        }
        if let Some(out_path) = output_file {
            let mut f = std::fs::File::create(out_path)
                .map_err(|e| anyhow!("cannot create '{}': {}", out_path, e))?;
            writeln!(f, "{}", result.share_url)?;
            if !result.pass_code.is_empty() {
                writeln!(f, "Password: {}", result.pass_code)?;
            }
            eprintln!("Written to '{}'", out_path);
        }
    }

    Ok(())
}

fn run_save(args: &[String]) -> Result<()> {
    let mut share_url: Option<&str> = None;
    let mut pass_code = "";
    let mut to_path: Option<&str> = None;
    let mut dry_run = false;
    let mut json = false;
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-S" | "--save" => {}
            "-n" | "--dry-run" => dry_run = true,
            "-J" | "--json" => json = true,
            "-p" | "--pass-code" => {
                pass_code = iter
                    .next()
                    .ok_or_else(|| anyhow!("-p requires a pass code"))?
                    .as_str();
            }
            "-t" | "--to" => {
                to_path = Some(
                    iter.next()
                        .ok_or_else(|| anyhow!("-t requires a path"))?
                        .as_str(),
                );
            }
            arg => {
                if share_url.is_none() {
                    share_url = Some(arg);
                } else {
                    return Err(anyhow!("unexpected argument: {}", arg));
                }
            }
        }
    }

    let share_url = share_url.ok_or_else(|| anyhow!("no share URL or ID provided"))?;

    let share_id = if share_url.contains("mypikpak.com/s/") {
        let trimmed = share_url.trim_end_matches('/');
        trimmed.rsplit('/').next().unwrap_or(trimmed)
    } else {
        share_url
    };

    let client = super::cli_client()?;

    let to_parent_id = match to_path {
        Some(path) => client.resolve_path(path)?,
        None => String::new(),
    };
    let dest_display = to_path.unwrap_or("/");

    if !json {
        println!("Fetching share info for '{}'...", share_id);
    }
    let info = client.share_info(share_id, pass_code)?;

    if info.files.is_empty() {
        return Err(anyhow!("share contains no files"));
    }

    if dry_run || !json {
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
