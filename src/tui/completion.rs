use crate::pikpak::EntryKind;

use super::App;

#[derive(Default)]
pub(super) struct PathInput {
    pub value: String,
    pub candidates: Vec<String>,
    pub candidate_idx: Option<usize>,
    pub completion_base: String,
    pub pending_request_id: Option<u64>,
}

impl PathInput {
    pub fn new() -> Self {
        Self {
            value: String::new(),
            candidates: Vec::new(),
            candidate_idx: None,
            completion_base: String::new(),
            pending_request_id: None,
        }
    }

    /// Mark one completion lookup as pending. Repeated Tab presses while that
    /// lookup is in flight reuse no resources and therefore cannot create an
    /// unbounded queue of network threads.
    fn begin_completion_request(&mut self, request_id: u64) -> bool {
        if self.pending_request_id.is_some() {
            return false;
        }
        self.pending_request_id = Some(request_id);
        true
    }

    pub(super) fn clear_completion(&mut self) {
        self.candidates.clear();
        self.candidate_idx = None;
        self.completion_base.clear();
    }
}

fn reserve_completion_request(
    in_flight: &mut Option<u64>,
    input: &mut PathInput,
    request_id: u64,
) -> bool {
    if in_flight.is_some() || !input.begin_completion_request(request_id) {
        return false;
    }
    *in_flight = Some(request_id);
    true
}

impl App {
    /// Cycle already-known candidates synchronously; otherwise fetch them on
    /// a worker thread (resolve + ls block for a network round-trip) and
    /// apply through OpResult::PathCandidates when they arrive.
    pub(super) fn tab_complete(&mut self, input: &mut PathInput) {
        if !input.candidates.is_empty() {
            let idx = match input.candidate_idx {
                Some(i) => (i + 1) % input.candidates.len(),
                None => 0,
            };
            input.candidate_idx = Some(idx);
            // Use stored completion_base instead of re-parsing the current value
            let parent = &input.completion_base;
            let selected = &input.candidates[idx];
            input.value = join_completed(parent, selected);
            return;
        }

        let (parent_path, prefix) = split_path_prefix(&input.value);
        let completion_parent = completion_parent_path(&self.current_path_display(), &parent_path);
        let request_id = self.next_async_request_id();
        if !reserve_completion_request(&mut self.path_completion_in_flight, input, request_id) {
            return;
        }
        let value_snapshot = input.value.clone();
        let current_folder = self.current_folder_id.clone();
        let folder_context = current_folder.clone();
        let client = std::sync::Arc::clone(&self.client);
        let tx = self.result_tx.clone();

        std::thread::spawn(move || {
            let matches = (|| {
                let parent_id = if parent_path.is_empty() {
                    // Relative: use current folder
                    current_folder
                } else {
                    client.resolve_path(&completion_parent).ok()?
                };

                let entries = client.ls(&parent_id).ok()?;
                let prefix_lower = prefix.to_lowercase();
                Some(
                    entries
                        .iter()
                        .filter(|e| e.kind == EntryKind::Folder)
                        .filter(|e| e.name.to_lowercase().starts_with(&prefix_lower))
                        .map(|e| e.name.clone())
                        .collect(),
                )
            })()
            .unwrap_or_default();

            let _ = tx.send(super::OpResult::PathCandidates {
                request_id,
                value: value_snapshot,
                folder_context,
                parent: completion_parent,
                matches,
            });
        });
    }
}

/// Fill the input from freshly computed candidates (single match completes
/// directly, several start a Tab cycle).
pub(super) fn apply_path_candidates(
    input: &mut PathInput,
    parent_path: String,
    matches: Vec<String>,
) {
    // Store the parent path as completion base for subsequent Tab presses
    input.completion_base = parent_path.clone();

    if matches.len() == 1 {
        input.value = join_completed(&parent_path, &matches[0]);
        input.candidates.clear();
        input.candidate_idx = None;
    } else {
        input.candidates = matches;
        input.candidate_idx = Some(0);
        input.value = join_completed(&parent_path, &input.candidates[0]);
    }
}

/// Consume a completion response only when it belongs to the currently
/// pending request and both its text and navigation context are unchanged.
/// A response for the matching request clears `pending_request_id` even when
/// stale, allowing a fresh Tab request immediately.
pub(super) fn path_candidate_result_matches(
    input: &mut PathInput,
    request_id: u64,
    requested_value: &str,
    current_folder_id: &str,
    requested_folder_id: &str,
) -> bool {
    if input.pending_request_id != Some(request_id) {
        return false;
    }
    input.pending_request_id = None;
    input.value == requested_value && current_folder_id == requested_folder_id
}

fn join_completed(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        format!("{}/", name)
    } else if parent == "/" {
        format!("/{}/", name)
    } else {
        format!("{}/{}/", parent, name)
    }
}

fn completion_parent_path(current_path: &str, typed_parent: &str) -> String {
    if typed_parent.starts_with('/') {
        typed_parent.to_string()
    } else if typed_parent.is_empty() {
        current_path.to_string()
    } else if current_path == "/" {
        format!("/{typed_parent}")
    } else {
        format!("{current_path}/{typed_parent}")
    }
}

/// Split a path input into (parent_path, prefix).
/// "/My Pack/sub" -> ("/My Pack", "sub")
/// "/My Pack/"    -> ("/My Pack", "")
/// "/"            -> ("/", "")
/// ""             -> ("", "")
/// "sub"          -> ("", "sub")
pub(super) fn split_path_prefix(input: &str) -> (String, String) {
    if input.is_empty() {
        return (String::new(), String::new());
    }
    if input == "/" {
        return ("/".to_string(), String::new());
    }
    if input.ends_with('/') {
        let trimmed = input.trim_end_matches('/');
        return (trimmed.to_string(), String::new());
    }
    match input.rsplit_once('/') {
        Some(("", name)) => ("/".to_string(), name.to_string()),
        Some((parent, name)) => (parent.to_string(), name.to_string()),
        None => (String::new(), input.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PathInput, completion_parent_path, join_completed, path_candidate_result_matches,
        reserve_completion_request,
    };

    #[test]
    fn relative_completion_parents_are_anchored_to_the_current_folder() {
        let cases = [
            ("/", "", "/"),
            ("/", "nested", "/nested"),
            ("/Movies", "", "/Movies"),
            ("/Movies", "nested", "/Movies/nested"),
            ("/Movies", "nested/deeper", "/Movies/nested/deeper"),
            ("/Movies", "/Shared", "/Shared"),
        ];

        for (current_path, typed_parent, expected) in cases {
            assert_eq!(
                completion_parent_path(current_path, typed_parent),
                expected,
                "{current_path:?} + {typed_parent:?}"
            );
        }

        let parent = completion_parent_path("/Movies", "nested");
        assert_eq!(join_completed(&parent, "Sci-Fi"), "/Movies/nested/Sci-Fi/");
    }

    #[test]
    fn one_pending_completion_blocks_duplicate_tab_requests() {
        let mut input = PathInput::new();
        input.value = "mov".to_string();

        let first = 1;
        assert!(input.begin_completion_request(first));
        assert!(!input.begin_completion_request(2));
        assert_eq!(input.pending_request_id, Some(first));
    }

    #[test]
    fn editing_candidates_does_not_release_the_live_request() {
        let mut input = PathInput::new();
        input.value = "mov".to_string();

        let first = 1;
        assert!(input.begin_completion_request(first));
        input.value.push('i');
        input.clear_completion();

        assert_eq!(input.pending_request_id, Some(first));
        assert!(
            !input.begin_completion_request(2),
            "editing and pressing Tab again must not start a second live request"
        );
    }

    #[test]
    fn one_live_request_blocks_a_new_dialog_until_the_result_returns() {
        let mut old_dialog = PathInput::new();
        let mut new_dialog = PathInput::new();
        let mut in_flight = None;

        assert!(reserve_completion_request(
            &mut in_flight,
            &mut old_dialog,
            10
        ));
        assert!(!reserve_completion_request(
            &mut in_flight,
            &mut new_dialog,
            11
        ));
        in_flight = None;
        assert!(reserve_completion_request(
            &mut in_flight,
            &mut new_dialog,
            11
        ));

        assert!(!path_candidate_result_matches(
            &mut new_dialog,
            10,
            "",
            "folder-a",
            "folder-a"
        ));
        assert_eq!(new_dialog.pending_request_id, Some(11));
    }

    #[test]
    fn completion_result_requires_unchanged_value_and_folder_context() {
        let mut input = PathInput::new();
        input.value = "mov".to_string();
        let request_id = 20;
        assert!(input.begin_completion_request(request_id));

        assert!(!path_candidate_result_matches(
            &mut input, request_id, "mov", "folder-b", "folder-a"
        ));
        // The matching request completed even though its stale result was
        // rejected, so the user can press Tab again immediately.
        assert!(input.pending_request_id.is_none());

        let mut edited_input = PathInput::new();
        edited_input.value = "movies".to_string();
        let request_id = 21;
        assert!(edited_input.begin_completion_request(request_id));
        edited_input.value.push('/');
        assert!(!path_candidate_result_matches(
            &mut edited_input,
            request_id,
            "movies",
            "folder-a",
            "folder-a"
        ));
        assert!(edited_input.pending_request_id.is_none());
    }
}
