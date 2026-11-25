use anyhow::{Context, Result, anyhow};
use std::collections::HashSet;
use std::time::{Duration, Instant};

use super::mutation::{MutationProgress, batch_mutation_error, pending};

use super::{
    CreateShareResponse, MyShare, PikPak, ShareEntry, ShareInfoResponse, ShareListResponse,
    json_or_api_error,
};

fn valid_restore_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[derive(Debug)]
pub struct ShareLocation {
    pub id: String,
    pub folders: Vec<String>,
    pub pass_code: Option<String>,
}

impl ShareLocation {
    pub fn parse(share_url: &str) -> Result<Self> {
        let invalid_share_error = || {
            anyhow!(
                "invalid share address; use a plain ID, the root /s/<share_id> URL or /s/<share_id>/<folder_id>. Use --parent-id for a folder and --file-id for a selection"
            )
        };
        // Validate the raw route first. URL parsing can normalize controls, backslashes and dot segments.
        if share_url.chars().any(char::is_control) || share_url.contains('\\') {
            return Err(invalid_share_error());
        }
        let input = share_url.trim();
        if valid_restore_id(input) {
            return Ok(Self {
                id: input.into(),
                folders: Vec::new(),
                pass_code: None,
            });
        }
        let (scheme, route) = input.split_once("://").ok_or_else(invalid_share_error)?;
        if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("http") {
            return Err(invalid_share_error());
        }
        let route = route.split(['?', '#']).next().unwrap_or("");
        let (authority, path) = route.split_once('/').ok_or_else(invalid_share_error)?;
        if authority.is_empty() {
            return Err(invalid_share_error());
        }
        let path = path.strip_suffix('/').unwrap_or(path);
        let mut segments = path
            .strip_prefix("s/")
            .ok_or_else(invalid_share_error)?
            .split('/');
        let id = segments
            .next()
            .filter(|s| valid_restore_id(s))
            .ok_or_else(invalid_share_error)?;
        let folders = segments
            .map(|segment| decode_folder_segment(id, segment).ok_or_else(invalid_share_error))
            .collect::<Result<Vec<_>>>()?;
        let url = reqwest::Url::parse(input).map_err(|_| invalid_share_error())?;
        let pass_code = url
            .query_pairs()
            .find(|(key, _)| key == "pass_code")
            .map(|(_, value)| value.into_owned());
        Ok(Self {
            id: id.into(),
            folders,
            pass_code,
        })
    }

    /// Resolve each URL component against its actual parent before opening or saving.
    pub fn breadcrumbs(&self, client: &PikPak, token: &str) -> Result<Vec<(String, String)>> {
        let mut path = Vec::new();
        let mut parent = "";
        for id in &self.folders {
            let listing = client.share_detail(&self.id, parent, token)?;
            let mut found = listing.into_iter().filter(|item| item.id == *id);
            let folder = found
                .next()
                .filter(ShareEntry::is_folder)
                .ok_or_else(|| anyhow!("folder from the share URL is missing or inaccessible"))?;
            anyhow::ensure!(found.next().is_none(), "ambiguous folder ID in share path");
            path.push((folder.id, folder.name));
            parent = id;
        }
        Ok(path)
    }

    pub fn folder<'a>(&'a self, explicit: Option<&'a str>) -> Result<&'a str> {
        if let Some(id) = explicit {
            anyhow::ensure!(valid_restore_id(id), "invalid --parent-id");
            anyhow::ensure!(
                self.folders
                    .last()
                    .map(String::as_str)
                    .is_none_or(|folder| folder == id),
                "--parent-id conflicts with the folder in the share URL"
            );
            return Ok(id);
        }
        Ok(self.folders.last().map(String::as_str).unwrap_or(""))
    }
}

// PikPak's web share router encodes matching-length IDs with a share-relative
// base64url prefix, the original two-character suffix, and a three-character check.
fn decode_folder_segment(share: &str, segment: &str) -> Option<String> {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    if !valid_restore_id(segment) {
        return None;
    }
    if segment.len() == share.len() {
        return Some(segment.to_owned());
    }
    if share.len() < 5 || segment.len() != share.len() + 4 {
        return None;
    }
    let (encoded, check) = segment.split_at(share.len());
    let prefix = check.strip_prefix('_')?;
    let left = URL_SAFE_NO_PAD.decode(&encoded[..encoded.len() - 2]).ok()?;
    let right = URL_SAFE_NO_PAD.decode(&share[..share.len() - 2]).ok()?;
    if left.len() != right.len() {
        return None;
    }
    let bytes: Vec<_> = left.into_iter().zip(right).map(|(a, b)| a ^ b).collect();
    let id = URL_SAFE_NO_PAD.encode(bytes) + &encoded[encoded.len() - 2..];
    id.starts_with(prefix).then_some(id)
}

#[derive(Debug, serde::Serialize)]
pub struct SharePreview {
    pub id: String,
    pub name: String,
    pub mime_type: Option<String>,
    pub size: Option<u64>,
    pub duration_seconds: Option<f64>,
    pub streams: Vec<ShareStream>,
    #[serde(skip)]
    pub thumbnail_link: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct ShareStream {
    label: String,
    available: bool,
    reason: Option<&'static str>,
}

impl SharePreview {
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![self.name.clone()];
        if let Some(mime) = &self.mime_type {
            lines.push(mime.clone());
        }
        if let Some(size) = self.size {
            lines.push(format!("{size} bytes"));
        }
        if let Some(seconds) = self.duration_seconds {
            lines.push(format!("{seconds:.1} seconds"));
        }
        for stream in &self.streams {
            lines.push(match stream.reason {
                Some(reason) => format!("{} ({reason})", stream.label),
                None => stream.label.clone(),
            });
        }
        lines
    }
}

fn restore_receipt(value: &serde_json::Value) -> String {
    let id = |key: &str| {
        value[key]
            .as_str()
            .filter(|id| valid_restore_id(id))
            .unwrap_or("unknown")
    };
    format!(
        "task {}, result folder {}",
        id("restore_task_id"),
        id("file_id")
    )
}

fn restore_error_detail(value: &serde_json::Value) -> Option<String> {
    match value["params"].get("error_detail") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(detail)) if detail.is_empty() => None,
        Some(serde_json::Value::String(detail)) => Some(super::sanitize(detail)),
        Some(detail) => Some(super::sanitize(&detail.to_string())),
    }
}

fn restore_result_ids(value: &serde_json::Value, requested: &[&str]) -> Result<Vec<String>> {
    let raw = value["params"]["trace_file_ids"]
        .as_str()
        .ok_or_else(|| anyhow!("missing trace_file_ids mapping"))?;
    let trace: serde_json::Value =
        serde_json::from_str(raw).context("invalid trace_file_ids JSON")?;
    let mapping = trace
        .as_object()
        .ok_or_else(|| anyhow!("trace_file_ids must be an ID mapping"))?;
    if mapping.len() != requested.len() {
        return Err(anyhow!(
            "trace_file_ids does not exactly cover the selected files"
        ));
    }
    let mut result = Vec::with_capacity(requested.len());
    let mut seen = HashSet::new();
    for source in requested {
        let destination = mapping
            .get(*source)
            .and_then(serde_json::Value::as_str)
            .filter(|id| valid_restore_id(id))
            .ok_or_else(|| anyhow!("missing or invalid restored ID for a selected file"))?;
        if !seen.insert(destination) {
            return Err(anyhow!("trace_file_ids contains duplicate restored IDs"));
        }
        result.push(destination.to_owned());
    }
    Ok(result)
}

impl PikPak {
    pub fn share_info(&self, share_id: &str, pass_code: &str) -> Result<ShareInfoResponse> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/share");

        let rb = self.http.get(&url).bearer_auth(&token).query(&[
            ("share_id", share_id),
            ("pass_code", pass_code),
            ("thumbnail_size", "SIZE_MEDIUM"),
        ]);
        let response = self.send_authed("share info", rb)?;

        let info: ShareInfoResponse = response.json().context("invalid share info json")?;
        if info.share_status != "OK" {
            return Err(anyhow!(
                "share is not available (status: {})",
                info.share_status
            ));
        }
        Ok(info)
    }

    pub fn share_preview(&self, share: &str, id: &str, token: &str) -> Result<SharePreview> {
        #[derive(serde::Deserialize)]
        struct Response {
            share_status: String,
            file_info: Option<super::FileInfoResponse>,
        }
        let rb = self
            .http
            .get(self.drive_url("drive/v1/share/file_info"))
            .bearer_auth(self.access_token()?)
            .query(&[
                ("share_id", share),
                ("file_id", id),
                ("pass_code_token", token),
            ]);
        let data: Response =
            json_or_api_error(self.send_authed("share preview", rb)?, "share preview")?;
        anyhow::ensure!(
            data.share_status == "OK",
            "share preview unavailable ({})",
            data.share_status
        );
        let file = data
            .file_info
            .ok_or_else(|| anyhow!("share file preview unavailable"))?;
        anyhow::ensure!(
            file.id.as_deref() == Some(id),
            "share preview returned an unexpected file"
        );
        let duration_seconds = file
            .medias
            .iter()
            .flatten()
            .filter(|m| m.is_visible != Some(false))
            .find_map(|m| m.video.as_ref()?.duration);
        let streams = file
            .medias
            .as_ref()
            .filter(|m| !m.is_empty())
            .map(|_| file.play_options())
            .unwrap_or_default()
            .into_iter()
            .map(|s| ShareStream {
                label: s.display_label(),
                available: s.available,
                reason: s.unavailable_reason,
            })
            .collect();
        Ok(SharePreview {
            id: id.into(),
            thumbnail_link: file.thumbnail_link,
            name: file.name,
            mime_type: file.mime_type,
            size: file.size.and_then(|s| s.parse().ok()),
            duration_seconds,
            streams,
        })
    }

    pub fn save_share(
        &self,
        share_id: &str,
        pass_code_token: &str,
        file_ids: &[&str],
        to_parent_id: &str,
    ) -> Result<()> {
        self.save_share_from(share_id, pass_code_token, file_ids, to_parent_id, &[])
    }

    pub fn save_share_from(
        &self,
        share_id: &str,
        pass_code_token: &str,
        file_ids: &[&str],
        to_parent_id: &str,
        ancestors: &[&str],
    ) -> Result<()> {
        anyhow::ensure!(!file_ids.is_empty(), "select at least one shared file");
        anyhow::ensure!(
            ancestors
                .iter()
                .all(|id| valid_restore_id(id) && !file_ids.contains(id))
                && ancestors.iter().copied().collect::<HashSet<_>>().len() == ancestors.len(),
            "invalid source folder chain"
        );
        if file_ids.iter().any(|id| !valid_restore_id(id))
            || (!to_parent_id.is_empty() && !valid_restore_id(to_parent_id))
            || file_ids.iter().copied().collect::<HashSet<_>>().len() != file_ids.len()
        {
            return Err(anyhow!(
                "save share requires distinct valid file IDs and a valid destination ID"
            ));
        }
        let mut completed = 0;
        let mut placed_ids = HashSet::new();
        for chunk in file_ids.chunks(500) {
            let mut accepted = false;
            let result = (|| {
                let mut payload = serde_json::json!({
                    "share_id": share_id,
                    "pass_code_token": pass_code_token,
                    "file_ids": chunk,
                    "parent_id": to_parent_id,
                    "specify_parent_id": true,
                    "params": { "trace_file_ids": chunk.join(",") },
                });
                if !ancestors.is_empty() {
                    payload["ancestor_ids"] = serde_json::json!(ancestors);
                }
                let rb = self
                    .http
                    .post(self.drive_url("drive/v1/share/restore"))
                    .bearer_auth(self.access_token()?)
                    .json(&payload);
                let response = self.send_authed("save share", rb).map_err(|error| {
                    if error.downcast_ref::<reqwest::Error>().is_some() {
                        // A lost response does not prove the POST was rejected.
                        accepted = true;
                        self.clear_ls_cache();
                        pending(format!(
                            "save share request outcome unknown; files may already be restored. Check the cloud drive before resubmitting: {error:#}"
                        ))
                    } else if format!("{error:#}").contains("file_restore_own") {
                        error.context("cannot save: these files already belong to your account")
                    } else {
                        error
                    }
                })?;
                accepted = true;
                self.clear_ls_cache();
                let value: serde_json::Value = response.json().map_err(|error| {
                    pending(format!(
                        "save share accepted but its response could not be read; files may already be restored. Check the cloud drive before resubmitting: {error}"
                    ))
                })?;
                let receipt = restore_receipt(&value);
                let container = value["file_id"]
                    .as_str()
                    .filter(|id| valid_restore_id(id))
                    .map(str::to_owned);
                if let Some(detail) = restore_error_detail(&value) {
                    return Err(anyhow!(
                        "save share accepted ({receipt}) but failed: {detail}; files may have been partially restored, check before resubmitting"
                    ));
                }
                if value["share_status"].as_str() != Some("OK") {
                    return Err(pending(format!(
                        "save share accepted ({receipt}) but share status is missing or unsuccessful; files may already be restored. Check before resubmitting"
                    )));
                }
                let completed_response = match value["restore_status"].as_str() {
                    Some("RESTORE_COMPLETE") => value,
                    Some("RESTORE_START") => {
                        let task_id = value["restore_task_id"]
                            .as_str()
                            .filter(|id| valid_restore_id(id))
                            .ok_or_else(|| pending(format!(
                                "save share accepted ({receipt}) but task identity is missing or invalid; files may already be restored. Check before resubmitting"
                            )))?;
                        self.wait_share_restore_task(task_id)?
                    }
                    _ => {
                        return Err(pending(format!(
                            "save share accepted ({receipt}) with unknown restore status; files may already be restored. Check before resubmitting"
                        )));
                    }
                };
                let restored_ids = restore_result_ids(&completed_response, chunk)
                    .map_err(|error| pending(format!(
                        "save share accepted ({receipt}) but restored file IDs could not be verified: {error:#}. Files may already be restored; check before resubmitting"
                    )))?;
                if restored_ids.iter().any(|id| placed_ids.contains(id)) {
                    return Err(pending(format!(
                        "save share accepted ({receipt}) but returned IDs from an earlier completed batch; check the cloud drive before resubmitting"
                    )));
                }
                // Some service versions accept the destination but restore into the default
                // container. Verify receipt IDs against a fresh destination listing before moving.
                let mut missing: HashSet<&str> = restored_ids.iter().map(String::as_str).collect();
                self.remove_listed_ids(to_parent_id, &mut missing).map_err(|error| pending(format!(
                    "share files were restored ({receipt}) as [{}], but destination verification failed; do not resubmit the share restore: {error:#}",
                    restored_ids.join(", ")
                )))?;
                if !missing.is_empty() {
                    let unchecked = || {
                        pending(format!(
                            "share restored ({receipt}) as [{}], but the location of some files is unconfirmed; check these IDs before resubmitting",
                            restored_ids.join(", ")
                        ))
                    };
                    // A missing destination listing can lag a successful direct restore.
                    // Only issue the fallback move once the receipt's container actually
                    // lists every missing ID. Never move the destination or the container.
                    let container = container
                        .as_deref()
                        .or_else(|| {
                            completed_response["file_id"]
                                .as_str()
                                .filter(|id| valid_restore_id(id))
                        })
                        .filter(|folder| *folder != to_parent_id)
                        .ok_or_else(unchecked)?;
                    if missing.contains(to_parent_id) || missing.contains(container) {
                        return Err(unchecked());
                    }
                    let mut unverified = missing.clone();
                    self.remove_listed_ids(container, &mut unverified)
                        .map_err(|error| {
                            unchecked().context(format!(
                                "checking the restore container failed: {error:#}"
                            ))
                        })?;
                    if !unverified.is_empty() {
                        return Err(unchecked());
                    }
                    let missing: Vec<_> = restored_ids
                        .iter()
                        .map(String::as_str)
                        .filter(|id| missing.contains(id))
                        .collect();
                    self.mv(&missing, to_parent_id).with_context(|| format!(
                        "share files were restored ({receipt}) as [{}], but placement in destination '{}' failed or is unconfirmed; do not resubmit the share restore",
                        missing.join(", "), if to_parent_id.is_empty() { "/" } else { to_parent_id },
                    ))?;
                }
                placed_ids.extend(restored_ids);
                Ok(())
            })();
            if let Err(cause) = result {
                let pending = super::mutation_progress(&cause).pending;
                return Err(batch_mutation_error(
                    "save share",
                    file_ids.len(),
                    MutationProgress {
                        completed_items: completed,
                        accepted,
                        pending,
                    },
                    cause,
                ));
            }
            completed += chunk.len();
        }
        Ok(())
    }

    fn remove_listed_ids(&self, folder: &str, remaining: &mut HashSet<&str>) -> Result<()> {
        for page in self.ls_pages(folder)? {
            for entry in page? {
                remaining.remove(entry.id.as_str());
            }
            if remaining.is_empty() {
                break;
            }
        }
        Ok(())
    }

    fn wait_share_restore_task(&self, task_id: &str) -> Result<serde_json::Value> {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(pending(format!(
                    "save share accepted as task {task_id}, still processing; files may already be restored. Check this task before resubmitting"
                )));
            }
            let task: serde_json::Value = (|| {
                let rb = self
                    .http
                    .get(format!("{}/{task_id}", self.drive_url("drive/v1/tasks")))
                    .bearer_auth(self.access_token()?)
                    .timeout(remaining.min(Duration::from_secs(10)));
                let response = self.send_authed("share restore task poll", rb)?;
                json_or_api_error(response, "share restore task poll")
            })().map_err(|error| pending(format!(
                "save share accepted as task {task_id}; completion check failed and files may already be restored. Check this task before resubmitting: {error:#}"
            )))?;
            if task["id"].as_str() != Some(task_id) {
                return Err(pending(format!(
                    "save share accepted as task {task_id}, but task lookup returned a different or missing identity. Check before resubmitting"
                )));
            }
            let error_detail = restore_error_detail(&task);
            if error_detail.is_some() || task["phase"].as_str() == Some("PHASE_TYPE_ERROR") {
                return Err(anyhow!(
                    "save share accepted as task {task_id} but failed: {} ({}); files may have been partially restored, check before resubmitting",
                    super::sanitize(task["message"].as_str().unwrap_or("restore task failed")),
                    error_detail.as_deref().unwrap_or("PHASE_TYPE_ERROR"),
                ));
            }
            match task["phase"].as_str() {
                Some("PHASE_TYPE_COMPLETE") => return Ok(task),
                Some("PHASE_TYPE_PENDING" | "PHASE_TYPE_RUNNING") => {}
                _ => {
                    return Err(pending(format!(
                        "save share accepted as task {task_id}, but its phase is paused, missing or unknown; completion is unconfirmed. Check before resubmitting"
                    )));
                }
            }
            std::thread::sleep(
                Duration::from_secs(1).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }

    pub fn create_share(
        &self,
        file_ids: &[&str],
        need_password: bool,
        expiration_days: i64,
    ) -> Result<CreateShareResponse> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/share");

        let payload = serde_json::json!({
            "file_ids": file_ids,
            "share_to": if need_password { "encryptedlink" } else { "publiclink" },
            "expiration_days": expiration_days,
            "pass_code_option": if need_password { "REQUIRED" } else { "NOT_REQUIRED" },
        });

        let rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        let response = self.send_authed("create share", rb)?;
        json_or_api_error(response, "create share")
    }

    /// List one folder inside a share (paginated). `parent_id` comes from a
    /// folder entry returned by `share_info` or a previous detail call —
    /// nested share folders are unreachable through `share_info` alone.
    pub fn share_detail(
        &self,
        share_id: &str,
        parent_id: &str,
        pass_code_token: &str,
    ) -> Result<Vec<ShareEntry>> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/share/detail");

        let mut entries: Vec<ShareEntry> = Vec::new();
        let mut page_token: Option<String> = None;
        let mut seen_page_tokens = HashSet::new();
        loop {
            let mut rb = self.http.get(&url).bearer_auth(&token).query(&[
                ("share_id", share_id),
                ("parent_id", parent_id),
                ("pass_code_token", pass_code_token),
                ("limit", "100"),
                ("thumbnail_size", "SIZE_SMALL"),
            ]);
            if let Some(ref pt) = page_token {
                rb = rb.query(&[("page_token", pt.as_str())]);
            }
            let response = self.send_authed("share detail", rb)?;
            let resp: super::ShareDetailResponse = json_or_api_error(response, "share detail")?;
            anyhow::ensure!(
                resp.files.iter().all(|entry| valid_restore_id(&entry.id)),
                "invalid file ID in share listing"
            );
            entries.extend(resp.files);

            match resp.next_page_token.filter(|t| !t.is_empty()) {
                Some(t) if seen_page_tokens.insert(t.clone()) => page_token = Some(t),
                Some(_) => return Err(anyhow!("share detail pagination repeated a page token")),
                None => break,
            }
        }
        Ok(entries)
    }

    pub fn list_shares(&self) -> Result<Vec<MyShare>> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/share/list");

        // Paginate: a single page silently dropped every share past the
        // first 100.
        let mut shares: Vec<MyShare> = Vec::new();
        let mut page_token: Option<String> = None;
        let mut seen_page_tokens = HashSet::new();
        loop {
            let mut rb = self
                .http
                .get(&url)
                .bearer_auth(&token)
                .query(&[("limit", "100"), ("thumbnail_size", "SIZE_SMALL")]);
            if let Some(ref pt) = page_token {
                rb = rb.query(&[("page_token", pt.as_str())]);
            }
            let response = self.send_authed("list shares", rb)?;
            let resp: ShareListResponse = json_or_api_error(response, "list shares")?;
            shares.extend(resp.data);

            match resp.next_page_token.filter(|t| !t.is_empty()) {
                Some(t) if seen_page_tokens.insert(t.clone()) => page_token = Some(t),
                Some(_) => break,
                None => break,
            }
        }
        Ok(shares)
    }

    pub fn delete_shares(&self, share_ids: &[&str]) -> Result<()> {
        self.batch_mutation(
            "delete shares",
            "drive/v1/share:batchDelete",
            share_ids,
            "ids",
            serde_json::json!({}),
        )
    }
}
