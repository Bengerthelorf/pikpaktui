use std::path::Path;

pub(super) struct LocalPathInput {
    pub value: String,
    /// (name, is_dir)
    pub candidates: Vec<(String, bool)>,
    pub candidate_idx: Option<usize>,
    pub completion_base: String,
    /// When true, files are included in tab completion (for upload).
    /// When false, only directories are completed (for download destination).
    pub include_files: bool,
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
            include_files: false,
        }
    }

    pub fn new_for_upload() -> Self {
        let default = dirs::home_dir()
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
            include_files: true,
        }
    }

    /// Compute candidates from the current value without changing it.
    /// Resets selection to the first candidate.
    pub fn open_candidates(&mut self) {
        let (dir_part, prefix) = split_local_path(&self.value);
        let dir_path = if dir_part.is_empty() { "." } else { &dir_part };
        let Ok(read_dir) = std::fs::read_dir(dir_path) else {
            self.candidates.clear();
            self.candidate_idx = None;
            return;
        };

        let prefix_lower = prefix.to_lowercase();
        let mut matches: Vec<(String, bool)> = Vec::new();

        for entry in read_dir.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            let is_dir = ft.is_dir();
            if !is_dir && !self.include_files {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') && !prefix.starts_with('.') {
                continue;
            }
            if fuzzy_match_lower(&name.to_lowercase(), &prefix_lower) {
                matches.push((name, is_dir));
            }
        }

        matches.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        self.completion_base = dir_part;
        self.candidates = matches;
        self.candidate_idx = if self.candidates.is_empty() { None } else { Some(0) };
    }

    /// Move selection forward (Tab / Down). Wraps around.
    pub fn navigate_next(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        let n = self.candidates.len();
        self.candidate_idx = Some(match self.candidate_idx {
            Some(i) => (i + 1) % n,
            None => 0,
        });
    }

    /// Move selection backward (Shift+Tab / Up). Wraps around.
    pub fn navigate_prev(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        let n = self.candidates.len();
        self.candidate_idx = Some(match self.candidate_idx {
            Some(0) | None => n - 1,
            Some(i) => i - 1,
        });
    }

    /// Apply the selected candidate to `value`. Returns true if a candidate was applied.
    pub fn confirm_selected(&mut self) -> bool {
        if let Some(idx) = self.candidate_idx {
            if let Some((name, is_dir)) = self.candidates.get(idx) {
                let suffix = if *is_dir { "/" } else { "" };
                self.value = join_path(&self.completion_base, &format!("{}{}", name, suffix));
                self.candidates.clear();
                self.candidate_idx = None;
                self.completion_base.clear();
                return true;
            }
        }
        false
    }

    /// Clear the candidate list without changing value.
    pub fn clear_candidates(&mut self) {
        self.candidates.clear();
        self.candidate_idx = None;
        self.completion_base.clear();
    }
}

/// Join a base directory path with a name.
fn join_path(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else if base.ends_with('/') {
        format!("{}{}", base, name)
    } else {
        format!("{}/{}", base, name)
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

/// Case-insensitive subsequence fuzzy match.
/// Returns true if every character of `pattern` appears in `name` in order.
/// E.g., "dwn" matches "Downloads".
fn fuzzy_match_lower(name: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }
    let mut pchars = pattern.chars();
    let mut pc = match pchars.next() {
        Some(c) => c,
        None => return true,
    };
    for nc in name.chars() {
        if nc == pc {
            match pchars.next() {
                Some(next) => pc = next,
                None => return true,
            }
        }
    }
    false
}
