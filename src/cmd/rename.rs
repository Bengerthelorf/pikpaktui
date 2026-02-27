use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    let mut dry_run = false;
    let mut rest: Vec<&str> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-n" | "--dry-run" => dry_run = true,
            _ => rest.push(arg),
        }
    }

    if rest.len() < 2 {
        return Err(anyhow!("Usage: pikpaktui rename [-n] <file_path> <new_name>"));
    }

    let client = super::cli_client()?;
    let (parent, name) = super::split_parent_name(rest[0])?;
    let parent_id = client.resolve_path(&parent)?;
    let entry = super::find_entry(&client, &parent_id, &name)?;

    if dry_run {
        println!("[dry-run] Would rename '{}' -> '{}' (id: {})", name, rest[1], entry.id);
        return Ok(());
    }

    client.rename(&entry.id, rest[1])?;
    println!("Renamed '{}' -> '{}'", name, rest[1]);
    Ok(())
}
