use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!("Usage: pikpaktui rm <file_path>"));
    }
    let client = super::cli_client()?;
    let (parent, name) = super::split_parent_name(&args[0])?;
    let parent_id = client.resolve_path(&parent)?;
    let entry = super::find_entry(&client, &parent_id, &name)?;
    client.remove(&[entry.id.as_str()])?;
    println!("Removed '{}' (to trash)", args[0]);
    Ok(())
}
