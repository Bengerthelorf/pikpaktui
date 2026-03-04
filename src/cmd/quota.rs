use anyhow::Result;

pub fn run(args: &[String]) -> Result<()> {
    let json = args.iter().any(|a| a == "-J" || a == "--json");

    let client = super::cli_client()?;
    let quota = client.quota()?;
    let tq = client.transfer_quota().ok();

    if json {
        let storage = quota.quota.as_ref().map(|d| {
            let limit = d.limit.as_deref().unwrap_or("0").parse::<u64>().unwrap_or(0);
            let used  = d.usage.as_deref().unwrap_or("0").parse::<u64>().unwrap_or(0);
            let trash = d.usage_in_trash.as_deref().unwrap_or("0").parse::<u64>().unwrap_or(0);
            serde_json::json!({
                "limit": limit,
                "used":  used,
                "trash": trash,
                "free":  limit.saturating_sub(used),
            })
        });

        let bandwidth = tq.as_ref().and_then(|t| t.base.as_ref()).map(|b| {
            let band = |slot: Option<&crate::pikpak::TransferBand>| {
                slot.map(|s| serde_json::json!({
                    "used":  s.assets.unwrap_or(0),
                    "total": s.total_assets.unwrap_or(0),
                }))
            };
            serde_json::json!({
                "download":    band(b.download.as_ref()),
                "upload":      band(b.upload.as_ref()),
                "offline":     band(b.offline.as_ref()),
                "expire_time": b.expire_time,
            })
        });

        let out = serde_json::json!({
            "storage":   storage,
            "bandwidth": bandwidth,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    if let Some(detail) = quota.quota {
        let limit_n: u64 = detail.limit.as_deref().unwrap_or("0").parse().unwrap_or(0);
        let usage_n: u64 = detail.usage.as_deref().unwrap_or("0").parse().unwrap_or(0);
        let trash_n: u64 = detail.usage_in_trash.as_deref().unwrap_or("0").parse().unwrap_or(0);

        println!("\x1b[1mStorage\x1b[0m");
        println!("  \x1b[36mQuota:\x1b[0m     {}", super::format_size(limit_n));
        if limit_n > 0 {
            let pct = (usage_n as f64 / limit_n as f64 * 100.0) as u64;
            let bar = usage_bar(pct, 20);
            println!("  \x1b[36mUsed:\x1b[0m      {}  {} {:>3}%", super::format_size(usage_n), bar, pct);
        } else {
            println!("  \x1b[36mUsed:\x1b[0m      {}", super::format_size(usage_n));
        }
        println!("  \x1b[36mTrash:\x1b[0m     {}", super::format_size(trash_n));
        if limit_n > 0 {
            println!("  \x1b[36mFree:\x1b[0m      {}", super::format_size(limit_n.saturating_sub(usage_n)));
        }
    } else {
        println!("No quota info available");
    }

    if let Some(base) = tq.and_then(|t| t.base) {
        println!("\x1b[1mBandwidth\x1b[0m");
        if let Some(ref exp) = base.expire_time {
            let date = super::format_date(exp);
            println!("  \x1b[36mExpires:\x1b[0m   \x1b[34m{}\x1b[0m", date);
        }
        if let Some(dl) = base.download {
            let total = dl.total_assets.unwrap_or(0);
            let used  = dl.assets.unwrap_or(0);
            if total > 0 {
                println!("  \x1b[36mDownload:\x1b[0m  {} / {} used", super::format_size(used), super::format_size(total));
            }
        }
        if let Some(ul) = base.upload {
            let total = ul.total_assets.unwrap_or(0);
            let used  = ul.assets.unwrap_or(0);
            if total > 0 {
                println!("  \x1b[36mUpload:\x1b[0m    {} / {} used", super::format_size(used), super::format_size(total));
            }
        }
        if let Some(of) = base.offline {
            let total = of.total_assets.unwrap_or(0);
            let used  = of.assets.unwrap_or(0);
            if total > 0 {
                println!("  \x1b[36mOffline:\x1b[0m   {} / {} used", super::format_size(used), super::format_size(total));
            }
        }
    }

    Ok(())
}

fn usage_bar(pct: u64, width: usize) -> String {
    let filled = (pct as usize * width / 100).min(width);
    let empty = width.saturating_sub(filled);
    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));
    let color = if pct >= 90 {
        "31" // red
    } else if pct >= 70 {
        "33" // yellow
    } else {
        "32" // green
    };
    format!("\x1b[{}m{}\x1b[0m", color, bar)
}
