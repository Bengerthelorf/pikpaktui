use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!(
            "usage: pikpaktui offline [--dry-run] <url> [--to <path>] [--name <name>]"
        ));
    }

    let client = super::cli_client()?;

    let file_url = &args[0];
    let mut parent_path: Option<&str> = None;
    let mut name: Option<&str> = None;
    let mut dry_run = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--to" | "-t" => {
                i += 1;
                parent_path = args.get(i).map(|s| s.as_str());
            }
            "--name" | "-n" => {
                i += 1;
                name = args.get(i).map(|s| s.as_str());
            }
            "--dry-run" => dry_run = true,
            _ => {}
        }
        i += 1;
    }

    let parent_id = match parent_path {
        Some(p) => Some(client.resolve_path(p)?),
        None => None,
    };

    if dry_run {
        let dest_display = parent_path.unwrap_or("/");
        print!("[dry-run] Would submit offline download: '{}'", file_url);
        if let Some(n) = name {
            print!(" as '{}'", n);
        }
        println!(" -> '{}'", dest_display);
        if let Some(id) = &parent_id {
            println!("  parent id: {}", id);
        }
        return Ok(());
    }

    let resp = client.offline_download(file_url, parent_id.as_deref(), name)?;
    if let Some(task) = &resp.task {
        println!("Offline task created: {}", task.name);
        println!("  ID:    {}", task.id);
        println!("  Phase: {}", task.phase);
        if let Some(fid) = &task.file_id {
            println!("  File:  {}", fid);
        }
    } else {
        println!("Offline download submitted");
    }

    Ok(())
}
