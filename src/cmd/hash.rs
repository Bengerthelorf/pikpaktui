use anyhow::{Context, Result, bail};
use std::{fs, path::Path};

pub fn run(args: &[String]) -> Result<()> {
    let mut json = false;
    let mut path = None;
    let mut positional_only = false;
    for arg in args {
        if !positional_only {
            match arg.as_str() {
                "--" => {
                    positional_only = true;
                    continue;
                }
                "-J" | "--json" => {
                    json = true;
                    continue;
                }
                option if option.starts_with('-') => {
                    bail!(
                        "hash: unknown option '{option}'\nUsage: pikpaktui hash [-J|--json] <local-file>"
                    );
                }
                _ => {}
            }
        }
        if path.replace(arg).is_some() {
            bail!(
                "hash requires exactly one local file\nUsage: pikpaktui hash [-J|--json] <local-file>"
            );
        }
    }
    let path = path.context(
        "hash requires exactly one local file\nUsage: pikpaktui hash [-J|--json] <local-file>",
    )?;
    let metadata = fs::metadata(path).with_context(|| format!("cannot stat '{path}'"))?;
    if !metadata.is_file() {
        bail!("hash requires a regular file: '{path}'");
    }
    let hash = crate::pikpak::pikpak_hash(Path::new(path))?;
    if json {
        println!(
            "{}",
            serde_json::json!({"path": path, "algorithm": "pikpak-gcid", "hash": hash})
        );
    } else {
        println!("{hash}");
    }
    Ok(())
}
