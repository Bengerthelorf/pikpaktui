//! Regression tests with synthetic responses; no account mutations are required.
use super::tests::{temp_test_dir, test_client, write_response};
use super::*;
use std::{
    net::TcpListener,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

type ScriptedServer = (String, Arc<Mutex<Vec<String>>>, std::thread::JoinHandle<()>);

fn scripted_server(responses: Vec<(u16, impl AsRef<str> + Send + 'static)>) -> ScriptedServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&requests);
    let handle = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(12);
        for (code, body) in responses {
            let mut stream = loop {
                match accept_test_connection(&listener) {
                    Ok(s) => break s,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => panic!("mock request missing: {e}"),
                }
            };
            captured
                .lock()
                .unwrap()
                .push(read_test_http_request(&mut stream).unwrap());
            let reason = match code {
                200 => "OK",
                204 => "No Content",
                401 => "Unauthorized",
                _ => "Internal Server Error",
            };
            write_response(&mut stream, code, reason, body.as_ref().as_bytes());
        }
    });
    (base, requests, handle)
}

const REFRESHED: &str = r#"{"access_token":"fresh-access","refresh_token":"rotated-refresh","expires_in":3600,"sub":"test-user","token_type":"Bearer"}"#;
const REJECTED: &str = r#"{"error":"UNAUTHENTICATED"}"#;

#[test]
fn thumbnail_source_size_changes_on_shared_workers_and_invalidates_cached_urls() {
    let (base, requests, handle) =
        scripted_server(vec![(200, r#"{"files":[]}"#), (200, r#"{"files":[]}"#)]);
    let dir = temp_test_dir("thumbnail-source-size");
    let client = Arc::new(test_client(base, dir.join("session.json")));
    let worker = Arc::clone(&client);
    worker.ls_cached("folder").unwrap();
    assert!(
        client
            .ls_cache
            .lock()
            .unwrap()
            .entries
            .contains_key("folder")
    );
    client.set_thumbnail_size("SIZE_LARGE");
    assert!(client.ls_cache.lock().unwrap().entries.is_empty());
    std::thread::spawn(move || worker.ls_cached("folder").unwrap())
        .join()
        .unwrap();
    handle.join().unwrap();
    let requests = requests.lock().unwrap();
    assert!(requests[0].contains("thumbnail_size=SIZE_MEDIUM"));
    assert!(requests[1].contains("thumbnail_size=SIZE_LARGE"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn server_rejection_refreshes_a_locally_unexpired_token() {
    let (base, requests, handle) = scripted_server(vec![
        (401, REJECTED),
        (200, REFRESHED),
        (200, r#"{"quota":{"limit":"100","usage":"1"}}"#),
    ]);
    let dir = temp_test_dir("parity-rejected-token");
    let mut client = test_client(base.clone(), dir.join("session.json"));
    client.auth_base_url = base;
    client.quota().unwrap();
    handle.join().unwrap();
    let requests = requests.lock().unwrap();
    assert!(
        requests[0]
            .to_lowercase()
            .contains("authorization: bearer test-access")
    );
    assert!(requests[1].starts_with("POST /v1/auth/token "));
    assert!(
        requests[2]
            .to_lowercase()
            .contains("authorization: bearer fresh-access")
    );
    assert!(!requests[2].contains("test-access"));
    assert_eq!(
        client.load_session().unwrap().unwrap().refresh_token,
        "rotated-refresh"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn repeated_authentication_rejection_stops_after_one_refresh() {
    let (base, requests, handle) =
        scripted_server(vec![(401, REJECTED), (200, REFRESHED), (401, REJECTED)]);
    let dir = temp_test_dir("parity-bounded-auth-retry");
    let mut client = test_client(base.clone(), dir.join("session.json"));
    client.auth_base_url = base;
    assert!(client.quota().unwrap_err().to_string().contains("401"));
    handle.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 3);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn concurrent_rejected_tokens_reuse_the_single_rotated_session() {
    let (base, requests, handle) = scripted_server(vec![(200, REFRESHED)]);
    let dir = temp_test_dir("parity-concurrent-refresh");
    let mut client = test_client(base.clone(), dir.join("session.json"));
    client.auth_base_url = base;
    let client = Arc::new(client);
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let client = Arc::clone(&client);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                client.refresh_rejected_token("test-access").unwrap()
            })
        })
        .collect();
    for thread in threads {
        assert_eq!(thread.join().unwrap(), "fresh-access");
    }
    handle.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 1);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn move_batches_wait_for_tasks_and_invalidate_cached_lists() {
    let (base, requests, handle) = scripted_server(vec![
        (200, r#"{"task_id":"move-task"}"#),
        (
            200,
            r#"{"tasks":[{"id":"move-task","phase":"PHASE_TYPE_RUNNING"}]}"#,
        ),
        (
            200,
            r#"{"tasks":[{"id":"move-task","phase":"PHASE_TYPE_COMPLETE"}]}"#,
        ),
        (204, ""),
    ]);
    let dir = temp_test_dir("parity-move-batches");
    let client = test_client(base, dir.join("session.json"));
    client
        .ls_cache
        .lock()
        .unwrap()
        .entries
        .insert("source".into(), Vec::new());
    let ids: Vec<_> = (0..601).map(|i| format!("file-{i}")).collect();
    client
        .mv(
            &ids.iter().map(String::as_str).collect::<Vec<_>>(),
            "destination",
        )
        .unwrap();
    handle.join().unwrap();
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    for (index, count) in [(0, 500), (3, 101)] {
        let body: serde_json::Value =
            serde_json::from_str(requests[index].split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["ids"].as_array().unwrap().len(), count);
        assert_eq!(body["to"]["parent_id"], "destination");
    }
    assert!(requests[1].starts_with("GET /drive/v1/tasks?"));
    assert!(requests[1].contains("type="));
    assert!(client.ls_cache.lock().unwrap().entries.is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn failed_later_batch_reports_partial_completion_and_does_not_replay() {
    let (base, requests, handle) =
        scripted_server(vec![(200, "{}"), (500, r#"{"error":"fixture failure"}"#)]);
    let dir = temp_test_dir("parity-partial-batch");
    let client = test_client(base, dir.join("session.json"));
    let ids: Vec<_> = (0..501).map(|i| format!("file-{i}")).collect();
    let err = client
        .untrash(&ids.iter().map(String::as_str).collect::<Vec<_>>())
        .unwrap_err();
    assert!(format!("{err:#}").contains("500/501 items completed"));
    let progress = mutation_progress(&err.context("cart submission"));
    assert_eq!(progress.completed_items, 500);
    assert!(!progress.accepted);
    assert!(!progress.pending);
    handle.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 2);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn accepted_task_failure_is_not_reported_as_move_success() {
    let (base, _, handle) = scripted_server(vec![
        (200, r#"{"task_id":"move-task"}"#),
        (
            200,
            r#"{"tasks":[{"id":"move-task","phase":"PHASE_TYPE_ERROR","message":"fixture failure"}]}"#,
        ),
    ]);
    let dir = temp_test_dir("parity-task-failure");
    let client = test_client(base, dir.join("session.json"));
    let err = client.mv(&["file"], "destination").unwrap_err();
    assert!(format!("{err:#}").contains("fixture failure"));
    let progress = mutation_progress(&err);
    assert!(progress.accepted);
    assert!(!progress.pending);
    handle.join().unwrap();
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn accepted_task_timeout_reports_pending_and_is_not_resubmitted() {
    let mut responses = vec![(200, r#"{"task_id":"pending-task"}"#)];
    responses.extend(std::iter::repeat_n(
        (
            200,
            r#"{"tasks":[{"id":"pending-task","phase":"PHASE_TYPE_RUNNING"}]}"#,
        ),
        10,
    ));
    let (base, requests, handle) = scripted_server(responses);
    let dir = temp_test_dir("parity-task-pending");
    let client = test_client(base, dir.join("session.json"));
    let err = client.cp(&["file"], "destination").unwrap_err();
    assert!(format!("{err:#}").contains("still processing"));
    let progress = mutation_progress(&err);
    assert!(progress.accepted && progress.pending);
    assert_eq!(progress.completed_items, 0);
    handle.join().unwrap();
    assert_eq!(
        requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.starts_with("POST "))
            .count(),
        1
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn accepted_response_with_missing_task_identity_remains_pending() {
    for body in [
        r#"{"task":{"phase":"PHASE_TYPE_RUNNING"}}"#,
        r#"{"task_id":42}"#,
    ] {
        let (base, requests, handle) = scripted_server(vec![(200, body)]);
        let dir = temp_test_dir("parity-missing-task-identity");
        let client = test_client(base, dir.join("session.json"));
        let err = client.mv(&["file"], "destination").unwrap_err();
        let progress = mutation_progress(&err);
        assert!(progress.accepted && progress.pending);
        assert_eq!(progress.completed_items, 0);
        handle.join().unwrap();
        assert_eq!(requests.lock().unwrap().len(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn malformed_accepted_response_is_not_reported_as_completed() {
    let (base, requests, handle) = scripted_server(vec![(200, "not-json")]);
    let dir = temp_test_dir("parity-invalid-accepted-response");
    let client = test_client(base, dir.join("session.json"));
    let err = client.mv(&["file"], "destination").unwrap_err();
    assert!(mutation_progress(&err).pending);
    handle.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 1);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn failed_completion_check_does_not_replay_accepted_mutation() {
    let (base, requests, handle) = scripted_server(vec![
        (200, r#"{"task_id":"move-task"}"#),
        (500, r#"{"error":"fixture unavailable"}"#),
    ]);
    let dir = temp_test_dir("parity-poll-unavailable");
    let client = test_client(base, dir.join("session.json"));
    let err = client.mv(&["file"], "destination").unwrap_err();
    assert!(mutation_progress(&err).pending);
    assert!(format!("{err:#}").contains("completion check failed"));
    handle.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 2);
    assert_eq!(
        requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.starts_with("POST "))
            .count(),
        1
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn explicit_completed_task_needs_no_followup_request() {
    let (base, requests, handle) = scripted_server(vec![(
        200,
        r#"{"task":{"id":"done-task","phase":"PHASE_TYPE_COMPLETE"}}"#,
    )]);
    let dir = temp_test_dir("parity-task-complete-response");
    let client = test_client(base, dir.join("session.json"));
    client.mv(&["file"], "destination").unwrap();
    handle.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 1);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn nullable_task_fields_keep_synchronous_response_compatibility() {
    let (base, requests, handle) = scripted_server(vec![(200, r#"{"task":null,"task_id":null}"#)]);
    let dir = temp_test_dir("parity-null-task-response");
    let client = test_client(base, dir.join("session.json"));
    client.mv(&["file"], "destination").unwrap();
    handle.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 1);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn offline_pagination_retains_paused_unknown_and_refresh_interval() {
    let (base, requests, handle) = scripted_server(vec![
        (
            200,
            r#"{"tasks":[],"next_page_token":"next","expires_in":5}"#,
        ),
        (
            200,
            r#"{"tasks":[{"id":"paused","phase":"PHASE_TYPE_PAUSED"},{"id":"unknown","phase":"PHASE_TYPE_UNKNOW"}],"expires_in":2}"#,
        ),
    ]);
    let dir = temp_test_dir("parity-offline-pagination");
    let client = test_client(base, dir.join("session.json"));
    let result = client.offline_list(u32::MAX, OFFLINE_TASK_PHASES).unwrap();
    assert_eq!(result.tasks.len(), 2);
    assert_eq!(result.expires_in, Some(2));
    assert!(result.next_page_token.is_none());
    handle.join().unwrap();
    let requests = requests.lock().unwrap();
    assert!(requests[1].contains("page_token=next"));
    assert!(requests[0].contains("PHASE_TYPE_PAUSED"));
    assert!(requests[0].contains("PHASE_TYPE_UNKNOW"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn offline_explicit_limit_and_repeated_cursor_are_handled() {
    let (base, requests, handle) = scripted_server(vec![(
        200,
        r#"{"tasks":[{"id":"one"}],"next_page_token":"more"}"#,
    )]);
    let dir = temp_test_dir("parity-task-limit");
    let client = test_client(base, dir.join("session.json"));
    let result = client.offline_list(1, OFFLINE_TASK_PHASES).unwrap();
    assert_eq!(result.tasks.len(), 1);
    assert_eq!(result.next_page_token.as_deref(), Some("more"));
    handle.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 1);
    fs::remove_dir_all(dir).unwrap();

    let (base, _, handle) = scripted_server(vec![
        (200, r#"{"next_page_token":"same"}"#),
        (200, r#"{"next_page_token":"same"}"#),
    ]);
    let dir = temp_test_dir("parity-task-cursor-cycle");
    let client = test_client(base, dir.join("session.json"));
    assert!(
        client
            .offline_list(50, OFFLINE_TASK_PHASES)
            .unwrap_err()
            .to_string()
            .contains("repeated page token")
    );
    handle.join().unwrap();
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn file_lists_request_complete_phase_and_retain_audit_metadata() {
    let (base, requests, handle) = scripted_server(vec![(
        200,
        r#"{"files":[{"id":"file","name":"fixture","phase":"PHASE_TYPE_COMPLETE","audit":{"status":"STATUS_OK"}}]}"#,
    )]);
    let dir = temp_test_dir("parity-list-filters");
    let client = test_client(base, dir.join("session.json"));
    let entries = client.ls("").unwrap();
    assert_eq!(entries[0].phase.as_deref(), Some("PHASE_TYPE_COMPLETE"));
    assert_eq!(entries[0].audit.as_ref().unwrap()["status"], "STATUS_OK");
    handle.join().unwrap();
    let requests = requests.lock().unwrap();
    assert!(requests[0].contains("with_audit=true"));
    assert!(requests[0].contains("PHASE_TYPE_COMPLETE"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn folder_stats_count_every_page_and_compare_file_modifications_as_instants() {
    let files: Vec<_> = (0..500)
        .map(|id| serde_json::json!({"id": id.to_string(), "name": "file", "size": "1"}))
        .collect();
    let first_page = serde_json::json!({"files": files, "next_page_token": "more"}).to_string();
    let (base, requests, handle) = scripted_server(vec![
        (200, first_page),
        (200, r#"{"files":[
            {"kind":"drive#folder","id":"child","name":"child","size":900,"modified_time":"2099-01-01T00:00:00Z"},
            {"kind":"drive#file","id":"other","name":"other","size":2,"modified_time":"2026-10-02T08:00:00+08:00"}
        ]}"#.into()),
        (200, r#"{"files":[{"kind":"drive#folder","id":"deep","name":"deep"}]}"#.into()),
        (200, r#"{"files":[{"kind":"drive#file","id":"leaf","name":"leaf","size":"7","modified_time":"2026-10-02T00:00:00.000000001Z"}]}"#.into()),
    ]);
    let dir = temp_test_dir("folder-stats-pages");
    let client = test_client(base, dir.join("session.json"));
    let stats = client.folder_stats("", &AtomicBool::new(false)).unwrap();
    assert_eq!(stats.total_size_bytes, 509);
    assert_eq!(
        stats.latest_modified_time.as_deref(),
        Some("2026-10-02T00:00:00.000000001Z")
    );
    assert!(!stats.is_empty);

    handle.join().unwrap();
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    for (request, (parent, token)) in requests.iter().zip([
        ("", None),
        ("", Some("more")),
        ("child", None),
        ("deep", None),
    ]) {
        let path = request.split_whitespace().nth(1).unwrap();
        let url = reqwest::Url::parse(&format!("http://localhost{path}")).unwrap();
        assert_eq!(url.path(), "/drive/v1/files");
        let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(query["parent_id"], parent);
        assert_eq!(query.get("page_token").map(|s| s.as_ref()), token);
        let filters: serde_json::Value = serde_json::from_str(&query["filters"]).unwrap();
        assert_eq!(
            filters,
            serde_json::json!({"trashed":{"eq":false},"phase":{"eq":"PHASE_TYPE_COMPLETE"}})
        );
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn folder_stats_distinguish_empty_folders_from_zero_bytes_without_valid_times() {
    let (base, _, handle) = scripted_server(vec![
        (200, r#"{"files":[]}"#),
        (
            200,
            r#"{"files":[{"kind":"drive#folder","id":"empty","name":"empty"}]}"#,
        ),
        (200, r#"{"files":[]}"#),
        (200, r#"{"files":[{"id":"zero","name":"zero","size":0}]}"#),
        (
            200,
            r#"{"files":[
            {"id":"a","name":"a","size":0,"created_time":"2099-01-01T00:00:00Z"},
            {"id":"b","name":"b","size":0,"modified_time":null},
            {"id":"c","name":"c","size":0,"modified_time":""},
            {"id":"d","name":"d","size":3,"modified_time":"2026-02-30T12:00:00Z"},
            {"id":"e","name":"e","size":4,"modified_time":12345}
        ]}"#,
        ),
    ]);
    let dir = temp_test_dir("folder-stats-empty");
    let client = test_client(base, dir.join("session.json"));
    for (folder, bytes, is_empty) in [
        ("empty", 0, true),
        ("subfolders-only", 0, false),
        ("zero-byte-file", 0, false),
        ("invalid-times", 7, false),
    ] {
        let stats = client
            .folder_stats(folder, &AtomicBool::new(false))
            .unwrap();
        assert_eq!(stats.total_size_bytes, bytes);
        assert_eq!(stats.latest_modified_time, None);
        assert_eq!(stats.is_empty, is_empty, "{folder}");
    }
    handle.join().unwrap();
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn folder_stats_fail_when_a_childs_later_page_is_unreadable() {
    let (base, requests, handle) = scripted_server(vec![
        (
            200,
            r#"{"files":[{"kind":"drive#folder","id":"child","name":"child"},{"id":"file","name":"file","size":100}]}"#,
        ),
        (
            200,
            r#"{"files":[{"id":"partial","name":"partial","size":20}],"next_page_token":"more"}"#,
        ),
        (500, r#"{"error":"child page unavailable"}"#),
    ]);
    let dir = temp_test_dir("folder-stats-child-error");
    let client = test_client(base, dir.join("session.json"));
    let err = client
        .folder_stats("root", &AtomicBool::new(false))
        .unwrap_err();
    let FolderStatsError::ReadFolder { folder_id, source } = err else {
        panic!("expected the failed folder and its cause, got {err:?}");
    };
    assert_eq!(folder_id, "child");
    assert!(format!("{source:#}").contains("child page unavailable"));
    handle.join().unwrap();
    let requests = requests.lock().unwrap();
    assert!(requests[2].contains("parent_id=child") && requests[2].contains("page_token=more"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn folder_stats_stop_before_another_page_when_cancelled_during_a_request() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let dir = temp_test_dir("folder-stats-cancel");
    let client = test_client(base, dir.join("session.json"));
    let cancelled = AtomicBool::new(false);
    let mut requests = 0;
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| client.folder_stats("root", &cancelled));
        while !worker.is_finished() {
            match accept_test_connection(&listener) {
                Ok(mut stream) => {
                    read_test_http_request(&mut stream).unwrap();
                    requests += 1;
                    cancelled.store(true, Ordering::Relaxed);
                    let body: &[u8] = if requests == 1 {
                        br#"{"files":[{"id":"partial","name":"partial","size":17}],"next_page_token":"more"}"#
                    } else {
                        br#"{"files":[]}"#
                    };
                    write_response(&mut stream, 200, "OK", body);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("failed to accept listing request: {e}"),
            }
        }
        assert!(matches!(
            worker.join().unwrap(),
            Err(FolderStatsError::Cancelled)
        ));
    });
    assert_eq!(requests, 1);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn folder_stats_honour_pre_cancellation_before_session_access() {
    let dir = temp_test_dir("folder-stats-pre-cancel");
    let session = dir.join("session.json");
    let client = test_client(String::new(), session.clone());
    fs::remove_file(session).unwrap();
    assert!(matches!(
        client.folder_stats("root", &AtomicBool::new(true)),
        Err(FolderStatsError::Cancelled)
    ));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn folder_stats_report_size_overflow() {
    let (base, _, handle) = scripted_server(vec![(
        200,
        r#"{"files":[
        {"id":"max","name":"max","size":"18446744073709551615"},
        {"id":"one","name":"one","size":1}
    ]}"#,
    )]);
    let dir = temp_test_dir("folder-stats-overflow");
    let client = test_client(base, dir.join("session.json"));
    assert!(matches!(
        client.folder_stats("root", &AtomicBool::new(false)),
        Err(FolderStatsError::SizeOverflow)
    ));
    handle.join().unwrap();
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn upload_waits_until_the_cloud_file_is_complete() {
    let (base, requests, handle) = scripted_server(vec![
        (200, r#"{"name":"fixture","phase":"PHASE_TYPE_RUNNING"}"#),
        (200, r#"{"name":"fixture","phase":"PHASE_TYPE_COMPLETE"}"#),
    ]);
    let dir = temp_test_dir("parity-upload-finalization");
    let client = test_client(base, dir.join("session.json"));
    client.wait_uploaded_file("file").unwrap();
    handle.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 2);
    fs::remove_dir_all(dir).unwrap();
}
