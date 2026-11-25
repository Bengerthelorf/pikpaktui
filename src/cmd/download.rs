use crate::pikpak::{EntryKind, GcidMismatch, Verification};
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path, sync::Mutex};

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

fn target_destination(
    target_dir: &Path,
    remote_name: &str,
    taken: &mut HashSet<String>,
) -> std::path::PathBuf {
    target_dir.join(crate::pikpak::unique_local_name(
        taken,
        &crate::pikpak::sanitize_filename(remote_name),
    ))
}

fn file_result(
    id: &str,
    dest: &Path,
    kind: EntryKind,
    result: Result<(u64, Verification)>,
) -> Value {
    let mut row = json!({"id":id,"destination":dest,"kind":if kind==EntryKind::Folder {"folder"}else{"file"}});
    match result {
        Ok((bytes, verified)) => {
            row["status"] = json!("downloaded");
            row["bytes"] = json!(bytes);
            row["verification"] = json!(match verified {
                Verification::Unchecked => "unchecked",
                Verification::Verified => "verified",
                Verification::Unavailable => "unavailable",
            });
        }
        Err(error) => {
            row["status"] = json!("failed");
            row["error"] = json!(format!("{error:#}"));
            if let Some(mismatch) = error.downcast_ref::<GcidMismatch>() {
                row["verification"] = json!("mismatch");
                row["expected_gcid"] = json!(mismatch.expected);
                row["actual_gcid"] = json!(mismatch.actual);
            }
        }
    }
    row
}

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!(
            "Usage: pikpaktui download [-J] [--verify] [-n] [-j <n>] [-o <output>] <path>\n       pikpaktui download [-J] [--verify] [-n] [-j <n>] -t <local_dir> <path...>\n\nIf <path> is a folder, the entire directory tree is downloaded recursively.\n-j / --jobs <n>  concurrent file downloads (default: 1)"
        ));
    }

    let mut output: Option<&str> = None;
    let mut target_dir: Option<&str> = None;
    let mut dry_run = false;
    let mut verify = false;
    let mut json_output = false;
    let mut jobs: usize = 1;
    let mut paths: Vec<&str> = Vec::new();
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--verify" => verify = true,
            "-J" | "--json" => json_output = true,
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

    if target_dir.is_some() && output.is_some() {
        return Err(anyhow!("cannot combine -t with -o"));
    }
    let client = super::cli_client()?;
    let rows = Mutex::new(Vec::new());
    let mut failed = 0;
    let mut taken = HashSet::new();
    let positional = if target_dir.is_none() && paths.len() == 2 {
        paths.pop()
    } else {
        None
    };
    for path in paths {
        let report =
            |id: &str, dest: &Path, kind: EntryKind, result: Result<(u64, Verification)>| {
                if json_output {
                    let mut row = file_result(id, dest, kind, result);
                    row["source"] = json!(path);
                    rows.lock().unwrap_or_else(|e| e.into_inner()).push(row);
                } else {
                    match result {
                        Ok((bytes, verify)) => println!(
                            "Downloaded '{}' ({}){}",
                            dest.display(),
                            super::format_size(bytes),
                            verify.suffix()
                        ),
                        Err(e) => eprintln!("Download failed '{}': {e:#}", dest.display()),
                    }
                }
            };
        let outcome = (|| -> Result<usize> {
            let (parent, name) = super::split_parent_name(path)?;
            let entry = super::find_entry(&client, &client.resolve_path(&parent)?, &name)?;
            let dest = match target_dir {
                Some(dir) => target_destination(Path::new(dir), &name, &mut taken),
                None => destination_path(output, positional, &name),
            };
            if dry_run {
                if json_output {
                    rows.lock().unwrap_or_else(|e|e.into_inner()).push(json!({
                        "source":path,"id":entry.id,"destination":dest,"status":"dry-run","bytes":entry.size,
                        "kind":if entry.kind==EntryKind::Folder {"folder"} else {"file"}
                    }));
                } else {
                    println!(
                        "[dry-run] Would download '{}' -> '{}'",
                        name,
                        dest.display()
                    );
                }
                return Ok(0);
            }
            if entry.kind == EntryKind::Folder {
                if !json_output {
                    println!("Downloading folder '{}' -> '{}'", name, dest.display());
                }
                match client.download_directory(&entry.id, &dest, jobs, verify, &report) {
                    Ok((ok, failed)) => {
                        if !json_output {
                            println!(
                                "Folder '{}' done: {} file(s) ok, {} failed",
                                name, ok, failed
                            );
                        }
                        Ok(failed)
                    }
                    Err(e) => {
                        report(&entry.id, &dest, EntryKind::Folder, Err(e));
                        Ok(1)
                    }
                }
            } else {
                if !json_output {
                    eprintln!(
                        "{} ({}) downloading...",
                        name,
                        super::format_size(entry.size)
                    );
                }
                let result = (|| {
                    if let Some(parent) = dest.parent()
                        && !parent.as_os_str().is_empty()
                    {
                        std::fs::create_dir_all(parent)?;
                    }
                    client.download_file(&entry.id, &dest, verify)
                })();
                let failed = usize::from(result.is_err());
                report(&entry.id, &dest, EntryKind::File, result);
                Ok(failed)
            }
        })();
        match outcome {
            Ok(count) => failed += count,
            Err(e) => {
                failed += 1;
                if json_output {
                    rows.lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(json!({"source":path,"status":"failed","error":format!("{e:#}")}));
                } else {
                    eprintln!("Download failed '{path}': {e:#}");
                }
            }
        }
    }
    if json_output {
        let mut rows = rows.into_inner().unwrap_or_else(|e| e.into_inner());
        rows.sort_by(|a, b| {
            a["source"]
                .as_str()
                .cmp(&b["source"].as_str())
                .then_with(|| a["destination"].as_str().cmp(&b["destination"].as_str()))
        });
        println!(
            "{}",
            serde_json::to_string(&json!({"files":rows,"failed":failed}))?
        );
    }
    if failed > 0 {
        return Err(anyhow!("{failed} download(s) failed"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_file_json_preserves_hash_outcomes_and_mismatch_details() {
        let path = Path::new("out/file");
        for (status, label) in [
            (Verification::Verified, "verified"),
            (Verification::Unavailable, "unavailable"),
            (Verification::Unchecked, "unchecked"),
        ] {
            let row = file_result("id", path, EntryKind::File, Ok((5, status)));
            assert_eq!(row["verification"], label);
            assert_eq!(row["status"], "downloaded");
            assert_eq!(row["bytes"], 5);
        }
        let err = GcidMismatch {
            expected: "expected".into(),
            actual: "actual".into(),
        };
        let row = file_result("id", path, EntryKind::File, Err(err.into()));
        assert_eq!(row["verification"], "mismatch");
        assert_eq!(row["expected_gcid"], "expected");
        assert_eq!(row["actual_gcid"], "actual");
        assert_eq!(row["status"], "failed");
    }

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
            target_destination(
                std::path::Path::new("/safe"),
                r"..\outside/file.txt",
                &mut HashSet::new()
            ),
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
