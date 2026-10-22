use anyhow::{Result, anyhow};

use crate::{
    config::TuiConfig,
    pikpak::PlayOption,
    player::{self, PlayContext},
};

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!(
            "Usage: pikpaktui play <path> [quality]\n\n\
             quality: \"original\", or a stream name like \"720p\", \"1080p\"\n\
             omit quality to list available streams"
        ));
    }

    let path = &args[0];
    let quality = args.get(1).map(|s| s.as_str());

    let config = super::cli_config();
    config.player.as_ref().ok_or_else(|| {
        anyhow!(
            "no player configured.\n\
             Set `player` in ~/.config/pikpaktui/config.toml at the top level, e.g.:\n\n  \
             player = \"mpv\""
        )
    })?;

    let client = super::cli_client()?;

    let (parent_path, name) = super::split_parent_name(path)?;
    let parent_id = client.resolve_path(&parent_path)?;
    let entry = super::find_entry(&client, &parent_id, &name)?;

    let context = playback_context(&parent_path, &entry.name, &entry.id);
    let options = client.file_info(&entry.id)?.play_options();
    if options.is_empty() {
        return Err(anyhow!("no playable streams found for '{}'", name));
    }

    match quality {
        None => {
            println!("Available streams for '{}':", name);
            for (i, opt) in options.iter().enumerate() {
                let status = opt
                    .unavailable_reason
                    .map(|r| format!(" ({r})"))
                    .unwrap_or_default();
                println!("  {}. {}{}", i + 1, opt.label, status);
            }
            println!();
            println!("Run: pikpaktui play \"{}\" <quality>", path);
            Ok(())
        }
        Some(q) => {
            if let Ok(num) = q.parse::<usize>() {
                if num >= 1 && num <= options.len() {
                    let opt = &options[num - 1];
                    if !opt.available {
                        return Err(anyhow!(
                            "stream '{}' is not available: {}",
                            opt.label,
                            opt.unavailable_reason.unwrap_or("unavailable")
                        ));
                    }
                    return launch_player(&config, &opt.url, &opt.label, &context);
                }
                return Err(anyhow!(
                    "invalid stream number: {}. Available: 1-{}",
                    num,
                    options.len()
                ));
            }

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
                        return Err(anyhow!(
                            "stream '{}' is not available: {}",
                            opt.label,
                            opt.unavailable_reason.unwrap_or("unavailable")
                        ));
                    }
                    launch_player(&config, &opt.url, &opt.label, &context)
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

fn playback_context(parent_path: &str, name: &str, id: &str) -> PlayContext {
    let mut components: Vec<&str> = parent_path
        .trim()
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    components.push(name);
    PlayContext {
        name: name.into(),
        path: format!("/{}", components.join("/")),
        id: id.into(),
    }
}

pub(crate) fn player_command(
    config: &TuiConfig,
    url: &str,
    context: &PlayContext,
) -> Result<std::process::Command> {
    let player = config
        .player
        .as_deref()
        .ok_or_else(|| anyhow!("no player configured"))?;
    player::build_command(player, &config.player_args, url, context)
}

fn launch_player(config: &TuiConfig, url: &str, label: &str, context: &PlayContext) -> Result<()> {
    let mut command = player_command(config, url, context)?;
    let program = command.get_program().to_string_lossy().into_owned();
    eprintln!("Playing '{}' with {}...", label, program);
    let mut child = command
        .spawn()
        .map_err(|e| anyhow!("failed to launch {}: {}", program, e))?;
    child.wait().map_err(|e| anyhow!("player error: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_context_matches_resolved_cloud_path_and_stable_id() {
        let context = playback_context(" //TV Shows///Season 1/ ", "--film {id}.mkv", "file-id");
        assert_eq!(context.path, "/TV Shows/Season 1/--film {id}.mkv");
        assert_eq!(context.name, "--film {id}.mkv");
        assert_eq!(context.id, "file-id");
        assert_eq!(playback_context("/", "film.mkv", "id").path, "/film.mkv");
    }
}
