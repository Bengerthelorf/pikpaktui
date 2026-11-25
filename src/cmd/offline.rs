use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!(
            "Usage: pikpaktui offline [--preview | --dry-run] [-J] [--stdin] [<url>...] [--to <path>] [--name <name>]"
        ));
    }

    let mut json = false;
    let mut stdin = false;

    // The URL is positional so flags may come before or after it — the usage
    // string itself shows `offline [--dry-run] <url>`.
    let mut urls = Vec::new();
    let mut parent_path: Option<&str> = None;
    let mut name: Option<&str> = None;
    let mut dry_run = false;
    let mut preview = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--to" | "-t" => {
                i += 1;
                parent_path = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow!("--to requires a path"))?
                        .as_str(),
                );
            }
            "--name" => {
                i += 1;
                name = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow!("--name requires a value"))?
                        .as_str(),
                );
            }
            "--stdin" | "-" => stdin = true,
            "-J" | "--json" => json = true,
            "--dry-run" | "-n" => dry_run = true,
            "--preview" | "-p" => preview = true,
            other if other.starts_with('-') && other != "-" => {
                return Err(anyhow!(
                    "unknown option: {other}\nRun `pikpaktui offline --help` for usage."
                ));
            }
            url => urls.push(url.to_owned()),
        }
        i += 1;
    }

    if stdin {
        use std::io::BufRead;
        for line in std::io::stdin().lock().lines() {
            let line = line?;
            if !line.trim().is_empty() {
                urls.push(line.trim().to_owned());
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    urls.retain(|u| seen.insert(u.clone()));
    anyhow::ensure!(!urls.is_empty(), "no URL given");
    anyhow::ensure!(
        name.is_none() || urls.len() == 1,
        "--name requires a single URL"
    );
    let client = super::cli_client()?;
    let parent = if preview {
        None
    } else {
        parent_path.map(|p| client.resolve_folder(p)).transpose()?
    };
    let multiple = urls.len() > 1;
    let mut records = Vec::new();
    let mut failures = 0;
    let mut previews = if preview {
        client.preview_resources(&urls).into_iter()
    } else {
        Vec::new().into_iter()
    };
    for url in urls {
        let outcome = (|| -> Result<serde_json::Value> {
            if preview {
                let parsed = previews
                    .next()
                    .ok_or_else(|| anyhow!("missing preview result"))??;
                if !json {
                    if multiple {
                        println!("{url}");
                    }
                    let resources = parsed.get("list").unwrap_or(&parsed)["resources"].as_array();
                    if let Some(items) = resources.filter(|r| !r.is_empty()) {
                        print_resources(items, 0);
                    } else {
                        println!("Nothing recognized at that URL.");
                    }
                }
                return Ok(serde_json::json!({"url":url,"ok":true,"preview":parsed}));
            }
            if dry_run {
                if !json {
                    print!("[dry-run] Would submit offline download: '{url}'");
                    if let Some(name) = name {
                        print!(" as '{name}'");
                    }
                    println!(" -> '{}'", parent_path.unwrap_or("/"));
                    if let Some(id) = &parent {
                        println!("  parent id: {id}");
                    }
                }
                return Ok(
                    serde_json::json!({"url":url,"ok":true,"dry_run":true,"parent_id":parent,"name":name}),
                );
            }
            let response = client.offline_download(&url, parent.as_deref(), name)?;
            if !json {
                if let Some(task) = &response.task {
                    println!(
                        "Offline task created: {}\n  ID:    {}\n  Phase: {}",
                        task.name, task.id, task.phase
                    );
                    if let Some(id) = &task.file_id {
                        println!("  File:  {id}");
                    }
                } else {
                    println!("Offline download submitted: {url}");
                }
            }
            Ok(serde_json::json!({"url":url,"ok":true,"task":response.task}))
        })();
        match outcome {
            Ok(record) => records.push(record),
            Err(error) => {
                failures += 1;
                if !json {
                    eprintln!("{url}: {error:#}");
                }
                records
                    .push(serde_json::json!({"url":url,"ok":false,"error":format!("{error:#}")}));
            }
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&records)?);
    }
    anyhow::ensure!(failures == 0, "{failures} offline request(s) failed");
    Ok(())
}

fn print_resources(resources: &[serde_json::Value], indent: usize) {
    for r in resources {
        let name = r["name"].as_str().unwrap_or("<unnamed>");
        let size = r["file_size"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| r["file_size"].as_u64())
            .map(super::format_size)
            .unwrap_or_default();
        let count = r["file_count"].as_i64().unwrap_or(0);
        let mut line = format!("{}{}", "  ".repeat(indent + 1), name);
        if !size.is_empty() && size != "0 B" {
            line.push_str(&format!("  ({size})"));
        }
        if count > 1 {
            line.push_str(&format!("  [{count} files]"));
        }
        println!("{line}");
        if let Some(children) = r["dir"]["resources"].as_array() {
            print_resources(children, indent + 1);
        }
    }
}
