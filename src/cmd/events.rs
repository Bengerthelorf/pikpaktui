use anyhow::Result;
use unicode_width::UnicodeWidthStr;

pub fn run(args: &[String]) -> Result<()> {
    let client = super::cli_client()?;
    let config = super::cli_config();
    let nerd_font = config.cli_nerd_font;

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

    let spinner = super::Spinner::new("Fetching events...");
    let resp = client.events(limit)?;
    drop(spinner);

    if json {
        let out = serde_json::to_string_pretty(&resp.events).unwrap_or_else(|_| "[]".into());
        println!("{}", out);
        return Ok(());
    }

    if resp.events.is_empty() {
        println!("No recent events");
        return Ok(());
    }

    struct Row {
        event: String,
        event_color: &'static str,
        name: String,
        kind_icon: &'static str,
        date: String,
    }

    let rows: Vec<Row> = resp
        .events
        .iter()
        .map(|ev| {
            // API returns "TYPE_RESTORE", "TYPE_DELETE", etc. — use type_name for display
            let raw_type = ev.event_type.as_deref().unwrap_or("");
            let display = ev.type_name.as_deref().unwrap_or(raw_type);
            let event = if display.is_empty() { raw_type.to_string() } else { display.to_string() };
            let event_color = match raw_type {
                t if t.contains("CREATE") || t.contains("UPLOAD") || t.contains("RESTORE") => "32",
                t if t.contains("DELETE") || t.contains("TRASH") => "31",
                t if t.contains("RENAME") || t.contains("MOVE") || t.contains("COPY") => "33",
                _ => "33",
            };
            let name = ev.file_name.as_deref().unwrap_or("?").to_string();
            let is_folder = ev.reference_resource.as_ref()
                .and_then(|r| r.kind.as_deref())
                .is_some_and(|k| k.contains("folder"));
            let kind_icon = if is_folder {
                if nerd_font { "\u{f07b} " } else { "[D]" }
            } else if nerd_font { "\u{f15b} " } else { "[F]" };
            let date = super::format_date(ev.created_time.as_deref().unwrap_or(""));
            Row { event, event_color, name, kind_icon, date }
        })
        .collect();

    // Compute column widths
    let w_event = rows.iter().map(|r| r.event.len()).max().unwrap_or(5).max(5);
    let w_icon = rows.iter().map(|r| UnicodeWidthStr::width(r.kind_icon)).max().unwrap_or(3).max(3);
    let w_name = rows.iter().map(|r| UnicodeWidthStr::width(r.name.as_str())).max().unwrap_or(4).max(4);
    let w_date = rows.iter().map(|r| r.date.len()).max().unwrap_or(7).max(7);

    // Clamp name to terminal width
    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(120);
    let fixed = w_event + 2 + w_icon + 2 + w_date + 8;
    let w_name = w_name.min(term_width.saturating_sub(fixed).max(12));

    // Dim header
    println!(
        "\x1b[2m{:<w_event$}  {:<w_icon$}  {:<w_name$}  TIME\x1b[0m",
        "EVENT", "", "NAME",
    );

    for r in &rows {
        let name = truncate(&r.name, w_name);
        println!(
            "\x1b[{ec}m{event:<w_event$}\x1b[0m  {icon:<w_icon$}  {name:<w_name$}  {date}",
            ec = r.event_color,
            event = r.event,
            icon = r.kind_icon,
            name = name,
            date = r.date,
        );
    }

    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if UnicodeWidthStr::width(s) <= max {
        s.to_string()
    } else {
        let mut w = 0;
        let mut out = String::new();
        for ch in s.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if w + cw + 1 > max {
                break;
            }
            out.push(ch);
            w += cw;
        }
        out.push('…');
        out
    }
}
