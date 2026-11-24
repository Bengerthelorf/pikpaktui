use std::{env, fs, process::Command};

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output.status.success().then(|| {
        String::from_utf8(output.stdout)
            .expect("invalid git output")
            .trim()
            .to_owned()
    })
}

fn main() {
    println!("cargo:rustc-check-cfg=cfg(pikpak_android_jni)");
    println!("cargo:rustc-cfg=pikpak_android_jni");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-cdylib-link-arg=-Wl,-z,max-page-size=16384");
    }

    // Read the same package version as the tag guard and scripts/release.sh.
    println!("cargo:rerun-if-changed=../Cargo.toml");
    let manifest = fs::read_to_string("../Cargo.toml").expect("read pikpaktui manifest");
    let version = manifest
        .lines()
        .find_map(|line| line.strip_prefix("version = \"")?.strip_suffix('"'))
        .expect("pikpaktui package version");
    println!("cargo:rustc-env=PIKPAKTUI_VERSION={version}");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    let mut refs = vec!["HEAD".to_owned()];
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
        refs.push(branch);
        refs.push("packed-refs".into());
    }
    for reference in refs {
        let path = git(&["rev-parse", "--git-path", &reference]).expect("locate git ref");
        // A packed branch can later acquire a loose ref without changing HEAD.
        if reference != "packed-refs" || std::path::Path::new(&path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    let head = git(&["rev-parse", "--verify", "HEAD^{commit}"]).expect("build from a git checkout");
    assert!(head.len() == 40 && head.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let source = if let Ok(sha) = env::var("GITHUB_SHA") {
        assert_eq!(sha, head, "GITHUB_SHA must match the checkout");
        "github-actions"
    } else {
        "local-worktree"
    };
    println!("cargo:rustc-env=PIKPAKTUI_REVISION={head}");
    println!("cargo:rustc-env=PIKPAKTUI_SOURCE={source}");
}
