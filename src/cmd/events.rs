use anyhow::Result;

pub fn run(args: &[String]) -> Result<()> {
    let client = super::cli_client()?;

    let limit = args
        .first()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(20);

    let resp = client.events(limit)?;

    if resp.events.is_empty() {
        println!("No recent events");
        return Ok(());
    }

    for ev in &resp.events {
        let event_type = ev.event.as_deref().unwrap_or("unknown");
        let name = ev.file_name.as_deref().unwrap_or("?");
        let time = ev.created_time.as_deref().unwrap_or("");
        let kind = ev.file_kind.as_deref().unwrap_or("");
        let icon = if kind.contains("folder") {
            "📁"
        } else {
            "📄"
        };
        println!("{} {} {}  {}", icon, event_type, name, time);
    }

    Ok(())
}
