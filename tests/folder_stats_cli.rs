use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Fixture {
    config: PathBuf,
    server: TcpListener,
}

impl Fixture {
    fn new() -> Self {
        let config = std::env::temp_dir().join(format!(
            "pikpaktui-stats-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&config).unwrap();
        std::fs::write(
            config.join("session.json"),
            json!({
                "user_id":"test-account","access_token":"test","refresh_token":"test",
                "expires_at_unix":4102444800_i64
            })
            .to_string(),
        )
        .unwrap();
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        server.set_nonblocking(true).unwrap();
        let fixture = Self { config, server };
        fixture.configure("");
        fixture
    }

    fn configure(&self, options: &str) {
        std::fs::write(
            self.config.join("config.toml"),
            format!("update_check='off'\n{options}"),
        )
        .unwrap();
    }

    // Drive a real CLI process, checking the type and order of all drive requests.
    fn run(&self, args: &[&str], replies: &[(&str, Value)]) -> Output {
        let origin = format!("http://{}", self.server.local_addr().unwrap());
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_pikpaktui"));
        cmd.args(args)
            .env_clear()
            .env("PIKPAKTUI_CONFIG_DIR", &self.config)
            .env("PIKPAK_AUTH_BASE_URL", &origin)
            .env("PIKPAK_DRIVE_BASE_URL", &origin)
            .env("HTTPS_PROXY", "http://127.0.0.1:9")
            .env("HTTP_PROXY", "http://127.0.0.1:9")
            .env("NO_PROXY", "127.0.0.1")
            .env("TMPDIR", &self.config)
            .env("TEMP", &self.config)
            .env("TMP", &self.config)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        for k in ["WINDIR", "SystemRoot"] {
            if let Some(value) = std::env::var_os(k) {
                cmd.env(k, value);
            }
        }
        let mut child = cmd.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = Vec::new();
        while child.try_wait().unwrap().is_none() {
            if Instant::now() > deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("command timed out: {args:?} after {seen:?}");
            }
            let mut stream = match self.server.accept() {
                Ok((stream, _)) => stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(3));
                    continue;
                }
                Err(e) => panic!("{e}"),
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut read = BufReader::new(&stream);
            let mut line = String::new();
            read.read_line(&mut line).unwrap();
            let target = line.split_whitespace().nth(1).unwrap();
            let url = reqwest::Url::parse(&format!("{origin}{target}")).unwrap();
            assert_eq!(url.path(), "/drive/v1/files");
            loop {
                line.clear();
                read.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
            }
            let q: std::collections::HashMap<_, _> = url.query_pairs().collect();
            let parent = &q["parent_id"];
            let op = if parent == "*" {
                if q["filters"].contains("modified_time") {
                    "delta"
                } else {
                    "full"
                }
            } else if parent.is_empty() {
                "root"
            } else {
                parent
            };
            let value = replies
                .get(seen.len())
                .map(|(_, v)| v)
                .cloned()
                .unwrap_or(json!({}));
            seen.push(op.to_string());
            let body = value.to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
        let out = child.wait_with_output().unwrap();
        let labels: Vec<_> = replies.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            seen,
            labels,
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.config);
    }
}

fn data(out: Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn failure(out: Output, message: &str) {
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(message),
        "{out:?}"
    );
}

#[test]
fn public_cli_sync_cache_sort_and_overrides_use_the_same_index() {
    let fx = Fixture::new();
    let t = "2026-01-01T00:00:00Z";
    let a = json!({"id":"a","name":"A","parent_id":"","kind":"drive#folder","size":1});
    let b = json!({"id":"b","name":"B","parent_id":"","kind":"drive#folder","size":500});
    let empty = json!({"id":"empty","name":"Empty","parent_id":"","kind":"drive#folder"});
    let nested = json!({"id":"d","name":"Deep","parent_id":"a","kind":"drive#folder"});
    let leaf = json!({"id":"leaf","name":"leaf","parent_id":"d","kind":"drive#file","size":80,"modified_time":"2020-01-01T00:00:00Z"});
    let recent = json!({"id":"recent","name":"recent","parent_id":"b","kind":"drive#file","size":7,"modified_time":t});
    let full = json!({"files":[leaf,recent,nested,empty,b,a],"sync_time":t});
    let root = json!({"files":[a,b,empty]});
    let delta = json!({"files":[],"sync_time":t});

    for args in [&["--help"][..], &["stats", "--help"][..]] {
        let out = fx.run(args, &[]);
        assert!(out.status.success() && String::from_utf8_lossy(&out.stdout).contains("stats"));
    }
    failure(
        fx.run(&["stats", "--cached", "-J"], &[]),
        "no cached statistics",
    );
    failure(
        fx.run(&["stats", "--cached", "/A"], &[]),
        "offline lookup requires",
    );
    failure(
        fx.run(&["stats", "--cached", "--refresh"], &[]),
        "cannot be combined",
    );
    let first = data(fx.run(
        &["stats", "-J", "/A"],
        &[("root", root.clone()), ("full", full.clone())],
    ));
    assert_eq!(first["total_size_bytes"], "80");
    assert_eq!(first["latest_modified_time"], leaf["modified_time"]);
    let cached = data(fx.run(&["stats", "--folder-id", "a", "--cached", "-J"], &[]));
    assert_eq!(first, cached);
    assert_eq!(
        data(fx.run(&["stats", "--cached", "-J", "--folder-id", "empty"], &[])),
        json!({"is_empty":true,"total_size_bytes":"0","latest_modified_time":null})
    );
    failure(
        fx.run(&["stats", "--cached", "--folder-id", "absent"], &[]),
        "no cached statistics",
    );

    // Legacy ls still sorts by the service's direct size without a stats request.
    let raw = data(fx.run(&["ls", "-J", "--sort=size"], &[("root", root.clone())]));
    assert_eq!(raw[0]["id"], "b");
    assert!(raw[0].get("folder_stats").is_none());
    let sizes = data(fx.run(
        &["ls", "-J", "--folder-stats", "--sort=size"],
        &[("root", root.clone()), ("delta", delta.clone())],
    ));
    assert_eq!(sizes[0]["id"], "a");
    assert_eq!(sizes[0]["folder_stats"], first);
    assert_eq!(sizes[0]["size"], 1);
    fx.configure("cli_folder_stats=true\n");
    let times = data(fx.run(
        &["ls", "-J", "--sort=content-modified"],
        &[("root", root.clone()), ("delta", delta.clone())],
    ));
    assert_eq!(times[0]["id"], "b");
    assert_eq!(times[2]["folder_stats"]["is_empty"], true);
    let long = fx.run(
        &["ls", "-l", "--sort=size"],
        &[("root", root.clone()), ("delta", delta.clone())],
    );
    let out = String::from_utf8_lossy(&long.stdout);
    assert!(long.status.success() && out.contains("80 B") && out.contains("2020-01-01"));
    let disabled = data(fx.run(
        &["ls", "-J", "--no-folder-stats", "--sort=size"],
        &[("root", root.clone())],
    ));
    assert_eq!(disabled, raw);
    failure(
        fx.run(&["ls", "--no-folder-stats", "--sort=content-modified"], &[]),
        "conflicts",
    );

    fx.configure("cli_folder_stats=false\n");
    let tree = fx.run(
        &["ls", "--tree", "--depth=2", "--sort=content-modified", "-l"],
        &[
            ("root", root.clone()),
            ("delta", delta.clone()),
            ("b", json!({"files":[recent]})),
            ("a", json!({"files":[nested]})),
            ("empty", json!({"files":[]})),
        ],
    );
    assert!(tree.status.success() && String::from_utf8_lossy(&tree.stdout).contains("Deep"));
    let mut changed = leaf.clone();
    changed["size"] = json!(81);
    let updated = data(fx.run(
        &["stats", "--folder-id", "a", "-J"],
        &[("delta", json!({"files":[changed],"sync_time":t}))],
    ));
    assert_eq!(updated["total_size_bytes"], "81");
    assert_eq!(
        data(fx.run(&["stats", "-J", "--cached", "/"], &[]))["total_size_bytes"],
        "88"
    );
    assert_eq!(
        data(fx.run(
            &["stats", "-J", "--refresh", "--folder-id=a"],
            &[("full", full)]
        )),
        first
    );
}

#[test]
fn missing_server_watermark_falls_back_once_and_fails_closed_on_a_child_error() {
    let fx = Fixture::new();
    let folder = json!({"id":"a","name":"A","parent_id":"","kind":"drive#folder"});
    let leaf = json!({"id":"f","name":"leaf","parent_id":"a","kind":"drive#file","size":5});
    let root = json!({"files":[folder]});
    let legacy = json!({"files":[folder,leaf]});
    let child = json!({"files":[leaf]});
    let stats = data(fx.run(
        &["ls", "-J", "--sort=content-modified"],
        &[
            ("root", root.clone()),
            ("full", legacy.clone()),
            ("a", child.clone()),
        ],
    ));
    assert_eq!(
        stats[0]["folder_stats"],
        json!({"total_size_bytes":"5","latest_modified_time":null,"is_empty":false})
    );
    assert_eq!(
        data(fx.run(
            &["stats", "--folder-id", "a", "-J"],
            &[("full", legacy.clone()), ("a", child)]
        )),
        stats[0]["folder_stats"]
    );
    failure(
        fx.run(&["stats", "--cached", "-J", "--folder-id", "a"], &[]),
        "no cached statistics",
    );
    failure(
        fx.run(
            &["ls", "--folder-stats", "-J"],
            &[
                ("root", root),
                ("full", legacy),
                ("a", json!({"files":"bad page"})),
            ],
        ),
        "could not list folder",
    );
}
