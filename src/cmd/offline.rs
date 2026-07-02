use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!(
            "Usage: pikpaktui offline [--dry-run] <url> [--to <path>] [--name <name>]"
        ));
    }

    let client = super::cli_client()?;

    // The URL is positional so flags may come before or after it — the usage
    // string itself shows `offline [--dry-run] <url>`.
    let mut file_url: Option<&str> = None;
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
            "--dry-run" | "-n" => dry_run = true,
            "--preview" | "-p" => preview = true,
            other if other.starts_with('-') && other != "-" => {
                return Err(anyhow!(
                    "unknown option: {other}\nRun `pikpaktui offline --help` for usage."
                ));
            }
            url => {
                if file_url.is_some() {
                    return Err(anyhow!("only one URL can be submitted at a time"));
                }
                file_url = Some(url);
            }
        }
        i += 1;
    }

    let file_url = file_url.ok_or_else(|| anyhow!("no URL given"))?;

    if preview {
        let spinner = super::Spinner::new("Parsing resource...");
        let parsed = client.parse_resource(file_url)?;
        drop(spinner);
        // {"list": {"resources": [...]}} on current deployments; render
        // defensively since only the web client documents this shape.
        let root = if parsed.get("list").is_some() {
            &parsed["list"]
        } else {
            &parsed
        };
        let Some(resources) = root["resources"].as_array().filter(|r| !r.is_empty()) else {
            println!("Nothing recognized at that URL.");
            return Ok(());
        };
        print_resources(resources, 0);
        return Ok(());
    }

    let parent_id = match parent_path {
        Some(p) => Some(client.resolve_path(p)?),
        None => None,
    };

    if dry_run {
        let dest_display = parent_path.unwrap_or("/");
        print!("[dry-run] Would submit offline download: '{}'", file_url);
        if let Some(n) = name {
            print!(" as '{}'", n);
        }
        println!(" -> '{}'", dest_display);
        if let Some(id) = &parent_id {
            println!("  parent id: {}", id);
        }
        return Ok(());
    }

    let resp = client.offline_download(file_url, parent_id.as_deref(), name)?;
    if let Some(task) = &resp.task {
        println!("Offline task created: {}", task.name);
        println!("  ID:    {}", task.id);
        println!("  Phase: {}", task.phase);
        if let Some(fid) = &task.file_id {
            println!("  File:  {}", fid);
        }
    } else {
        println!("Offline download submitted");
    }

    Ok(())
}

/// Print parsed torrent/URL contents: name, size, file count, one level of
/// nesting per "dir.resources" the server includes.
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
