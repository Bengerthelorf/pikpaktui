use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    let mut json = false;
    let mut path_arg: Option<&String> = None;

    for arg in args {
        match arg.as_str() {
            "-J" | "--json" => json = true,
            _ => {
                if path_arg.is_none() {
                    path_arg = Some(arg);
                }
            }
        }
    }

    let path = path_arg.ok_or_else(|| anyhow!("usage: pikpaktui info [-J|--json] <path>"))?;
    let client = super::cli_client()?;

    let (parent_path, name) = super::split_parent_name(path)?;
    let parent_id = client.resolve_path(&parent_path)?;
    let entry = super::find_entry(&client, &parent_id, &name)?;
    let info = client.file_info(&entry.id)?;

    if json {
        let out = serde_json::to_string_pretty(&info).unwrap_or_else(|_| "{}".into());
        println!("{}", out);
        return Ok(());
    }

    let cat = crate::theme::categorize(&entry);
    let colored_name = crate::theme::cli_colored(&info.name, cat);
    println!("\x1b[36mName:\x1b[0m     {}", colored_name);

    if let Some(kind) = &info.kind {
        let display = if kind.contains("folder") { "folder" } else { "file" };
        println!("\x1b[36mType:\x1b[0m     {}", display);
    }

    if let Some(size) = &info.size {
        if let Ok(bytes) = size.parse::<u64>() {
            println!("\x1b[36mSize:\x1b[0m     \x1b[1;32m{}\x1b[0m ({})", super::format_size(bytes), size);
        } else {
            println!("\x1b[36mSize:\x1b[0m     {}", size);
        }
    }

    if let Some(hash) = &info.hash {
        println!("\x1b[36mHash:\x1b[0m     \x1b[2m{}\x1b[0m", hash);
    }

    if let Some(mime) = &info.mime_type {
        println!("\x1b[36mMIME:\x1b[0m     {}", mime);
    }

    if let Some(created) = &info.created_time {
        let date = super::format_date(created);
        println!("\x1b[36mCreated:\x1b[0m  \x1b[34m{}\x1b[0m", date);
    }

    if let Some(medias) = &info.medias {
        for media in medias {
            if let Some(video) = &media.video {
                println!();
                println!("\x1b[36mMedia:\x1b[0m    {}", media.media_name.as_deref().unwrap_or("-"));
                if let (Some(w), Some(h)) = (video.width, video.height) {
                    println!("  \x1b[36mResolution:\x1b[0m {}x{}", w, h);
                }
                if let Some(dur) = video.duration {
                    let mins = (dur / 60.0) as u64;
                    let secs = (dur % 60.0) as u64;
                    println!("  \x1b[36mDuration:\x1b[0m   {}:{:02}", mins, secs);
                }
                if let Some(br) = video.bit_rate {
                    println!("  \x1b[36mBitrate:\x1b[0m    {} kbps", br / 1000);
                }
                if let Some(vc) = &video.video_codec {
                    println!("  \x1b[36mVideo:\x1b[0m      {}", vc);
                }
                if let Some(ac) = &video.audio_codec {
                    println!("  \x1b[36mAudio:\x1b[0m      {}", ac);
                }
            }
        }
    }

    Ok(())
}
