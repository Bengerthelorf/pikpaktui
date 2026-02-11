use crate::pikpak::EntryKind;

use super::App;

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
    pub(super) fn tab_complete(&self, input: &mut PathInput) {
        // If we have candidates and user presses Tab again, cycle through them
        if !input.candidates.is_empty() {
            let idx = match input.candidate_idx {
                Some(i) => (i + 1) % input.candidates.len(),
                None => 0,
            };
            input.candidate_idx = Some(idx);
            // Use stored completion_base instead of re-parsing the current value
            let parent = &input.completion_base;
            let selected = &input.candidates[idx];
            input.value = if parent.is_empty() {
                format!("{}/", selected)
            } else if parent == "/" {
                format!("/{}/", selected)
            } else {
                format!("{}/{}/", parent, selected)
            };
            return;
        }

        // Parse: split into parent path + prefix to complete
        let (parent_path, prefix) = split_path_prefix(&input.value);

        // Resolve parent folder
        let parent_id = if parent_path.is_empty() {
            // Relative: use current folder
            self.current_folder_id.clone()
        } else {
            match self.client.resolve_path(&parent_path) {
                Ok(id) => id,
                Err(_) => return,
            }
        };

        // List entries in parent
        let entries = match self.client.ls(&parent_id) {
            Ok(e) => e,
            Err(_) => return,
        };

        // Filter folders matching prefix
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

        // Store the parent path as completion base for subsequent Tab presses
        input.completion_base = parent_path.clone();

        if matches.len() == 1 {
            // Single match: autocomplete directly
            let name = &matches[0];
            input.value = if parent_path.is_empty() {
                format!("{}/", name)
            } else if parent_path == "/" {
                format!("/{}/", name)
            } else {
                format!("{}/{}/", parent_path, name)
            };
            input.candidates.clear();
            input.candidate_idx = None;
        } else {
            // Multiple: show candidates, apply first
            input.candidates = matches;
            input.candidate_idx = Some(0);
            let first = &input.candidates[0];
            input.value = if parent_path.is_empty() {
                format!("{}/", first)
            } else if parent_path == "/" {
                format!("/{}/", first)
            } else {
                format!("{}/{}/", parent_path, first)
            };
        }
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
    // If ends with '/', the prefix is empty, parent is the whole path (without trailing /)
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
