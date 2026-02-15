mod cmd;
mod config;
mod pikpak;
mod theme;
mod tui;

use crate::config::{AppConfig, TuiConfig};
use crate::pikpak::PikPak;
use anyhow::{Result, anyhow};
use std::env;
use std::process::exit;

fn main() {
    if let Err(e) = entry() {
        eprintln!("Error: {e:#}");
        exit(1);
    }
}

fn entry() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        return run_tui();
    }

    match args[0].as_str() {
        "--version" | "-V" | "version" => {
            println!("pikpaktui {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "--help" | "-h" | "help" => cmd::help::run(),
        "ls" => cmd::ls::run(&args[1..]),
        "mv" => cmd::mv::run(&args[1..]),
        "cp" => cmd::cp::run(&args[1..]),
        "rename" => cmd::rename::run(&args[1..]),
        "rm" => cmd::rm::run(&args[1..]),
        "mkdir" => cmd::mkdir::run(&args[1..]),
        "download" => cmd::download::run(&args[1..]),
        "upload" => cmd::upload::run(&args[1..]),
        "share" => cmd::share::run(&args[1..]),
        "quota" => cmd::quota::run(),
        "offline" => cmd::offline::run(&args[1..]),
        "tasks" => cmd::tasks::run(&args[1..]),
        "star" => cmd::star::run(&args[1..]),
        "unstar" => cmd::unstar::run(&args[1..]),
        "starred" => cmd::starred::run(&args[1..]),
        "events" => cmd::events::run(&args[1..]),
        "vip" => cmd::vip::run(),
        "completions" => cmd::completions::run(&args[1..]),
        "__complete_path" => cmd::complete_path::run(&args[1..]),
        other => Err(anyhow!(
            "unknown command: {other}\nRun `pikpaktui --help` for usage."
        )),
    }
}

fn run_tui() -> Result<()> {
    let client = PikPak::new()?;
    let tui_config = TuiConfig::load();

    if client.has_valid_session() {
        return tui::run(client, tui_config);
    }

    // Check login.yaml for credentials
    let cfg = AppConfig::load()?;
    let credentials = match (cfg.username, cfg.password) {
        (Some(u), Some(p)) if !u.is_empty() && !p.is_empty() => Some((u, p)),
        _ => None,
    };

    tui::run_with_credentials(client, credentials, tui_config)
}
