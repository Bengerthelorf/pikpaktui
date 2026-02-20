use std::path::Path;

pub(super) struct LocalPathInput {
    pub value: String,
    pub candidates: Vec<(String, bool)>, // (name, is_dir)
    pub candidate_idx: Option<usize>,
    pub completion_base: String,
    pub include_files: bool, // false = dirs only (download dest), true = files too (upload)
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

    /// Populate candidates from the current value (does not modify value).
    pub fn open_candidates(&mut self) {
        let (dir_part, prefix) = split_local_path(&self.value);
        let dir_path = if dir_part.is_empty() { "." } else { &dir_part };
        let Ok(read_dir) = std::fs::read_dir(dir_path) else {
            self.candidates.clear();
            self.candidate_idx = None;
            return;
        };

        let prefix_lower = prefix.to_lowercase();
        // (name, is_dir, match_start_pos) — score used for sorting only
        let mut matches: Vec<(String, bool, usize)> = Vec::new();

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
            if let Some(score) = fuzzy_score_lower(&name.to_lowercase(), &prefix_lower) {
                matches.push((name, is_dir, score));
            }
        }

        matches.sort_by(|a, b| {
            b.1.cmp(&a.1)                    // dirs first
                .then_with(|| a.2.cmp(&b.2)) // earlier match position = better
                .then_with(|| a.0.cmp(&b.0)) // alphabetical tiebreak
        });

        self.completion_base = dir_part;
        self.candidates = matches.into_iter().map(|(n, d, _)| (n, d)).collect();
        self.candidate_idx = if self.candidates.is_empty() { None } else { Some(0) };
    }

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

    /// Write the selected candidate into value. Returns true if applied.
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

    pub fn clear_candidates(&mut self) {
        self.candidates.clear();
        self.candidate_idx = None;
        self.completion_base.clear();
    }
}

fn join_path(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else if base.ends_with('/') {
        format!("{}{}", base, name)
    } else {
        format!("{}/{}", base, name)
    }
}

/// Split into (directory, prefix).
/// "/Users/foo/Down" -> ("/Users/foo", "Down")
/// "/Users/foo/"     -> ("/Users/foo/", "")
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

/// Case-insensitive subsequence match. Returns the index of the first matched character,
/// or None if no match. Lower index = better quality (e.g. prefix match returns 0).
fn fuzzy_score_lower(name: &str, pattern: &str) -> Option<usize> {
    if pattern.is_empty() {
        return Some(0);
    }
    let mut pchars = pattern.chars();
    let mut pc = pchars.next().unwrap();
    let mut first_pos = None;
    for (i, nc) in name.chars().enumerate() {
        if nc == pc {
            if first_pos.is_none() {
                first_pos = Some(i);
            }
            match pchars.next() {
                Some(next) => pc = next,
                None => return first_pos,
            }
        }
    }
    None
}
