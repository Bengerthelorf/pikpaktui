use anyhow::{Context, Result, anyhow};

use super::drive::{DriveFileResponse, DriveListResponse};
use super::{Entry, FileInfoResponse, PikPak, ensure_success, json_or_api_error};

impl PikPak {
    pub fn ls(&self, parent_id: &str) -> Result<Vec<Entry>> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files");

        let filters = r#"{"trashed":{"eq":false}}"#;
        let mut all_entries: Vec<Entry> = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut rb = self.http.get(&url).bearer_auth(&token).query(&[
                ("parent_id", parent_id),
                ("limit", "500"),
                ("filters", filters),
                ("thumbnail_size", self.thumbnail_size.as_str()),
            ]);
            if let Some(ref pt) = page_token {
                rb = rb.query(&[("page_token", pt.as_str())]);
            }
            rb = self.authed_headers(rb);

            let response = rb.send().context("ls request failed")?;
            let payload: DriveListResponse = json_or_api_error(response, "ls")?;
            let next = payload.next_page_token.filter(|t| !t.is_empty());

            all_entries.extend(payload.files.into_iter().map(|f| f.into_entry()));

            match next {
                // A server echoing the same token forever would hang the
                // client and grow the list without bound.
                Some(t) if page_token.as_deref() == Some(t.as_str()) => {
                    return Err(anyhow!("ls pagination stuck: server repeated page token"));
                }
                Some(t) => page_token = Some(t),
                None => break,
            }
        }

        Ok(all_entries)
    }

    /// Like `ls()` but caches results by parent_id for the lifetime of this client.
    /// Used by path-resolution helpers so repeated segments (e.g. the same parent
    /// folder appearing in every argument of a batch command) only hit the API once.
    /// TUI code that needs a fresh listing should call `ls()` directly.
    pub fn ls_cached(&self, parent_id: &str) -> Result<Vec<Entry>> {
        if let Some(cached) = self
            .ls_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(parent_id)
        {
            return Ok(cached.clone());
        }
        let entries = self.ls(parent_id)?;
        let result = entries.clone();
        self.ls_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(parent_id.to_string(), entries);
        Ok(result)
    }

    /// Resolve a cloud path like `/My Files/Movies` to a folder ID and breadcrumb.
    ///
    /// Returns `(final_folder_id, breadcrumb)` where breadcrumb is a vec of
    /// `(parent_id, folder_name)` pairs — the same format used by the TUI App.
    ///
    /// Lists fresh (uncached): this backs the interactive `:goto` jump, where
    /// the long-lived TUI client must reflect the current tree even after an
    /// external change on another device.
    pub fn resolve_path_nav(&self, path: &str) -> Result<(String, Vec<(String, String)>)> {
        let mut current_id = String::new(); // root
        let mut breadcrumb: Vec<(String, String)> = Vec::new();

        for name in path_components(path) {
            let entries = self.ls(&current_id)?;
            let child = pick_unique(entries, name, true, path)?;
            breadcrumb.push((current_id, name.to_string()));
            current_id = child.id;
        }

        Ok((current_id, breadcrumb))
    }

    /// List trash contents, following pagination until `limit` entries are
    /// collected or the listing is exhausted (a single page silently dropped
    /// everything past the first 500 items). Pass `u32::MAX` for all of it.
    pub fn ls_trash(&self, limit: u32) -> Result<Vec<Entry>> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files");

        let filters = r#"{"trashed":{"eq":true}}"#;
        let mut all_entries: Vec<Entry> = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let page_size = (limit as u64 - all_entries.len() as u64)
                .min(500)
                .to_string();
            let mut rb = self.http.get(&url).bearer_auth(&token).query(&[
                ("parent_id", "*"),
                ("limit", page_size.as_str()),
                ("filters", filters),
                ("thumbnail_size", self.thumbnail_size.as_str()),
            ]);
            if let Some(ref pt) = page_token {
                rb = rb.query(&[("page_token", pt.as_str())]);
            }
            rb = self.authed_headers(rb);

            let response = rb.send().context("ls_trash request failed")?;
            let payload: DriveListResponse = json_or_api_error(response, "ls_trash")?;
            let next = payload.next_page_token.filter(|t| !t.is_empty());

            all_entries.extend(payload.files.into_iter().map(|f| f.into_entry()));
            if all_entries.len() as u64 >= limit as u64 {
                all_entries.truncate(limit as usize);
                break;
            }

            match next {
                Some(t) if page_token.as_deref() == Some(t.as_str()) => {
                    return Err(anyhow!(
                        "ls_trash pagination stuck: server repeated page token"
                    ));
                }
                Some(t) => page_token = Some(t),
                None => break,
            }
        }

        Ok(all_entries)
    }

    pub fn mv(&self, ids: &[&str], to_parent_id: &str) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files:batchMove");

        let payload = serde_json::json!({
            "ids": ids,
            "to": { "parent_id": to_parent_id },
        });

        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("move request failed")?;
        ensure_success(response, "move")?;
        self.clear_ls_cache();
        Ok(())
    }

    pub fn cp(&self, ids: &[&str], to_parent_id: &str) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files:batchCopy");

        let payload = serde_json::json!({
            "ids": ids,
            "to": { "parent_id": to_parent_id },
        });

        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("copy request failed")?;
        ensure_success(response, "copy")?;
        self.clear_ls_cache();
        Ok(())
    }

    pub fn rename(&self, file_id: &str, new_name: &str) -> Result<()> {
        let token = self.access_token()?;
        let url = format!("{}/{}", self.drive_url("drive/v1/files"), file_id);

        let payload = serde_json::json!({ "name": new_name });
        let mut rb = self.http.patch(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("rename request failed")?;
        ensure_success(response, "rename")?;
        self.clear_ls_cache();
        Ok(())
    }

    pub fn remove(&self, ids: &[&str]) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files:batchTrash");

        let payload = serde_json::json!({ "ids": ids });
        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("remove request failed")?;
        ensure_success(response, "remove")?;
        self.clear_ls_cache();
        Ok(())
    }

    pub fn delete_permanent(&self, ids: &[&str]) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files:batchDelete");

        let payload = serde_json::json!({ "ids": ids });
        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("permanent delete request failed")?;
        ensure_success(response, "permanent delete")?;
        self.clear_ls_cache();
        Ok(())
    }

    /// Server-side "empty trash": one call clears everything, with none of
    /// the list/delete paging races of draining it page by page.
    pub fn empty_trash(&self) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files/trash:empty");

        let mut rb = self.http.patch(&url).bearer_auth(&token);
        rb = self.authed_headers(rb);

        let response = rb.send().context("empty trash request failed")?;
        ensure_success(response, "empty trash")?;
        self.clear_ls_cache();
        Ok(())
    }

    pub fn untrash(&self, ids: &[&str]) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files:batchUntrash");

        let payload = serde_json::json!({ "ids": ids });
        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("untrash request failed")?;
        ensure_success(response, "untrash")?;
        self.clear_ls_cache();
        Ok(())
    }

    pub fn mkdir(&self, parent_id: &str, name: &str) -> Result<Entry> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files");

        let payload = serde_json::json!({
            "kind": "drive#folder",
            "parent_id": parent_id,
            "name": name,
        });

        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("mkdir request failed")?;
        let resp: DriveFileResponse = json_or_api_error(response, "mkdir")?;
        self.clear_ls_cache();
        Ok(resp.file.into_folder_entry())
    }

    pub fn file_info(&self, file_id: &str) -> Result<FileInfoResponse> {
        let token = self.access_token()?;
        let url = format!("{}/{}", self.drive_url("drive/v1/files"), file_id);

        // Download-link fetches are the endpoint PikPak rate-guards with
        // error_code 9 (riskLimited / captcha token expired). Reference
        // clients refresh the captcha token for this exact action and retry
        // once; do the same instead of surfacing an opaque 403.
        for attempt in 0..2 {
            let mut rb = self.http.get(&url).bearer_auth(&token);
            rb = self.authed_headers(rb);

            let response = rb.send().context("file_info request failed")?;
            let status = response.status();
            if status.is_success() {
                return response.json().context("invalid file_info json");
            }
            let body = response.text().unwrap_or_default();
            if attempt == 0
                && super::api_error_code(&body) == Some(9)
                && self
                    .refresh_captcha_for_action(&format!("GET:/drive/v1/files/{file_id}"))
                    .is_ok()
            {
                continue;
            }
            return Err(anyhow!(
                "file_info failed ({}): {}",
                status,
                super::sanitize(&body)
            ));
        }
        unreachable!("both file_info attempts returned early");
    }

    pub fn star(&self, ids: &[&str]) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files:star");

        let payload = serde_json::json!({ "ids": ids });
        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("star request failed")?;
        ensure_success(response, "star")?;
        self.clear_ls_cache();
        Ok(())
    }

    pub fn unstar(&self, ids: &[&str]) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files:unstar");

        let payload = serde_json::json!({ "ids": ids });
        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("unstar request failed")?;
        ensure_success(response, "unstar")?;
        self.clear_ls_cache();
        Ok(())
    }

    pub fn starred_list(&self, limit: u32) -> Result<Vec<Entry>> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files");

        let filters = r#"{"trashed":{"eq":false},"system_tag":{"in":"STAR"}}"#;
        let mut rb = self.http.get(&url).bearer_auth(&token).query(&[
            ("parent_id", "*"),
            ("limit", &limit.to_string()),
            ("filters", filters),
            ("thumbnail_size", self.thumbnail_size.as_str()),
        ]);
        rb = self.authed_headers(rb);

        let response = rb.send().context("starred list request failed")?;
        let payload: DriveListResponse = json_or_api_error(response, "starred list")?;
        let entries = payload
            .files
            .into_iter()
            .map(|f| Entry {
                starred: true,
                ..f.into_entry()
            })
            .collect();
        Ok(entries)
    }

    /// Resolve a cloud path to its file/folder ID.
    ///
    /// Uses the lifetime cache (`ls_cached`) so a batch command resolving many
    /// paths under a shared parent only lists that parent once. Any staleness is
    /// bounded: local mutations clear the cache, and a stale hit resolves to a
    /// PikPak ID, which is stable across renames — so it targets the same
    /// object or fails cleanly, never silently the wrong one. `:goto` uses the
    /// uncached `resolve_path_nav` when fresh navigation is what matters.
    pub fn resolve_path(&self, path: &str) -> Result<String> {
        let path = path.trim();
        if path.is_empty() || path == "/" {
            return Ok(String::new());
        }

        let comps = path_components(path);
        let mut current_id = String::new();
        for (i, seg) in comps.iter().enumerate() {
            let entries = self.ls_cached(&current_id)?;
            // Intermediate segments must be folders: a file id passed to a
            // later ls() yields a bogus "not found" (or worse) instead of a
            // clear error here.
            let folders_only = i + 1 != comps.len();
            current_id = pick_unique(entries, seg, folders_only, path)?.id;
        }

        Ok(current_id)
    }

    /// Like `resolve_path`, but every segment — including the last — must be
    /// a folder. Use for destinations: batchMove/batchCopy take the id as
    /// `to.parent_id`, and handing them a file id has server-defined results.
    pub fn resolve_folder(&self, path: &str) -> Result<String> {
        let path = path.trim();
        if path.is_empty() || path == "/" {
            return Ok(String::new());
        }

        let mut current_id = String::new();
        for seg in path_components(path) {
            let entries = self.ls_cached(&current_id)?;
            current_id = pick_unique(entries, seg, true, path)?.id;
        }

        Ok(current_id)
    }
}

/// Select the single entry named `name`, erroring on zero or several matches.
/// PikPak folders can hold duplicate names, and first-match resolution would
/// silently operate on whichever one the API lists first.
fn pick_unique(
    entries: Vec<Entry>,
    name: &str,
    folders_only: bool,
    full_path: &str,
) -> Result<Entry> {
    let mut matches: Vec<Entry> = entries
        .into_iter()
        .filter(|e| e.name == name && (!folders_only || e.kind == crate::pikpak::EntryKind::Folder))
        .collect();
    match matches.len() {
        0 => Err(anyhow!(
            "{} not found: '{}' in path '{}'",
            if folders_only { "folder" } else { "entry" },
            name,
            full_path
        )),
        1 => Ok(matches.remove(0)),
        n => {
            let ids: Vec<String> = matches.iter().map(|e| format!("  id: {}", e.id)).collect();
            Err(anyhow!(
                "'{}' in path '{}' is ambiguous: {} entries share this name:\n{}\nrename one first (or operate on it in the TUI)",
                name,
                full_path,
                n,
                ids.join("\n")
            ))
        }
    }
}

/// Split a cloud path into its non-empty `/`-separated components.
fn path_components(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
}
