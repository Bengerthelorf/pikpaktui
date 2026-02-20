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

    pub fn tab_complete(&mut self) {
        // Cycle through existing candidates
        if !self.candidates.is_empty() {
            let idx = match self.candidate_idx {
                Some(i) => (i + 1) % self.candidates.len(),
                None => 0,
            };
            self.candidate_idx = Some(idx);
            let base = self.completion_base.clone();
            let (name, is_dir) = &self.candidates[idx];
            let suffix = if *is_dir { "/" } else { "" };
            self.value = join_path(&base, &format!("{}{}", name, suffix));
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
        let mut matches: Vec<(String, bool)> = Vec::new();

        for entry in read_dir.flatten() {
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            let is_dir = ft.is_dir();
            if !is_dir && !self.include_files {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') && !prefix.starts_with('.') {
                continue; // skip hidden unless user typed a dot
            }
            if name.to_lowercase().starts_with(&prefix_lower) {
                matches.push((name, is_dir));
            }
        }

        matches.sort_by(|a, b| {
            // Dirs first, then alphabetical
            b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0))
        });

        if matches.is_empty() {
            return;
        }

        self.completion_base = dir_part.clone();

        if matches.len() == 1 {
            let (name, is_dir) = &matches[0];
            let suffix = if *is_dir { "/" } else { "" };
            self.value = join_path(&dir_part, &format!("{}{}", name, suffix));
            self.candidates.clear();
            self.candidate_idx = None;
        } else {
            self.candidate_idx = Some(0);
            let (first_name, first_is_dir) = &matches[0];
            let suffix = if *first_is_dir { "/" } else { "" };
            self.value = join_path(&dir_part, &format!("{}{}", first_name, suffix));
            self.candidates = matches;
        }
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
