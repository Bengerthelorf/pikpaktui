use crate::pikpak::ShareLocation;
use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.first().is_some_and(|arg| arg == "--manage") {
        return super::share_manage::run(&args[1..]);
    }
    if args.is_empty() {
        return Err(anyhow!(
            "Usage:\n  pikpaktui share [-p] [-d <days>] [-J] [-o <file>] <path...>\n  pikpaktui share -S [-n] [-p <code>] [-t <path>] [--parent-id <id>] [--file-id <id>...] [-J] <url>\n  pikpaktui share -b [-p <code>] [--parent-id <id>] [-J] <url> [path]\n  pikpaktui share --preview --file-id <id> [-p <code>] [-J] <url>\n  pikpaktui share -l [-J]\n  pikpaktui share -D <share_id...>"
        ));
    }

    let list_mode = args.iter().any(|a| a == "-l" || a == "--list");
    let delete_mode = args.iter().any(|a| a == "-D" || a == "--delete");
    let save_mode = args.iter().any(|a| a == "-S" || a == "--save");
    let preview_mode = args.iter().any(|a| a == "--preview");
    let browse_mode = args.iter().any(|a| a == "-b" || a == "--browse");

    if [list_mode, delete_mode, save_mode, browse_mode, preview_mode]
        .into_iter()
        .filter(|selected| *selected)
        .count()
        > 1
    {
        return Err(anyhow!(
            "share modes --list, --delete, --save, --browse and --preview cannot be combined"
        ));
    }

    if list_mode {
        run_list(args)
    } else if delete_mode {
        run_delete(args)
    } else if save_mode {
        run_save(args)
    } else if browse_mode || preview_mode {
        run_browse(args)
    } else {
        super::share_manage::run_create(args)
    }
}

fn load_share_path(
    inner_path: &str,
    mut load_folder: impl FnMut(&str) -> Result<Vec<crate::pikpak::ShareEntry>>,
) -> Result<Vec<crate::pikpak::ShareEntry>> {
    // `/share` establishes status and pass_code_token, but its `files` field is
    // only one page. Always enter through the paginated `/share/detail` loader,
    // including for the root folder.
    let mut entries = load_folder("")?;
    let mut walked = String::new();
    for seg in inner_path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
    {
        let matches: Vec<_> = entries
            .iter()
            .filter(|entry| entry.name == seg && entry.is_folder())
            .collect();
        let folder_id = match matches.as_slice() {
            [] => {
                return Err(anyhow!(
                    "folder not found in share: '{}{}'",
                    terminal_safe_text(&walked),
                    terminal_safe_text(seg)
                ));
            }
            [folder] => folder.id.clone(),
            duplicates => {
                let ids = duplicates
                    .iter()
                    .map(|entry| format!("  id: {}", terminal_safe_text(&entry.id)))
                    .collect::<Vec<_>>()
                    .join("\n");
                return Err(anyhow!(
                    "folder '{}' in share path '{}{}' is ambiguous: {} folders share this name:\n{}",
                    terminal_safe_text(seg),
                    terminal_safe_text(&walked),
                    terminal_safe_text(seg),
                    duplicates.len(),
                    ids
                ));
            }
        };
        entries = load_folder(&folder_id)?;
        walked.push_str(seg);
        walked.push('/');
    }
    Ok(entries)
}

pub(crate) fn terminal_safe_text(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch.is_control()
                || matches!(
                    ch,
                    '\u{061c}'
                        | '\u{200e}'
                        | '\u{200f}'
                        | '\u{202a}'..='\u{202e}'
                        | '\u{2066}'..='\u{2069}'
                )
            {
                '\u{fffd}'
            } else {
                ch
            }
        })
        .collect()
}

fn run_browse(args: &[String]) -> Result<()> {
    let mut share_url: Option<&str> = None;
    let preview = args.iter().any(|a| a == "--preview");
    let mut file_id = None;
    let mut inner_path: Option<&str> = None;
    let mut parent_id: Option<&str> = None;
    let mut pass_code = "";
    let mut json = false;
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-b" | "--browse" | "--preview" => {}
            "--file-id" if preview && file_id.is_none() => {
                file_id = Some(
                    iter.next()
                        .filter(|id| !id.is_empty() && !id.starts_with('-'))
                        .ok_or_else(|| anyhow!("--file-id requires a file ID"))?,
                );
            }
            "-J" | "--json" => json = true,
            "--parent-id" => {
                if parent_id.is_some() {
                    return Err(anyhow!("--parent-id may only be specified once"));
                }
                parent_id = Some(
                    iter.next()
                        .filter(|id| !id.trim().is_empty() && !id.starts_with('-'))
                        .ok_or_else(|| anyhow!("--parent-id requires a folder ID"))?
                        .as_str(),
                );
            }
            "-p" | "--pass-code" => {
                pass_code = iter
                    .next()
                    .ok_or_else(|| anyhow!("-p requires a pass code"))?
                    .as_str();
            }
            s if s.starts_with('-') && s != "-" => {
                return Err(anyhow!("unknown option: {s}"));
            }
            arg => {
                if share_url.is_none() {
                    share_url = Some(arg);
                } else if inner_path.is_none() {
                    inner_path = Some(arg);
                } else {
                    return Err(anyhow!("unexpected argument: {}", arg));
                }
            }
        }
    }

    if parent_id.is_some() && inner_path.is_some() {
        return Err(anyhow!(
            "a share name path and --parent-id cannot be combined"
        ));
    }

    let share_url = share_url.ok_or_else(|| anyhow!("no share URL or ID provided"))?;
    let location = ShareLocation::parse(share_url)?;
    let parent_id = location.folder(parent_id)?;
    let share_id = &location.id;
    if preview && file_id.is_none() {
        return Err(anyhow!("--preview requires one --file-id"));
    }
    let url_pass_code = location.pass_code.as_ref();
    let pass_code = if args.iter().any(|a| a == "-p" || a == "--pass-code") {
        pass_code
    } else {
        url_pass_code.map_or(pass_code, String::as_str)
    };

    let client = super::cli_client()?;
    let spinner = super::Spinner::new("Fetching share...");
    let info = client.share_info(share_id, pass_code)?;
    location.breadcrumbs(&client, &info.pass_code_token)?;

    let entries = load_share_path(inner_path.unwrap_or(""), |id| {
        client.share_detail(
            share_id,
            if id.is_empty() { parent_id } else { id },
            &info.pass_code_token,
        )
    })?;
    if let Some(id) = file_id {
        let mut matches = entries.iter().filter(|entry| &entry.id == id);
        let entry = matches
            .next()
            .ok_or_else(|| anyhow!("file ID not found in the selected share folder"))?;
        anyhow::ensure!(
            matches.next().is_none() && !entry.is_folder(),
            "preview requires one unambiguous file"
        );
        let preview = client.share_preview(share_id, id, &info.pass_code_token)?;
        drop(spinner);
        if json {
            println!("{}", serde_json::to_string_pretty(&preview)?);
        } else {
            for line in preview.lines() {
                println!("{}", terminal_safe_text(&line));
            }
        }
        return Ok(());
    }
    drop(spinner);

    if json {
        let out: Vec<_> = entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "name": e.name,
                    "folder": e.is_folder(),
                    "size": e.size.as_deref().and_then(|s| s.parse::<u64>().ok()),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    if entries.is_empty() {
        println!("(empty)");
        return Ok(());
    }
    for e in &entries {
        let name = terminal_safe_text(&e.name);
        if e.is_folder() {
            println!("  \x1b[1;34m{}/\x1b[0m", name);
        } else {
            let size = e
                .size
                .as_deref()
                .and_then(|s| s.parse::<u64>().ok())
                .map(super::format_size)
                .unwrap_or_default();
            println!("  {}  \x1b[2m{}\x1b[0m", name, size);
        }
    }
    Ok(())
}

fn run_save(args: &[String]) -> Result<()> {
    let mut share_url: Option<&str> = None;
    let mut pass_code = "";
    let mut to_path: Option<&str> = None;
    let mut parent_id: Option<&str> = None;
    let mut requested_ids: Vec<&str> = Vec::new();
    let mut dry_run = false;
    let mut json = false;
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-S" | "--save" => {}
            "-n" | "--dry-run" => dry_run = true,
            "-J" | "--json" => json = true,
            "--parent-id" => {
                if parent_id.is_some() {
                    return Err(anyhow!("--parent-id may only be specified once"));
                }
                parent_id = Some(
                    iter.next()
                        .filter(|id| !id.trim().is_empty() && !id.starts_with('-'))
                        .ok_or_else(|| anyhow!("--parent-id requires a folder ID"))?
                        .as_str(),
                );
            }
            "--file-id" => {
                let id = iter
                    .next()
                    .filter(|id| !id.trim().is_empty() && !id.starts_with('-'))
                    .ok_or_else(|| anyhow!("--file-id requires a file ID"))?
                    .as_str();
                if !requested_ids.contains(&id) {
                    requested_ids.push(id);
                }
            }
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
            s if s.starts_with('-') && s != "-" => {
                return Err(anyhow!("unknown option: {s}"));
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
    let location = ShareLocation::parse(share_url)?;
    let parent_id = location.folder(parent_id)?;
    let share_id = location.id.as_str();
    let url_pass_code = location.pass_code.as_ref();
    let pass_code = if args.iter().any(|a| a == "-p" || a == "--pass-code") {
        pass_code
    } else {
        url_pass_code.map_or(pass_code, String::as_str)
    };

    let client = super::cli_client()?;

    let to_parent_id = match to_path {
        Some(path) => client.resolve_folder(path)?,
        None => String::new(),
    };
    let dest_display = to_path.unwrap_or("/");

    if !json {
        println!(
            "Fetching share info for '{}'...",
            terminal_safe_text(share_id)
        );
    }
    let info = client.share_info(share_id, pass_code)?;
    location.breadcrumbs(&client, &info.pass_code_token)?;
    let mut entries = client.share_detail(share_id, parent_id, &info.pass_code_token)?;

    // Validate every explicit ID against the complete folder listing before
    // constructing the restore payload. Never widen a failed selection to all.
    if !requested_ids.is_empty() {
        let mut selected = Vec::new();
        for id in requested_ids {
            let matches: Vec<_> = entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.id == id)
                .map(|(index, _)| index)
                .collect();
            match matches.as_slice() {
                [] => {
                    return Err(anyhow!(
                        "file ID '{}' not found in the selected share folder",
                        terminal_safe_text(id)
                    ));
                }
                [index] => selected.push(entries.swap_remove(*index)),
                _ => {
                    return Err(anyhow!(
                        "file ID '{}' is ambiguous in the selected share folder",
                        terminal_safe_text(id)
                    ));
                }
            }
        }
        entries = selected;
    }

    if entries.is_empty() {
        return Err(anyhow!("share contains no files"));
    }

    if !json {
        println!("Found {} item(s):", entries.len());
        for f in &entries {
            println!("  {}", terminal_safe_text(&f.name));
        }
    }

    if dry_run {
        if json {
            let out = serde_json::json!({
                "dry_run": true,
                "would_save": entries.len(),
                "to": dest_display,
                "files": entries.iter().map(|f| serde_json::json!({
                    "id": f.id,
                    "name": f.name,
                })).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
        } else {
            println!(
                "[dry-run] Would save {} item(s) to '{}'",
                entries.len(),
                terminal_safe_text(dest_display)
            );
        }
        return Ok(());
    }

    let file_ids: Vec<&str> = entries.iter().map(|f| f.id.as_str()).collect();
    if !json {
        println!("Saving to '{}'...", terminal_safe_text(dest_display));
    }
    let ancestors: Vec<_> = location.folders.iter().map(String::as_str).collect();
    client.save_share_from(
        share_id,
        &info.pass_code_token,
        &file_ids,
        &to_parent_id,
        &ancestors,
    )?;

    if json {
        let out = serde_json::json!({
            "saved": entries.len(),
            "to": dest_display,
            "files": entries.iter().map(|f| serde_json::json!({
                "id": f.id,
                "name": f.name,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!(
            "Saved {} item(s) to '{}'",
            entries.len(),
            terminal_safe_text(dest_display)
        );
    }

    Ok(())
}

fn run_list(args: &[String]) -> Result<()> {
    use unicode_width::UnicodeWidthStr;

    let json = args.iter().any(|a| a == "-J" || a == "--json");

    for arg in args {
        match arg.as_str() {
            "-l" | "--list" | "-J" | "--json" => {}
            s if s.starts_with('-') => return Err(anyhow!("unknown option: {s}")),
            _ => return Err(anyhow!("unexpected argument: {arg}")),
        }
    }

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
        let out: Vec<_> = shares
            .iter()
            .map(|s| {
                serde_json::json!({
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
                })
            })
            .collect();
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
                title: terminal_safe_text(&s.title),
                expiry: terminal_safe_text(&expiry),
                files: terminal_safe_text(&files),
                views: terminal_safe_text(&views),
                saves: terminal_safe_text(&saves),
                date: terminal_safe_text(&date),
                url: terminal_safe_text(&s.share_url),
            }
        })
        .collect();

    let w_type = 7usize; // "private"
    let w_title = rows
        .iter()
        .map(|r| UnicodeWidthStr::width(r.title.as_str()))
        .max()
        .unwrap_or(5)
        .max(5);
    let w_expiry = rows
        .iter()
        .map(|r| r.expiry.len())
        .max()
        .unwrap_or(6)
        .max(6);
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
        let title = super::truncate(&r.title, w_title);
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

fn run_delete(args: &[String]) -> Result<()> {
    let mut ids: Vec<&str> = Vec::new();
    for a in args {
        match a.as_str() {
            "-D" | "--delete" => {}
            // Share ids never start with '-'; a stray flag here would be
            // submitted to the API as an id.
            s if s.starts_with('-') => {
                return Err(anyhow!("unknown option: {s}"));
            }
            s => ids.push(s),
        }
    }

    if ids.is_empty() {
        return Err(anyhow!("share -D requires at least one share_id"));
    }

    let client = super::cli_client()?;
    client.delete_shares(&ids)?;
    println!("Deleted {} share(s).", ids.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pikpak::ShareEntry;

    fn folder(id: &str, name: &str) -> ShareEntry {
        ShareEntry {
            id: id.to_string(),
            name: name.to_string(),
            kind: "drive#folder".to_string(),
            size: None,
        }
    }

    fn file(id: &str, name: &str) -> ShareEntry {
        ShareEntry {
            id: id.to_string(),
            name: name.to_string(),
            kind: "drive#file".to_string(),
            size: Some("7".to_string()),
        }
    }

    #[test]
    fn browse_empty_path_loads_the_paginated_root() {
        let mut requested = Vec::new();
        let entries = load_share_path("", |parent_id| {
            requested.push(parent_id.to_string());
            Ok(vec![file("second-page", "visible.txt")])
        })
        .unwrap();

        assert_eq!(requested, vec![""]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "second-page");
    }

    #[test]
    fn browse_rejects_duplicate_folder_names() {
        let err = load_share_path("docs", |_| {
            Ok(vec![folder("first", "docs"), folder("second", "docs")])
        })
        .unwrap_err();

        assert!(format!("{err:#}").contains("ambiguous"));
    }

    #[test]
    fn terminal_text_contains_no_untrusted_control_characters() {
        let safe = terminal_safe_text("report\u{1b}]52;c;Zm9v\u{7}\rforged\n.txt");

        assert!(!safe.chars().any(char::is_control), "{safe:?}");
        assert!(safe.contains("report"));
        assert!(safe.contains("forged"));
    }

    #[test]
    fn terminal_text_replaces_bidi_formatting_controls() {
        let safe = terminal_safe_text(
            "left\u{061c}\u{200e}\u{200f}\u{202a}\u{202b}\u{202c}\u{202d}\u{202e}\u{2066}\u{2067}\u{2068}\u{2069}right",
        );

        assert_eq!(
            safe,
            "left\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}right"
        );
    }
}

#[cfg(test)]
mod share_url_tests {
    use super::*;

    #[test]
    fn root_share_identity_preserves_query_and_fragment_support() {
        for (input, expected_id) in [
            ("SHARE_ID", "SHARE_ID"),
            ("Share-id_09", "Share-id_09"),
            ("https://mypikpak.com/s/SHARE_ID", "SHARE_ID"),
            ("http://mypikpak.com/s/Share-id_09", "Share-id_09"),
            ("HTTPS://mypikpak.com/s/Share-id_09", "Share-id_09"),
            ("https://mypikpak.com/s/SHARE_ID/", "SHARE_ID"),
            ("https://mypikpak.com/s/SHARE_ID#1234", "SHARE_ID"),
            (
                "https://mypikpak.com/s/SHARE_ID?utm_source=test",
                "SHARE_ID",
            ),
            (
                "https://mypikpak.com/s/SHARE_ID/?pass_code=a%2Bb#encrypted",
                "SHARE_ID",
            ),
        ] {
            assert_eq!(
                ShareLocation::parse(input).unwrap().id,
                expected_id,
                "{input}"
            );
        }
        assert_eq!(
            ShareLocation::parse("https://mypikpak.com/s/SHARE_ID?pass_code=a%2Bb#encrypted")
                .unwrap()
                .pass_code
                .as_deref(),
            Some("a+b")
        );
        assert_eq!(
            ShareLocation::parse("https://mypikpak.com/s/SHARE_ID#1234")
                .unwrap()
                .pass_code,
            None
        );
        assert!(ShareLocation::parse("https://mypikpak.com/s/").is_err());
        assert!(ShareLocation::parse("not/a/url").is_err());
    }

    #[test]
    fn official_folder_link_decodes_to_the_api_file_id() {
        let loc = ShareLocation::parse(
            "https://mypikpak.com/s/VOIQ0QFrIMiPE8EFPZYJKKxpo1/AAAeZ9plmSBi6NG9KffhrhCAo1_VOI",
        )
        .unwrap();
        assert_eq!(loc.folders, ["VOIOttsOuejt-xC4FGHohrzpo1"]);
        assert!(
            ShareLocation::parse(
                "https://mypikpak.com/s/VOIQ0QFrIMiPE8EFPZYJKKxpo1/AAAeZ9plmSBi6NG9KffhrhCAo1_BAD"
            )
            .is_err()
        );
    }

    #[test]
    fn deep_share_routes_are_never_widened_to_the_root() {
        for input in [
            "https://mypikpak.com/s/SHARE_ID/folder/%2e",
            "https://mypikpak.com/s/SHARE_ID/folder/child?pass_code=abcd#encrypted",
        ] {
            let error = ShareLocation::parse(input).unwrap_err().to_string();
            assert!(error.contains("--parent-id"), "{input}: {error}");
            assert!(error.contains("--file-id"), "{input}: {error}");
        }
    }
}
