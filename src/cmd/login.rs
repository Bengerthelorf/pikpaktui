use anyhow::{Result, anyhow};
use crate::config::AppConfig;
use crate::pikpak::PikPak;

pub fn run(args: &[String]) -> Result<()> {
    if super::wants_help(args) {
        return super::print_command_help("login");
    }

    let mut user: Option<String> = None;
    let mut password: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-u" | "--user" => {
                i += 1;
                user = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow!("missing value for --user"))?
                        .clone(),
                );
            }
            "-p" | "--password" => {
                i += 1;
                password = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow!("missing value for --password"))?
                        .clone(),
                );
            }
            other => {
                return Err(anyhow!("unknown flag: {other}\nRun `pikpaktui login --help` for usage."));
            }
        }
        i += 1;
    }

    let user = user
        .or_else(|| std::env::var("PIKPAK_USER").ok())
        .ok_or_else(|| {
            anyhow!(
                "no username provided.\n\
                 Use -u <email> or set the PIKPAK_USER environment variable.\n\
                 Run `pikpaktui login --help` for usage."
            )
        })?;

    let password = password
        .or_else(|| std::env::var("PIKPAK_PASS").ok())
        .ok_or_else(|| {
            anyhow!(
                "no password provided.\n\
                 Use -p <password> or set the PIKPAK_PASS environment variable.\n\
                 Run `pikpaktui login --help` for usage."
            )
        })?;

    let spinner = super::Spinner::new("Logging in...");
    let mut client = PikPak::new()?;
    client.login(&user, &password)?;
    drop(spinner);

    AppConfig::save_credentials(&user, &password)?;

    println!("\x1b[32m✓\x1b[0m Logged in as \x1b[1m{}\x1b[0m", user);
    println!("\x1b[2mCredentials saved to login.toml\x1b[0m");

    Ok(())
}
