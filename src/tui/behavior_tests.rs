use super::*;
use crate::pikpak::{CreateShareResponse, MyShare, OfflineListResponse, test_batch_failure};
use operations::MutationReport;

fn app() -> App {
    App::new_login(PikPak::new().unwrap(), None, TuiConfig::default())
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
        OpResult::Err("Background fixture failed".into()),
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
