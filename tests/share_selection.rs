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

#[derive(Clone, Copy, PartialEq)]
enum RestoreBehavior {
    Complete,
    CompleteFailure,
    AsyncComplete,
    TaskFailure,
    Unknown,
    MissingMapping,
    MissingSelected,
    ExtraMapping,
    DuplicateDestination,
    MoveFailure,
    SecondBatchFailure,
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
        Self::with_restore(duplicate_id, RestoreBehavior::Complete)
    }

    fn with_restore(duplicate_id: bool, behavior: RestoreBehavior) -> Self {
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
            let mut restore_count = 0;
            let mut task_polls = 0;
            let mut restored_mapping = json!({});
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
                let mut status = "200 OK";
                let response = match request.url.path() {
                    "/drive/v1/share" => json!({"share_status":"OK", "pass_code_token":"share-token"}),
                    "/drive/v1/share/detail" => match (request.query("parent_id").as_deref(), request.query("page_token").as_deref()) {
                        (Some("nested"), None) => json!({"files":[{"id":"first", "name":"first.txt", "kind":"drive#file", "size":"7"}], "next_page_token":"next"}),
                        (Some("nested"), Some("next")) => json!({"files":[{"id":if duplicate_id { "first" } else { "second" }, "name":"second.txt", "kind":"drive#file", "size":"9"}]}),
                        (Some(""), None) if behavior == RestoreBehavior::SecondBatchFailure => json!({"files": (0..501).map(|i| json!({"id":format!("file-{i}"),"name":format!("file-{i}.txt"),"kind":"drive#file","size":"1"})).collect::<Vec<_>>()}),
                        (Some(""), None) => json!({"files":[{"id":"nested", "name":"docs", "kind":"drive#folder"}, {"id":"other", "name":"unselected.txt", "kind":"drive#file", "size":"11"}]}),
                        _ => panic!("unexpected folder request: {request:?}"),
                    },
                    "/drive/v1/share/restore" => {
                        restore_count += 1;
                        restored_mapping = Value::Object(request.body["file_ids"].as_array().unwrap().iter().map(|id| {
                            let id = id.as_str().unwrap();
                            (id.to_owned(), json!(format!("saved-{id}")))
                        }).collect());
                        match behavior {
                            RestoreBehavior::CompleteFailure => json!({"share_status":"OK","restore_status":"RESTORE_COMPLETE","file_id":"shared-default","params":{"trace_file_ids":restored_mapping.to_string(),"error_detail":"file_space_not_enough"}}),
                            RestoreBehavior::Unknown => json!({}),
                            RestoreBehavior::MissingMapping => json!({"share_status":"OK","restore_status":"RESTORE_COMPLETE","file_id":"shared-default"}),
                            RestoreBehavior::AsyncComplete | RestoreBehavior::TaskFailure => json!({"share_status":"OK","restore_status":"RESTORE_START","restore_task_id":"restore-task","file_id":"shared-default","params":{}}),
                            RestoreBehavior::SecondBatchFailure if restore_count == 2 => {
                                status = "400 Bad Request";
                                json!({"error":"file_space_not_enough"})
                            },
                            _ => {
                                if behavior == RestoreBehavior::MissingSelected {
                                    restored_mapping.as_object_mut().unwrap().remove("second");
                                } else if behavior == RestoreBehavior::ExtraMapping {
                                    restored_mapping["unselected"] = json!("saved-unselected");
                                } else if behavior == RestoreBehavior::DuplicateDestination {
                                    for id in restored_mapping.as_object_mut().unwrap().values_mut() {
                                        *id = json!("saved-same");
                                    }
                                }
                                json!({"share_status":"OK","restore_status":"RESTORE_COMPLETE","file_id":"shared-default","params":{"trace_file_ids":restored_mapping.to_string()}})
                            }
                        }
                    },
                    "/drive/v1/tasks/restore-task" => {
                        task_polls += 1;
                        if behavior == RestoreBehavior::TaskFailure {
                            json!({"id":"restore-task","phase":"PHASE_TYPE_ERROR","message":"restore failed","params":{"error_detail":"file_space_not_enough"}})
                        } else if task_polls == 1 {
                            // Early mappings are not proof that the task completed.
                            json!({"id":"restore-task","phase":"PHASE_TYPE_RUNNING","params":{"trace_file_ids":restored_mapping.to_string()}})
                        } else {
                            json!({"id":"restore-task","phase":"PHASE_TYPE_COMPLETE","params":{"trace_file_ids":restored_mapping.to_string()}})
                        }
                    },
                    "/drive/v1/files" => json!({"files":[
                        {"id":"destination-id","name":"destination","kind":"drive#folder","size":"0"},
                        {"id":"regular-file","name":"report.txt","kind":"drive#file","size":"10"}
                    ]}),
                    "/drive/v1/files:batchMove" => {
                        if behavior == RestoreBehavior::MoveFailure {
                            status = "400 Bad Request";
                            json!({"error":"destination unavailable"})
                        } else {
                            json!({})
                        }
                    },
                    _ => panic!("unexpected request: {request:?}"),
                }.to_string();
                captured.lock().unwrap().push(request);
                write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
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
        let mut command = Command::new(env!("CARGO_BIN_EXE_pikpaktui"));
        command
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
            .env("PIKPAK_AUTH_BASE_URL", &self.base);
        // Winsock provider DLL paths can contain these Windows variables.
        // Preserve only the runtime allowlist, not the parent's account env.
        #[cfg(windows)]
        for name in ["SystemRoot", "WINDIR"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command.output().unwrap()
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
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[1].query("parent_id").as_deref(), Some("nested"));
    assert_eq!(requests[2].query("page_token").as_deref(), Some("next"));
    assert_eq!(requests[3].method, "POST");
    assert_eq!(requests[3].url.path(), "/drive/v1/share/restore");
    assert_eq!(
        requests[3].body,
        json!({"share_id":"SHARE", "pass_code_token":"share-token", "file_ids":["second", "first"], "params":{"trace_file_ids":"second,first"}})
    );
    assert_eq!(requests[4].url.path(), "/drive/v1/files:batchMove");
    assert_eq!(
        requests[4].body,
        json!({"ids":["saved-second","saved-first"],"to":{"parent_id":""}})
    );
}

#[test]
fn save_to_regular_file_fails_before_restore_in_preview_and_write_modes() {
    let fixture = Fixture::new(false);
    for dry_run in [true, false] {
        let mut args = vec!["-S", "--to", "/report.txt", "SHARE"];
        if dry_run {
            args.push("--dry-run");
        }
        let result = fixture.run(&args);
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("folder not found"));
    }
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| request.method == "GET" && request.url.path() == "/drive/v1/files")
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

#[test]
fn restore_waits_for_completion_then_moves_only_selected_new_ids() {
    let fixture = Fixture::with_restore(false, RestoreBehavior::AsyncComplete);
    let result = Fixture::output_json(fixture.run(&[
        "-S",
        "-J",
        "--parent-id",
        "nested",
        "--file-id",
        "second",
        "-t",
        "/destination",
        "SHARE",
    ]));
    assert_eq!(result["saved"], 1);
    assert_eq!(result["to"], "/destination");
    let requests = fixture.requests.lock().unwrap();
    let writes: Vec<_> = requests.iter().filter(|r| r.method == "POST").collect();
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[0].body["file_ids"], json!(["second"]));
    assert_eq!(writes[0].body["params"]["trace_file_ids"], "second");
    assert!(writes[0].body.get("to").is_none());
    assert_eq!(
        writes[1].body,
        json!({"ids":["saved-second"],"to":{"parent_id":"destination-id"}})
    );
    let polls: Vec<_> = requests
        .iter()
        .enumerate()
        .filter(|(_, r)| r.url.path() == "/drive/v1/tasks/restore-task")
        .collect();
    assert_eq!(polls.len(), 2, "must wait beyond an early trace mapping");
    assert_eq!(
        requests.last().unwrap().url.path(),
        "/drive/v1/files:batchMove"
    );
}

#[test]
fn restore_task_failure_is_not_reported_as_saved_or_retried() {
    let fixture = Fixture::with_restore(false, RestoreBehavior::TaskFailure);
    let result = fixture.run(&["-S", "-J", "SHARE"]);
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(error.contains("restore-task"), "{error}");
    assert!(error.contains("file_space_not_enough"), "{error}");
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.url.path() == "/drive/v1/share/restore")
            .count(),
        1
    );
    assert!(
        requests
            .iter()
            .all(|r| r.url.path() != "/drive/v1/files:batchMove")
    );
}

#[test]
fn restore_unknown_or_incomplete_mapping_never_reports_saved_or_moves_files() {
    for behavior in [
        RestoreBehavior::Unknown,
        RestoreBehavior::MissingMapping,
        RestoreBehavior::MissingSelected,
        RestoreBehavior::ExtraMapping,
        RestoreBehavior::DuplicateDestination,
    ] {
        let fixture = Fixture::with_restore(false, behavior);
        let result = fixture.run(&["-S", "-J", "--parent-id", "nested", "SHARE"]);
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(
            error.contains("accepted") || error.contains("restored"),
            "{error}"
        );
        assert!(error.contains("resubmit"), "{error}");
        let requests = fixture.requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.url.path() == "/drive/v1/share/restore")
                .count(),
            1
        );
        assert!(
            requests
                .iter()
                .all(|r| r.url.path() != "/drive/v1/files:batchMove")
        );
    }
}

#[test]
fn move_failure_preserves_restored_ids_and_does_not_repeat_restore() {
    let fixture = Fixture::with_restore(false, RestoreBehavior::MoveFailure);
    let result = fixture.run(&[
        "-S",
        "-J",
        "--parent-id",
        "nested",
        "--file-id",
        "second",
        "SHARE",
    ]);
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(error.contains("saved-second"), "{error}");
    assert!(error.contains("restored"), "{error}");
    assert!(error.contains("resubmit"), "{error}");
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.url.path() == "/drive/v1/share/restore")
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.url.path() == "/drive/v1/files:batchMove")
            .count(),
        1
    );
}

#[test]
fn restore_batches_keep_completed_progress_and_never_replay_first_batch() {
    let fixture = Fixture::with_restore(false, RestoreBehavior::SecondBatchFailure);
    let result = fixture.run(&["-S", "-J", "SHARE"]);
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(error.contains("500/501"), "{error}");
    let requests = fixture.requests.lock().unwrap();
    let restores: Vec<_> = requests
        .iter()
        .filter(|r| r.url.path() == "/drive/v1/share/restore")
        .collect();
    assert_eq!(restores.len(), 2);
    assert_eq!(restores[0].body["file_ids"].as_array().unwrap().len(), 500);
    assert_eq!(restores[1].body["file_ids"], json!(["file-500"]));
    let moves: Vec<_> = requests
        .iter()
        .filter(|r| r.url.path() == "/drive/v1/files:batchMove")
        .collect();
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].body["ids"].as_array().unwrap().len(), 500);
}

#[test]
fn completed_restore_with_error_detail_is_not_moved_or_reported_as_saved() {
    let fixture = Fixture::with_restore(false, RestoreBehavior::CompleteFailure);
    let result = fixture.run(&["-S", "-J", "SHARE"]);
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(error.contains("file_space_not_enough"), "{error}");
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.url.path() == "/drive/v1/share/restore")
            .count(),
        1
    );
    assert!(
        requests
            .iter()
            .all(|r| r.url.path() != "/drive/v1/files:batchMove")
    );
}
