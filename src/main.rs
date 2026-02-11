mod backend;
mod native;
mod pikpak;
mod tui;

use crate::backend::Backend;
use crate::native::{NativeBackend, auth::SessionToken};
use crate::pikpak::CliBackend;
use anyhow::Result;
use std::env;
use std::process::{Command, exit};

fn main() {
    if let Err(e) = entry() {
        eprintln!("Error: {e:#}");
        exit(1);
    }
}

fn entry() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && args[1] == "--smoke-auth" {
        smoke_auth_roundtrip()?;
        return Ok(());
    }

    if args.len() == 1 {
        let backend = select_backend()?;
        tui::run(backend)?;
        return Ok(());
    }

    let status = Command::new("pikpakcli")
        .args(&args[1..])
        .status()
        .expect("failed to execute pikpakcli");

    exit(status.code().unwrap_or(1));
}

fn select_backend() -> Result<Box<dyn Backend>> {
    let choice = env::var("PIKPAKTUI_BACKEND").unwrap_or_else(|_| "cli".into());
    if choice.eq_ignore_ascii_case("native") {
        Ok(Box::new(NativeBackend::new()?))
    } else {
        Ok(Box::new(CliBackend))
    }
}

fn smoke_auth_roundtrip() -> Result<()> {
    let backend = NativeBackend::new()?;
    let auth = backend.auth();

    let token = SessionToken {
        access_token: "smoke-access".into(),
        refresh_token: "smoke-refresh".into(),
        expires_at_unix: 4_102_444_800,
    };

    auth.save_session(&token)?;
    let restored = auth.load_session()?.expect("session should exist");
    auth.clear_session()?;

    println!(
        "smoke-auth-ok path={} expires={}",
        auth.session_path().display(),
        restored.expires_at_unix
    );

    Ok(())
}
