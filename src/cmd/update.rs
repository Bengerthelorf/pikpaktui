use anyhow::Result;
use self_update::cargo_crate_version;

fn platform_target() -> &'static str {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "linux") => "x86_64-linux",
        ("x86_64", "macos") => "x86_64-macos",
        ("aarch64", "macos") => "aarch64-macos",
        ("x86_64", "windows") => "x86_64-windows",
        (arch, os) => {
            eprintln!("Unsupported platform: {}-{}", arch, os);
            std::process::exit(1);
        }
    }
}

pub fn check_for_update() -> Option<String> {
    let releases = self_update::backends::github::ReleaseList::configure()
        .repo_owner("Bengerthelorf")
        .repo_name("pikpaktui")
        .build()
        .ok()?
        .fetch()
        .ok()?;

    let latest = releases.first()?;
    let current = cargo_crate_version!();

    if version_newer(&latest.version, current) {
        Some(latest.version.clone())
    } else {
        None
    }
}

fn version_newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u32> {
        v.trim_start_matches('v')
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect()
    };
    parse(latest) > parse(current)
}

pub fn run() -> Result<()> {
    let current = cargo_crate_version!();
    println!("Current version: {}", current);
    println!("Checking for updates...");

    let status = self_update::backends::github::Update::configure()
        .repo_owner("Bengerthelorf")
        .repo_name("pikpaktui")
        .bin_name("pikpaktui")
        .target(platform_target())
        .show_download_progress(true)
        .current_version(current)
        .build()?
        .update()?;

    if status.updated() {
        println!("Updated to version {}!", status.version());
    } else {
        println!("Already up to date.");
    }

    Ok(())
}
