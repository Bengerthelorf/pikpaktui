use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!("usage: pikpaktui cat <path>"));
    }

    let path = &args[0];
    let client = super::cli_client()?;
    let config = super::cli_config();

    let (parent_path, name) = super::split_parent_name(path)?;
    let parent_id = client.resolve_path(&parent_path)?;
    let entry = super::find_entry(&client, &parent_id, &name)?;

    let max_bytes = config.preview_max_size;
    let (_name, content, _file_size, truncated) =
        client.fetch_text_preview(&entry.id, max_bytes)?;

    print!("{}", content);
    if truncated {
        eprintln!("\n(truncated at {} bytes)", max_bytes);
    }

    Ok(())
}
