use crate::pikpak::EntryKind;
use anyhow::{Result, anyhow};

fn destination_path(
    output: Option<&str>,
    positional_output: Option<&str>,
    remote_name: &str,
) -> std::path::PathBuf {
    match output.or(positional_output) {
        Some(explicit) => std::path::PathBuf::from(explicit),
        None => std::path::PathBuf::from(crate::pikpak::sanitize_filename(remote_name)),
    }
}

fn target_destination(target_dir: &std::path::Path, remote_name: &str) -> std::path::PathBuf {
    target_dir.join(crate::pikpak::sanitize_filename(remote_name))
}

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!(
            "Usage: pikpaktui download [-n] [-j <n>] [-o <output>] <path>\n       pikpaktui download [-n] [-j <n>] -t <local_dir> <path...>\n\nIf <path> is a folder, the entire directory tree is downloaded recursively.\n-j / --jobs <n>  concurrent file downloads (default: 1)"
        ));
    }

    let mut output: Option<&str> = None;
    let mut target_dir: Option<&str> = None;
    let mut dry_run = false;
    let mut jobs: usize = 1;
    let mut paths: Vec<&str> = Vec::new();
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-n" | "--dry-run" => dry_run = true,
            "-j" | "--jobs" => {
                let val = iter.next().ok_or_else(|| anyhow!("-j requires a number"))?;
                jobs = val
                    .parse::<usize>()
                    .map_err(|_| anyhow!("-j requires a positive integer"))?;
                if jobs == 0 {
                    return Err(anyhow!("-j must be at least 1"));
                }
                if jobs > 16 {
                    return Err(anyhow!("-j must be at most 16"));
                }
            }
            "-o" | "--output" => {
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
            s if s.starts_with('-') && s != "-" => {
                return Err(anyhow!("unknown option: {s}"));
            }
            _ => paths.push(arg),
        }
    }

    if paths.is_empty() {
        return Err(anyhow!("no file path specified"));
    }
    if target_dir.is_none() {
        // Single-file form takes `<path> [output]`; extra positionals were
        // silently dropped, and a positional output was silently beaten by -o.
        if paths.len() > 2 {
            return Err(anyhow!(
                "too many arguments: expected <path> [output] (use -t <dir> for multiple sources)"
            ));
        }
        if output.is_some() && paths.len() > 1 {
            return Err(anyhow!("both -o and a positional output were given"));
        }
    }

    let client = super::cli_client()?;

    if let Some(dir) = target_dir {
        let dir = std::path::Path::new(dir);
        for path in &paths {
            let (parent, name) = super::split_parent_name(path)?;
            let parent_id = client.resolve_path(&parent)?;
            let entry = super::find_entry(&client, &parent_id, &name)?;

            if dry_run {
                let kind_tag = if entry.kind == EntryKind::Folder {
                    "folder".to_string()
                } else {
                    super::format_size(entry.size)
                };
                println!(
                    "[dry-run] Would download '{}' ({}) -> '{}'",
                    name,
                    kind_tag,
                    target_destination(dir, &name).display()
                );
                continue;
            }

            if entry.kind == EntryKind::Folder {
                println!(
                    "Downloading folder '{}' -> '{}'{}",
                    name,
                    dir.display(),
                    if jobs > 1 {
                        format!(" ({jobs} concurrent)")
                    } else {
                        String::new()
                    }
                );
                let (ok, failed) = client.download_dir(&entry.id, &name, dir, jobs)?;
                println!(
                    "Folder '{}' done: {} file(s) ok, {} failed",
                    name, ok, failed
                );
                if failed > 0 {
                    return Err(anyhow!("{} file(s) failed in '{}'", failed, name));
                }
            } else {
                let dest = target_destination(dir, &name);
                if let Some(parent) = dest.parent()
                    && !parent.as_os_str().is_empty()
                {
                    std::fs::create_dir_all(parent)?;
                }
                eprintln!(
                    "{} ({}) downloading...",
                    name,
                    super::format_size(entry.size)
                );
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

        let dest = destination_path(output, paths.get(1).map(|s| s.as_ref()), &name);

        if dry_run {
            let kind_tag = if entry.kind == EntryKind::Folder {
                "folder".to_string()
            } else {
                super::format_size(entry.size)
            };
            println!(
                "[dry-run] Would download '{}' ({}) -> '{}'",
                name,
                kind_tag,
                dest.display()
            );
            return Ok(());
        }

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
            println!(
                "Downloading folder '{}' -> '{}'{}",
                name,
                dest.display(),
                if jobs > 1 {
                    format!(" ({jobs} concurrent)")
                } else {
                    String::new()
                }
            );
            let (ok, failed) = client.download_dir(&entry.id, &folder_name, &parent_dest, jobs)?;
            println!(
                "Folder '{}' done: {} file(s) ok, {} failed",
                name, ok, failed
            );
            if failed > 0 {
                return Err(anyhow!("{} file(s) failed in '{}'", failed, name));
            }
        } else {
            if let Some(parent) = dest.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent)?;
            }
            eprintln!(
                "{} ({}) downloading...",
                name,
                super::format_size(entry.size)
            );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jobs_above_sixteen_are_rejected_before_client_setup() {
        let err = run(&["-j".into(), "17".into()]).unwrap_err();
        assert!(
            err.to_string().contains("at most 16"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn implicit_destination_sanitizes_remote_path_components() {
        assert_eq!(
            destination_path(None, None, r"..\outside/file.txt"),
            std::path::PathBuf::from("__outside_file.txt")
        );
        assert_eq!(
            target_destination(std::path::Path::new("/safe"), r"..\outside/file.txt"),
            std::path::PathBuf::from("/safe/__outside_file.txt")
        );
    }

    #[test]
    fn explicit_destination_is_preserved_verbatim() {
        assert_eq!(
            destination_path(Some("../chosen/name"), None, "remote.txt"),
            std::path::PathBuf::from("../chosen/name")
        );
        assert_eq!(
            destination_path(None, Some("../positional/name"), "remote.txt"),
            std::path::PathBuf::from("../positional/name")
        );
    }
}
