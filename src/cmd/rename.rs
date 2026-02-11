use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(anyhow!("Usage: pikpaktui rename <file_path> <new_name>"));
    }
    let client = super::cli_client()?;
    let (parent, name) = super::split_parent_name(&args[0])?;
    let parent_id = client.resolve_path(&parent)?;
    let entry = super::find_entry(&client, &parent_id, &name)?;
    client.rename(&entry.id, &args[1])?;
    println!("Renamed '{}' -> '{}'", name, args[1]);
    Ok(())
}
