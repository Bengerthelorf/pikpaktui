use anyhow::Result;

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RESET: &str = "\x1b[0m";

pub fn run() -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");

    println!(
        "{BOLD}{CYAN}pikpaktui{RESET} {DIM}v{version}{RESET}  {DIM}─{RESET}  A TUI and CLI client for PikPak cloud storage"
    );
    println!();
    println!("{BOLD}Usage:{RESET}  {GREEN}pikpaktui{RESET} {DIM}[command] [args...]{RESET}");
    println!();
    println!("{BOLD}Commands:{RESET}");
    println!(
        "  {YELLOW}{BOLD}(no command){RESET}                    Launch interactive TUI"
    );

    let commands: &[(&str, &str)] = &[
        ("ls [-l] [path]",           "List files (colored grid; long with -l)"),
        ("mv <src> <dst>",           "Move a file or folder"),
        ("cp <src> <dst>",           "Copy a file or folder"),
        ("rename <path> <new_name>", "Rename a file or folder"),
        ("rm [-f] <path>",           "Remove to trash (-f permanent)"),
        ("mkdir <parent> <name>",    "Create a new folder"),
        ("download <path> [local]",  "Download a file"),
        ("upload <local> [remote]",  "Upload a local file"),
        ("share <path> [-o file]",   "Share file(s) as PikPak links"),
        ("quota",                    "Show storage quota"),
        ("offline <url> [--to path]","Cloud download a URL/magnet"),
        ("tasks [list|retry|rm]",   "Manage offline download tasks"),
        ("star <path...>",          "Star files"),
        ("unstar <path...>",        "Unstar files"),
        ("starred [limit]",         "List starred files"),
        ("events [limit]",          "Recent file events"),
        ("vip",                     "Show VIP & account info"),
    ];

    for (cmd, desc) in commands {
        // Split command into name and args parts
        let (name, args) = match cmd.find(' ') {
            Some(i) => (&cmd[..i], &cmd[i..]),
            None => (*cmd, ""),
        };
        println!(
            "  {GREEN}{name}{RESET}{DIM}{args}{RESET}  {:>width$}{DIM}{desc}{RESET}",
            "",
            width = 28usize.saturating_sub(cmd.len()),
        );
    }

    println!();
    println!("{BOLD}Options:{RESET}");
    println!("  {GREEN}-h{RESET}, {GREEN}--help{RESET}                   Show this help message");
    println!("  {GREEN}-V{RESET}, {GREEN}--version{RESET}                Show version");

    Ok(())
}
