use anyhow::{Result, anyhow};
use std::io::Write as _;

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!(
            "Usage: pikpaktui share [-p] [-d <days>] [-J] [-o <file>] <path...>"
        ));
    }

    let mut paths: Vec<&str> = Vec::new();
    let mut need_password = false;
    let mut expiration_days: i64 = -1;
    let mut output_file: Option<&str> = None;
    let mut json = false;
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-p" | "--password" => need_password = true,
            "-J" | "--json" => json = true,
            "-d" | "--days" => {
                let val = iter.next().ok_or_else(|| anyhow!("-d requires a number"))?;
                expiration_days = val
                    .parse::<i64>()
                    .map_err(|_| anyhow!("-d requires an integer"))?;
            }
            "-o" => {
                output_file = Some(
                    iter.next()
                        .ok_or_else(|| anyhow!("-o requires a file path"))?
                        .as_str(),
                );
            }
            _ => paths.push(arg),
        }
    }

    if paths.is_empty() {
        return Err(anyhow!("no path specified"));
    }

    let client = super::cli_client()?;

    let mut file_ids: Vec<String> = Vec::new();
    for path in &paths {
        let (parent, name) = super::split_parent_name(path)?;
        let parent_id = client.resolve_path(&parent)?;
        let entry = super::find_entry(&client, &parent_id, &name)?;
        file_ids.push(entry.id);
    }

    let id_refs: Vec<&str> = file_ids.iter().map(|s| s.as_str()).collect();
    let result = client.create_share(&id_refs, need_password, expiration_days)?;

    if json {
        let out = serde_json::json!({
            "share_id": result.share_id,
            "share_url": result.share_url,
            "pass_code": if result.pass_code.is_empty() { None } else { Some(&result.pass_code) },
            "share_text": if result.share_text.is_empty() { None } else { Some(&result.share_text) },
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("{}", result.share_url);
        if !result.pass_code.is_empty() {
            println!("Password: {}", result.pass_code);
        }
        if let Some(out_path) = output_file {
            let mut f = std::fs::File::create(out_path)
                .map_err(|e| anyhow!("cannot create '{}': {}", out_path, e))?;
            writeln!(f, "{}", result.share_url)?;
            if !result.pass_code.is_empty() {
                writeln!(f, "Password: {}", result.pass_code)?;
            }
            eprintln!("Written to '{}'", out_path);
        }
    }

    Ok(())
}
