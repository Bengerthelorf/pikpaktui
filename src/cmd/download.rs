use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!(
            "Usage: pikpaktui download <file_path> [local_path]"
        ));
    }
    let client = super::cli_client()?;
    let (parent, name) = super::split_parent_name(&args[0])?;
    let parent_id = client.resolve_path(&parent)?;
    let entry = super::find_entry(&client, &parent_id, &name)?;

    let dest = if args.len() > 1 {
        std::path::PathBuf::from(&args[1])
    } else {
        std::path::PathBuf::from(&name)
    };

    let total = client.download_to(&entry.id, &dest)?;
    println!(
        "Downloaded '{}' -> '{}' ({})",
        name,
        dest.display(),
        super::format_size(total)
    );
    Ok(())
}
