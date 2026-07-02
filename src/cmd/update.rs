use anyhow::Result;
use self_update::cargo_crate_version;

fn platform_target() -> &'static str {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "linux") => "x86_64-linux",
        ("aarch64", "linux") => "aarch64-linux",
        ("x86_64", "macos") => "x86_64-macos",
        ("aarch64", "macos") => "aarch64-macos",
        ("x86_64", "windows") => "x86_64-windows",
        ("aarch64", "windows") => "aarch64-windows",
        ("x86_64", "freebsd") => "x86_64-freebsd",
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
    // Dropping non-numeric segments would make "v0.6.0-rc1" equal "v0.6.0".
    fn parse(v: &str) -> (Vec<u32>, Option<&str>) {
        let v = v.trim_start_matches('v');
        let (core, pre) = match v.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (v, None),
        };
        (
            core.split('.').filter_map(|s| s.parse().ok()).collect(),
            pre,
        )
    }
    let (latest_core, latest_pre) = parse(latest);
    let (current_core, current_pre) = parse(current);
    if latest_core != current_core {
        return latest_core > current_core;
    }
    // Same numeric core: a release outranks any pre-release; between
    // pre-releases, lexical order is good enough for rc1 < rc2.
    match (latest_pre, current_pre) {
        (None, Some(_)) => true,
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
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

#[cfg(test)]
mod tests {
    use super::version_newer;

    #[test]
    fn plain_numeric_comparison() {
        assert!(version_newer("v0.0.57", "0.0.56"));
        assert!(!version_newer("0.0.56", "0.0.56"));
        assert!(!version_newer("v0.0.55", "0.0.56"));
    }

    #[test]
    fn prerelease_is_older_than_release() {
        assert!(!version_newer("v0.6.0-rc1", "0.6.0"));
        assert!(version_newer("v0.6.0", "0.6.0-rc1"));
    }

    #[test]
    fn prereleases_compare_between_themselves() {
        assert!(version_newer("0.6.0-rc2", "0.6.0-rc1"));
        assert!(!version_newer("0.6.0-rc1", "0.6.0-rc2"));
    }
}
