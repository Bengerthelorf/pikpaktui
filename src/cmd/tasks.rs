use anyhow::Result;

pub fn run(args: &[String]) -> Result<()> {
    let client = super::cli_client()?;

    // Sub-commands: list (default), retry <id>, delete <id...>
    let sub = args.first().map(|s| s.as_str()).unwrap_or("list");

    match sub {
        "list" | "ls" => {
            let mut limit = 50u32;
            let mut json = false;
            for a in &args[1..] {
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
            let resp = client.offline_list(limit, phases)?;

            if json {
                let out = serde_json::to_string_pretty(&resp.tasks).unwrap_or_else(|_| "[]".into());
                println!("{}", out);
                return Ok(());
            }

            if resp.tasks.is_empty() {
                println!("No offline tasks");
                return Ok(());
            }

            for t in &resp.tasks {
                let size = t
                    .file_size
                    .as_deref()
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(|n| super::format_size(n))
                    .unwrap_or_default();

                let icon = match t.phase.as_str() {
                    "PHASE_TYPE_COMPLETE" => "✓",
                    "PHASE_TYPE_RUNNING" => "↓",
                    "PHASE_TYPE_PENDING" => "…",
                    "PHASE_TYPE_ERROR" => "✗",
                    _ => "?",
                };

                let extra = if t.phase == "PHASE_TYPE_ERROR" {
                    t.message.as_deref().unwrap_or("")
                } else {
                    t.created_time.as_deref().unwrap_or("")
                };
                println!(
                    "{} {:>3}%  {:>10}  {}  {}  {}",
                    icon,
                    t.progress,
                    size,
                    &t.id[..8.min(t.id.len())],
                    t.name,
                    extra,
                );
            }

            Ok(())
        }
        "retry" => {
            let mut dry_run = false;
            let mut rest_args: Vec<&str> = Vec::new();
            for a in &args[1..] {
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
            for a in &args[1..] {
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
