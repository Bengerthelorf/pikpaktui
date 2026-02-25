use anyhow::{Result, anyhow};
use crate::pikpak::EntryKind;

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!(
            "Usage: pikpaktui download [-o <output>] <path>\n       pikpaktui download -t <local_dir> <path...>\n\nIf <path> is a folder, the entire directory tree is downloaded recursively."
        ));
    }

    let mut output: Option<&str> = None;
    let mut target_dir: Option<&str> = None;
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
            "-t" => {
                target_dir = Some(
                    iter.next()
                        .ok_or_else(|| anyhow!("-t requires a directory path"))?
                        .as_str(),
                );
            }
            _ => paths.push(arg),
        }
    }

    if paths.is_empty() {
        return Err(anyhow!("no file path specified"));
    }

    let client = super::cli_client()?;

    if let Some(dir) = target_dir {
        let dir = std::path::Path::new(dir);
        for path in &paths {
            let (parent, name) = super::split_parent_name(path)?;
            let parent_id = client.resolve_path(&parent)?;
            let entry = super::find_entry(&client, &parent_id, &name)?;
            if entry.kind == EntryKind::Folder {
                println!("Downloading folder '{}' -> '{}'", name, dir.display());
                let (ok, failed) = client.download_dir(&entry.id, &name, dir)?;
                println!("Folder '{}' done: {} file(s) ok, {} failed", name, ok, failed);
                if failed > 0 {
                    return Err(anyhow!("{} file(s) failed in '{}'", failed, name));
                }
            } else {
                let dest = dir.join(&name);
                let total = client.download_to(&entry.id, &dest)?;
                println!(
                    "Downloaded '{}' -> '{}' ({})",
                    name,
                    dest.display(),
                    super::format_size(total)
                );
            }
        }
    } else {
        let (parent, name) = super::split_parent_name(paths[0])?;
        let parent_id = client.resolve_path(&parent)?;
        let entry = super::find_entry(&client, &parent_id, &name)?;

        let dest = std::path::PathBuf::from(
            output.unwrap_or_else(|| paths.get(1).map(|s| s.as_ref()).unwrap_or(&name)),
        );

        if entry.kind == EntryKind::Folder {
            let parent_dest = dest
                .parent()
                .map(|p| p.to_path_buf())
                .filter(|p| p != std::path::Path::new(""))
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let folder_name = dest
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| name.clone());
            println!("Downloading folder '{}' -> '{}'", name, dest.display());
            let (ok, failed) = client.download_dir(&entry.id, &folder_name, &parent_dest)?;
            println!("Folder '{}' done: {} file(s) ok, {} failed", name, ok, failed);
            if failed > 0 {
                return Err(anyhow!("{} file(s) failed in '{}'", failed, name));
            }
        } else {
            let total = client.download_to(&entry.id, &dest)?;
            println!(
                "Downloaded '{}' -> '{}' ({})",
                name,
                dest.display(),
                super::format_size(total)
            );
        }
    }
    Ok(())
}
