use anyhow::Result;

pub fn run() -> Result<()> {
    let client = super::cli_client()?;
    let resp = client.vip_info()?;

    if let Some(data) = resp.data {
        let vip_type = data.vip_type.as_deref().unwrap_or("none");
        let status = data.status.as_deref().unwrap_or("unknown");
        let expire = data.expire.as_deref().unwrap_or("N/A");

        println!("VIP Type:   {}", vip_type);
        println!("Status:     {}", status);
        println!("Expires:    {}", expire);
    } else {
        println!("No VIP info available");
    }

    if let Ok(code) = client.invite_code() { println!("Invite Code: {}", code) }

    if let Ok(tq) = client.transfer_quota()
        && let Some(base) = tq.base {
            let fmt = |used: u64, total: u64| -> String {
                format!("{} / {} used", super::format_size(used), super::format_size(total))
            };
            if let Some(dl) = base.download {
                let total = dl.total_assets.unwrap_or(0);
                if total > 0 {
                    println!("Download BW: {}", fmt(dl.assets.unwrap_or(0), total));
                }
            }
            if let Some(ul) = base.upload {
                let total = ul.total_assets.unwrap_or(0);
                if total > 0 {
                    println!("Upload BW:   {}", fmt(ul.assets.unwrap_or(0), total));
                }
            }
            if let Some(of) = base.offline {
                let total = of.total_assets.unwrap_or(0);
                if total > 0 {
                    println!("Offline BW:  {}", fmt(of.assets.unwrap_or(0), total));
                }
            }
        }

    Ok(())
}
