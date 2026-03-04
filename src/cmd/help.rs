use anyhow::Result;

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
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
        (LIGHT_BLUE,    r#" dMP     dMP dMP"AMF dMP     dMP dMP dMP"AMF    dMP   dMP.aMP dMP   "#),
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

    // Commands — generated from the single source of truth
    println!("{BOLD}COMMANDS:{RESET}");
    println!(
        "  {YELLOW}{BOLD}(no command){RESET}                    {DIM}Launch interactive TUI{RESET}"
    );
    println!();

    for (group, cmds) in super::COMMAND_GROUPS {
        println!("  {MAGENTA}{BOLD}{group}{RESET}");
        for cmd in *cmds {
            let (usage, desc, _) = super::command_help_text(cmd);
            // usage starts with "cmd ...", print it as: green name + dim args + right-align desc
            let (name, args) = match usage.find(' ') {
                Some(i) => (&usage[..i], &usage[i..]),
                None => (usage, ""),
            };
            println!(
                "    {GREEN}{name}{RESET}{DIM}{args}{RESET}  {:>width$}{DIM}{desc}{RESET}",
                "",
                width = 26usize.saturating_sub(usage.len()),
            );
        }
        println!();
    }

    println!("{BOLD}OPTIONS:{RESET}");
    println!("  {GREEN}-h{RESET}, {GREEN}--help{RESET}                   Show this help message");
    println!("  {GREEN}-V{RESET}, {GREEN}--version{RESET}                Show version");
    println!();
    println!(
        "{DIM}TIP: Run {RESET}{GREEN}pikpaktui <command> --help{RESET}{DIM} for detailed command help.{RESET}"
    );
    println!(
        "{DIM}     Launch the TUI (no command) and press {RESET}{YELLOW}h{RESET}{DIM} for interactive help.{RESET}"
    );

    Ok(())
}
