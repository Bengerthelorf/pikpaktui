use anyhow::{Result, anyhow};

use crate::pikpak::PikPak;

struct PlayOption {
    label: String,
    url: String,
    available: bool,
}

fn build_play_options(client: &PikPak, file_id: &str) -> Result<Vec<PlayOption>> {
    let info = client.file_info(file_id)?;
    let mut options = Vec::new();

    // Original via web_content_link
    if let Some(ref url) = info.web_content_link {
        if !url.is_empty() {
            let size_str = info
                .size
                .as_deref()
                .and_then(|s| s.parse::<u64>().ok())
                .map(super::format_size)
                .unwrap_or_default();
            options.push(PlayOption {
                label: format!("original ({})", size_str),
                url: url.clone(),
                available: true,
            });
        }
    }

    // Transcoded streams
    if let Some(ref medias) = info.medias {
        for m in medias {
            if m.is_origin.unwrap_or(false) {
                continue;
            }
            let url = m
                .link
                .as_ref()
                .and_then(|l| l.url.as_deref())
                .unwrap_or("")
                .to_string();
            if url.is_empty() {
                continue;
            }
            let label = m
                .media_name
                .as_deref()
                .unwrap_or("unknown")
                .to_string();
            let available = PikPak::check_stream_available(&url);
            options.push(PlayOption {
                label,
                url,
                available,
            });
        }
    }

    Ok(options)
}

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!(
            "usage: pikpaktui play <path> [quality]\n\n\
             quality: \"original\", or a stream name like \"720p\", \"1080p\"\n\
             omit quality to list available streams"
        ));
    }

    let path = &args[0];
    let quality = args.get(1).map(|s| s.as_str());

    let config = super::cli_config();
    let player = config.player.ok_or_else(|| {
        anyhow!(
            "no player configured.\n\
             Set `player` in ~/.config/pikpaktui/config.toml under [tui], e.g.:\n\n  \
             player = \"mpv\""
        )
    })?;

    let client = super::cli_client()?;

    let (parent_path, name) = super::split_parent_name(path)?;
    let parent_id = client.resolve_path(&parent_path)?;
    let entry = super::find_entry(&client, &parent_id, &name)?;

    let options = build_play_options(&client, &entry.id)?;
    if options.is_empty() {
        return Err(anyhow!("no playable streams found for '{}'", name));
    }

    match quality {
        None => {
            // List available streams
            println!("Available streams for '{}':", name);
            for (i, opt) in options.iter().enumerate() {
                let status = if opt.available { "" } else { " (unavailable)" };
                println!("  {}. {}{}", i + 1, opt.label, status);
            }
            println!();
            println!("Run: pikpaktui play \"{}\" <quality>", path);
            Ok(())
        }
        Some(q) => {
            // Try to match by number first
            if let Ok(num) = q.parse::<usize>() {
                if num >= 1 && num <= options.len() {
                    let opt = &options[num - 1];
                    if !opt.available {
                        return Err(anyhow!("stream '{}' is not available (cold storage)", opt.label));
                    }
                    return launch_player(&player, &opt.url, &opt.label);
                }
                return Err(anyhow!(
                    "invalid stream number: {}. Available: 1-{}",
                    num,
                    options.len()
                ));
            }

            // Match by name (case-insensitive, substring)
            let q_lower = q.to_lowercase();
            let matched: Vec<&PlayOption> = options
                .iter()
                .filter(|o| o.label.to_lowercase().contains(&q_lower))
                .collect();

            match matched.len() {
                0 => {
                    let available: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
                    Err(anyhow!(
                        "no stream matching '{}'\nAvailable: {}",
                        q,
                        available.join(", ")
                    ))
                }
                1 => {
                    let opt = matched[0];
                    if !opt.available {
                        return Err(anyhow!("stream '{}' is not available (cold storage)", opt.label));
                    }
                    launch_player(&player, &opt.url, &opt.label)
                }
                _ => {
                    let names: Vec<&str> = matched.iter().map(|o| o.label.as_str()).collect();
                    Err(anyhow!(
                        "'{}' matches multiple streams: {}\nBe more specific.",
                        q,
                        names.join(", ")
                    ))
                }
            }
        }
    }
}

fn launch_player(player_cmd: &str, url: &str, label: &str) -> Result<()> {
    let parts: Vec<&str> = player_cmd.split_whitespace().collect();
    if parts.is_empty() {
        return Err(anyhow!("player command is empty"));
    }
    let program = parts[0];
    let mut args: Vec<&str> = parts[1..].to_vec();
    args.push(url);

    eprintln!("Playing '{}' with {}...", label, program);
    let mut child = std::process::Command::new(program)
        .args(&args)
        .spawn()
        .map_err(|e| anyhow!("failed to launch {}: {}", program, e))?;

    child.wait().map_err(|e| anyhow!("player error: {}", e))?;
    Ok(())
}
