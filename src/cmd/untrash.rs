use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!("usage: pikpaktui untrash <name...>"));
    }

    let client = super::cli_client()?;
    let trash_entries = client.ls_trash(500)?;

    let mut ids = Vec::new();
    for name in args {
        let entry = trash_entries
            .iter()
            .find(|e| e.name == *name)
            .ok_or_else(|| anyhow!("'{}' not found in trash", name))?;
        ids.push(entry.id.clone());
    }

    let id_refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
    client.untrash(&id_refs)?;
    println!("Restored {} item(s) from trash", ids.len());

    Ok(())
}
