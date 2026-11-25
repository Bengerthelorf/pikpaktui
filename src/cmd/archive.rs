use anyhow::{Result, anyhow, ensure};
use std::collections::HashSet;

pub fn run(args: &[String]) -> Result<()> {
    let (action, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("expected archive list, extract, or progress"))?;
    ensure!(
        matches!(action.as_str(), "list" | "extract" | "progress"),
        "unknown archive command: {action}"
    );
    let mut json = false;
    let mut all = false;
    let mut stdin_password = false;
    let mut directory = None;
    let mut members = Vec::new();
    let mut source = None;
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-J" | "--json" => json = true,
            "--all" if action == "extract" => all = true,
            "--member" if action == "extract" => members.push(
                iter.next()
                    .ok_or_else(|| anyhow!("--member requires a member path"))?,
            ),
            "--at" if action != "progress" => {
                directory = Some(
                    iter.next()
                        .ok_or_else(|| anyhow!("--at requires an archive directory path"))?,
                )
            }
            "--password-stdin" if action != "progress" => stdin_password = true,
            s if s.starts_with('-') => anyhow::bail!("unknown archive {action} option: {s}"),
            _ => ensure!(
                source.replace(arg).is_none(),
                "expected one archive path or task ID"
            ),
        }
    }
    let source = source.ok_or_else(|| anyhow!("expected one archive path or task ID"))?;
    if action == "extract" {
        ensure!(
            all == members.is_empty(),
            "choose --all or one or more --member paths"
        );
        ensure!(
            !(all && directory.is_some()),
            "--all extracts the whole archive; use --member for a directory"
        );
    }
    let client = super::cli_client()?;
    if action == "progress" {
        let progress = client.archive_progress(source)?;
        if json {
            println!("{}", serde_json::to_string(&progress)?);
        } else {
            println!(
                "{}% {} {}",
                progress.progress,
                super::share::terminal_safe_text(&progress.phase),
                super::share::terminal_safe_text(&progress.file_id)
            );
        }
        ensure!(
            !progress.failed(),
            "archive extraction failed: {}",
            super::share::terminal_safe_text(&progress.error_description)
        );
        return Ok(());
    }
    let (parent, name) = super::split_parent_name(source)?;
    let entry = super::find_entry(&client, &client.resolve_path(&parent)?, &name)?;
    let gcid = client.archive_gcid(&entry.id)?;
    let mut password = String::new();
    if stdin_password {
        std::io::stdin().read_line(&mut password)?;
        password.truncate(password.trim_end_matches(['\r', '\n']).len());
    }
    let listing = client.archive_list(&entry.id, &gcid, directory.map_or("", |s| s), &password)?;
    if listing.needs_password() {
        anyhow::bail!("archive requires a valid password; use --password-stdin");
    }
    listing.check()?;
    if action == "list" {
        if json {
            println!("{}", serde_json::to_string(&listing)?);
        } else {
            for file in &listing.files {
                println!(
                    "{}\t{}",
                    if file.is_folder() { "folder" } else { "file" },
                    super::share::terminal_safe_text(&file.path)
                );
            }
            if !listing.task_id.is_empty() {
                println!(
                    "Task {}",
                    super::share::terminal_safe_text(&listing.task_id)
                );
            }
        }
        return Ok(());
    }
    ensure!(
        listing.task_id.is_empty(),
        "archive already has a task: {}",
        super::share::terminal_safe_text(&listing.task_id)
    );
    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    for path in members {
        if !seen.insert(path) {
            continue;
        }
        let mut matches = listing.files.iter().filter(|file| &file.path == path);
        let member = matches
            .next()
            .ok_or_else(|| anyhow!("member not found in directory: {path}"))?;
        ensure!(matches.next().is_none(), "ambiguous member path: {path}");
        selected.push(member.clone());
    }
    let task = client.archive_extract(&entry.id, &gcid, &password, &selected)?;
    if json {
        println!("{}", serde_json::json!({"task_id":task}));
    } else {
        println!(
            "Extraction task {}",
            super::share::terminal_safe_text(&task)
        );
    }
    Ok(())
}
