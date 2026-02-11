use anyhow::Result;

pub fn run() -> Result<()> {
    let client = super::cli_client()?;
    let quota = client.quota()?;

    if let Some(detail) = quota.quota {
        let limit = detail.limit.as_deref().unwrap_or("unknown");
        let usage = detail.usage.as_deref().unwrap_or("0");
        let trash = detail.usage_in_trash.as_deref().unwrap_or("0");

        let limit_n: u64 = limit.parse().unwrap_or(0);
        let usage_n: u64 = usage.parse().unwrap_or(0);

        println!("Quota:  {}", super::format_size(limit_n));
        println!("Used:   {}", super::format_size(usage_n));
        println!("Trash:  {}", super::format_size(trash.parse().unwrap_or(0)));
        if limit_n > 0 {
            println!("Free:   {}", super::format_size(limit_n.saturating_sub(usage_n)));
        }
    } else {
        println!("No quota info available");
    }
    Ok(())
}
