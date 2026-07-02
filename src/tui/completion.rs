use crate::pikpak::EntryKind;

use super::App;

#[derive(Default)]
pub(super) struct PathInput {
    pub value: String,
    pub candidates: Vec<String>,
    pub candidate_idx: Option<usize>,
    pub completion_base: String,
}

impl PathInput {
    pub fn new() -> Self {
        Self {
            value: String::new(),
            candidates: Vec::new(),
            candidate_idx: None,
            completion_base: String::new(),
        }
    }
}

impl App {
    /// Cycle already-known candidates synchronously; otherwise fetch them on
    /// a worker thread (resolve + ls block for a network round-trip) and
    /// apply through OpResult::PathCandidates when they arrive.
    pub(super) fn tab_complete(&self, input: &mut PathInput) {
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
        let value_snapshot = input.value.clone();
        let current_folder = self.current_folder_id.clone();
        let client = std::sync::Arc::clone(&self.client);
        let tx = self.result_tx.clone();

        std::thread::spawn(move || {
            let parent_id = if parent_path.is_empty() {
                // Relative: use current folder
                current_folder
            } else {
                match client.resolve_path(&parent_path) {
                    Ok(id) => id,
                    Err(_) => return,
                }
            };

            let entries = match client.ls(&parent_id) {
                Ok(e) => e,
                Err(_) => return,
            };

            let prefix_lower = prefix.to_lowercase();
            let matches: Vec<String> = entries
                .iter()
                .filter(|e| e.kind == EntryKind::Folder)
                .filter(|e| e.name.to_lowercase().starts_with(&prefix_lower))
                .map(|e| e.name.clone())
                .collect();

            if matches.is_empty() {
                return;
            }

            let _ = tx.send(super::OpResult::PathCandidates {
                value: value_snapshot,
                parent: parent_path,
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

fn join_completed(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        format!("{}/", name)
    } else if parent == "/" {
        format!("/{}/", name)
    } else {
        format!("{}/{}/", parent, name)
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
