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
    use anyhow::{Context, anyhow};

    let current = cargo_crate_version!();
    println!("Current version: {}", current);
    println!("Checking for updates...");

    let releases = self_update::backends::github::ReleaseList::configure()
        .repo_owner("Bengerthelorf")
        .repo_name("pikpaktui")
        .build()?
        .fetch()?;
    let release = releases
        .first()
        .ok_or_else(|| anyhow!("no releases found"))?;

    if !version_newer(&release.version, current) {
        println!("Already up to date.");
        return Ok(());
    }
    println!("Updating to v{}...", release.version);

    let target = platform_target();
    let asset = release
        .asset_for(target, None)
        .ok_or_else(|| anyhow!("no release asset for this platform ({target})"))?;

    // The release publishes a sha256sums.txt alongside the archives; refuse
    // to replace the running binary unless the download matches it, so a
    // tampered asset or a corrupted transfer can't execute as us.
    let sums_asset = release
        .assets
        .iter()
        .find(|a| a.name == "sha256sums.txt")
        .ok_or_else(|| anyhow!("release has no sha256sums.txt; refusing to update unverified"))?;
    let mut sums = Vec::new();
    release_asset_download(&sums_asset.download_url)
        .download_to(&mut sums)
        .context("failed to download sha256sums.txt")?;
    let sums = String::from_utf8_lossy(&sums);
    let expected = sums
        .lines()
        .find_map(|line| {
            let (hash, file) = line.split_once("  ").or_else(|| line.split_once(' '))?;
            (file.trim() == asset.name).then(|| hash.trim().to_lowercase())
        })
        .ok_or_else(|| anyhow!("sha256sums.txt has no entry for '{}'", asset.name))?;

    let tmp_dir = self_update::TempDir::new().context("cannot create temp dir")?;
    let archive_path = tmp_dir.path().join(&asset.name);
    {
        let archive = std::fs::File::create(&archive_path)
            .with_context(|| format!("cannot create '{}'", archive_path.display()))?;
        release_asset_download(&asset.download_url)
            .show_progress(true)
            .download_to(archive)
            .context("failed to download release archive")?;
    }

    let actual = sha256_file(&archive_path)?;
    if actual != expected {
        return Err(anyhow!(
            "checksum mismatch for {} — refusing to install.\n  expected {}\n  actual   {}",
            asset.name,
            expected,
            actual
        ));
    }
    println!("Checksum verified.");

    let bin_name = if cfg!(windows) {
        "pikpaktui.exe"
    } else {
        "pikpaktui"
    };
    self_update::Extract::from_source(&archive_path)
        .extract_file(tmp_dir.path(), bin_name)
        .context("failed to extract binary from archive")?;
    let new_exe = tmp_dir.path().join(bin_name);
    self_update::self_replace::self_replace(&new_exe).context("failed to replace binary")?;

    println!("Updated to v{}!", release.version);
    Ok(())
}

fn release_asset_download(url: &str) -> self_update::Download {
    use reqwest::header::{ACCEPT, HeaderValue};

    // GitHub asset URLs serve JSON metadata unless this media type is requested.
    let mut download = self_update::Download::from_url(url);
    download.set_header(ACCEPT, HeaderValue::from_static("application/octet-stream"));
    download
}

fn sha256_file(path: &std::path::Path) -> Result<String> {
    use anyhow::Context;
    use sha2::{Digest, Sha256};
    let mut file =
        std::fs::File::open(path).with_context(|| format!("cannot open '{}'", path.display()))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).context("failed to hash archive")?;
    Ok(hex_lower(&hasher.finalize()))
}

fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{:02x}", b);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::version_newer;

    // GitHub's asset API returns metadata unless the request asks for bytes.
    fn asset_server(body: &'static [u8]) -> (String, std::thread::JoinHandle<()>) {
        use std::{io::Write, net::TcpListener, time::Duration};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/releases/assets/1",
            listener.local_addr().unwrap()
        );
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let request = crate::pikpak::read_test_http_request(&mut stream).unwrap();
            let wants_bytes = request.lines().any(|line| {
                line.split_once(':').is_some_and(|(name, value)| {
                    name.eq_ignore_ascii_case("accept")
                        && value.trim() == "application/octet-stream"
                })
            });
            let response = if wants_bytes {
                body
            } else {
                br#"{"name":"asset","size":123,"browser_download_url":"https://example.invalid/asset"}"#
            };
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response.len()
            )
            .unwrap();
            stream.write_all(response).unwrap();
        });
        (url, server)
    }

    #[test]
    fn checksum_download_receives_manifest_instead_of_api_metadata() {
        let manifest = b"a69cde963cb0191b05816b1307c38eb78489e7e889d0e4c6e4ffb6346964de4f  pikpaktui-aarch64-macos.tar.gz\n";
        let (url, server) = asset_server(manifest);
        let mut downloaded = Vec::new();
        super::release_asset_download(&url)
            .download_to(&mut downloaded)
            .unwrap();
        server.join().unwrap();
        assert_eq!(downloaded, manifest);
    }

    #[test]
    fn archive_download_receives_binary_bytes_instead_of_api_metadata() {
        let archive = b"\x1f\x8b\x08\x00release-archive\xff";
        let (url, server) = asset_server(archive);
        let dir = self_update::TempDir::new().unwrap();
        let path = dir.path().join("pikpaktui-aarch64-macos.tar.gz");
        super::release_asset_download(&url)
            .show_progress(true)
            .download_to(std::fs::File::create(&path).unwrap())
            .unwrap();
        server.join().unwrap();
        assert_eq!(std::fs::read(path).unwrap(), archive);
    }

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

    #[test]
    fn hex_lower_is_zero_padded_lowercase() {
        assert_eq!(super::hex_lower(&[0x00, 0x0f, 0xff, 0xa0]), "000fffa0");
    }

    #[test]
    fn sha256_matches_known_vector() {
        let dir = std::env::temp_dir().join(format!("pk-sha-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hello.bin");
        std::fs::write(&path, b"hello").unwrap();
        assert_eq!(
            super::sha256_file(&path).unwrap(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
