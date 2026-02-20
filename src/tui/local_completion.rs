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
        // (name, is_dir, score) — score used for sorting only
        let mut matches: Vec<(String, bool, i32)> = Vec::new();

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
            b.2.cmp(&a.2)                    // higher score = better
                .then_with(|| b.1.cmp(&a.1)) // dirs before files within same score
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

/// Fuzzy match score (both inputs pre-lowercased). Returns None if no subsequence match.
/// Higher score = better quality. Inspired by fzf's scoring model:
///   +15  consecutive matched characters
///   +10  match at word boundary (start, or after - _ . space)
///   -pos distance penalty (how far into the string the match starts)
fn fuzzy_score_lower(name: &str, pattern: &str) -> Option<i32> {
    if pattern.is_empty() {
        return Some(0);
    }
    let nc: Vec<char> = name.chars().collect();
    let pc: Vec<char> = pattern.chars().collect();
    if pc.len() > nc.len() {
        return None;
    }

    // Greedy forward pass to collect match positions
    let mut positions: Vec<usize> = Vec::with_capacity(pc.len());
    let mut pi = 0;
    for (i, &c) in nc.iter().enumerate() {
        if c == pc[pi] {
            positions.push(i);
            pi += 1;
            if pi == pc.len() {
                break;
            }
        }
    }
    if pi < pc.len() {
        return None;
    }

    let mut score = 0i32;
    let mut prev_pos: Option<usize> = None;
    for &pos in &positions {
        score -= pos as i32;
        if prev_pos == Some(pos.wrapping_sub(1)) {
            score += 15; // consecutive run
        }
        let at_boundary = pos == 0
            || matches!(nc[pos - 1], '-' | '_' | ' ' | '.' | '(' | '[');
        if at_boundary {
            score += 10; // word boundary
        }
        prev_pos = Some(pos);
    }
    Some(score)
}
