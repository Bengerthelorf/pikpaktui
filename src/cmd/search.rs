use crate::pikpak::{EntryKind, SearchQuery};
use anyhow::{Context, Result, bail, ensure};

pub fn run(args: &[String]) -> Result<()> {
    let mut query = SearchQuery {
        limit: 100,
        ..Default::default()
    };
    let mut args = args.iter().map(String::as_str);
    let mut cached = false;
    let mut json = false;
    let mut words = Vec::new();
    while let Some(arg) = args.next() {
        match arg {
            "--in" => {
                query.scope_path = Some(
                    args.next()
                        .context("--in requires a folder path")?
                        .to_owned(),
                )
            }
            "--cached" => cached = true,
            "--type" => {
                query.kind = Some(match args.next() {
                    Some("file") => EntryKind::File,
                    Some("folder") => EntryKind::Folder,
                    _ => bail!("--type accepts file or folder"),
                })
            }
            "--min-size" => {
                query.min_size = Some(args.next().context("missing byte count")?.parse()?)
            }
            "--max-size" => {
                query.max_size = Some(args.next().context("missing byte count")?.parse()?)
            }
            "--limit" => query.limit = args.next().context("missing limit")?.parse()?,
            "--json" | "-J" => json = true,
            "--" => {
                words.extend(args);
                break;
            }
            flag if flag.starts_with('-') => bail!("unknown search option {flag}"),
            text => words.push(text),
        }
    }
    ensure!(
        query.min_size.unwrap_or(0) <= query.max_size.unwrap_or(u64::MAX),
        "minimum size exceeds maximum size"
    );
    query.name = words.join(" ");
    let hits = if cached {
        crate::pikpak::PikPak::new()?.cached_search(&query)?
    } else {
        super::cli_client()?.search_drive(&query, &false.into())?
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&hits)?);
    } else {
        for hit in hits {
            println!("{}\t{}\t{}", hit.id, super::format_size(hit.size), hit.path);
        }
    }
    Ok(())
}
