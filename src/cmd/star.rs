use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!("usage: pikpaktui star [-n] <path...>"));
    }

    let mut dry_run = false;
    let mut paths: Vec<&str> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-n" | "--dry-run" => dry_run = true,
            _ => paths.push(arg),
        }
    }

    if paths.is_empty() {
        return Err(anyhow!("usage: pikpaktui star [-n] <path...>"));
    }

    let client = super::cli_client()?;

    let mut resolved: Vec<(&str, String)> = Vec::new();
    for path in &paths {
        let (parent_path, name) = super::split_parent_name(path)?;
        let parent_id = client.resolve_path(&parent_path)?;
        let entry = super::find_entry(&client, &parent_id, &name)?;
        resolved.push((path, entry.id));
    }

    if dry_run {
        println!("[dry-run] Would star {} item(s):", resolved.len());
        for (path, id) in &resolved {
            println!("  {} (id: {})", path, id);
        }
        return Ok(());
    }

    let id_refs: Vec<&str> = resolved.iter().map(|(_, id)| id.as_str()).collect();
    client.star(&id_refs)?;
    println!("Starred {} item(s)", resolved.len());

    Ok(())
}
