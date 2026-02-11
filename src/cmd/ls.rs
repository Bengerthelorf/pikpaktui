use anyhow::{Result, anyhow};
use unicode_width::UnicodeWidthStr;

use crate::pikpak::{self, EntryKind};
use crate::theme;

const USAGE: &str = "Usage: pikpaktui ls [-l|--long] [path]";

#[derive(Debug, PartialEq, Eq)]
struct LsArgs {
    path: String,
    long: bool,
}

fn parse_args(args: &[String]) -> Result<LsArgs> {
    let mut path: Option<String> = None;
    let mut long = false;
    let mut options_done = false;

    for arg in args {
        if !options_done {
            match arg.as_str() {
                "-l" | "--long" => {
                    long = true;
                    continue;
                }
                "--" => {
                    options_done = true;
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

    Ok(LsArgs {
        path: path.unwrap_or_else(|| "/".to_string()),
        long,
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
    let entries = client.ls(&parent_id)?;

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
            }
        );
        assert_eq!(
            parse_args(&s(&["-l", "/foo"])).unwrap(),
            LsArgs {
                path: "/foo".to_string(),
                long: true,
            }
        );
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
