use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!("usage: pikpaktui info <path>"));
    }

    let path = &args[0];
    let client = super::cli_client()?;

    let (parent_path, name) = super::split_parent_name(path)?;
    let parent_id = client.resolve_path(&parent_path)?;
    let entry = super::find_entry(&client, &parent_id, &name)?;
    let info = client.file_info(&entry.id)?;

    println!("Name:     {}", info.name);

    if let Some(kind) = &info.kind {
        let display = if kind.contains("folder") {
            "folder"
        } else {
            "file"
        };
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

    // Video media info
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
