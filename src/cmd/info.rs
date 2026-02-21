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

    println!("Name:     {}", info.name);

    if let Some(kind) = &info.kind {
        let display = if kind.contains("folder") { "folder" } else { "file" };
        println!("Type:     {}", display);
    }

    if let Some(size) = &info.size {
        if let Ok(bytes) = size.parse::<u64>() {
            println!("Size:     {} ({})", super::format_size(bytes), size);
        } else {
            println!("Size:     {}", size);
        }
    }

    if let Some(hash) = &info.hash {
        println!("Hash:     {}", hash);
    }

    if let Some(mime) = &info.mime_type {
        println!("MIME:     {}", mime);
    }

    if let Some(created) = &info.created_time {
        println!("Created:  {}", created);
    }

    if let Some(medias) = &info.medias {
        for media in medias {
            if let Some(video) = &media.video {
                println!();
                println!("Media:    {}", media.media_name.as_deref().unwrap_or("-"));
                if let (Some(w), Some(h)) = (video.width, video.height) {
                    println!("  Resolution: {}x{}", w, h);
                }
                if let Some(dur) = video.duration {
                    let mins = (dur / 60.0) as u64;
                    let secs = (dur % 60.0) as u64;
                    println!("  Duration:   {}:{:02}", mins, secs);
                }
                if let Some(br) = video.bit_rate {
                    println!("  Bitrate:    {} kbps", br / 1000);
                }
                if let Some(vc) = &video.video_codec {
                    println!("  Video:      {}", vc);
                }
                if let Some(ac) = &video.audio_codec {
                    println!("  Audio:      {}", ac);
                }
            }
        }
    }

    Ok(())
}
