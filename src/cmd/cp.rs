use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(anyhow!("Usage: pikpaktui cp <source_path> <dest_folder_path>"));
    }
    let client = super::cli_client()?;
    let (src_parent, src_name) = super::split_parent_name(&args[0])?;
    let src_parent_id = client.resolve_path(&src_parent)?;
    let entry = super::find_entry(&client, &src_parent_id, &src_name)?;
    let dest_id = client.resolve_path(&args[1])?;
    client.cp(&[entry.id.as_str()], &dest_id)?;
    println!("Copied '{}' -> '{}'", args[0], args[1]);
    Ok(())
}
