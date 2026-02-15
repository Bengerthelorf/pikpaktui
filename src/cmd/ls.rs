use anyhow::{Result, anyhow};
use unicode_width::UnicodeWidthStr;

use crate::config::SortField;
use crate::pikpak::{self, EntryKind};
use crate::theme;

const USAGE: &str = "Usage: pikpaktui ls [-l|--long] [-s|--sort=<field>] [-r|--reverse] [path]\n\nSort fields: name, size, created, type, extension, none";

#[derive(Debug, PartialEq, Eq)]
struct LsArgs {
    path: String,
    long: bool,
    sort_field: SortField,
    reverse: bool,
}

fn parse_sort_field(s: &str) -> Result<SortField> {
    match s {
        "name" => Ok(SortField::Name),
        "size" => Ok(SortField::Size),
        "created" => Ok(SortField::Created),
        "type" => Ok(SortField::Type),
        "extension" | "ext" => Ok(SortField::Extension),
        "none" => Ok(SortField::None),
        _ => Err(anyhow!("unknown sort field: {s}\nValid fields: name, size, created, type, extension, none")),
    }
}

fn parse_args(args: &[String]) -> Result<LsArgs> {
    let mut path: Option<String> = None;
    let mut long = false;
    let mut sort_field = SortField::default();
    let mut reverse = false;
    let mut options_done = false;
    let mut expect_sort = false;

    for arg in args {
        if expect_sort {
            sort_field = parse_sort_field(arg)?;
            expect_sort = false;
            continue;
        }

        if !options_done {
            match arg.as_str() {
                "-l" | "--long" => {
                    long = true;
                    continue;
                }
                "-r" | "--reverse" => {
                    reverse = true;
                    continue;
                }
                "-s" | "--sort" => {
                    expect_sort = true;
                    continue;
                }
                "--" => {
                    options_done = true;
                    continue;
                }
                _ if arg.starts_with("--sort=") => {
                    let val = &arg["--sort=".len()..];
                    sort_field = parse_sort_field(val)?;
                    continue;
                }
                _ if arg.starts_with("-s=") => {
                    let val = &arg["-s=".len()..];
                    sort_field = parse_sort_field(val)?;
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

    Ok(LsArgs {
        path: path.unwrap_or_else(|| "/".to_string()),
        long,
        sort_field,
        reverse,
    })
}

fn format_date(iso: &str) -> String {
    // Input like "2026-01-15T12:30:45.000Z" -> "2026-01-15 12:30"
    if iso.len() >= 16 {
        let s = iso.replace('T', " ");
        s[..16].to_string()
    } else if iso.is_empty() {
        "-".to_string()
    } else {
        iso.to_string()
    }
}

pub fn run(args: &[String]) -> Result<()> {
    let parsed = parse_args(args)?;
    let config = super::cli_config();
    let nerd_font = config.cli_nerd_font;
    let client = super::cli_client()?;
    let parent_id = client.resolve_path(&parsed.path)?;
    let mut entries = client.ls(&parent_id)?;
    crate::config::sort_entries(&mut entries, parsed.sort_field, parsed.reverse);

    if entries.is_empty() {
        println!("(empty)");
        return Ok(());
    }

    if parsed.long {
        print_long(&entries, nerd_font);
    } else {
        print_short(&entries, nerd_font);
    }

    Ok(())
}

fn print_short(entries: &[pikpak::Entry], nerd_font: bool) {
    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);

    // Calculate display width of each entry (icon + name)
    let display_widths: Vec<usize> = entries
        .iter()
        .map(|e| {
            let cat = theme::categorize(e);
            let icon = theme::cli_icon(cat, nerd_font);
            UnicodeWidthStr::width(icon) + UnicodeWidthStr::width(e.name.as_str())
        })
        .collect();

    let max_width = display_widths.iter().copied().max().unwrap_or(1);
    let col_width = max_width + 2; // 2 chars gap
    let num_cols = (term_width / col_width).max(1);
    let num_rows = (entries.len() + num_cols - 1) / num_cols;

    // Column-major order: fill top-to-bottom, then left-to-right
    for row in 0..num_rows {
        for col in 0..num_cols {
            let idx = col * num_rows + row;
            if idx >= entries.len() {
                break;
            }

            let e = &entries[idx];
            let cat = theme::categorize(e);
            let icon = theme::cli_icon(cat, nerd_font);
            let display = format!("{}{}", icon, e.name);
            let colored = theme::cli_colored(&display, cat);

            let is_last_col = col + 1 == num_cols || (col + 1) * num_rows + row >= entries.len();
            if is_last_col {
                print!("{}", colored);
            } else {
                let padding = col_width.saturating_sub(display_widths[idx]);
                print!("{}{}", colored, " ".repeat(padding));
            }
        }
        println!();
    }
}

fn print_long(entries: &[pikpak::Entry], nerd_font: bool) {
    for e in entries {
        let cat = theme::categorize(e);
        let icon = theme::cli_icon(cat, nerd_font);

        let size_str = if e.kind == EntryKind::Folder {
            format!("{:>8}", "-")
        } else {
            format!("{:>8}", super::format_size(e.size))
        };

        let date = format_date(&e.created_time);
        let name_display = format!("{}{}", icon, e.name);
        let colored_name = theme::cli_colored(&name_display, cat);

        // eza-style: dim id, bold green size, blue date
        let colored_id = format!("\x1b[2m{}\x1b[0m", e.id);
        let colored_size = format!("\x1b[1;32m{}\x1b[0m", size_str);
        let padded_date = format!("{:16}", date);
        let colored_date = format!("\x1b[34m{}\x1b[0m", padded_date);

        println!(
            "{}  {}  {}  {}",
            colored_id, colored_size, colored_date, colored_name
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{LsArgs, format_date, parse_args};
    use crate::config::SortField;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parse_defaults_to_root_short_output() {
        assert_eq!(
            parse_args(&s(&[])).unwrap(),
            LsArgs {
                path: "/".to_string(),
                long: false,
                sort_field: SortField::Name,
                reverse: false,
            }
        );
    }

    #[test]
    fn parse_supports_path_and_long_in_any_order() {
        assert_eq!(
            parse_args(&s(&["/foo", "-l"])).unwrap(),
            LsArgs {
                path: "/foo".to_string(),
                long: true,
                sort_field: SortField::Name,
                reverse: false,
            }
        );
        assert_eq!(
            parse_args(&s(&["-l", "/foo"])).unwrap(),
            LsArgs {
                path: "/foo".to_string(),
                long: true,
                sort_field: SortField::Name,
                reverse: false,
            }
        );
    }

    #[test]
    fn parse_sort_field_flag() {
        assert_eq!(
            parse_args(&s(&["--sort=size"])).unwrap(),
            LsArgs {
                path: "/".to_string(),
                long: false,
                sort_field: SortField::Size,
                reverse: false,
            }
        );
        assert_eq!(
            parse_args(&s(&["-s", "created"])).unwrap(),
            LsArgs {
                path: "/".to_string(),
                long: false,
                sort_field: SortField::Created,
                reverse: false,
            }
        );
        assert_eq!(
            parse_args(&s(&["--sort", "extension"])).unwrap(),
            LsArgs {
                path: "/".to_string(),
                long: false,
                sort_field: SortField::Extension,
                reverse: false,
            }
        );
    }

    #[test]
    fn parse_reverse_flag() {
        assert_eq!(
            parse_args(&s(&["-r", "--sort=size"])).unwrap(),
            LsArgs {
                path: "/".to_string(),
                long: false,
                sort_field: SortField::Size,
                reverse: true,
            }
        );
        assert_eq!(
            parse_args(&s(&["--reverse"])).unwrap(),
            LsArgs {
                path: "/".to_string(),
                long: false,
                sort_field: SortField::Name,
                reverse: true,
            }
        );
    }

    #[test]
    fn parse_sort_rejects_invalid_field() {
        let err = parse_args(&s(&["--sort=bogus"])).unwrap_err();
        assert!(err.to_string().contains("unknown sort field"));
    }

    #[test]
    fn parse_sort_requires_value() {
        let err = parse_args(&s(&["--sort"])).unwrap_err();
        assert!(err.to_string().contains("--sort requires a value"));
    }

    #[test]
    fn parse_rejects_unknown_options() {
        let err = parse_args(&s(&["-a"])).unwrap_err();
        assert!(err.to_string().contains("unknown option for ls"));
    }

    #[test]
    fn parse_rejects_multiple_paths() {
        let err = parse_args(&s(&["/a", "/b"])).unwrap_err();
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
            starred: false,
            thumbnail_link: None,
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
        // Folders still first, but reversed within each group
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
