use anyhow::Result;

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";
const MAGENTA: &str = "\x1b[35m";
const LIGHT_CYAN: &str = "\x1b[96m";
const LIGHT_BLUE: &str = "\x1b[94m";
const LIGHT_MAGENTA: &str = "\x1b[95m";
const RESET: &str = "\x1b[0m";

pub fn run() -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");

    // ASCII art banner with gradient
    let art: &[(&str, &str)] = &[
        (LIGHT_CYAN,    r#"    dMMMMb  dMP dMP dMP dMMMMb  .aMMMb  dMP dMP dMMMMMMP dMP dMP dMP"#),
        (CYAN,          r#"   dMP.dMP amr dMP.dMP dMP.dMP dMP"dMP dMP.dMP    dMP   dMP dMP amr "#),
        (LIGHT_BLUE,    r#"  dMMMMP" dMP dMMMMK" dMMMMP" dMMMMMP dMMMMK"    dMP   dMP dMP dMP  "#),
        (BLUE,          r#" dMP     dMP dMP"AMF dMP     dMP dMP dMP"AMF    dMP   dMP.aMP dMP   "#),
        (LIGHT_MAGENTA, r#"dMP     dMP dMP dMP dMP     dMP dMP dMP dMP    dMP    VMMMP" dMP    "#),
    ];

    println!();
    for (color, line) in art {
        println!("  {BOLD}{color}{line}{RESET}");
    }
    println!();
    println!(
        "  {BOLD}{CYAN}pikpaktui{RESET} {DIM}v{version}{RESET}  {DIM}─{RESET}  A TUI and CLI client for PikPak cloud storage"
    );
    println!();

    // Usage
    println!("{BOLD}USAGE:{RESET}  {GREEN}pikpaktui{RESET} {DIM}[command] [args...]{RESET}");
    println!();

    // Commands — grouped by category
    println!("{BOLD}COMMANDS:{RESET}");
    println!(
        "  {YELLOW}{BOLD}(no command){RESET}                    {DIM}Launch interactive TUI{RESET}"
    );
    println!();

    // File Management
    println!("  {MAGENTA}{BOLD}File Management{RESET}");
    let file_cmds: &[(&str, &str)] = &[
        ("ls [-l] [-s field] [path]", "List files (sort: name,size,created,type,ext)"),
        ("search [-l] <keyword>",     "Search files by name across the drive"),
        ("mv [-t dst] <src> [dst]",  "Move file(s) (-t for batch)"),
        ("cp [-t dst] <src> [dst]",  "Copy file(s) (-t for batch)"),
        ("rename <path> <new_name>", "Rename a file or folder"),
        ("rm [-r] [-f] <path...>",   "Remove to trash (-r folder, -f permanent)"),
        ("mkdir [-p] <parent> <name>","Create folder (-p recursive)"),
        ("info <path>",             "Show detailed file/folder info"),
        ("cat <path>",              "Preview text file contents"),
    ];
    print_commands(file_cmds);
    println!();

    // Playback
    println!("  {MAGENTA}{BOLD}Playback{RESET}");
    let play_cmds: &[(&str, &str)] = &[
        ("play <path> [quality]",   "Play video with external player"),
    ];
    print_commands(play_cmds);
    println!();

    // Transfer
    println!("  {MAGENTA}{BOLD}Transfer{RESET}");
    let transfer_cmds: &[(&str, &str)] = &[
        ("download [-o out] <path>", "Download (-o output, -t dir for batch)"),
        ("upload [-t remote] <local>","Upload (-t remote dir for batch)"),
        ("share <path> [-o file]",   "Share file(s) as PikPak links"),
    ];
    print_commands(transfer_cmds);
    println!();

    // Cloud Download
    println!("  {MAGENTA}{BOLD}Cloud Download{RESET}");
    let cloud_cmds: &[(&str, &str)] = &[
        ("offline <url> [--to path]","Cloud download a URL or magnet link"),
        ("tasks [list|retry|rm]",    "Manage offline download tasks"),
    ];
    print_commands(cloud_cmds);
    println!();

    // Trash
    println!("  {MAGENTA}{BOLD}Trash{RESET}");
    let trash_cmds: &[(&str, &str)] = &[
        ("trash [limit]",            "List trashed files"),
        ("untrash <name...>",        "Restore files from trash"),
    ];
    print_commands(trash_cmds);
    println!();

    // Starred & Activity
    println!("  {MAGENTA}{BOLD}Starred & Activity{RESET}");
    let star_cmds: &[(&str, &str)] = &[
        ("star <path...>",           "Star files"),
        ("unstar <path...>",         "Unstar files"),
        ("starred [limit]",          "List starred files"),
        ("events [limit]",           "Recent file events"),
    ];
    print_commands(star_cmds);
    println!();

    // Account
    println!("  {MAGENTA}{BOLD}Account{RESET}");
    let acct_cmds: &[(&str, &str)] = &[
        ("quota",                    "Show storage quota"),
        ("vip",                      "Show VIP & account info"),
    ];
    print_commands(acct_cmds);
    println!();

    // Utility
    println!("  {MAGENTA}{BOLD}Utility{RESET}");
    let util_cmds: &[(&str, &str)] = &[
        ("completions <shell>",      "Generate shell completions (zsh)"),
    ];
    print_commands(util_cmds);

    println!();
    println!("{BOLD}OPTIONS:{RESET}");
    println!("  {GREEN}-h{RESET}, {GREEN}--help{RESET}                   Show this help message");
    println!("  {GREEN}-V{RESET}, {GREEN}--version{RESET}                Show version");
    println!();
    println!(
        "{DIM}TIP: Launch the TUI (no command) and press {RESET}{YELLOW}h{RESET}{DIM} for interactive help.{RESET}"
    );

    Ok(())
}

fn print_commands(cmds: &[(&str, &str)]) {
    for (cmd, desc) in cmds {
        let (name, args) = match cmd.find(' ') {
            Some(i) => (&cmd[..i], &cmd[i..]),
            None => (*cmd, ""),
        };
        println!(
            "    {GREEN}{name}{RESET}{DIM}{args}{RESET}  {:>width$}{DIM}{desc}{RESET}",
            "",
            width = 26usize.saturating_sub(cmd.len()),
        );
    }
}
