use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!("Usage: pikpaktui rm [-f] <file_path>"));
    }

    let (force, path) = if args[0] == "-f" {
        if args.len() < 2 {
            return Err(anyhow!("Usage: pikpaktui rm -f <file_path>"));
        }
        (true, &args[1])
    } else {
        (false, &args[0])
    };

    let client = super::cli_client()?;
    let (parent, name) = super::split_parent_name(path)?;
    let parent_id = client.resolve_path(&parent)?;
    let entry = super::find_entry(&client, &parent_id, &name)?;

    if force {
        client.delete_permanent(&[entry.id.as_str()])?;
        println!("Permanently deleted '{}'", path);
    } else {
        client.remove(&[entry.id.as_str()])?;
        println!("Removed '{}' (to trash)", path);
    }
    Ok(())
}
