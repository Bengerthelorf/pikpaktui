use crate::pikpak::{
    ShareLocation,
    share_manage::{ShareFeed, share_link},
};
use anyhow::{Context, Result, anyhow, bail, ensure};
use serde_json::{Value, json};
use std::io::Write as _;

pub(super) fn run_create(args: &[String]) -> Result<()> {
    let mut paths: Vec<&str> = Vec::new();
    let mut need_password = false;
    let mut expiration_days: i64 = -1;
    let mut output_file: Option<&str> = None;
    let mut separate = false;
    let mut view = None;
    let mut order = None;
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
            "--separate" => separate = true,
            "--view" => view = Some(iter.next().context("--view requires a value")?.as_str()),
            "--order" => order = Some(iter.next().context("--order requires a value")?.as_str()),
            "-o" => {
                output_file = Some(
                    iter.next()
                        .ok_or_else(|| anyhow!("-o requires a file path"))?
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
        return Err(anyhow!("no path specified"));
    }

    ensure!(
        expiration_days >= -1,
        "expiry must be -1 or a nonnegative number of days"
    );
    share_link("https://mypikpak.com/s/check", view, order)?;
    let client = super::cli_client()?;

    let mut file_ids: Vec<String> = Vec::new();
    for path in &paths {
        let (parent, name) = super::split_parent_name(path)?;
        let parent_id = client.resolve_path(&parent)?;
        let entry = super::find_entry(&client, &parent_id, &name)?;
        file_ids.push(entry.id);
    }

    let id_refs: Vec<&str> = file_ids.iter().map(String::as_str).collect();
    let per_link = if separate { 1 } else { id_refs.len() };
    let mut output = None;
    let mut rows = Vec::new();
    for (index, group) in id_refs.chunks(per_link).enumerate() {
        let result = match client.create_share(group, need_password, expiration_days) {
            Ok(result) => result,
            Err(error) => {
                if json && separate {
                    println!("{}", serde_json::to_string_pretty(&rows)?);
                }
                return Err(error.context(format!("{} link(s) created before this request failed. Check My Shares before retrying", rows.len())));
            }
        };
        let url = share_link(&result.share_url, view, order).with_context(|| {
            format!(
                "link {} created but could not be formatted",
                result.share_id
            )
        })?;
        let mut row = serde_json::json!({
            "share_id": result.share_id,
            "share_url": url,
            "pass_code": (!result.pass_code.is_empty()).then_some(&result.pass_code),
            "share_text": (!result.share_text.is_empty()).then_some(&result.share_text),
        });
        if separate {
            row["path"] = serde_json::json!(paths[index]);
        }
        rows.push(row);
        if !json {
            println!("\x1b[1;36m{url}\x1b[0m");
            if !result.pass_code.is_empty() {
                println!(
                    "\x1b[33mPassword:\x1b[0m \x1b[1;33m{}\x1b[0m",
                    result.pass_code
                );
            }
        }
        if output.is_none()
            && let Some(path) = output_file
        {
            output = Some(std::fs::File::create(path).with_context(|| {
                format!(
                    "link {} created; could not open output file {path}",
                    result.share_id
                )
            })?);
        }
        if let Some(out) = output.as_mut() {
            writeln!(out, "{url}")?;
            if !result.pass_code.is_empty() {
                writeln!(out, "Password: {}", result.pass_code)?;
            }
        }
    }
    if json {
        let out = if separate {
            Value::Array(rows)
        } else {
            rows.remove(0)
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
    }
    if let Some(path) = output_file {
        eprintln!("Written to '{path}'");
    }
    Ok(())
}

const HELP: &str = "pikpaktui share --manage <action>
  status <cloud-path>...                       Look up file sharing status
  edit <share> --expires <RFC3339|-1>          Change expiration
  edit <share> --password <code> | --public    Change or remove password
  edit <share> --allow-follow | --forbid-follow
  link <share> [--view list|grid|flow] [--order name_asc|name_desc|mod_asc|mod_desc|size_asc|size_desc]
  code <phrase>                               Resolve a share code
  code-info <share>                           List short codes for the share
  code-add <share> <new> | code-edit <share> <old> <new> | code-remove <share> <old>
  follow <share> [-p <code>] | unfollow <share>
  following | updates [share]                 List subscriptions or their updates
  update-files <record-id>                    List the files in one update
  save-update <share> <record-id> --file-id <id>... [-t <dest>] [-p <code>] [--dry-run]
Queries produce JSON. Use share IDs or root share URLs.";

fn value<'a>(args: &mut Vec<&'a str>, flag: &str) -> Result<Option<&'a str>> {
    let Some(i) = args.iter().position(|s| *s == flag) else {
        return Ok(None);
    };
    args.remove(i);
    ensure!(
        i < args.len() && (!args[i].starts_with('-') || args[i] == "-1"),
        "{flag} requires a value"
    );
    Ok(Some(args.remove(i)))
}

fn flag(args: &mut Vec<&str>, name: &str) -> bool {
    if let Some(i) = args.iter().position(|s| *s == name) {
        args.remove(i);
        true
    } else {
        false
    }
}
fn root_share(args: &[&str]) -> Result<ShareLocation> {
    ensure!(
        args.len() == 1 && !args[0].starts_with('-'),
        "provide exactly one share ID or root URL"
    );
    let share = ShareLocation::parse(args[0])?;
    ensure!(
        share.folders.is_empty(),
        "use the root share URL for this action"
    );
    Ok(share)
}

pub(super) fn run(args: &[String]) -> Result<()> {
    let Some((action, rest)) = args.split_first() else {
        bail!("{HELP}")
    };
    let mut args: Vec<&str> = rest.iter().map(String::as_str).collect();
    let json = flag(&mut args, "-J") | flag(&mut args, "--json");
    if action == "help" || action == "--help" {
        println!("{HELP}");
        return Ok(());
    }
    let out = match action.as_str() {
        "link" => {
            let view = value(&mut args, "--view")?;
            let order = value(&mut args, "--order")?;
            let share = root_share(&args)?;
            let url = if args[0].contains("://") {
                args[0].to_owned()
            } else {
                format!("https://mypikpak.com/s/{}", share.id)
            };
            let link = share_link(&url, view, order)?;
            if json {
                println!("{}", serde_json::to_string(&link)?);
            } else {
                println!("{link}");
            }
            return Ok(());
        }
        "status" => {
            ensure!(
                !args.is_empty() && args.iter().all(|a| !a.starts_with('-')),
                "provide cloud paths to look up"
            );
            let client = super::cli_client()?;
            let mut ids = Vec::new();
            for path in args {
                let (parent, name) = super::split_parent_name(path)?;
                let parent = client.resolve_path(&parent)?;
                ids.push(super::find_entry(&client, &parent, &name)?.id);
            }
            client.share_file_status(&ids.iter().map(String::as_str).collect::<Vec<_>>())?
        }
        "edit" => {
            let expiry = value(&mut args, "--expires")?;
            let mut password = value(&mut args, "--password")?;
            ensure!(
                password.is_none_or(|code| !code.is_empty()),
                "use --public to remove the password"
            );
            if flag(&mut args, "--public") {
                ensure!(password.is_none(), "choose either --password or --public");
                password = Some("");
            }
            let allow = flag(&mut args, "--allow-follow");
            let forbid = flag(&mut args, "--forbid-follow");
            ensure!(!(allow && forbid), "choose one follow setting");
            let share = root_share(&args)?;
            super::cli_client()?.edit_share(
                &share.id,
                expiry,
                password,
                (allow || forbid).then_some(forbid),
            )?;
            json!({"share_id":share.id,"edited":true})
        }
        "code" => {
            ensure!(
                args.len() == 1 && !args[0].starts_with('-'),
                "provide one phrase"
            );
            json!(super::cli_client()?.resolve_share_code(args[0])?)
        }
        "code-add" | "code-edit" | "code-remove" => {
            let editing = action == "code-edit";
            ensure!(
                args.len() == if editing { 3 } else { 2 },
                "provide a share and the code(s) for this action"
            );
            let location = root_share(&args[..1])?;
            let (old, new) = match action.as_str() {
                "code-add" => (None, args[1]),
                "code-edit" => (Some(args[1]), args[2]),
                _ => (Some(args[1]), ""),
            };
            if action != "code-remove" {
                ensure!(!new.trim().is_empty(), "enter a new code");
            }
            super::cli_client()?.set_share_code(&location.id, old, new)?;
            json!({"share_id":location.id,"updated":true})
        }
        "code-info" => {
            let share = root_share(&args)?;
            super::cli_client()?.share_code_info(&share.id)?
        }
        "follow" | "unfollow" => {
            let follow = action == "follow";
            let code = if follow {
                value(&mut args, "-p")?
            } else {
                None
            };
            let share = root_share(&args)?;
            super::cli_client()?.set_share_followed(
                &share.id,
                code.or(share.pass_code.as_deref()).unwrap_or(""),
                follow,
            )?;
            json!({"share_id":share.id,"followed":follow})
        }
        "following" => {
            ensure!(args.is_empty(), "following takes no arguments");
            json!(super::cli_client()?.share_feed(ShareFeed::Following)?)
        }
        "updates" => {
            let share = if args.is_empty() {
                None
            } else {
                Some(root_share(&args)?)
            };
            json!(
                super::cli_client()?
                    .share_feed(ShareFeed::Updates(share.as_ref().map(|s| s.id.as_str())))?
            )
        }
        "update-files" => {
            ensure!(args.len() == 1, "provide one numeric update record ID");
            json!(super::cli_client()?.update_file_ids(args[0].parse()?)?)
        }
        "save-update" => {
            let dry = flag(&mut args, "--dry-run");
            let dest = value(&mut args, "-t")?;
            let pass = value(&mut args, "-p")?;
            let mut selected = Vec::new();
            while let Some(id) = value(&mut args, "--file-id")? {
                ensure!(!selected.contains(&id), "duplicate file ID");
                selected.push(id);
            }
            ensure!(
                args.len() == 2 && !selected.is_empty(),
                "provide share, update record ID and at least one --file-id"
            );
            let share = root_share(&args[..1])?;
            let record = args[1].parse::<u64>()?;
            let client = super::cli_client()?;
            let parent = dest
                .map(|path| client.resolve_folder(path))
                .transpose()?
                .unwrap_or_default();
            client.save_update_selection(
                &share.id,
                record,
                &selected,
                pass.or(share.pass_code.as_deref()).unwrap_or(""),
                &parent,
                dry,
            )?;
            json!({"share_id":share.id,"record_id":record,"file_ids":selected,"destination":dest.unwrap_or("/"),"saved":!dry})
        }
        _ => bail!("unknown share management action '{action}'\n{HELP}"),
    };
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}
