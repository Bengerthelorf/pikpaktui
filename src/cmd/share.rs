use anyhow::{Result, anyhow};
use std::io::Write as _;

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!(
            "Usage:\n  pikpaktui share [-p] [-d <days>] [-J] [-o <file>] <path...>\n  pikpaktui share -S [-n] [-p <code>] [-t <path>] [-J] <url>\n  pikpaktui share -l [-J]\n  pikpaktui share -D <share_id...>"
        ));
    }

    let list_mode   = args.iter().any(|a| a == "-l" || a == "--list");
    let delete_mode = args.iter().any(|a| a == "-D" || a == "--delete");
    let save_mode   = args.iter().any(|a| a == "-S" || a == "--save");

    if list_mode {
        run_list(args)
    } else if delete_mode {
        run_delete(args)
    } else if save_mode {
        run_save(args)
    } else {
        run_create(args)
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
        println!("\x1b[1;36m{}\x1b[0m", result.share_url);
        if !result.pass_code.is_empty() {
            println!("\x1b[33mPassword:\x1b[0m \x1b[1;33m{}\x1b[0m", result.pass_code);
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

fn run_list(args: &[String]) -> Result<()> {
    use unicode_width::UnicodeWidthStr;

    let json = args.iter().any(|a| a == "-J" || a == "--json");

    let client = super::cli_client()?;

    let spinner = super::Spinner::new("Fetching shares...");
    let shares = client.list_shares()?;
    drop(spinner);

    if shares.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No shares found.");
        }
        return Ok(());
    }

    if json {
        let out: Vec<_> = shares.iter().map(|s| serde_json::json!({
            "share_id":      s.share_id,
            "share_url":     s.share_url,
            "title":         s.title,
            "pass_code":     if s.pass_code.is_empty() { None } else { Some(&s.pass_code) },
            "share_to":      s.share_to,
            "create_time":   s.create_time,
            "expiration_days": s.expiration_days.parse::<i64>().unwrap_or(-1),
            "view_count":    s.view_count.parse::<u64>().unwrap_or(0),
            "restore_count": s.restore_count.parse::<u64>().unwrap_or(0),
            "file_num":      s.file_num.parse::<u64>().unwrap_or(0),
            "share_status":  s.share_status,
        })).collect();
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    struct Row {
        type_str: &'static str,
        type_color: &'static str,
        title: String,
        expiry: String,
        files: String,
        views: String,
        saves: String,
        date: String,
        url: String,
    }

    let rows: Vec<Row> = shares
        .iter()
        .map(|s| {
            let is_pw = !s.pass_code.is_empty() || s.share_to.contains("encrypted");
            let (type_str, type_color) = if is_pw {
                ("private", "33") // yellow
            } else {
                ("public", "32") // green
            };
            let expiry = match s.expiration_days.as_str() {
                "-1" | "" | "0" => "permanent".to_string(),
                d => format!("{}d", d),
            };
            let files = s.file_num.clone();
            let views = s.view_count.clone();
            let saves = s.restore_count.clone();
            let date = super::format_date(&s.create_time);
            Row {
                type_str,
                type_color,
                title: s.title.clone(),
                expiry,
                files,
                views,
                saves,
                date,
                url: s.share_url.clone(),
            }
        })
        .collect();

    let w_type = 7usize; // "private"
    let w_title = rows.iter().map(|r| UnicodeWidthStr::width(r.title.as_str())).max().unwrap_or(5).max(5);
    let w_expiry = rows.iter().map(|r| r.expiry.len()).max().unwrap_or(6).max(6);
    let w_files = rows.iter().map(|r| r.files.len()).max().unwrap_or(5).max(5);
    let w_views = rows.iter().map(|r| r.views.len()).max().unwrap_or(5).max(5);
    let w_saves = rows.iter().map(|r| r.saves.len()).max().unwrap_or(5).max(5);
    let w_date = rows.iter().map(|r| r.date.len()).max().unwrap_or(7).max(7);

    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(120);
    let fixed = w_type + 2 + w_expiry + 2 + w_files + 2 + w_views + 2 + w_saves + 2 + w_date + 12;
    let w_title = w_title.min(term_width.saturating_sub(fixed).max(12));

    println!(
        "\x1b[2mTYPE     {:<w_title$}  {:<w_expiry$}  {:>w_files$}  {:>w_views$}  {:>w_saves$}  CREATED\x1b[0m",
        "TITLE", "EXPIRY", "FILES", "VIEWS", "SAVES",
    );

    for r in &rows {
        let title = truncate(&r.title, w_title);
        println!(
            "\x1b[{tc}m{t:<w_type$}\x1b[0m  {:<w_title$}  {:<w_expiry$}  {:>w_files$}  {:>w_views$}  {:>w_saves$}  {}",
            title,
            r.expiry,
            r.files,
            r.views,
            r.saves,
            r.date,
            tc = r.type_color,
            t = r.type_str,
        );
        println!("         \x1b[2m{}\x1b[0m", r.url);
    }

    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    if UnicodeWidthStr::width(s) <= max {
        s.to_string()
    } else {
        let mut w = 0;
        let mut out = String::new();
        for ch in s.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if w + cw + 1 > max {
                break;
            }
            out.push(ch);
            w += cw;
        }
        out.push('…');
        out
    }
}

fn run_delete(args: &[String]) -> Result<()> {
    let ids: Vec<&str> = args
        .iter()
        .filter(|a| *a != "-D" && *a != "--delete")
        .map(|a| a.as_str())
        .collect();

    if ids.is_empty() {
        return Err(anyhow!("share -D requires at least one share_id"));
    }

    let client = super::cli_client()?;
    client.delete_shares(&ids)?;
    println!("Deleted {} share(s).", ids.len());
    Ok(())
}
