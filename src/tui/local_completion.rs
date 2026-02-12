use std::path::Path;

pub(super) struct LocalPathInput {
    pub value: String,
    pub candidates: Vec<String>,
    pub candidate_idx: Option<usize>,
    pub completion_base: String,
}

impl LocalPathInput {
    pub fn new() -> Self {
        let default = dirs::download_dir()
            .or_else(dirs::home_dir)
            .map(|p| {
                let mut s = p.to_string_lossy().to_string();
                if !s.ends_with('/') {
                    s.push('/');
                }
                s
            })
            .unwrap_or_default();

        Self {
            value: default,
            candidates: Vec::new(),
            candidate_idx: None,
            completion_base: String::new(),
        }
    }

    pub fn tab_complete(&mut self) {
        // Cycle through existing candidates
        if !self.candidates.is_empty() {
            let idx = match self.candidate_idx {
                Some(i) => (i + 1) % self.candidates.len(),
                None => 0,
            };
            self.candidate_idx = Some(idx);
            let base = &self.completion_base;
            let selected = &self.candidates[idx];
            self.value = if base.is_empty() {
                format!("{}/", selected)
            } else if base.ends_with('/') {
                format!("{}{}/", base, selected)
            } else {
                format!("{}/{}/", base, selected)
            };
            return;
        }

        // Parse input into directory part + prefix
        let (dir_part, prefix) = split_local_path(&self.value);

        // List directory entries
        let dir_path = if dir_part.is_empty() { "." } else { &dir_part };
        let Ok(read_dir) = std::fs::read_dir(dir_path) else {
            return;
        };

        let prefix_lower = prefix.to_lowercase();
        let mut matches: Vec<String> = Vec::new();

        for entry in read_dir.flatten() {
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if !ft.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') && !prefix.starts_with('.') {
                continue; // skip hidden unless user typed a dot
            }
            if name.to_lowercase().starts_with(&prefix_lower) {
                matches.push(name);
            }
        }

        matches.sort();

        if matches.is_empty() {
            return;
        }

        self.completion_base = dir_part.clone();

        if matches.len() == 1 {
            let name = &matches[0];
            self.value = if dir_part.is_empty() {
                format!("{}/", name)
            } else if dir_part.ends_with('/') {
                format!("{}{}/", dir_part, name)
            } else {
                format!("{}/{}/", dir_part, name)
            };
            self.candidates.clear();
            self.candidate_idx = None;
        } else {
            self.candidates = matches;
            self.candidate_idx = Some(0);
            let first = &self.candidates[0];
            self.value = if dir_part.is_empty() {
                format!("{}/", first)
            } else if dir_part.ends_with('/') {
                format!("{}{}/", dir_part, first)
            } else {
                format!("{}/{}/", dir_part, first)
            };
        }
    }
}

/// Split a local path into (directory, prefix).
/// "/Users/foo/Down" -> ("/Users/foo", "Down")
/// "/Users/foo/"     -> ("/Users/foo/", "")
/// "sub"             -> ("", "sub")
fn split_local_path(input: &str) -> (String, String) {
    if input.is_empty() {
        return (String::new(), String::new());
    }
    if input.ends_with('/') {
        return (input.to_string(), String::new());
    }
    let path = Path::new(input);
    match path.parent() {
        Some(parent) => {
            let parent_str = parent.to_string_lossy().to_string();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            (parent_str, name)
        }
        None => (String::new(), input.to_string()),
    }
}
