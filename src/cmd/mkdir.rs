use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(anyhow!("Usage: pikpaktui mkdir <parent_path> <folder_name>"));
    }
    let client = super::cli_client()?;
    let parent_id = client.resolve_path(&args[0])?;
    let created = client.mkdir(&parent_id, &args[1])?;
    println!("Created folder '{}' (id={})", created.name, created.id);
    Ok(())
}
