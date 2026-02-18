use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(anyhow!(
            "Usage: pikpaktui cp <src> <dst>\n       pikpaktui cp -t <dst> <src...>"
        ));
    }

    let mut target: Option<&str> = None;
    let mut paths: Vec<&str> = Vec::new();
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-t" => {
                target = Some(
                    iter.next()
                        .ok_or_else(|| anyhow!("-t requires a destination path"))?
                        .as_str(),
                );
            }
            _ => paths.push(arg),
        }
    }

    let client = super::cli_client()?;

    if let Some(dst) = target {
        if paths.is_empty() {
            return Err(anyhow!("Usage: pikpaktui cp -t <dst> <src...>"));
        }
        let dest_id = client.resolve_path(dst)?;
        let mut ids: Vec<String> = Vec::new();
        for path in &paths {
            let (parent, name) = super::split_parent_name(path)?;
            let parent_id = client.resolve_path(&parent)?;
            let entry = super::find_entry(&client, &parent_id, &name)?;
            ids.push(entry.id);
        }
        let id_refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
        client.cp(&id_refs, &dest_id)?;
        println!("Copied {} item(s) -> '{}'", paths.len(), dst);
    } else {
        if paths.len() < 2 {
            return Err(anyhow!("Usage: pikpaktui cp <src> <dst>"));
        }
        let (src_parent, src_name) = super::split_parent_name(paths[0])?;
        let src_parent_id = client.resolve_path(&src_parent)?;
        let entry = super::find_entry(&client, &src_parent_id, &src_name)?;
        let dest_id = client.resolve_path(paths[1])?;
        client.cp(&[entry.id.as_str()], &dest_id)?;
        println!("Copied '{}' -> '{}'", paths[0], paths[1]);
    }
    Ok(())
}
