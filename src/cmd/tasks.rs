use anyhow::Result;
use unicode_width::UnicodeWidthStr;

pub fn run(args: &[String]) -> Result<()> {
    let client = super::cli_client()?;

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

            struct Row {
                icon: &'static str,
                color: &'static str,
                progress: String,
                name: String,
                size: String,
                id: String,
                last: String,
            }

            let rows: Vec<Row> = resp
                .tasks
                .iter()
                .map(|t| {
                    let (icon, color) = match t.phase.as_str() {
                        "PHASE_TYPE_COMPLETE" => ("✓", "32"),
                        "PHASE_TYPE_RUNNING" => ("↓", "36"),
                        "PHASE_TYPE_PENDING" => ("…", "2;37"),
                        "PHASE_TYPE_ERROR" => ("✗", "31"),
                        _ => ("?", "33"),
                    };
                    let progress = if t.phase == "PHASE_TYPE_RUNNING" {
                        format!("{}%", t.progress)
                    } else {
                        String::new()
                    };
                    let size = t
                        .file_size
                        .as_deref()
                        .and_then(|s| s.parse::<u64>().ok())
                        .map(super::format_size)
                        .unwrap_or_default();
                    let id = t.id[..8.min(t.id.len())].to_string();
                    let last = if t.phase == "PHASE_TYPE_ERROR" {
                        t.message.as_deref().unwrap_or("").to_string()
                    } else {
                        super::format_date(t.created_time.as_deref().unwrap_or(""))
                    };
                    Row { icon, color, progress, name: t.name.clone(), size, id, last }
                })
                .collect();

            let w_name = rows.iter().map(|r| UnicodeWidthStr::width(r.name.as_str())).max().unwrap_or(4).max(4);
            let w_prog = rows.iter().map(|r| r.progress.len()).max().unwrap_or(0).max(4);
            let w_size = rows.iter().map(|r| r.size.len()).max().unwrap_or(4).max(4);
            let w_id = 8usize;
            let w_last = rows.iter().map(|r| UnicodeWidthStr::width(r.last.as_str())).max().unwrap_or(7).max(7);

            let term_width = crossterm::terminal::size()
                .map(|(w, _)| w as usize)
                .unwrap_or(120);
            let fixed = 8 + w_prog + 2 + w_size + 2 + w_id + 2 + w_last + 8;
            let w_name = w_name.min(term_width.saturating_sub(fixed).max(12));

            println!(
                "\x1b[2mSTATUS  {:<w_prog$}  {:<w_name$}  {:>w_size$}  {:>w_id$}  CREATED\x1b[0m",
                "PROGRESS", "NAME", "SIZE", "ID",
            );

            for r in &rows {
                let name = super::truncate(&r.name, w_name);
                println!(
                    "\x1b[{color}m{icon}\x1b[0m       {:<w_prog$}  {:<w_name$}  {:>w_size$}  {:>w_id$}  {}",
                    r.progress,
                    name,
                    r.size,
                    r.id,
                    r.last,
                    color = r.color,
                    icon = r.icon,
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

