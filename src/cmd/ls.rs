use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

use anyhow::{Result, anyhow, ensure};
use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::config::SortField;
use crate::pikpak::{
    Entry, EntryKind, FolderStats, FolderStatsSession, PikPak, RootListing, UnsupportedIndex,
};

const USAGE: &str = "Usage: pikpaktui ls [-l|--long] [-J|--json] [-s|--sort=<field>] [-r|--reverse] [--folder-stats|--no-folder-stats] [--tree] [--depth=N] [path]\n\nSort fields: name, size, created, modified, content-modified, type, extension, none";

#[derive(Debug, PartialEq, Eq)]
struct LsArgs {
    path: String,
    long: bool,
    json: bool,
    folder_stats: bool,
    sort_field: SortField,
    reverse: bool,
    tree: bool,
    max_depth: Option<usize>,
}

fn parse_sort_field(s: &str) -> Result<SortField> {
    match s {
        "name" => Ok(SortField::Name),
        "size" => Ok(SortField::Size),
        "created" => Ok(SortField::Created),
        "modified" => Ok(SortField::Modified),
        "content-modified" => Ok(SortField::ContentModified),
        "type" => Ok(SortField::Type),
        "extension" | "ext" => Ok(SortField::Extension),
        "none" => Ok(SortField::None),
        _ => Err(anyhow!(
            "unknown sort field: {s}\nValid fields: name, size, created, modified, content-modified, type, extension, none"
        )),
    }
}

fn parse_args(args: &[String], default_stats: bool) -> Result<LsArgs> {
    let mut path: Option<String> = None;
    let mut long = false;
    let mut json = false;
    let mut folder_stats = None;
    let mut sort_field = SortField::default();
    let mut reverse = false;
    let mut tree = false;
    let mut max_depth: Option<usize> = None;
    let mut options_done = false;
    let mut expect_sort = false;
    let mut expect_depth = false;

    for arg in args {
        if expect_sort {
            sort_field = parse_sort_field(arg)?;
            expect_sort = false;
            continue;
        }
        if expect_depth {
            max_depth = Some(
                arg.parse::<usize>()
                    .map_err(|_| anyhow!("--depth requires a positive integer"))?,
            );
            expect_depth = false;
            continue;
        }

        if !options_done {
            match arg.as_str() {
                "-l" | "--long" => {
                    long = true;
                    continue;
                }
                "-J" | "--json" => {
                    json = true;
                    continue;
                }
                "-r" | "--reverse" => {
                    reverse = true;
                    continue;
                }
                "--folder-stats" | "--no-folder-stats" => {
                    folder_stats = Some(arg == "--folder-stats");
                    continue;
                }
                "--tree" => {
                    tree = true;
                    continue;
                }
                "-s" | "--sort" => {
                    expect_sort = true;
                    continue;
                }
                "--depth" => {
                    expect_depth = true;
                    continue;
                }
                "--" => {
                    options_done = true;
                    continue;
                }
                _ if arg.starts_with("--sort=") => {
                    sort_field = parse_sort_field(&arg["--sort=".len()..])?;
                    continue;
                }
                _ if arg.starts_with("-s=") => {
                    sort_field = parse_sort_field(&arg["-s=".len()..])?;
                    continue;
                }
                _ if arg.starts_with("--depth=") => {
                    let val = &arg["--depth=".len()..];
                    max_depth = Some(
                        val.parse::<usize>()
                            .map_err(|_| anyhow!("--depth requires a positive integer"))?,
                    );
                    continue;
                }
                _ if arg.starts_with('-') => {
                    return Err(anyhow!("unknown option for ls: {arg}\n{USAGE}"));
                }
                _ => {}
            }
        }

        if path.is_some() {
            return Err(anyhow!("ls accepts at most one path\n{USAGE}"));
        }
        path = Some(arg.clone());
    }

    if expect_sort {
        return Err(anyhow!("--sort requires a value\n{USAGE}"));
    }
    if expect_depth {
        return Err(anyhow!("--depth requires a value\n{USAGE}"));
    }
    if max_depth.is_some() {
        tree = true;
    }

    ensure!(
        folder_stats != Some(false) || sort_field != SortField::ContentModified,
        "--no-folder-stats conflicts with --sort=content-modified"
    );
    Ok(LsArgs {
        path: path.unwrap_or_else(|| "/".to_string()),
        long,
        json,
        folder_stats: folder_stats.unwrap_or(default_stats)
            || sort_field == SortField::ContentModified,
        sort_field,
        reverse,
        tree,
        max_depth,
    })
}

// One synchronization serves every directory in this invocation, including
// tree output. Legacy services share a single scoped traversal session.
#[derive(Default)]
enum StatSource {
    #[default]
    Unchecked,
    Indexed,
    Scoped(Box<FolderStatsSession>),
}

impl StatSource {
    fn read(
        &mut self,
        client: &PikPak,
        folder_id: &str,
        entries: &[Entry],
    ) -> Result<HashMap<String, FolderStats>> {
        let ids: Vec<_> = entries
            .iter()
            .filter(|e| e.kind == EntryKind::Folder)
            .map(|e| e.id.clone())
            .collect();
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let cancel = AtomicBool::new(false);
        if matches!(self, Self::Unchecked) {
            let result = if folder_id.is_empty() {
                client.sync_folder_index_for_root(
                    &cancel,
                    false,
                    &RootListing::from_entries(entries)?,
                )
            } else {
                client.sync_folder_index(&cancel, false)
            };
            *self = match result {
                Ok(()) => Self::Indexed,
                Err(error) if error.is::<UnsupportedIndex>() => Self::Scoped(Box::default()),
                Err(error) => return Err(error),
            };
        }
        match self {
            Self::Indexed => {
                let totals = client.cached_folder_stats_batch(&ids)?;
                for id in ids {
                    ensure!(
                        totals.contains_key(&id),
                        "folder {id:?} absent from synchronized index"
                    );
                }
                Ok(totals)
            }
            Self::Scoped(session) => ids
                .into_iter()
                .map(|id| {
                    let stats = session.stats(client, &id, &cancel)?;
                    Ok((id, stats))
                })
                .collect(),
            Self::Unchecked => unreachable!("source selected above"),
        }
    }
}

struct Listing<'a> {
    client: &'a PikPak,
    args: &'a LsArgs,
    nerd_font: bool,
    source: StatSource,
}

impl Listing<'_> {
    fn entries(&mut self, folder_id: &str) -> Result<(Vec<Entry>, HashMap<String, FolderStats>)> {
        let mut entries = self.client.ls(folder_id)?;
        let (sort, reverse) = (self.args.sort_field, self.args.reverse);
        if !self.args.folder_stats {
            crate::config::sort_entries(&mut entries, sort, reverse);
            return Ok((entries, HashMap::new()));
        }
        let totals = self.source.read(self.client, folder_id, &entries)?;
        crate::config::sort_entries_with_stats(&mut entries, sort, reverse, true, |id| {
            totals.get(id)
        });
        Ok((entries, totals))
    }

    fn long_prefix(&self, entry: &Entry, totals: &HashMap<String, FolderStats>) -> String {
        if !self.args.folder_stats && self.args.sort_field != SortField::Modified {
            return super::long_entry_prefix(entry);
        }
        let stats = totals.get(&entry.id);
        let bytes = match entry.kind {
            EntryKind::File => Some(entry.size),
            EntryKind::Folder => stats.map(|s| s.total_size_bytes),
        };
        let stamp = if self.args.sort_field == SortField::Modified || entry.kind == EntryKind::File
        {
            let own = entry.modified_time.as_str();
            OffsetDateTime::parse(own, &Rfc3339).ok().map(|_| own)
        } else {
            stats.and_then(|s| s.latest_modified_time.as_deref())
        };
        super::long_prefix(entry, bytes, stamp.unwrap_or_default())
    }

    fn print_tree(&mut self, folder_id: &str, prefix: &str, depth: usize) -> Result<()> {
        use crate::theme;
        if self.args.max_depth.is_some_and(|max| depth > max) {
            return Ok(());
        }
        let (entries, totals) = self.entries(folder_id)?;

        let count = entries.len();
        for (i, entry) in entries.iter().enumerate() {
            let is_last = i + 1 == count;
            let connector = if is_last { "└── " } else { "├── " };
            let cat = theme::categorize(entry);
            let icon = theme::cli_icon(cat, self.nerd_font);
            let name_display = format!("{}{}", icon, entry.name);
            let colored_name = theme::cli_colored(&name_display, cat);

            if self.args.long {
                println!(
                    "{}{}{}{}",
                    self.long_prefix(entry, &totals),
                    prefix,
                    connector,
                    colored_name
                );
            } else {
                println!("{}{}{}", prefix, connector, colored_name);
            }

            if entry.kind == EntryKind::Folder {
                let child_prefix = if is_last {
                    format!("{}    ", prefix)
                } else {
                    format!("{}│   ", prefix)
                };
                self.print_tree(&entry.id, &child_prefix, depth + 1)?;
            }
        }

        Ok(())
    }
}

#[derive(Serialize)]
struct EntryWithStats<'a> {
    #[serde(flatten)]
    entry: &'a Entry,
    #[serde(skip_serializing_if = "Option::is_none")]
    folder_stats: Option<&'a FolderStats>,
}

pub fn run(args: &[String]) -> Result<()> {
    let config = super::cli_config();
    let parsed = parse_args(args, config.cli_folder_stats)?;
    let nerd_font = config.cli_nerd_font;
    let client = super::cli_client()?;
    let folder_id = client.resolve_path(&parsed.path)?;

    let mut listing = Listing {
        client: &client,
        args: &parsed,
        nerd_font,
        source: StatSource::default(),
    };
    if parsed.tree {
        let root_label = parsed.path.trim_end_matches('/');
        let root_label = if root_label.is_empty() {
            "/"
        } else {
            root_label
        };
        println!("{}", root_label);
        return listing.print_tree(&folder_id, "", 1);
    }

    let (entries, totals) = listing.entries(&folder_id)?;

    if parsed.json {
        if parsed.folder_stats {
            let rows: Vec<_> = entries
                .iter()
                .map(|entry| EntryWithStats {
                    entry,
                    folder_stats: totals.get(&entry.id),
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&rows)?);
        } else {
            super::print_entries_json(&entries);
        }
        return Ok(());
    }

    if entries.is_empty() {
        println!("(empty)");
        return Ok(());
    }

    if parsed.long {
        super::print_entries_with_prefix(&entries, nerd_font, |e| listing.long_prefix(e, &totals));
    } else {
        super::print_entries_short(&entries, nerd_font);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::format_date;
    use super::{LsArgs, parse_args};
    use crate::config::SortField;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parse_defaults_to_root_short_output() {
        assert_eq!(
            parse_args(&s(&[]), false).unwrap(),
            LsArgs {
                path: "/".to_string(),
                long: false,
                json: false,
                folder_stats: false,
                sort_field: SortField::Name,
                reverse: false,
                tree: false,
                max_depth: None,
            }
        );
    }

    #[test]
    fn parse_supports_path_and_long_in_any_order() {
        assert_eq!(
            parse_args(&s(&["/foo", "-l"]), false).unwrap(),
            LsArgs {
                path: "/foo".to_string(),
                long: true,
                json: false,
                folder_stats: false,
                sort_field: SortField::Name,
                reverse: false,
                tree: false,
                max_depth: None,
            }
        );
        assert_eq!(
            parse_args(&s(&["-l", "/foo"]), false).unwrap(),
            LsArgs {
                path: "/foo".to_string(),
                long: true,
                json: false,
                folder_stats: false,
                sort_field: SortField::Name,
                reverse: false,
                tree: false,
                max_depth: None,
            }
        );
    }

    #[test]
    fn parse_sort_field_flag() {
        assert_eq!(
            parse_args(&s(&["--sort=size"]), false).unwrap(),
            LsArgs {
                path: "/".to_string(),
                long: false,
                json: false,
                folder_stats: false,
                sort_field: SortField::Size,
                reverse: false,
                tree: false,
                max_depth: None,
            }
        );
        assert_eq!(
            parse_args(&s(&["-s", "created"]), false).unwrap(),
            LsArgs {
                path: "/".to_string(),
                long: false,
                json: false,
                folder_stats: false,
                sort_field: SortField::Created,
                reverse: false,
                tree: false,
                max_depth: None,
            }
        );
        assert_eq!(
            parse_args(&s(&["--sort", "extension"]), false).unwrap(),
            LsArgs {
                path: "/".to_string(),
                long: false,
                json: false,
                folder_stats: false,
                sort_field: SortField::Extension,
                reverse: false,
                tree: false,
                max_depth: None,
            }
        );
    }

    #[test]
    fn parse_reverse_flag() {
        assert_eq!(
            parse_args(&s(&["-r", "--sort=size"]), false).unwrap(),
            LsArgs {
                path: "/".to_string(),
                long: false,
                json: false,
                folder_stats: false,
                sort_field: SortField::Size,
                reverse: true,
                tree: false,
                max_depth: None,
            }
        );
        assert_eq!(
            parse_args(&s(&["--reverse"]), false).unwrap(),
            LsArgs {
                path: "/".to_string(),
                long: false,
                json: false,
                folder_stats: false,
                sort_field: SortField::Name,
                reverse: true,
                tree: false,
                max_depth: None,
            }
        );
    }

    #[test]
    fn parse_tree_flag() {
        assert_eq!(
            parse_args(&s(&["--tree", "/Movies"]), false).unwrap(),
            LsArgs {
                path: "/Movies".to_string(),
                long: false,
                json: false,
                folder_stats: false,
                sort_field: SortField::Name,
                reverse: false,
                tree: true,
                max_depth: None,
            }
        );
        assert_eq!(
            parse_args(&s(&["--depth=2", "/Movies"]), false).unwrap(),
            LsArgs {
                path: "/Movies".to_string(),
                long: false,
                json: false,
                folder_stats: false,
                sort_field: SortField::Name,
                reverse: false,
                tree: true,
                max_depth: Some(2),
            }
        );
        assert_eq!(
            parse_args(&s(&["--depth", "3"]), false).unwrap(),
            LsArgs {
                path: "/".to_string(),
                long: false,
                json: false,
                folder_stats: false,
                sort_field: SortField::Name,
                reverse: false,
                tree: true,
                max_depth: Some(3),
            }
        );
        assert_eq!(
            parse_args(&s(&["--tree", "-l"]), false).unwrap(),
            LsArgs {
                path: "/".to_string(),
                long: true,
                json: false,
                folder_stats: false,
                sort_field: SortField::Name,
                reverse: false,
                tree: true,
                max_depth: None,
            }
        );
    }

    #[test]
    fn parse_sort_rejects_invalid_field() {
        let err = parse_args(&s(&["--sort=bogus"]), false).unwrap_err();
        assert!(err.to_string().contains("unknown sort field"));
    }

    #[test]
    fn parse_sort_requires_value() {
        let err = parse_args(&s(&["--sort"]), false).unwrap_err();
        assert!(err.to_string().contains("--sort requires a value"));
    }

    #[test]
    fn parse_rejects_unknown_options() {
        let err = parse_args(&s(&["-a"]), false).unwrap_err();
        assert!(err.to_string().contains("unknown option for ls"));
    }

    #[test]
    fn parse_rejects_multiple_paths() {
        let err = parse_args(&s(&["/a", "/b"]), false).unwrap_err();
        assert!(err.to_string().contains("at most one path"));
    }

    #[test]
    fn format_date_parses_iso() {
        assert_eq!(format_date("2026-01-15T12:30:45.000Z"), "2026-01-15 12:30");
    }

    #[test]
    fn format_date_handles_empty() {
        assert_eq!(format_date(""), "-");
    }
}

#[cfg(test)]
mod sort_tests {
    use crate::config::{SortField, sort_entries};
    use crate::pikpak::{Entry, EntryKind};

    fn entry(name: &str, kind: EntryKind, size: u64, created: &str) -> Entry {
        Entry {
            id: name.to_string(),
            name: name.to_string(),
            kind,
            size,
            created_time: created.to_string(),
            modified_time: String::new(),
            starred: false,
            thumbnail_link: None,
            phase: None,
            audit: None,
        }
    }

    #[test]
    fn sort_by_name_case_insensitive() {
        let mut entries = vec![
            entry("Bravo", EntryKind::File, 100, ""),
            entry("alpha", EntryKind::File, 200, ""),
            entry("Charlie", EntryKind::File, 50, ""),
        ];
        sort_entries(&mut entries, SortField::Name, false);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "Bravo", "Charlie"]);
    }

    #[test]
    fn sort_folders_before_files() {
        let mut entries = vec![
            entry("file_a", EntryKind::File, 100, ""),
            entry("dir_b", EntryKind::Folder, 0, ""),
            entry("file_c", EntryKind::File, 200, ""),
            entry("dir_a", EntryKind::Folder, 0, ""),
        ];
        sort_entries(&mut entries, SortField::Name, false);
        assert_eq!(entries[0].kind, EntryKind::Folder);
        assert_eq!(entries[1].kind, EntryKind::Folder);
        assert_eq!(entries[2].kind, EntryKind::File);
        assert_eq!(entries[3].kind, EntryKind::File);
    }

    #[test]
    fn sort_by_size_largest_first() {
        let mut entries = vec![
            entry("small", EntryKind::File, 10, ""),
            entry("big", EntryKind::File, 1000, ""),
            entry("medium", EntryKind::File, 500, ""),
        ];
        sort_entries(&mut entries, SortField::Size, false);
        assert_eq!(entries[0].name, "big");
        assert_eq!(entries[1].name, "medium");
        assert_eq!(entries[2].name, "small");
    }

    #[test]
    fn sort_by_created_newest_first() {
        let mut entries = vec![
            entry("old", EntryKind::File, 0, "2024-01-01T00:00:00Z"),
            entry("new", EntryKind::File, 0, "2026-01-01T00:00:00Z"),
            entry("mid", EntryKind::File, 0, "2025-06-01T00:00:00Z"),
        ];
        sort_entries(&mut entries, SortField::Created, false);
        assert_eq!(entries[0].name, "new");
        assert_eq!(entries[1].name, "mid");
        assert_eq!(entries[2].name, "old");
    }

    #[test]
    fn sort_none_preserves_order() {
        let mut entries = vec![
            entry("c", EntryKind::File, 0, ""),
            entry("a", EntryKind::File, 0, ""),
            entry("b", EntryKind::File, 0, ""),
        ];
        sort_entries(&mut entries, SortField::None, false);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["c", "a", "b"]);
    }

    #[test]
    fn sort_reverse_within_groups() {
        let mut entries = vec![
            entry("a", EntryKind::File, 10, ""),
            entry("b", EntryKind::File, 20, ""),
            entry("dir_a", EntryKind::Folder, 0, ""),
            entry("dir_b", EntryKind::Folder, 0, ""),
        ];
        sort_entries(&mut entries, SortField::Name, true);
        assert_eq!(entries[0].name, "dir_b");
        assert_eq!(entries[1].name, "dir_a");
        assert_eq!(entries[2].name, "b");
        assert_eq!(entries[3].name, "a");
    }

    #[test]
    fn sort_by_extension() {
        let mut entries = vec![
            entry("file.zip", EntryKind::File, 0, ""),
            entry("doc.txt", EntryKind::File, 0, ""),
            entry("pic.jpg", EntryKind::File, 0, ""),
        ];
        sort_entries(&mut entries, SortField::Extension, false);
        assert_eq!(entries[0].name, "pic.jpg");
        assert_eq!(entries[1].name, "doc.txt");
        assert_eq!(entries[2].name, "file.zip");
    }
}
