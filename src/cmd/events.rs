use anyhow::Result;

pub fn run(args: &[String]) -> Result<()> {
    let client = super::cli_client()?;

    let mut json = false;
    let mut limit = 20u32;

    for arg in args {
        match arg.as_str() {
            "-J" | "--json" => json = true,
            _ => {
                if let Ok(n) = arg.parse::<u32>() {
                    limit = n;
                }
            }
        }
    }

    let resp = client.events(limit)?;

    if json {
        let out = serde_json::to_string_pretty(&resp.events).unwrap_or_else(|_| "[]".into());
        println!("{}", out);
        return Ok(());
    }

    if resp.events.is_empty() {
        println!("No recent events");
        return Ok(());
    }

    for ev in &resp.events {
        let event_type = ev.event.as_deref().unwrap_or("unknown");
        let name = ev.file_name.as_deref().unwrap_or("?");
        let time = ev.created_time.as_deref().unwrap_or("");
        let kind = ev.file_kind.as_deref().unwrap_or("");

        let is_folder = kind.contains("folder");
        let colored_name = if is_folder {
            format!("\x1b[1;34m{}\x1b[0m", name)
        } else {
            name.to_string()
        };

        let date = super::format_date(time);
        let colored_date = format!("\x1b[34m{:16}\x1b[0m", date);
        let colored_event = format!("\x1b[33m{:12}\x1b[0m", event_type);

        println!("{}  {}  {}", colored_event, colored_date, colored_name);
    }

    Ok(())
}
