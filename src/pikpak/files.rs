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
    pub fn resolve_path_nav(&self, path: &str) -> Result<(String, Vec<(String, String)>)> {
        use anyhow::anyhow;
        let components: Vec<&str> = path
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        let mut current_id = String::new(); // root
        let mut breadcrumb: Vec<(String, String)> = Vec::new();

        for name in components {
            let entries = self.ls_cached(&current_id)?;
            let child = entries
                .into_iter()
                .find(|e| e.name == name && e.kind == crate::pikpak::EntryKind::Folder)
                .ok_or_else(|| anyhow!("folder not found: {name}"))?;
            breadcrumb.push((current_id, name.to_string()));
            current_id = child.id;
        }

        Ok((current_id, breadcrumb))
    }

    pub fn ls_trash(&self, limit: u32) -> Result<Vec<Entry>> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files");

        let filters = r#"{"trashed":{"eq":true}}"#;
        let mut rb = self.http.get(&url).bearer_auth(&token).query(&[
            ("parent_id", "*"),
            ("limit", &limit.to_string()),
            ("filters", filters),
            ("thumbnail_size", "SIZE_MEDIUM"),
        ]);
        rb = self.authed_headers(rb);

        let response = rb.send().context("ls_trash request failed")?;
        let payload: DriveListResponse = json_or_api_error(response, "ls_trash")?;
        let entries = payload.files.into_iter().map(|f| f.into_entry()).collect();
        Ok(entries)
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

        let mut rb = self.http.get(&url).bearer_auth(&token);
        rb = self.authed_headers(rb);

        let response = rb.send().context("file_info request failed")?;
        json_or_api_error(response, "file_info")
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
            ("thumbnail_size", "SIZE_MEDIUM"),
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

    pub fn resolve_path(&self, path: &str) -> Result<String> {
        let path = path.trim();
        if path.is_empty() || path == "/" {
            return Ok(String::new());
        }

        let segments: Vec<&str> = path
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        let mut current_id = String::new();
        for seg in &segments {
            let entries = self.ls_cached(&current_id)?;
            let found = entries
                .into_iter()
                .find(|e| e.name == *seg)
                .ok_or_else(|| anyhow!("not found: '{}' in path '{}'", seg, path))?;
            current_id = found.id;
        }

        Ok(current_id)
    }
}
