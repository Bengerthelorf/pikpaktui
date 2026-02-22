use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    let mut json = false;
    let mut media = false;
    let mut copy = false;
    let mut path_arg: Option<&String> = None;

    for arg in args {
        match arg.as_str() {
            "-J" | "--json" => json = true,
            "--media" | "-m" => media = true,
            "--copy" | "-c" => copy = true,
            _ => {
                if path_arg.is_none() {
                    path_arg = Some(arg);
                }
            }
        }
    }

    let path = path_arg.ok_or_else(|| {
        anyhow!("usage: pikpaktui link [-J] [-m|--media] [-c|--copy] <path>")
    })?;

    let client = super::cli_client()?;
    let (parent_path, name) = super::split_parent_name(path)?;
    let parent_id = client.resolve_path(&parent_path)?;
    let entry = super::find_entry(&client, &parent_id, &name)?;

    if entry.kind == crate::pikpak::EntryKind::Folder {
        return Err(anyhow!("'{}' is a folder; link only works for files", name));
    }

    let info = client.file_info(&entry.id)?;

    let download_url = info
        .web_content_link
        .as_deref()
        .or_else(|| {
            info.links
                .as_ref()
                .and_then(|l| l.get("application/octet-stream"))
                .and_then(|v| v.url.as_deref())
        })
        .ok_or_else(|| anyhow!("no download link available for '{}'", name))?;

    // Collect media streaming URLs (videos have transcoded streams in medias[])
    let media_urls: Vec<(String, String)> = if media {
        info.medias
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter_map(|m| {
                let url = m.link.as_ref()?.url.as_deref()?;
                if url.is_empty() {
                    return None;
                }
                let label = m.media_name.as_deref().unwrap_or("stream").to_string();
                Some((label, url.to_string()))
            })
            .collect()
    } else {
        Vec::new()
    };

    if json {
        let size = info
            .size
            .as_deref()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        let mut out = serde_json::json!({
            "name": info.name,
            "url":  download_url,
            "size": size,
        });

        if media {
            out["medias"] = serde_json::json!(
                media_urls
                    .iter()
                    .map(|(n, u)| serde_json::json!({ "name": n, "url": u }))
                    .collect::<Vec<_>>()
            );
        }

        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    } else {
        println!("{}", download_url);

        for (label, url) in &media_urls {
            println!("[{}] {}", label, url);
        }
    }

    if copy {
        copy_to_clipboard(download_url)?;
        eprintln!("Copied to clipboard.");
    }

    Ok(())
}

fn copy_to_clipboard(text: &str) -> Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("pbcopy", &[] as &[&str])]
    } else {
        &[
            ("wl-copy", &[] as &[&str]),
            ("xclip", &["-selection", "clipboard"]),
        ]
    };

    for &(cmd, args) in candidates {
        let Ok(mut child) = Command::new(cmd).args(args).stdin(Stdio::piped()).spawn() else {
            continue;
        };
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(text.as_bytes());
        }
        child.wait()?;
        return Ok(());
    }

    Err(anyhow!(
        "no clipboard tool found (need pbcopy on macOS, wl-copy on Wayland, or xclip on X11)"
    ))
}
