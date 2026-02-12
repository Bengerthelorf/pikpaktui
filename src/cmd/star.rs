use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!("usage: pikpaktui star <path...>"));
    }

    let client = super::cli_client()?;

    let mut ids = Vec::new();
    for path in args {
        let (parent_path, name) = super::split_parent_name(path)?;
        let parent_id = client.resolve_path(&parent_path)?;
        let entry = super::find_entry(&client, &parent_id, &name)?;
        ids.push(entry.id);
    }

    let id_refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
    client.star(&id_refs)?;
    println!("Starred {} item(s)", ids.len());

    Ok(())
}
