pub mod cat;
pub mod complete_path;
pub mod completions;
pub mod cp;
pub mod download;
pub mod events;
pub mod help;
pub mod info;
pub mod link;
pub mod ls;
pub mod mkdir;
pub mod mv;
pub mod offline;
pub mod play;
pub mod quota;
pub mod rename;
pub mod rm;
pub mod share;
pub mod star;
pub mod starred;
pub mod tasks;
pub mod trash;
pub mod unstar;
pub mod untrash;
pub mod upload;
pub mod vip;

use crate::config::AppConfig;
use crate::pikpak::{self, PikPak};
use anyhow::{Result, anyhow};

// ── Per-command help ────────────────────────────────────────────

const G: &str = "\x1b[32m";  // green
const _C: &str = "\x1b[36m"; // cyan (reserved)
const D: &str = "\x1b[2m";   // dim
const B: &str = "\x1b[1m";   // bold
const R: &str = "\x1b[0m";   // reset

/// Returns true if the arg slice contains `-h` or `--help`.
pub fn wants_help(args: &[String]) -> bool {
    args.iter().any(|a| a == "-h" || a == "--help")
}

/// Print per-command help. Returns `Ok(())` so it can be used as an early return.
pub fn print_command_help(cmd: &str) -> Result<()> {
    let (usage, desc, body) = command_help_text(cmd);
    println!("{B}pikpaktui {G}{cmd}{R} {D}─{R} {desc}");
    println!();
    println!("{B}USAGE:{R}  {G}pikpaktui{R} {usage}");
    println!();
    print!("{body}");
    Ok(())
}

fn command_help_text(cmd: &str) -> (&'static str, &'static str, String) {
    match cmd {
        "ls" => (
            "ls [options] [path]",
            "List files and folders",
            format!(
                "{B}OPTIONS:{R}\n\
                 {opt}  -l, --long       {d}Long format (id, size, date, name){R}\n\
                 {opt}  -J, --json       {d}Output as JSON{R}\n\
                 {opt}  -s, --sort=FIELD {d}Sort by: name, size, created, type, extension, none{R}\n\
                 {opt}  -r, --reverse    {d}Reverse sort order{R}\n\
                 {opt}  --tree           {d}Tree view{R}\n\
                 {opt}  --depth=N        {d}Max tree depth{R}\n\
                 \n{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui ls{R}\n\
                 {ex}  pikpaktui ls -l /Movies{R}\n\
                 {ex}  pikpaktui ls --tree --depth=2 /{R}\n",
                opt = G, d = D, ex = D,
            ),
        ),
        "mv" => (
            "mv [options] <src> <dst>",
            "Move (rename) files or folders",
            format!(
                "{B}OPTIONS:{R}\n\
                 {opt}  -n, --dry-run    {d}Preview without executing{R}\n\
                 {opt}  -t <dst>         {d}Batch mode: move multiple <src> into <dst>{R}\n\
                 \n{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui mv /file.txt /Archive/{R}\n\
                 {ex}  pikpaktui mv -t /Dest /a.txt /b.txt{R}\n",
                opt = G, d = D, ex = D,
            ),
        ),
        "cp" => (
            "cp [options] <src> <dst>",
            "Copy files or folders",
            format!(
                "{B}OPTIONS:{R}\n\
                 {opt}  -n, --dry-run    {d}Preview without executing{R}\n\
                 {opt}  -t <dst>         {d}Batch mode: copy multiple <src> into <dst>{R}\n\
                 \n{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui cp /file.txt /Backup/{R}\n\
                 {ex}  pikpaktui cp -t /Dest /a.txt /b.txt{R}\n",
                opt = G, d = D, ex = D,
            ),
        ),
        "rename" => (
            "rename [options] <path> <new_name>",
            "Rename a file or folder",
            format!(
                "{B}OPTIONS:{R}\n\
                 {opt}  -n, --dry-run    {d}Preview without executing{R}\n\
                 \n{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui rename /old.txt new.txt{R}\n",
                opt = G, d = D, ex = D,
            ),
        ),
        "rm" => (
            "rm [options] <path...>",
            "Remove files or folders",
            format!(
                "{B}OPTIONS:{R}\n\
                 {opt}  -r, --recursive  {d}Remove folders recursively{R}\n\
                 {opt}  -f, --force      {d}Permanently delete (skip trash){R}\n\
                 \n{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui rm /file.txt{R}\n\
                 {ex}  pikpaktui rm -rf /old-folder{R}\n",
                opt = G, d = D, ex = D,
            ),
        ),
        "mkdir" => (
            "mkdir [options] <parent> <name>",
            "Create a new folder",
            format!(
                "{B}OPTIONS:{R}\n\
                 {opt}  -n, --dry-run    {d}Preview without executing{R}\n\
                 {opt}  -p               {d}Create intermediate directories{R}\n\
                 \n{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui mkdir / NewFolder{R}\n\
                 {ex}  pikpaktui mkdir -p / path/to/deep/folder{R}\n",
                opt = G, d = D, ex = D,
            ),
        ),
        "download" => (
            "download [options] <path>",
            "Download files or folders",
            format!(
                "{B}OPTIONS:{R}\n\
                 {opt}  -o, --output <file> {d}Output file name{R}\n\
                 {opt}  -t <local_dir>      {d}Batch: download multiple paths into dir{R}\n\
                 {opt}  -j, --jobs <n>      {d}Concurrent downloads (default: 1){R}\n\
                 {opt}  -n, --dry-run       {d}Preview without downloading{R}\n\
                 \n{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui download /movie.mkv{R}\n\
                 {ex}  pikpaktui download -j4 -t ./local /Movies{R}\n",
                opt = G, d = D, ex = D,
            ),
        ),
        "upload" => (
            "upload [options] <local_path>",
            "Upload files to PikPak",
            format!(
                "{B}OPTIONS:{R}\n\
                 {opt}  -t <remote_dir>  {d}Batch: upload multiple files into dir{R}\n\
                 {opt}  -n, --dry-run    {d}Preview without uploading{R}\n\
                 \n{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui upload file.txt{R}\n\
                 {ex}  pikpaktui upload -t /Remote a.txt b.txt{R}\n",
                opt = G, d = D, ex = D,
            ),
        ),
        "share" => (
            "share [options] <path...>",
            "Create, list, save, or delete share links",
            format!(
                "{B}MODES:{R}\n\
                 {opt}  share <path...>        {d}Create a share link{R}\n\
                 {opt}  share -l               {d}List your shares{R}\n\
                 {opt}  share -S <url>         {d}Save a share to your drive{R}\n\
                 {opt}  share -D <id...>       {d}Delete share(s){R}\n\
                 \n{B}OPTIONS (create):{R}\n\
                 {opt}  -p, --password   {d}Protect with a password{R}\n\
                 {opt}  -d, --days <n>   {d}Expiry in days (-1 = permanent){R}\n\
                 {opt}  -o <file>        {d}Write share URL to file{R}\n\
                 {opt}  -J, --json       {d}Output as JSON{R}\n\
                 \n{B}OPTIONS (save):{R}\n\
                 {opt}  -p <code>        {d}Pass code for protected shares{R}\n\
                 {opt}  -t <path>        {d}Destination folder{R}\n\
                 {opt}  -n, --dry-run    {d}Preview without saving{R}\n\
                 \n{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui share /movie.mkv{R}\n\
                 {ex}  pikpaktui share -p -d 7 /folder{R}\n\
                 {ex}  pikpaktui share -l{R}\n\
                 {ex}  pikpaktui share -S https://mypikpak.com/s/abc123{R}\n\
                 {ex}  pikpaktui share -D abc123{R}\n",
                opt = G, d = D, ex = D,
            ),
        ),
        "offline" => (
            "offline [options] <url>",
            "Cloud download a URL or magnet link",
            format!(
                "{B}OPTIONS:{R}\n\
                 {opt}  --to <path>      {d}Destination folder in PikPak{R}\n\
                 {opt}  -n, --dry-run    {d}Preview without creating task{R}\n\
                 \n{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui offline https://example.com/file.zip{R}\n\
                 {ex}  pikpaktui offline --to /Downloads magnet:?xt=...{R}\n",
                opt = G, d = D, ex = D,
            ),
        ),
        "tasks" => (
            "tasks [subcommand] [options]",
            "Manage offline download tasks",
            format!(
                "{B}SUBCOMMANDS:{R}\n\
                 {opt}  list, ls         {d}List tasks (default){R}\n\
                 {opt}  retry <id>       {d}Retry a failed task{R}\n\
                 {opt}  delete, rm <id...> {d}Delete task(s){R}\n\
                 \n{B}OPTIONS:{R}\n\
                 {opt}  -J, --json       {d}Output as JSON{R}\n\
                 {opt}  -n, --dry-run    {d}Preview without executing{R}\n\
                 {opt}  <number>         {d}Limit results (default: 50){R}\n\
                 \n{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui tasks{R}\n\
                 {ex}  pikpaktui tasks list 10{R}\n\
                 {ex}  pikpaktui tasks retry abc12345{R}\n\
                 {ex}  pikpaktui tasks delete abc12345{R}\n",
                opt = G, d = D, ex = D,
            ),
        ),
        "info" => (
            "info [options] <path>",
            "Show detailed file or folder info",
            format!(
                "{B}OPTIONS:{R}\n\
                 {opt}  -J, --json       {d}Output as JSON{R}\n\
                 \n{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui info /movie.mkv{R}\n",
                opt = G, d = D, ex = D,
            ),
        ),
        "link" => (
            "link [options] <path>",
            "Get direct download URL",
            format!(
                "{B}OPTIONS:{R}\n\
                 {opt}  -m, --media      {d}Show media stream URLs{R}\n\
                 {opt}  -c, --copy       {d}Copy URL to clipboard{R}\n\
                 {opt}  -J, --json       {d}Output as JSON{R}\n\
                 \n{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui link /movie.mkv{R}\n\
                 {ex}  pikpaktui link -mc /movie.mkv{R}\n",
                opt = G, d = D, ex = D,
            ),
        ),
        "cat" => (
            "cat <path>",
            "Preview text file contents",
            format!(
                "{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui cat /notes.txt{R}\n",
                ex = D,
            ),
        ),
        "play" => (
            "play <path> [quality]",
            "Play video with external player",
            format!(
                "{B}ARGUMENTS:{R}\n\
                 {opt}  quality          {d}Stream quality (e.g. 720, 1080, original){R}\n\
                 \n{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui play /movie.mkv{R}\n\
                 {ex}  pikpaktui play /movie.mkv 1080{R}\n",
                opt = G, d = D, ex = D,
            ),
        ),
        "quota" => (
            "quota [options]",
            "Show storage quota and bandwidth",
            format!(
                "{B}OPTIONS:{R}\n\
                 {opt}  -J, --json       {d}Output as JSON{R}\n\
                 \n{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui quota{R}\n",
                opt = G, d = D, ex = D,
            ),
        ),
        "events" => (
            "events [options] [limit]",
            "List recent file events",
            format!(
                "{B}OPTIONS:{R}\n\
                 {opt}  -J, --json       {d}Output as JSON{R}\n\
                 {opt}  <number>         {d}Limit results (default: 20){R}\n\
                 \n{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui events{R}\n\
                 {ex}  pikpaktui events 50{R}\n",
                opt = G, d = D, ex = D,
            ),
        ),
        "trash" => (
            "trash [limit]",
            "List trashed files",
            format!(
                "{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui trash{R}\n\
                 {ex}  pikpaktui trash 50{R}\n",
                ex = D,
            ),
        ),
        "untrash" => (
            "untrash <name...>",
            "Restore files from trash",
            format!(
                "{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui untrash file.txt{R}\n",
                ex = D,
            ),
        ),
        "star" => (
            "star <path...>",
            "Star files",
            format!(
                "{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui star /movie.mkv /photo.jpg{R}\n",
                ex = D,
            ),
        ),
        "unstar" => (
            "unstar <path...>",
            "Unstar files",
            format!(
                "{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui unstar /movie.mkv{R}\n",
                ex = D,
            ),
        ),
        "starred" => (
            "starred [limit]",
            "List starred files",
            format!(
                "{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui starred{R}\n\
                 {ex}  pikpaktui starred 50{R}\n",
                ex = D,
            ),
        ),
        "vip" => (
            "vip",
            "Show VIP and account info",
            String::new(),
        ),
        "completions" => (
            "completions <shell>",
            "Generate shell completions",
            format!(
                "{B}SUPPORTED SHELLS:{R}\n\
                 {opt}  zsh{R}\n\
                 \n{B}EXAMPLES:{R}\n\
                 {ex}  pikpaktui completions zsh > _pikpaktui{R}\n",
                opt = G, ex = D,
            ),
        ),
        _ => (
            "<command>",
            "Unknown command",
            format!("Run {G}pikpaktui --help{R} for a list of all commands.\n"),
        ),
    }
}

pub fn cli_config() -> crate::config::TuiConfig {
    crate::config::TuiConfig::load()
}

pub fn cli_client() -> Result<PikPak> {
    let mut client = PikPak::new()?;
    client.thumbnail_size = cli_config().thumbnail_size.as_api_str().to_string();

    if client.has_valid_session() {
        return Ok(client);
    }

    // Try login.yaml
    let cfg = AppConfig::load()?;
    match (cfg.username, cfg.password) {
        (Some(u), Some(p)) if !u.is_empty() && !p.is_empty() => {
            client.login(&u, &p)?;
            Ok(client)
        }
        _ => Err(anyhow!(
            "not logged in. Run `pikpaktui` (TUI) to login first, or set credentials in login.yaml"
        )),
    }
}

pub fn split_parent_name(path: &str) -> Result<(String, String)> {
    let path = path.trim().trim_end_matches('/');
    if path.is_empty() || path == "/" {
        return Err(anyhow!("invalid path: cannot operate on root"));
    }
    match path.rsplit_once('/') {
        Some(("", name)) => Ok(("/".to_string(), name.to_string())),
        Some((parent, name)) => Ok((parent.to_string(), name.to_string())),
        None => Ok(("/".to_string(), path.to_string())),
    }
}

pub fn find_entry(client: &PikPak, parent_id: &str, name: &str) -> Result<pikpak::Entry> {
    let entries = client.ls_cached(parent_id)?;
    entries
        .into_iter()
        .find(|e| e.name == name)
        .ok_or_else(|| anyhow!("'{}' not found", name))
}

/// eza-style grid output (column-major) for a list of entries.
pub fn print_entries_short(entries: &[pikpak::Entry], nerd_font: bool) {
    use crate::theme;
    use unicode_width::UnicodeWidthStr;

    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);

    let display_widths: Vec<usize> = entries
        .iter()
        .map(|e| {
            let cat = theme::categorize(e);
            let icon = theme::cli_icon(cat, nerd_font);
            UnicodeWidthStr::width(icon) + UnicodeWidthStr::width(e.name.as_str())
        })
        .collect();

    let max_width = display_widths.iter().copied().max().unwrap_or(1);
    let col_width = max_width + 2;
    let num_cols = (term_width / col_width).max(1);
    let num_rows = (entries.len() + num_cols - 1) / num_cols;

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

/// Returns the colored `id  size  date  ` prefix used in long-format output.
/// Shared between `print_entries_long` and tree long mode.
pub fn long_entry_prefix(e: &pikpak::Entry) -> String {
    let size_str = if e.kind == pikpak::EntryKind::Folder {
        format!("{:>9}", "-")
    } else {
        format!("{:>9}", format_size(e.size))
    };
    let date = format_date(&e.created_time);
    let colored_id = format!("\x1b[2m{}\x1b[0m", e.id);
    let colored_size = format!("\x1b[1;32m{}\x1b[0m", size_str);
    let colored_date = format!("\x1b[34m{:16}\x1b[0m", date);
    format!("{}  {}  {}  ", colored_id, colored_size, colored_date)
}

/// eza-style long format output: id, size, date, icon+name.
pub fn print_entries_long(entries: &[pikpak::Entry], nerd_font: bool) {
    use crate::theme;

    for e in entries {
        let cat = theme::categorize(e);
        let icon = theme::cli_icon(cat, nerd_font);
        let name_display = format!("{}{}", icon, e.name);
        let colored_name = theme::cli_colored(&name_display, cat);
        println!("{}{}", long_entry_prefix(e), colored_name);
    }
}

pub fn print_entries_json(entries: &[pikpak::Entry]) {
    let json = serde_json::to_string_pretty(entries).unwrap_or_else(|_| "[]".into());
    println!("{}", json);
}

pub fn format_date(iso: &str) -> String {
    if iso.len() >= 16 {
        let s = iso.replace('T', " ");
        s[..16].to_string()
    } else if iso.is_empty() {
        "-".to_string()
    } else {
        iso.to_string()
    }
}

/// A simple CLI loading spinner on stderr.
pub struct Spinner {
    running: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Spinner {
    pub fn new(msg: &str) -> Self {
        use std::io::Write;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        // Only show spinner if stderr is a terminal
        if !std::io::stderr().is_terminal() {
            return Self {
                running: Arc::new(AtomicBool::new(false)),
                handle: None,
            };
        }

        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();
        let msg = msg.to_string();
        let handle = std::thread::spawn(move || {
            let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let mut i = 0;
            while r.load(Ordering::Relaxed) {
                eprint!("\r\x1b[36m{}\x1b[0m {}", frames[i % frames.len()], msg);
                let _ = std::io::stderr().flush();
                i += 1;
                std::thread::sleep(std::time::Duration::from_millis(80));
            }
            let clear_len = msg.len() + 4;
            eprint!("\r{}\r", " ".repeat(clear_len));
            let _ = std::io::stderr().flush();
        });
        Self {
            running,
            handle: Some(handle),
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

use std::io::IsTerminal;

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    const TB: u64 = 1024 * GB;

    if bytes >= TB {
        format!("{:.1} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
