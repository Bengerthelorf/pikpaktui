use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

struct Sandbox(PathBuf);

impl Sandbox {
    fn new() -> Self {
        static NEXT_SANDBOX: AtomicUsize = AtomicUsize::new(0);
        loop {
            let sequence = NEXT_SANDBOX.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "pikpaktui-hash-cli-{}-{sequence}",
                std::process::id(),
            ));
            match fs::create_dir(&path) {
                Ok(()) => {
                    let sandbox = Self(path);
                    fs::create_dir(sandbox.0.join("home")).unwrap();
                    return sandbox;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot allocate sandbox '{}': {error}", path.display()),
            }
        }
    }

    fn fixture(&self, name: &str, size: usize) {
        let bytes: Vec<u8> = (0..size).map(|i| ((i * 31 + 7) % 256) as u8).collect();
        fs::write(self.0.join(name), bytes).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        let home = self.0.join("home");
        let mut child = Command::new(env!("CARGO_BIN_EXE_pikpaktui"))
            .args(args)
            .current_dir(&self.0)
            .env_clear()
            .env("PIKPAKTUI_CONFIG_DIR", self.config_dir())
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("APPDATA", &home)
            .env("LOCALAPPDATA", &home)
            // Any accidental network attempt stays on loopback.
            .env("HTTP_PROXY", "http://127.0.0.1:9")
            .env("HTTPS_PROXY", "http://127.0.0.1:9")
            .env("ALL_PROXY", "http://127.0.0.1:9")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if child.try_wait().unwrap().is_some() {
                return child.wait_with_output().unwrap();
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let output = child.wait_with_output().unwrap();
                panic!("hash command hung: {:?}", output);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn config_dir(&self) -> PathBuf {
        self.0.join("isolated-config")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn assert_success(output: &Output) {
    assert!(output.status.success(), "{:?}", output);
    assert!(output.stderr.is_empty(), "{:?}", output);
}

// Removing hash dispatch, constructing a client, or changing the hash bytes fails this test.
#[test]
fn hashes_official_vectors_without_login_or_creating_state() {
    let sandbox = Sandbox::new();
    // Independent official WASM vectors already recorded in upload.rs.
    for (size, expected) in [
        (0, "DA39A3EE5E6B4B0D3255BFEF95601890AFD80709"),
        (19, "CF0E686EBD77BC4A32FC73BE5FFD20F9C90CB3BD"),
        (262143, "2C0647A6C3B97A4A97920F5C7921161676FC0A6A"),
        (262144, "96C09F7EDBDED8C0081C3CA005ED0FE2A12074C6"),
        (262145, "BEA4CDB9F63971DD6F4E5359E6C508ED23DD6C53"),
    ] {
        sandbox.fixture("fixture.bin", size);
        let output = sandbox.run(&["hash", "fixture.bin"]);
        assert_success(&output);
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("{expected}\n")
        );
    }
    assert_eq!(fs::read_dir(sandbox.0.join("home")).unwrap().count(), 0);
    assert!(!sandbox.config_dir().exists());
}

// Losing -- handling or splitting local names fails this test.
#[test]
fn accepts_spaces_and_dash_prefixed_paths_after_separator() {
    let sandbox = Sandbox::new();
    for name in ["a local file.bin", "-J", "--help"] {
        sandbox.fixture(name, 1);
        let output = sandbox.run(&["hash", "--", name]);
        assert_success(&output);
        assert_eq!(output.stdout, b"8AACFDE27B33A25EA9797815353FA580F744B3CB\n");
    }
}

// Mixing human text into JSON, losing a path, or ignoring either JSON switch fails this test.
#[test]
fn emits_one_json_object_with_original_path() {
    let sandbox = Sandbox::new();
    for name in [
        "local 数据 file.bin",
        #[cfg(unix)]
        "local \"quoted\" file.bin",
    ] {
        sandbox.fixture(name, 19);
        for args in [["hash", "-J", name], ["hash", name, "--json"]] {
            let output = sandbox.run(&args);
            assert_success(&output);
            let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(
                value,
                serde_json::json!({
                    "path": name,
                    "algorithm": "pikpak-gcid",
                    "hash": "CF0E686EBD77BC4A32FC73BE5FFD20F9C90CB3BD"
                })
            );
        }
    }
}

// Dropping arity or option validation fails these error cases.
#[test]
fn rejects_invalid_arguments() {
    let sandbox = Sandbox::new();
    sandbox.fixture("one", 0);
    sandbox.fixture("two", 0);
    for (args, reason) in [
        (vec!["hash"], "exactly one"),
        (vec!["hash", "-J"], "exactly one"),
        (vec!["hash", "--"], "exactly one"),
        (vec!["hash", "one", "two"], "exactly one"),
        (vec!["hash", "--bogus", "one"], "unknown option"),
        (vec!["hash", "-Jone"], "unknown option"),
    ] {
        let output = sandbox.run(&args);
        assert!(!output.status.success(), "{:?}", output);
        assert!(output.stdout.is_empty(), "{:?}", output);
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("hash") && error.contains(reason), "{error}");
    }
}

// Treating missing files or directories as empty files fails this test.
#[test]
fn rejects_missing_files_and_directories() {
    let sandbox = Sandbox::new();
    for name in ["missing.bin", "."] {
        let output = sandbox.run(&["hash", name]);
        assert!(!output.status.success(), "{:?}", output);
        assert!(output.stdout.is_empty(), "{:?}", output);
        assert!(String::from_utf8(output.stderr).unwrap().contains(name));
    }
}

// Reading config or invoking updates can change stdout/stderr or state under notify.
#[test]
fn remains_local_with_update_notifications_enabled() {
    let sandbox = Sandbox::new();
    sandbox.fixture("fixture.bin", 0);
    fs::create_dir_all(sandbox.config_dir()).unwrap();
    let config = sandbox.config_dir().join("config.toml");
    let bytes = b"update_check = \"notify\"\n";
    fs::write(&config, bytes).unwrap();
    let output = sandbox.run(&["hash", "fixture.bin"]);
    assert_success(&output);
    assert_eq!(output.stdout, b"DA39A3EE5E6B4B0D3255BFEF95601890AFD80709\n");
    assert_eq!(fs::read(&config).unwrap(), bytes);
    assert_eq!(fs::read_dir(sandbox.config_dir()).unwrap().count(), 1);
}

#[cfg(unix)]
fn fifo(path: &std::path::Path) {
    assert!(Command::new("mkfifo").arg(path).status().unwrap().success());
}

// Opening a FIFO before checking its file type would hang, rather than fail promptly.
#[cfg(unix)]
#[test]
fn rejects_fifo_and_device_without_opening_them() {
    let sandbox = Sandbox::new();
    fifo(&sandbox.0.join("pipe"));
    for name in ["pipe", "/dev/null"] {
        let output = sandbox.run(&["hash", name]);
        assert!(!output.status.success(), "{:?}", output);
        assert!(output.stdout.is_empty(), "{:?}", output);
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("regular file")
        );
    }
}

// Any config load in the hash path blocks on this FIFO, even before an update starts.
#[cfg(unix)]
#[test]
fn bypasses_config_loading_entirely() {
    let sandbox = Sandbox::new();
    sandbox.fixture("fixture.bin", 0);
    fs::create_dir_all(sandbox.config_dir()).unwrap();
    fifo(&sandbox.config_dir().join("config.toml"));
    let output = sandbox.run(&["hash", "fixture.bin"]);
    assert_success(&output);
    assert_eq!(output.stdout, b"DA39A3EE5E6B4B0D3255BFEF95601890AFD80709\n");
}
