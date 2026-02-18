use anyhow::{Result, anyhow};
use crate::pikpak::EntryKind;

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!("Usage: pikpaktui rm [-r] [-f] <file_path>"));
    }

    let mut force = false;
    let mut recursive = false;
    let mut paths: Vec<&str> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-f" => force = true,
            "-r" => recursive = true,
            "-rf" | "-fr" => { recursive = true; force = true; }
            _ => paths.push(arg),
        }
    }

    if paths.is_empty() {
        return Err(anyhow!("Usage: pikpaktui rm [-r] [-f] <file_path>"));
    }

    let client = super::cli_client()?;
    let path = paths[0];
    let (parent, name) = super::split_parent_name(path)?;
    let parent_id = client.resolve_path(&parent)?;
    let entry = super::find_entry(&client, &parent_id, &name)?;

    if entry.kind == EntryKind::Folder && !recursive {
        return Err(anyhow!(
            "'{}' is a folder. Use -r to remove folders.",
            path
        ));
    }

    if force {
        client.delete_permanent(&[entry.id.as_str()])?;
        println!("Permanently deleted '{}'", path);
    } else {
        client.remove(&[entry.id.as_str()])?;
        println!("Removed '{}' (to trash)", path);
    }
    Ok(())
}
