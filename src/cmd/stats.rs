use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, bail, ensure};

use crate::pikpak::PikPak;

const USAGE: &str =
    "Usage: pikpaktui stats [-J|--json] [--cached|--refresh] [--folder-id ID | PATH]";

pub fn run(args: &[String]) -> Result<()> {
    let mut path = None;
    let mut folder_id = None;
    let mut cached = false;
    let mut refresh = false;
    let mut json = false;
    let mut positional = false;
    let mut args = args.iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--" if !positional => positional = true,
            "-J" | "--json" if !positional => json = true,
            "--cached" if !positional => cached = true,
            "--refresh" if !positional => refresh = true,
            s if !positional && (s == "--folder-id" || s.starts_with("--folder-id=")) => {
                let id = match s.strip_prefix("--folder-id=") {
                    Some(id) => id,
                    None => args
                        .next()
                        .filter(|s| !s.starts_with('-'))
                        .context("--folder-id requires an ID")?,
                };
                ensure!(
                    folder_id.replace(id).is_none(),
                    "provide only one folder ID"
                );
                ensure!(
                    id != "*",
                    "use / or omit the target to query the root folder"
                );
            }
            s if !positional && s.starts_with('-') => bail!("unknown option: {s}\n{USAGE}"),
            s => ensure!(path.replace(s).is_none(), "{USAGE}"),
        }
    }
    ensure!(
        folder_id.is_none() || path.is_none(),
        "choose either a folder path or --folder-id\n{USAGE}"
    );
    ensure!(
        !(cached && refresh),
        "--cached and --refresh cannot be combined"
    );
    ensure!(
        !cached || path.is_none_or(|s| s == "/"),
        "offline lookup requires --folder-id ID or the root folder"
    );
    let client = if cached {
        PikPak::new()?
    } else {
        super::cli_client()?
    };
    let id = match folder_id {
        Some(id) => id.to_owned(),
        None => client.resolve_folder(path.unwrap_or("/"))?,
    };
    let cancel = AtomicBool::new(false);
    let result = if cached {
        client
            .cached_folder_stats(&id)?
            .context("no cached statistics for this folder")?
    } else if refresh {
        client.sync_folder_index(&cancel, true)?;
        client
            .cached_folder_stats(&id)?
            .context("folder absent from the rebuilt index")?
    } else {
        client.folder_stats(&id, &cancel)?
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("Total size: {} bytes", result.total_size_bytes);
        println!(
            "Latest content modification: {}",
            result.latest_modified_time.as_deref().unwrap_or("-")
        );
        println!("Empty: {}", result.is_empty);
    }
    Ok(())
}
