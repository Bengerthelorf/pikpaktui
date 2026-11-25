use super::*;
use crate::pikpak::{CreateShareResponse, MyShare, OfflineListResponse, test_batch_failure};
use operations::MutationReport;

fn app() -> App {
    App::new_login(PikPak::new().unwrap(), None, TuiConfig::default())
}

#[test]
fn cancelled_folder_scan_holds_slot_and_its_late_result_is_ignored() {
    use std::sync::atomic::Ordering;
    let mut app = app();
    app.entries = vec![Entry {
        kind: EntryKind::Folder,
        ..entry("folder")
    }];
    let (id, token) = app
        .folder_stats
        .next_folder(&app.entries, std::iter::empty(), true)
        .unwrap();
    app.invalidate_main_listing();
    assert!(token.load(Ordering::Relaxed));
    assert!(
        app.folder_stats
            .next_folder(&app.entries, std::iter::empty(), true)
            .is_none()
    );

    // A response can have been sent before the user refreshed the same folder.
    let old_stats = crate::pikpak::FolderStats {
        total_size_bytes: 99,
        latest_modified_time: None,
        is_empty: false,
    };
    app.result_tx
        .send(OpResult::FolderStats(id.clone(), Ok(old_stats)))
        .unwrap();
    app.poll_results();
    assert!(app.folder_stats.completed(&id).is_none());
    assert!(!app.folder_stats.failed(&id));
    assert!(
        app.folder_stats
            .next_folder(&app.entries, std::iter::empty(), true)
            .is_some()
    );
}

#[test]
fn folder_scan_queue_visits_all_folders_once_and_retries_failures_only_after_refresh() {
    let mut app = app();
    app.entries = vec![entry("file")];
    for id in ["first", "second"] {
        app.entries.push(Entry {
            kind: EntryKind::Folder,
            ..entry(id)
        });
    }
    let (first, _) = app
        .folder_stats
        .next_folder(&app.entries, std::iter::empty(), true)
        .unwrap();
    assert_eq!(first, "first");
    app.result_tx
        .send(OpResult::FolderStats(
            first.clone(),
            Err(crate::pikpak::FolderStatsError::SizeOverflow),
        ))
        .unwrap();
    app.poll_results();
    assert!(app.folder_stats.failed(&first));
    let (second, _) = app
        .folder_stats
        .next_folder(&app.entries, std::iter::empty(), true)
        .unwrap();
    assert_eq!(second, "second");
    app.folder_stats.finish(
        second,
        Ok(crate::pikpak::FolderStats {
            total_size_bytes: 0,
            latest_modified_time: None,
            is_empty: true,
        }),
    );
    assert!(
        app.folder_stats
            .next_folder(&app.entries, std::iter::empty(), true)
            .is_none()
    );
    app.invalidate_main_listing();
    assert_eq!(
        app.folder_stats
            .next_folder(&app.entries, std::iter::empty(), true)
            .unwrap()
            .0,
        first
    );
}

#[test]
fn browsing_scans_selected_then_visible_folders_and_global_sort_includes_the_rest() {
    let mut app = app();
    app.entries = ["hidden", "visible", "selected"]
        .map(|id| Entry {
            kind: EntryKind::Folder,
            ..entry(id)
        })
        .to_vec();
    let visible = [2, 1, 2];
    for wanted in ["selected", "visible"] {
        let (id, _) = app
            .folder_stats
            .next_folder(&app.entries, visible.into_iter(), false)
            .unwrap();
        assert_eq!(id, wanted);
        app.folder_stats
            .finish(id, Err(crate::pikpak::FolderStatsError::SizeOverflow));
    }
    assert!(
        app.folder_stats
            .next_folder(&app.entries, visible.into_iter(), false)
            .is_none()
    );
    assert_eq!(
        app.folder_stats
            .next_folder(&app.entries, visible.into_iter(), true)
            .unwrap()
            .0,
        "hidden"
    );
}

#[test]
fn cached_index_values_survive_failed_checks_but_never_leak_across_views() {
    use super::folder_stats::IndexUpdate;
    use std::collections::HashMap;
    let mut app = app();
    app.entries = vec![Entry {
        kind: EntryKind::Folder,
        ..entry("one")
    }];
    let start = |app: &mut App| {
        app.folder_stats
            .next_folder(&app.entries, [0].into_iter(), false)
            .unwrap()
            .1
    };
    let value = crate::pikpak::FolderStats {
        total_size_bytes: 15,
        latest_modified_time: None,
        is_empty: false,
    };
    let snapshot = || HashMap::from([("one".to_owned(), value.clone())]);
    let token = start(&mut app);
    app.apply_folder_index(&token, IndexUpdate::Cached(snapshot()));
    app.apply_folder_index(&token, IndexUpdate::Failed("offline".into()));
    assert_eq!(app.folder_stats.completed("one"), Some(&value));
    assert!(app.folder_stats.is_cached("one"));
    app.folder_stats.leave_listing();
    let new_token = start(&mut app);
    app.apply_folder_index(&token, IndexUpdate::Synced(snapshot()));
    assert!(app.folder_stats.completed("one").is_none());
    app.apply_folder_index(&new_token, IndexUpdate::Cached(snapshot()));
    app.apply_folder_index(&new_token, IndexUpdate::Synced(HashMap::new()));
    assert!(app.folder_stats.completed("one").is_none());
    assert!(!app.folder_stats.is_cached("one"));
}

#[test]
fn folder_own_timestamp_does_not_launch_a_recursive_scan() {
    let mut app = app();
    app.input = InputMode::Normal;
    app.config.folder_stats = true;
    app.config.sort_field = crate::config::SortField::Modified;
    app.entries = vec![Entry {
        kind: EntryKind::Folder,
        ..entry("visible")
    }];
    app.schedule_folder_stats();
    assert!(!app.folder_stats.running());
}

#[test]
fn root_drive_scan_reconciles_visible_siblings_and_falls_back_once() {
    let mut app = app();
    app.entries = ["first", "second", "third", "fourth"]
        .map(|id| Entry {
            kind: EntryKind::Folder,
            ..entry(id)
        })
        .to_vec();
    let value = crate::pikpak::FolderStats {
        total_size_bytes: 0,
        latest_modified_time: None,
        is_empty: true,
    };
    app.folder_stats.record("first".into(), Ok(value.clone()));
    app.folder_stats.record("second".into(), Ok(value.clone()));
    assert!(
        app.folder_stats.begin_drive(&app.entries).is_none(),
        "only half the folders are cold"
    );
    app.folder_stats.invalidate();
    assert!(app.folder_stats.begin_drive(&app.entries).is_some());
    let incomplete =
        std::collections::HashMap::from([(String::new(), value.clone()), ("first".into(), value)]);
    assert!(!app.folder_stats.finish_drive(Ok(incomplete), &app.entries));
    assert!(app.folder_stats.completed("first").is_none());
    assert!(app.folder_stats.begin_drive(&app.entries).is_none());
    assert!(
        app.folder_stats
            .next_folder(&app.entries, std::iter::empty(), true)
            .is_some()
    );
}

#[test]
fn text_preview_keeps_content_open_while_paging_and_scrolling() {
    let mut app = app();
    app.text_preview_page.set(10);
    let request = app.begin_modal_request(AsyncRequestKind::FilePreview, "file");
    app.input = InputMode::InfoLoading;
    app.result_tx
        .send(OpResult::PreviewText(
            request,
            Ok(PreparedTextPreview {
                name: "notes.txt".into(),
                lines: (0..100)
                    .map(|i| ratatui::text::Line::from(i.to_string()))
                    .collect(),
                size: 1000,
                truncated: true,
            }),
        ))
        .unwrap();
    app.poll_results();
    app.handle_key(KeyCode::PageDown, KeyModifiers::NONE)
        .unwrap();
    assert!(matches!(
        app.input,
        InputMode::TextPreviewView { scroll: 10, .. }
    ));
    app.handle_mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::ScrollDown,
        column: 5,
        row: 5,
        modifiers: KeyModifiers::NONE,
    });
    assert!(matches!(
        app.input,
        InputMode::TextPreviewView { scroll: 13, .. }
    ));
    app.handle_key(KeyCode::End, KeyModifiers::NONE).unwrap();
    assert!(matches!(
        app.input,
        InputMode::TextPreviewView { scroll: 90, .. }
    ));
    app.handle_key(KeyCode::Esc, KeyModifiers::NONE).unwrap();
    assert!(matches!(app.input, InputMode::Normal));
}

#[test]
fn queued_results_leave_room_for_input() {
    let mut app = app();
    app.input = InputMode::Normal;
    app.entries = vec![entry("a"), entry("b")];
    for _ in 0..100 {
        app.result_tx.send(OpResult::UpdateAvailable(None)).unwrap();
    }
    app.poll_results();
    app.handle_key(KeyCode::Char('j'), KeyModifiers::NONE)
        .unwrap();
    assert_eq!(app.selected, 1);
    assert!(
        app.result_rx.try_recv().is_ok(),
        "result flood must not be drained in one frame"
    );
}

#[test]
fn background_upload_and_cancellation_keep_quit_guard_until_worker_finishes() {
    let mut app = app();
    let control = Arc::new(crate::pikpak::UploadControl::default());
    app.active_upload = Some(Arc::clone(&control));
    app.input = InputMode::UploadView;
    app.handle_key(KeyCode::Char('x'), KeyModifiers::NONE)
        .unwrap();
    assert!(control.is_cancelled());
    app.handle_key(KeyCode::Esc, KeyModifiers::NONE).unwrap();
    assert!(matches!(app.input, InputMode::Normal));
    assert!(
        !app.handle_key(KeyCode::Char('q'), KeyModifiers::NONE)
            .unwrap()
    );
    assert!(matches!(app.input, InputMode::ConfirmQuit));
    app.handle_key(KeyCode::Esc, KeyModifiers::NONE).unwrap();
    assert!(
        !app.handle_key(KeyCode::Char('c'), KeyModifiers::CONTROL)
            .unwrap()
    );
    assert!(matches!(app.input, InputMode::ConfirmQuit));
    app.active_upload = None;
    app.handle_key(KeyCode::Esc, KeyModifiers::NONE).unwrap();
    assert!(
        app.handle_key(KeyCode::Char('q'), KeyModifiers::NONE)
            .unwrap()
    );
}

#[test]
fn pending_file_mutations_require_quit_confirmation() {
    let mut app = app();
    app.input = InputMode::Normal;
    app.file_mutations_pending = 1;
    assert!(
        !app.handle_key(KeyCode::Char('q'), KeyModifiers::NONE)
            .unwrap()
    );
    assert!(matches!(app.input, InputMode::ConfirmQuit));
    app.record_mutation_result(MutationReport {
        op: "Rename",
        success: "Renamed".into(),
        result: Ok(()),
        cart_ids: None,
    });
    assert_eq!(app.file_mutations_pending, 0);
}

#[test]
fn detached_download_worker_and_closed_write_dialog_still_guard_quit() {
    let mut app = app();
    app.input = InputMode::Normal;
    app.download_state.active_ids.insert(99);
    assert!(
        !app.handle_key(KeyCode::Char('q'), KeyModifiers::NONE)
            .unwrap()
    );
    app.download_state.active_ids.clear();
    app.handle_key(KeyCode::Esc, KeyModifiers::NONE).unwrap();
    let guard = operations::WriteGuard::new(&app.background_writes);
    app.invalidate_modal_request();
    assert!(app.has_active_work());
    drop(guard);
    assert!(!app.has_active_work());
    assert!(
        app.handle_key(KeyCode::Char('q'), KeyModifiers::NONE)
            .unwrap()
    );
}

#[test]
fn listing_error_survives_toast_and_rejects_stale_failure() {
    let mut app = app();
    app.input = InputMode::Normal;
    app.current_folder_id = "current".into();
    app.main_listing_request_id = 2;
    app.main_listing_loading = true;
    app.result_tx
        .send(OpResult::Ls(
            1,
            "older".into(),
            Err(anyhow::anyhow!("stale")),
        ))
        .unwrap();
    app.poll_results();
    assert!(app.main_listing_loading);
    assert!(app.main_listing_error.is_none());
    app.result_tx
        .send(OpResult::Ls(
            2,
            "current".into(),
            Err(anyhow::anyhow!("timeout")),
        ))
        .unwrap();
    app.poll_results();
    assert!(!app.main_listing_loading);
    app.status_message = None;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("Listing failed"));
    assert!(text.contains("retry"));
}
fn entry(id: &str) -> Entry {
    Entry {
        id: id.into(),
        name: id.into(),
        kind: EntryKind::File,
        size: 1,
        created_time: String::new(),
        modified_time: String::new(),
        starred: false,
        thumbnail_link: None,
        phase: None,
        audit: None,
    }
}
fn share(id: &str) -> MyShare {
    serde_json::from_value(
        serde_json::json!({"share_id":id,"share_url":"https://example.invalid/s/fixture"}),
    )
    .unwrap()
}
fn report(ids: Option<Vec<String>>, result: Result<()>) -> MutationReport {
    MutationReport {
        op: "Move",
        success: "Moved fixture".into(),
        result,
        cart_ids: ids,
    }
}
fn fill_cart(app: &mut App, ids: &[&str]) {
    app.cart = ids.iter().map(|id| entry(id)).collect();
    app.cart_ids = ids.iter().map(|id| (*id).to_owned()).collect();
    app.cart_mutation_in_flight = true;
}

#[test]
fn ordinary_key_during_loading_does_not_dismiss_view_or_lose_request() {
    let mut app = app();
    app.input = InputMode::InfoLoading;
    app.loading = true;
    let request = app.begin_modal_request(AsyncRequestKind::OfflineTasks, "tasks");
    app.handle_key(KeyCode::Char('j'), KeyModifiers::NONE)
        .unwrap();
    assert!(matches!(app.input, InputMode::InfoLoading));
    assert!(app.loading);
    assert_eq!(app.modal_request.as_ref(), Some(&request));
}

#[test]
fn closing_loading_trash_blocks_late_list_and_late_operation() {
    let mut app = app();
    app.input = InputMode::TrashView {
        entries: vec![],
        selected: 0,
        expanded: false,
    };
    app.loading = true;
    let request = app.begin_modal_request(AsyncRequestKind::Trash, "trash");
    app.handle_key(KeyCode::Esc, KeyModifiers::NONE).unwrap();
    assert!(matches!(app.input, InputMode::Normal));
    app.result_tx
        .send(OpResult::TrashList(
            request.clone(),
            Ok(vec![entry("late")]),
        ))
        .unwrap();
    app.result_tx
        .send(OpResult::TrashOp(request, Ok("Restored fixture".into())))
        .unwrap();
    app.poll_results();
    assert!(matches!(app.input, InputMode::Normal));
    assert!(app.modal_request.is_none() && !app.loading);
}

#[test]
fn older_trash_list_cannot_replace_reopened_view() {
    let mut app = app();
    app.input = InputMode::TrashView {
        entries: vec![entry("current")],
        selected: 0,
        expanded: false,
    };
    let old = app.begin_modal_request(AsyncRequestKind::Trash, "trash");
    let current = app.begin_modal_request(AsyncRequestKind::Trash, "trash");
    app.apply_trash_list(old, Ok(vec![entry("stale")]));
    assert!(
        matches!(&app.input, InputMode::TrashView { entries, .. } if entries[0].id == "current")
    );
    assert_eq!(app.modal_request.as_ref(), Some(&current));
}

#[test]
fn trash_refresh_preserves_selected_identity_after_reordering() {
    let mut app = app();
    app.trash_entries = vec![entry("a"), entry("b")];
    app.trash_selected = 1;
    app.input = InputMode::TrashView {
        entries: app.trash_entries.clone(),
        selected: 1,
        expanded: true,
    };
    app.trash_expanded = true;
    let request = app.begin_modal_request(AsyncRequestKind::Trash, "trash");
    app.apply_trash_list(request, Ok(vec![entry("b"), entry("a")]));
    assert!(matches!(
        app.input,
        InputMode::TrashView {
            selected: 0,
            expanded: true,
            ..
        }
    ));
}

#[test]
fn failed_trash_refresh_retains_the_visible_snapshot() {
    let mut app = app();
    app.input = InputMode::TrashView {
        entries: vec![entry("a")],
        selected: 0,
        expanded: false,
    };
    let request = app.begin_modal_request(AsyncRequestKind::Trash, "trash");
    app.apply_trash_list(request, Err(anyhow::anyhow!("fixture failure")));
    assert!(matches!(&app.input, InputMode::TrashView { entries, .. } if entries[0].id == "a"));
}

#[test]
fn late_offline_operation_does_not_replace_a_new_modal_or_release_its_spinner() {
    let mut app = app();
    let old = app.begin_modal_request(AsyncRequestKind::OfflineOp, "old-task");
    let current = app.begin_modal_request(AsyncRequestKind::Info, "current-file");
    app.input = InputMode::InfoLoading;
    app.loading = true;
    app.apply_task_operation(old, Ok("Deleted task fixture".into()), false);
    assert!(matches!(app.input, InputMode::InfoLoading));
    assert_eq!(app.modal_request.as_ref(), Some(&current));
    assert!(app.loading);
}

#[test]
fn old_automatic_refresh_cannot_take_over_a_pending_offline_operation() {
    let mut app = app();
    let old = app.begin_modal_request(AsyncRequestKind::OfflineTasks, "tasks");
    let current = app.begin_modal_request(AsyncRequestKind::OfflineOp, "retry-task");
    app.input = InputMode::InfoLoading;
    app.apply_offline_tasks(
        old,
        Ok(OfflineListResponse {
            tasks: vec![],
            next_page_token: None,
            expires_in: Some(2),
        }),
    );
    assert_eq!(app.modal_request.as_ref(), Some(&current));
    assert!(matches!(app.input, InputMode::InfoLoading));
}

#[test]
fn closed_and_reopened_share_list_rejects_an_older_response() {
    let mut app = app();
    app.input = InputMode::MySharesView {
        shares: vec![share("old")],
        selected: 0,
        confirm_delete: None,
    };
    let old = app.begin_modal_request(AsyncRequestKind::MyShares, "my-shares");
    app.handle_key(KeyCode::Esc, KeyModifiers::NONE).unwrap();
    let current = app.begin_modal_request(AsyncRequestKind::MyShares, "my-shares");
    app.input = InputMode::MySharesView {
        shares: vec![share("current")],
        selected: 0,
        confirm_delete: None,
    };
    app.apply_my_shares(old, Ok(vec![share("stale")]));
    assert_eq!(app.modal_request.as_ref(), Some(&current));
    assert!(
        matches!(&app.input, InputMode::MySharesView { shares, .. } if shares[0].share_id == "current")
    );
}

#[test]
fn share_refresh_preserves_selected_identity() {
    let mut app = app();
    app.input = InputMode::MySharesView {
        shares: vec![share("a"), share("b")],
        selected: 1,
        confirm_delete: None,
    };
    let request = app.begin_modal_request(AsyncRequestKind::MyShares, "my-shares");
    app.handle_key(KeyCode::Char('z'), KeyModifiers::NONE)
        .unwrap();
    assert_eq!(app.modal_request.as_ref(), Some(&request));
    app.apply_my_shares(request, Ok(vec![share("b"), share("a")]));
    assert!(matches!(
        app.input,
        InputMode::MySharesView { selected: 0, .. }
    ));
}

#[test]
fn closing_share_creation_blocks_late_popup_and_clipboard_updates() {
    let mut app = app();
    app.input = InputMode::ShareCreatedView { shares: vec![] };
    app.share_creations_pending = 1;
    app.loading = true;
    let request = app.begin_modal_request(AsyncRequestKind::ShareCreate, "create-shares");
    app.handle_key(KeyCode::Esc, KeyModifiers::CONTROL).unwrap();
    assert!(!app.loading);
    let response: CreateShareResponse = serde_json::from_value(
        serde_json::json!({"share_id":"fixture","share_url":"https://example.invalid/s/fixture"}),
    )
    .unwrap();
    assert!(
        app.record_share_created(request, "fixture".into(), Ok(response))
            .is_none()
    );
    assert!(matches!(app.input, InputMode::CartView));
}

#[test]
fn failed_cart_operation_keeps_items_and_releases_busy_state() {
    let mut app = app();
    fill_cart(&mut app, &["a", "b"]);
    assert!(!app.record_mutation_result(report(
        Some(vec!["a".into(), "b".into()]),
        Err(anyhow::anyhow!("fixture failure"))
    )));
    assert_eq!(app.cart.len(), 2);
    assert!(app.cart_ids.contains("a") && app.cart_ids.contains("b"));
    assert!(!app.cart_mutation_in_flight);
}

#[test]
fn cart_success_removes_only_the_submitted_snapshot() {
    let mut app = app();
    fill_cart(&mut app, &["a", "b", "added-later"]);
    assert!(app.record_mutation_result(report(Some(vec!["a".into(), "b".into()]), Ok(()))));
    assert_eq!(
        app.cart.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        vec!["added-later"]
    );
    assert_eq!(app.cart_ids, HashSet::from(["added-later".to_owned()]));
}

#[test]
fn partial_cart_failure_removes_only_confirmed_completed_items() {
    let mut app = app();
    fill_cart(&mut app, &["a", "b", "c"]);
    let error = test_batch_failure(1, false, false).context("outer context");
    assert!(app.record_mutation_result(report(
        Some(vec!["a".into(), "b".into(), "c".into()]),
        Err(error)
    )));
    assert_eq!(
        app.cart.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        vec!["b", "c"]
    );
    assert!(!app.cart_ids.contains("a"));
}

#[test]
fn pending_mutation_is_a_warning_and_requests_refresh_without_losing_cart() {
    let mut app = app();
    fill_cart(&mut app, &["a"]);
    let error = test_batch_failure(0, true, true);
    assert!(app.record_mutation_result(report(Some(vec!["a".into()]), Err(error))));
    assert_eq!(app.cart.len(), 1);
    assert_eq!(app.status_message.unwrap().kind, StatusKind::Warning);
}

#[test]
fn background_mutation_failure_does_not_clear_new_modal_loading_state() {
    let mut app = app();
    app.input = InputMode::InfoLoading;
    app.loading = true;
    let request = app.begin_modal_request(AsyncRequestKind::Info, "new-info");
    app.record_mutation_result(report(None, Err(anyhow::anyhow!("fixture failure"))));
    assert_eq!(app.modal_request.as_ref(), Some(&request));
    assert!(app.loading);
}

#[test]
fn background_listing_and_error_do_not_clear_current_modal_spinner() {
    let mut app = app();
    app.input = InputMode::InfoLoading;
    app.loading = true;
    app.loading_label = Some("Loading current info...".into());
    let request = app.begin_modal_request(AsyncRequestKind::Info, "current-info");
    for result in [
        OpResult::Ls(
            app.main_listing_request_id,
            app.current_folder_id.clone(),
            Ok(vec![]),
        ),
        OpResult::Ls(
            app.main_listing_request_id,
            app.current_folder_id.clone(),
            Err(anyhow::anyhow!("fixture failure")),
        ),
        OpResult::Clipboard(Err(anyhow::anyhow!("Background fixture failed"))),
    ] {
        app.result_tx.send(result).unwrap();
        app.poll_results();
        assert_eq!(app.modal_request.as_ref(), Some(&request));
        assert!(matches!(app.input, InputMode::InfoLoading));
        assert!(app.loading);
        assert_eq!(
            app.loading_label.as_deref(),
            Some("Loading current info...")
        );
    }
}

#[test]
fn share_batch_releases_loading_after_all_success_and_failure_results() {
    let mut app = app();
    app.input = InputMode::ShareCreatedView { shares: vec![] };
    app.share_creations_pending = 2;
    app.loading = true;
    let request = app.begin_modal_request(AsyncRequestKind::ShareCreate, "create-shares");
    let response: CreateShareResponse = serde_json::from_value(
        serde_json::json!({"share_id":"fixture","share_url":"https://example.invalid/s/fixture"}),
    )
    .unwrap();
    assert!(
        app.record_share_created(request.clone(), "fixture".into(), Ok(response))
            .is_some()
    );
    assert_eq!(app.share_creations_pending, 1);
    assert!(app.loading);
    assert!(
        app.record_share_created(
            request,
            "failed".into(),
            Err(anyhow::anyhow!("fixture failure"))
        )
        .is_none()
    );
    assert_eq!(app.share_creations_pending, 0);
    assert!(app.modal_request.is_none());
    assert!(!app.loading);
    assert!(matches!(app.input, InputMode::ShareCreatedView { ref shares } if shares.len() == 1));
}

#[test]
fn old_share_batch_cannot_decrement_or_clear_reopened_batch() {
    let mut app = app();
    let old = app.begin_modal_request(AsyncRequestKind::ShareCreate, "create-shares");
    let new = app.begin_modal_request(AsyncRequestKind::ShareCreate, "create-shares");
    app.input = InputMode::ShareCreatedView { shares: vec![] };
    app.share_creations_pending = 2;
    app.loading = true;
    app.record_share_created(old, "old".into(), Err(anyhow::anyhow!("fixture failure")));
    assert_eq!(app.share_creations_pending, 2);
    assert_eq!(app.modal_request.as_ref(), Some(&new));
    assert!(app.loading);
}

#[test]
fn duplicate_cart_submission_is_rejected_before_worker_starts() {
    let mut app = app();
    fill_cart(&mut app, &["a"]);
    app.spawn_file_mutation(
        operations::FileAction::Trash,
        vec!["a".into()],
        "fixture".into(),
        true,
    );
    assert!(matches!(app.input, InputMode::CartView));
    assert!(app.cart_mutation_in_flight);
    assert_eq!(app.cart.len(), 1);
    assert_eq!(app.status_message.unwrap().kind, StatusKind::Warning);
    assert!(app.result_rx.try_recv().is_err());
}

#[test]
fn outgoing_share_cart_options_and_update_selection_follow_keyboard_input() {
    let mut app = app();
    app.cart = vec![entry("a"), entry("b")];
    app.input = InputMode::CartView;
    for key in ['s', 'g', '7'] {
        app.handle_key(KeyCode::Char(key), KeyModifiers::NONE)
            .unwrap();
    }
    assert!(matches!(&app.input,InputMode::SharePrompt{grouped:true,days} if days=="7"));
    app.handle_key(KeyCode::Esc, KeyModifiers::NONE).unwrap();
    assert!(matches!(app.input, InputMode::CartView));
    app.input = InputMode::SharePage {
        page: share_views::SharePage::Files("share".into(), 17),
        rows: vec![serde_json::json!("a"), serde_json::json!("b")],
        selected: 0,
        marked: Default::default(),
    };
    for key in ['j', ' ', 's'] {
        app.handle_key(KeyCode::Char(key), KeyModifiers::NONE)
            .unwrap();
    }
    assert!(
        matches!(&app.input,InputMode::ShareEdit{field:share_views::ShareField::Save {share,record:17,ids,destination:None},..} if share=="share" && ids==&["b"])
    );
    app.handle_key(KeyCode::Char('X'), KeyModifiers::NONE)
        .unwrap();
    app.handle_key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
    assert!(
        matches!(&app.input,InputMode::ShareEdit{field:share_views::ShareField::Save{destination:Some(path),..},..} if path=="/X")
    );
}
