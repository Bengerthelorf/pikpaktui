mod pikpak;
mod tui;

use std::env;
use std::process::{Command, exit};

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() == 1 {
        if let Err(e) = tui::run() {
            eprintln!("Error: {e:#}");
            exit(1);
        }
        return;
    }

    let status = Command::new("pikpakcli")
        .args(&args[1..])
        .status()
        .expect("failed to execute pikpakcli");

    exit(status.code().unwrap_or(1));
}
