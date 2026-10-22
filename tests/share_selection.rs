use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Command, Output},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

#[derive(Debug)]
struct Request {
    method: String,
    url: reqwest::Url,
    body: Value,
}

impl Request {
    fn query(&self, key: &str) -> Option<String> {
        self.url
            .query_pairs()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.into_owned())
    }
}

struct Fixture {
    home: PathBuf,
    config: PathBuf,
    base: String,
    requests: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Fixture {
    fn new(duplicate_id: bool) -> Self {
        static NEXT_HOME: AtomicUsize = AtomicUsize::new(0);
        let home = loop {
            let sequence = NEXT_HOME.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "pikpaktui-share-selection-{}-{sequence}",
                std::process::id(),
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => break path,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot allocate test home '{}': {error}", path.display()),
            }
        };
        // Keep the explicit app directory separate from the fake home so an
        // ignored override fails on Unix as well as on Windows.
        let config = home.join("isolated-config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("config.toml"), "update_check = 'off'\n").unwrap();
        std::fs::write(config.join("session.json"), json!({"access_token":"test-access", "refresh_token":"test-refresh", "expires_at_unix":4102444800_i64}).to_string()).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let captured = Arc::clone(&requests);
        let stopped = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            while !stopped.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("accept: {error}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let (header_end, content_len) = loop {
                    let mut buf = [0; 4096];
                    let len = stream.read(&mut buf).unwrap();
                    assert_ne!(len, 0, "request ended before headers");
                    bytes.extend_from_slice(&buf[..len]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let len = headers
                            .lines()
                            .filter_map(|line| line.split_once(':'))
                            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                            .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                            .unwrap_or(0);
                        break (end + 4, len);
                    }
                };
                while bytes.len() < header_end + content_len {
                    let mut buf = [0; 4096];
                    let len = stream.read(&mut buf).unwrap();
                    assert_ne!(len, 0, "request ended before body");
                    bytes.extend_from_slice(&buf[..len]);
                }
                let headers = String::from_utf8_lossy(&bytes[..header_end]);
                let mut first = headers.lines().next().unwrap().split_whitespace();
                let method = first.next().unwrap().to_string();
                let url = reqwest::Url::parse(&format!("http://fixture{}", first.next().unwrap()))
                    .unwrap();
                let body = if content_len == 0 {
                    Value::Null
                } else {
                    serde_json::from_slice(&bytes[header_end..header_end + content_len]).unwrap()
                };
                let request = Request { method, url, body };
                let response = match request.url.path() {
                    "/drive/v1/share" => json!({"share_status":"OK", "pass_code_token":"share-token"}),
                    "/drive/v1/share/detail" => match (request.query("parent_id").as_deref(), request.query("page_token").as_deref()) {
                        (Some("nested"), None) => json!({"files":[{"id":"first", "name":"first.txt", "kind":"drive#file", "size":"7"}], "next_page_token":"next"}),
                        (Some("nested"), Some("next")) => json!({"files":[{"id":if duplicate_id { "first" } else { "second" }, "name":"second.txt", "kind":"drive#file", "size":"9"}]}),
                        (Some(""), None) => json!({"files":[{"id":"nested", "name":"docs", "kind":"drive#folder"}, {"id":"other", "name":"unselected.txt", "kind":"drive#file", "size":"11"}]}),
                        _ => panic!("unexpected folder request: {request:?}"),
                    },
                    "/drive/v1/share/restore" => json!({}),
                    _ => panic!("unexpected request: {request:?}"),
                }.to_string();
                captured.lock().unwrap().push(request);
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
            }
        });
        Self {
            home,
            config,
            base,
            requests,
            stop,
            handle: Some(handle),
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_pikpaktui"))
            .arg("share")
            .args(args)
            .env_clear()
            .env("PIKPAKTUI_CONFIG_DIR", &self.config)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("APPDATA", &self.home)
            .env("LOCALAPPDATA", &self.home)
            .env("HTTP_PROXY", "http://127.0.0.1:9")
            .env("HTTPS_PROXY", "http://127.0.0.1:9")
            .env("ALL_PROXY", "http://127.0.0.1:9")
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env("PIKPAK_DRIVE_BASE_URL", &self.base)
            .env("PIKPAK_AUTH_BASE_URL", &self.base)
            .output()
            .unwrap()
    }

    fn output_json(output: Output) -> Value {
        assert!(
            output.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "stdout is not a single JSON value ({error}): {}",
                String::from_utf8_lossy(&output.stdout)
            )
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let joined = self.handle.take().unwrap().join();
        std::fs::remove_dir_all(&self.home).unwrap();
        if !thread::panicking() {
            joined.unwrap();
        }
    }
}

// If settings or login credentials ignore the app-directory override, the
// command will miss these malformed files instead of reporting their errors.
#[test]
fn explicit_config_directory_is_used_for_settings_and_login() {
    let fixture = Fixture::new(false);
    std::fs::remove_file(fixture.config.join("session.json")).unwrap();
    std::fs::write(
        fixture.config.join("config.toml"),
        "update_check = 'invalid-mode'\n",
    )
    .unwrap();
    std::fs::write(fixture.config.join("login.toml"), "username = [").unwrap();
    let result = fixture.run(&["-b", "-J", "SHARE"]);
    assert!(!result.status.success());
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(error.contains("failed to parse config.toml"), "{error}");
    assert!(error.contains("failed to parse login.toml"), "{error}");
    assert!(fixture.requests.lock().unwrap().is_empty());
}

// A wrong parent query, missing pagination, or widening selection to all entries
// must change the actual restore request and fail this boundary test.
#[test]
fn save_selected_nested_ids_across_pages_deduplicates_restore_payload() {
    let fixture = Fixture::new(false);
    let result = Fixture::output_json(fixture.run(&[
        "-S",
        "-J",
        "--parent-id",
        "nested",
        "--file-id",
        "second",
        "--file-id",
        "first",
        "--file-id",
        "second",
        "SHARE",
    ]));
    assert_eq!(result["saved"], 2);
    assert_eq!(
        result["files"],
        json!([{"id":"second", "name":"second.txt"}, {"id":"first", "name":"first.txt"}])
    );
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[1].query("parent_id").as_deref(), Some("nested"));
    assert_eq!(requests[2].query("page_token").as_deref(), Some("next"));
    assert_eq!(requests[3].method, "POST");
    assert_eq!(requests[3].url.path(), "/drive/v1/share/restore");
    assert_eq!(
        requests[3].body,
        json!({"share_id":"SHARE", "pass_code_token":"share-token", "file_ids":["second", "first"], "to":{"parent_id":""}})
    );
}

#[test]
fn missing_id_fails_after_listing_without_restore() {
    let fixture = Fixture::new(false);
    let result = fixture.run(&[
        "-S",
        "-J",
        "--parent-id",
        "nested",
        "--file-id",
        "first",
        "--file-id",
        "missing",
        "SHARE",
    ]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("not found"));
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|request| request.method == "GET"));
}

#[test]
fn ambiguous_id_across_pages_fails_without_restore() {
    let fixture = Fixture::new(true);
    let result = fixture.run(&[
        "-S",
        "-J",
        "--parent-id",
        "nested",
        "--file-id",
        "first",
        "SHARE",
    ]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("ambiguous"));
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|request| request.method == "GET"));
}

#[test]
fn dry_run_json_preserves_selected_files_without_restore() {
    let fixture = Fixture::new(false);
    let result = Fixture::output_json(fixture.run(&[
        "-S",
        "-n",
        "-J",
        "--parent-id",
        "nested",
        "--file-id",
        "second",
        "SHARE",
    ]));
    assert_eq!(result["dry_run"], true);
    assert_eq!(result["would_save"], 1);
    assert_eq!(result["to"], "/");
    assert_eq!(
        result["files"],
        json!([{"id":"second", "name":"second.txt"}])
    );
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|request| request.method == "GET"));
}

#[test]
fn browse_parent_id_loads_only_that_folder_including_all_pages() {
    let fixture = Fixture::new(false);
    let result = Fixture::output_json(fixture.run(&["-b", "-J", "--parent-id", "nested", "SHARE"]));
    assert_eq!(
        result,
        json!([{"id":"first", "name":"first.txt", "folder":false, "size":7}, {"id":"second", "name":"second.txt", "folder":false, "size":9}])
    );
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(
        requests[1..]
            .iter()
            .all(|request| request.query("parent_id").as_deref() == Some("nested"))
    );
}

#[test]
fn root_save_default_still_saves_all_root_entries_and_decodes_password() {
    let fixture = Fixture::new(false);
    let result = Fixture::output_json(fixture.run(&[
        "-S",
        "-J",
        "https://mypikpak.com/s/SHARE?pass_code=a%2Bb",
    ]));
    assert_eq!(result["saved"], 2);
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests[0].query("pass_code").as_deref(), Some("a+b"));
    assert_eq!(requests[2].body["file_ids"], json!(["nested", "other"]));
}

#[test]
fn deep_share_url_fails_before_any_account_or_network_use() {
    let fixture = Fixture::new(false);
    std::fs::remove_file(fixture.config.join("session.json")).unwrap();
    for mode in ["-S", "-b"] {
        let result = fixture.run(&[
            mode,
            "https://mypikpak.com/s/SHARE/folder/child?pass_code=abcd",
        ]);
        assert!(!result.status.success());
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(error.contains("--parent-id"), "{error}");
        assert!(error.contains("--file-id"), "{error}");
    }
    assert!(fixture.requests.lock().unwrap().is_empty());
}

#[test]
fn normalized_deep_share_routes_fail_before_any_account_or_network_use() {
    let fixture = Fixture::new(false);
    std::fs::remove_file(fixture.config.join("session.json")).unwrap();
    for route in [
        "folder/..",
        "folder/%2e%2e",
        "folder/.%2E",
        "folder/%2E.",
        "folder/../?pass_code=1234",
        "folder/../..",
        ".",
        "%2e",
    ] {
        let url = format!("https://mypikpak.com/s/SHARE/{route}");
        for mode in ["-S", "-b"] {
            let result = fixture.run(&[mode, &url]);
            assert!(!result.status.success());
            let error = String::from_utf8_lossy(&result.stderr);
            assert!(error.contains("--parent-id"), "{mode} {url}: {error}");
            assert!(error.contains("--file-id"), "{mode} {url}: {error}");
        }
    }
    assert!(fixture.requests.lock().unwrap().is_empty());
}

#[test]
fn malformed_share_urls_fail_before_any_account_or_network_use() {
    let fixture = Fixture::new(false);
    std::fs::remove_file(fixture.config.join("session.json")).unwrap();
    let mut failures = Vec::new();
    for input in [
        "https:/mypikpak.com/s/SHARE/folder/..",
        "https:mypikpak.com/s/SHARE/folder/..",
        "https:/mypikpak.com/s/SHARE",
        "https:mypikpak.com/s/SHARE",
        "https:///mypikpak.com/s/SHARE",
        "https:////mypikpak.com/s/SHARE",
        r"https:\mypikpak.com\s\SHARE\folder\..",
        r"https:\\mypikpak.com\s\SHARE",
        r"https:/\mypikpak.com/s/SHARE/folder/..",
        r"https:\/mypikpak.com/s/SHARE/folder/..",
        r"https://mypikpak.com\s\SHARE",
        r"https://mypikpak.com/s\SHARE",
        "https://mypikpak.com//s/SHARE",
        "https://mypikpak.com/s//SHARE",
        "https://mypikpak.com/s/SHARE//",
        "ht\ttps://mypikpak.com/s/SHARE",
        "https://mypikpak.com/s/SH\nARE",
        "https://mypikpak.com/s/SHARE\r",
        "https://mypikpak.com/s/SHARE?pass_code=ab\tcd",
        "SH\nARE",
        "SHARE:OTHER",
        r"SHARE\OTHER",
        "SHARE.OTHER",
        "https://mypikpak.com/s/SHARE%2fOTHER",
    ] {
        for mode in ["-S", "-b"] {
            let result = fixture.run(&[mode, input]);
            let error = String::from_utf8_lossy(&result.stderr);
            if result.status.success()
                || !error.contains("root /s/<share_id>")
                || !error.contains("--parent-id")
                || !error.contains("--file-id")
                || error.contains("not logged in")
            {
                failures.push(format!("{mode} {input:?}: {error}"));
            }
        }
    }
    assert!(fixture.requests.lock().unwrap().is_empty());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn incompatible_selectors_modes_and_unknown_options_fail_before_network() {
    let fixture = Fixture::new(false);
    std::fs::remove_file(fixture.config.join("session.json")).unwrap();
    for args in [
        vec!["-S", "--wat", "SHARE"],
        vec!["-b", "--parent-id", "nested", "SHARE", "docs"],
        vec!["-S", "-b", "SHARE"],
        vec!["-l", "-S", "SHARE"],
        vec!["-l", "--wat"],
        vec!["--wat", "/docs"],
        vec![
            "-S",
            "--parent-id",
            "nested",
            "--parent-id",
            "other",
            "SHARE",
        ],
        vec!["-S", "--file-id", "", "SHARE"],
    ] {
        let result = fixture.run(&args);
        assert!(!result.status.success(), "{args:?}");
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(
            !error.contains("not logged in"),
            "argument validation did not happen before client setup: {args:?}: {error}"
        );
    }
    assert!(fixture.requests.lock().unwrap().is_empty());
}
