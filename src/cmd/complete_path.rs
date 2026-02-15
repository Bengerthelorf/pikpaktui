use anyhow::Result;

/// Internal command used by shell completion scripts.
///
/// Given a partial cloud path prefix, lists the entries in the parent directory
/// as plain text (one per line). Folders get a trailing `/`.
///
/// Usage: `pikpaktui __complete_path [partial_path]`
///
/// Examples:
///   __complete_path           → list root
///   __complete_path /         → list root
///   __complete_path /Movies   → list root (prefix "Movies" filtered by shell)
///   __complete_path /Movies/  → list /Movies
pub fn run(args: &[String]) -> Result<()> {
    let prefix = args.first().map(|s| s.as_str()).unwrap_or("/");
    let (dir, _partial) = split_for_completion(prefix);

    // Silently succeed with no output if not logged in or on error
    let client = match super::cli_client() {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    let parent_id = match client.resolve_path(&dir) {
        Ok(id) => id,
        Err(_) => return Ok(()),
    };

    let entries = match client.ls(&parent_id) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in &entries {
        let suffix = if entry.kind == crate::pikpak::EntryKind::Folder {
            "/"
        } else {
            ""
        };
        println!("{}{}", entry.name, suffix);
    }

    Ok(())
}

/// Split a partial path into (directory_to_list, partial_name_prefix).
///
/// The directory portion is what we need to `ls`; the partial prefix is what
/// the shell uses to filter candidates.
///
/// Rules:
/// - Empty / "/" → ("/", "")
/// - "/foo" → ("/", "foo")            — list root, shell filters by "foo"
/// - "/foo/" → ("/foo", "")           — list /foo
/// - "/foo/bar" → ("/foo", "bar")     — list /foo, shell filters by "bar"
/// - "/a/b/c" → ("/a/b", "c")
fn split_for_completion(prefix: &str) -> (String, String) {
    let prefix = if prefix.is_empty() { "/" } else { prefix };

    if prefix == "/" {
        return ("/".to_string(), String::new());
    }

    // If it ends with '/', the whole thing is the directory
    if prefix.ends_with('/') {
        let dir = prefix.trim_end_matches('/');
        let dir = if dir.is_empty() { "/" } else { dir };
        return (dir.to_string(), String::new());
    }

    // Split at last '/'
    match prefix.rsplit_once('/') {
        Some(("", name)) => ("/".to_string(), name.to_string()),
        Some((dir, name)) => (dir.to_string(), name.to_string()),
        None => ("/".to_string(), prefix.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::split_for_completion;

    #[test]
    fn empty_input_lists_root() {
        assert_eq!(split_for_completion(""), ("/".into(), "".into()));
    }

    #[test]
    fn root_slash() {
        assert_eq!(split_for_completion("/"), ("/".into(), "".into()));
    }

    #[test]
    fn single_name_at_root() {
        assert_eq!(split_for_completion("/Movies"), ("/".into(), "Movies".into()));
    }

    #[test]
    fn trailing_slash_lists_directory() {
        assert_eq!(
            split_for_completion("/Movies/"),
            ("/Movies".into(), "".into())
        );
    }

    #[test]
    fn nested_partial() {
        assert_eq!(
            split_for_completion("/Movies/act"),
            ("/Movies".into(), "act".into())
        );
    }

    #[test]
    fn deep_nested() {
        assert_eq!(
            split_for_completion("/a/b/c"),
            ("/a/b".into(), "c".into())
        );
    }

    #[test]
    fn deep_nested_trailing_slash() {
        assert_eq!(
            split_for_completion("/a/b/c/"),
            ("/a/b/c".into(), "".into())
        );
    }

    #[test]
    fn bare_name_no_leading_slash() {
        assert_eq!(
            split_for_completion("Movies"),
            ("/".into(), "Movies".into())
        );
    }
}
