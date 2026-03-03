use anyhow::{Result, anyhow};

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!(
            "Usage: pikpaktui share-info [-p <code>] [-J] <url_or_id>"
        ));
    }

    let mut share_arg: Option<&str> = None;
    let mut pass_code = "";
    let mut json = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-J" | "--json" => json = true,
            "-p" | "--pass-code" => {
                i += 1;
                if i >= args.len() {
                    return Err(anyhow!("--pass-code requires a value"));
                }
                pass_code = &args[i];
            }
            arg => {
                if share_arg.is_none() {
                    share_arg = Some(arg);
                } else {
                    return Err(anyhow!("unexpected argument: {}", arg));
                }
            }
        }
        i += 1;
    }

    let share_arg = share_arg.ok_or_else(|| anyhow!("no share URL or ID provided"))?;

    let share_id = if share_arg.contains("mypikpak.com/s/") {
        let trimmed = share_arg.trim_end_matches('/');
        trimmed.rsplit('/').next().unwrap_or(trimmed)
    } else {
        share_arg
    };

    let client = super::cli_client()?;
    let info = client.share_info(share_id, pass_code)?;

    if json {
        let out = serde_json::json!({
            "share_id": share_id,
            "file_count": info.files.len(),
            "files": info.files.iter().map(|f| serde_json::json!({
                "id": f.id,
                "name": f.name,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("Share '{}' — {} item(s):", share_id, info.files.len());
        for f in &info.files {
            println!("  {}", f.name);
        }
    }

    Ok(())
}
