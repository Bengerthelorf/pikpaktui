use anyhow::Result;

pub fn run(args: &[String]) -> Result<()> {
    let client = super::cli_client()?;

    // Sub-commands: list (default), retry <id>, delete <id...>
    let sub = args.first().map(|s| s.as_str()).unwrap_or("list");
    let rest = if args.is_empty() { &[][..] } else { &args[1..] };

    match sub {
        "list" | "ls" => {
            let mut limit = 50u32;
            let mut json = false;
            for a in rest {
                match a.as_str() {
                    "-J" | "--json" => json = true,
                    _ => {
                        if let Ok(n) = a.parse::<u32>() {
                            limit = n;
                        }
                    }
                }
            }

            let phases = &[
                "PHASE_TYPE_RUNNING",
                "PHASE_TYPE_PENDING",
                "PHASE_TYPE_COMPLETE",
                "PHASE_TYPE_ERROR",
            ];

            let spinner = super::Spinner::new("Fetching tasks...");
            let resp = client.offline_list(limit, phases)?;
            drop(spinner);

            if json {
                let out = serde_json::to_string_pretty(&resp.tasks).unwrap_or_else(|_| "[]".into());
                println!("{}", out);
                return Ok(());
            }

            if resp.tasks.is_empty() {
                println!("No offline tasks");
                return Ok(());
            }

            let term_width = crossterm::terminal::size()
                .map(|(w, _)| w as usize)
                .unwrap_or(80);
            // NAME column gets remaining space after fixed columns
            // STATUS(6) + SIZE(10) + ID(10) + DATE(18) + gaps(8) = ~52
            let name_max = term_width.saturating_sub(52).max(16);

            // Header
            println!(
                "\x1b[1mSTATUS  {:<name_max$}  {:>10}  {:>8}  {}\x1b[0m",
                "NAME", "SIZE", "ID", "CREATED",
                name_max = name_max,
            );

            for t in &resp.tasks {
                let size = t
                    .file_size
                    .as_deref()
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(|n| super::format_size(n))
                    .unwrap_or("-".to_string());

                let (icon, color) = match t.phase.as_str() {
                    "PHASE_TYPE_COMPLETE" => ("✓", "32"),  // green
                    "PHASE_TYPE_RUNNING" => ("↓", "36"),   // cyan
                    "PHASE_TYPE_PENDING" => ("…", "2;37"), // dim
                    "PHASE_TYPE_ERROR" => ("✗", "31"),     // red
                    _ => ("?", "33"),                       // yellow
                };

                // Status column: icon + optional progress for non-complete
                let status = if t.phase == "PHASE_TYPE_RUNNING" {
                    format!("\x1b[{}m{}\x1b[0m {:>3}%", color, icon, t.progress)
                } else if t.phase == "PHASE_TYPE_COMPLETE" {
                    format!("\x1b[{}m{}\x1b[0m     ", color, icon)
                } else {
                    format!("\x1b[{}m{}\x1b[0m     ", color, icon)
                };

                let name = truncate_name(&t.name, name_max);
                let id_short = &t.id[..8.min(t.id.len())];

                let last_col = if t.phase == "PHASE_TYPE_ERROR" {
                    let msg = t.message.as_deref().unwrap_or("");
                    format!("\x1b[31m{}\x1b[0m", truncate_name(msg, 20))
                } else {
                    let date = t.created_time.as_deref().unwrap_or("");
                    format!("\x1b[34m{}\x1b[0m", super::format_date(date))
                };

                println!(
                    "{}  {:<name_max$}  {:>10}  \x1b[2m{:>8}\x1b[0m  {}",
                    status,
                    name,
                    size,
                    id_short,
                    last_col,
                    name_max = name_max,
                );
            }

            Ok(())
        }
        "retry" => {
            let mut dry_run = false;
            let mut rest_args: Vec<&str> = Vec::new();
            for a in rest {
                match a.as_str() {
                    "-n" | "--dry-run" => dry_run = true,
                    _ => rest_args.push(a),
                }
            }
            let task_id = rest_args
                .first()
                .copied()
                .ok_or_else(|| anyhow::anyhow!("usage: pikpaktui tasks retry [-n] <task_id>"))?;
            if dry_run {
                println!("[dry-run] Would retry task '{}'", task_id);
                return Ok(());
            }
            client.offline_task_retry(task_id)?;
            println!("Task {} retried", task_id);
            Ok(())
        }
        "delete" | "rm" => {
            let mut dry_run = false;
            let mut ids: Vec<&str> = Vec::new();
            for a in rest {
                match a.as_str() {
                    "-n" | "--dry-run" => dry_run = true,
                    _ => ids.push(a),
                }
            }
            if ids.is_empty() {
                return Err(anyhow::anyhow!(
                    "usage: pikpaktui tasks delete [-n] <task_id...>"
                ));
            }
            if dry_run {
                println!("[dry-run] Would delete {} task(s):", ids.len());
                for id in &ids {
                    println!("  {}", id);
                }
                return Ok(());
            }
            client.delete_tasks(&ids, false)?;
            println!("Deleted {} task(s)", ids.len());
            Ok(())
        }
        _ => Err(anyhow::anyhow!(
            "unknown tasks sub-command: {sub}\nUsage: pikpaktui tasks [list|retry|delete]"
        )),
    }
}

fn truncate_name(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", truncated)
    }
}
