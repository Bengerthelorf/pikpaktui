use anyhow::Result;

pub fn run(args: &[String]) -> Result<()> {
    let client = super::cli_client()?;

    // Sub-commands: list (default), retry <id>, delete <id...>
    let sub = args.first().map(|s| s.as_str()).unwrap_or("list");

    match sub {
        "list" | "ls" => {
            let limit = args
                .get(1)
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(50);

            let phases = &[
                "PHASE_TYPE_RUNNING",
                "PHASE_TYPE_PENDING",
                "PHASE_TYPE_COMPLETE",
                "PHASE_TYPE_ERROR",
            ];
            let resp = client.offline_list(limit, phases)?;

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
            let task_id = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("usage: pikpaktui tasks retry <task_id>"))?;
            client.offline_task_retry(task_id)?;
            println!("Task {} retried", task_id);
            Ok(())
        }
        "delete" | "rm" => {
            let ids: Vec<&str> = args[1..].iter().map(|s| s.as_str()).collect();
            if ids.is_empty() {
                return Err(anyhow::anyhow!(
                    "usage: pikpaktui tasks delete <task_id...>"
                ));
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
