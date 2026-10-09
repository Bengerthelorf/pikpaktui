use std::path::Path;
use std::sync::mpsc::{self, Receiver};

struct Candidates {
    value: String,
    base: String,
    matches: Vec<(String, bool)>,
}

pub(super) struct LocalPathInput {
    pub value: String,
    pub completion_open: bool,
    worker: Option<Receiver<Candidates>>,
    pub candidates: Vec<(String, bool)>, // (name, is_dir)
    pub candidate_idx: Option<usize>,
    pub completion_base: String,
    pub include_files: bool, // false = dirs only (download dest), true = files too (upload)
}

impl Default for LocalPathInput {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalPathInput {
    pub fn new() -> Self {
        let default = dirs::download_dir()
            .or_else(dirs::home_dir)
            .map(|p| {
                let mut s = p.to_string_lossy().into_owned();
                if !s.ends_with('/') {
                    s.push('/');
                }
                s
            })
            .unwrap_or_default();

        Self {
            value: default,
            completion_open: false,
            worker: None,
            candidates: Vec::new(),
            candidate_idx: None,
            completion_base: String::new(),
            include_files: false,
        }
    }

    pub fn new_for_upload() -> Self {
        let default = dirs::home_dir()
            .map(|p| {
                let mut s = p.to_string_lossy().into_owned();
                if !s.ends_with('/') {
                    s.push('/');
                }
                s
            })
            .unwrap_or_default();

        Self {
            value: default,
            completion_open: false,
            worker: None,
            candidates: Vec::new(),
            candidate_idx: None,
            completion_base: String::new(),
            include_files: true,
        }
    }

    /// One filesystem worker per dialog. Edits queue the latest value rather
    /// than starting another scan while a slow volume is still responding.
    pub fn open_candidates(&mut self) {
        self.completion_open = true;
        self.candidates.clear();
        self.candidate_idx = None;
        if self.worker.is_some() {
            return;
        }
        let value = self.value.clone();
        let include_files = self.include_files;
        let (tx, rx) = mpsc::channel();
        self.worker = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(scan_candidates(value, include_files));
        });
    }

    pub fn completion_loading(&self) -> bool {
        self.completion_open && self.worker.is_some()
    }

    pub fn poll_candidates(&mut self) {
        let Some(rx) = &self.worker else { return };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.worker = None;
                return;
            }
        };
        self.worker = None;
        if !self.completion_open {
            return;
        }
        if result.value != self.value {
            self.open_candidates();
            return;
        }
        self.completion_base = result.base;
        self.candidates = result.matches;
        self.candidate_idx = (!self.candidates.is_empty()).then_some(0);
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
        if let Some(idx) = self.candidate_idx
            && let Some((name, is_dir)) = self.candidates.get(idx)
        {
            let suffix = if *is_dir { "/" } else { "" };
            self.value = join_path(&self.completion_base, &format!("{}{}", name, suffix));
            self.clear_candidates();
            return true;
        }
        false
    }

    pub fn clear_candidates(&mut self) {
        self.completion_open = false;
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
            let parent_str = parent.to_string_lossy().into_owned();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
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
            score += 15;
        }
        let at_boundary = pos == 0 || matches!(nc[pos - 1], '-' | '_' | ' ' | '.' | '(' | '[');
        if at_boundary {
            score += 10;
        }
        prev_pos = Some(pos);
    }
    Some(score)
}

fn scan_candidates(value: String, include_files: bool) -> Candidates {
    let (dir_part, prefix) = split_local_path(&value);
    let dir_path = if dir_part.is_empty() { "." } else { &dir_part };
    let Ok(read_dir) = std::fs::read_dir(dir_path) else {
        return Candidates {
            value,
            base: dir_part,
            matches: Vec::new(),
        };
    };

    let prefix_lower = prefix.to_lowercase();
    let mut matches: Vec<(String, bool, i32)> = Vec::new();

    for entry in read_dir.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        let is_dir = ft.is_dir();
        if !is_dir && !include_files {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') && !prefix.starts_with('.') {
            continue;
        }
        if let Some(score) = fuzzy_score_lower(&name.to_lowercase(), &prefix_lower) {
            matches.push((name, is_dir, score));
        }
    }

    matches.sort_by(|a, b| {
        b.2.cmp(&a.2) // higher score = better
            .then_with(|| b.1.cmp(&a.1)) // dirs before files within same score
            .then_with(|| a.0.cmp(&b.0)) // alphabetical tiebreak
    });

    Candidates {
        value,
        base: dir_part,
        matches: matches.into_iter().map(|(n, d, _)| (n, d)).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn late_scan_never_restores_old_text_or_cancelled_candidates() {
        let mut input = LocalPathInput::new_for_upload();
        let (tx, rx) = mpsc::channel();
        input.worker = Some(rx);
        input.completion_open = true;
        input.value = "new/path".into();
        tx.send(Candidates {
            value: "old/path".into(),
            base: "old".into(),
            matches: vec![("file".into(), false)],
        })
        .unwrap();
        input.poll_candidates();
        assert_eq!(input.value, "new/path");
        assert!(input.candidates.is_empty());
        input.clear_candidates();
        let (tx, rx) = mpsc::channel();
        input.worker = Some(rx);
        tx.send(Candidates {
            value: input.value.clone(),
            base: "new".into(),
            matches: vec![("file".into(), false)],
        })
        .unwrap();
        input.poll_candidates();
        assert!(input.candidates.is_empty());
        assert!(!input.completion_open);
    }
    #[test]
    fn asynchronous_completion_returns_real_files_and_directories() {
        let dir = std::env::temp_dir().join(format!("pikpaktui-completion-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("folder")).unwrap();
        std::fs::write(dir.join("file.txt"), b"test").unwrap();
        let mut input = LocalPathInput::new_for_upload();
        input.value = format!("{}/", dir.display());
        input.open_candidates();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while input.completion_loading() && std::time::Instant::now() < end {
            input.poll_candidates();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(
            input.candidates,
            vec![("folder".into(), true), ("file.txt".into(), false)]
        );
        assert!(input.confirm_selected());
        assert!(input.value.ends_with("/folder/"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
