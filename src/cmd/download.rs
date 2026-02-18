use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!(
            "Usage: pikpaktui download [-o <output>] <file_path>"
        ));
    }

    let mut output: Option<&str> = None;
    let mut paths: Vec<&str> = Vec::new();
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-o" => {
                output = Some(
                    iter.next()
                        .ok_or_else(|| anyhow!("-o requires an output path"))?
                        .as_str(),
                );
            }
            _ => paths.push(arg),
        }
    }

    if paths.is_empty() {
        return Err(anyhow!(
            "Usage: pikpaktui download [-o <output>] <file_path>"
        ));
    }

    let client = super::cli_client()?;
    let (parent, name) = super::split_parent_name(paths[0])?;
    let parent_id = client.resolve_path(&parent)?;
    let entry = super::find_entry(&client, &parent_id, &name)?;

    let dest = std::path::PathBuf::from(
        output.unwrap_or_else(|| paths.get(1).map(|s| s.as_ref()).unwrap_or(&name)),
    );

    let total = client.download_to(&entry.id, &dest)?;
    println!(
        "Downloaded '{}' -> '{}' ({})",
        name,
        dest.display(),
        super::format_size(total)
    );
    Ok(())
}
