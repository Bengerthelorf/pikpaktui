use anyhow::{Result, anyhow};

use super::{
    OfflineListResponse, OfflineTask, OfflineTaskResponse, PikPak, ensure_success,
    json_or_api_error,
};

pub const OFFLINE_TASK_PHASES: &[&str] = &[
    "PHASE_TYPE_RUNNING",
    "PHASE_TYPE_PENDING",
    "PHASE_TYPE_COMPLETE",
    "PHASE_TYPE_ERROR",
    "PHASE_TYPE_PAUSED",
    "PHASE_TYPE_UNKNOW",
];

impl PikPak {
    pub fn offline_download(
        &self,
        file_url: &str,
        parent_id: Option<&str>,
        name: Option<&str>,
    ) -> Result<OfflineTaskResponse> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/files");

        let mut payload = serde_json::json!({
            "kind": "drive#file",
            "upload_type": "UPLOAD_TYPE_URL",
            "url": { "url": file_url },
        });
        if let Some(pid) = parent_id {
            payload["parent_id"] = serde_json::json!(pid);
            payload["folder_type"] = serde_json::json!("");
        } else {
            payload["folder_type"] = serde_json::json!("DOWNLOAD");
        }
        if let Some(n) = name {
            payload["name"] = serde_json::json!(n);
        }

        let rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        let response = self.send_authed("offline download", rb)?;
        self.clear_ls_cache();
        json_or_api_error(response, "offline download")
    }

    pub fn offline_list(&self, limit: u32, phases: &[&str]) -> Result<OfflineListResponse> {
        let mut result = OfflineListResponse {
            tasks: Vec::new(),
            next_page_token: None,
            expires_in: None,
        };
        let mut seen = std::collections::HashSet::new();
        while result.tasks.len() < limit as usize {
            let page_size = (limit as usize - result.tasks.len()).min(500) as u32;
            let mut page =
                self.offline_list_page(page_size, phases, result.next_page_token.as_deref())?;
            result.expires_in = match (result.expires_in, page.expires_in) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            result.tasks.append(&mut page.tasks);
            result.tasks.truncate(limit as usize);
            result.next_page_token = page.next_page_token.filter(|t| !t.is_empty());
            match &result.next_page_token {
                Some(token) if !seen.insert(token.clone()) => {
                    return Err(anyhow!(
                        "offline list pagination stuck: repeated page token"
                    ));
                }
                Some(_) => {}
                None => break,
            }
        }
        Ok(result)
    }

    fn offline_list_page(
        &self,
        limit: u32,
        phases: &[&str],
        page_token: Option<&str>,
    ) -> Result<OfflineListResponse> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/tasks");
        let filters = serde_json::json!({"phase": {"in": phases.join(",")}});
        let mut rb = self.http.get(&url).bearer_auth(&token).query(&[
            ("type", "offline"),
            ("thumbnail_size", "SIZE_SMALL"),
            ("limit", &limit.to_string()),
            ("filters", &filters.to_string()),
            ("with", "reference_resource"),
        ]);
        if let Some(token) = page_token {
            rb = rb.query(&[("page_token", token)]);
        }
        let response = self.send_authed("offline list", rb)?;
        json_or_api_error(response, "offline list")
    }

    /// Ask the server what a magnet/URL contains without adding a task
    /// (the web client's landing preview).
    pub fn parse_resource(&self, resource_url: &str) -> Result<serde_json::Value> {
        self.resource_list(serde_json::json!({
            "urls": resource_url, "page_size": 500, "thumbnail_type": "FROM_HASH",
        }))
    }

    fn resource_list(&self, payload: serde_json::Value) -> Result<serde_json::Value> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/resource/list");
        let rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        let response = self.send_authed("resource parse", rb)?;
        json_or_api_error(response, "resource parse")
    }

    /// Fetch one task's fresh state (rclone's getTask poll endpoint).
    pub fn offline_task(&self, task_id: &str) -> Result<OfflineTask> {
        let token = self.access_token()?;
        let url = format!("{}/{}", self.drive_url("drive/v1/tasks"), task_id);

        let rb = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .query(&[("type", "offline"), ("checkPhase", "true")]);
        let response = self.send_authed("task poll", rb)?;
        let value: serde_json::Value = json_or_api_error(response, "task poll")?;
        // Some deployments wrap the task, some return it bare.
        let task = if value.get("task").is_some() {
            value["task"].clone()
        } else {
            value
        };
        serde_json::from_value(task).map_err(|e| anyhow!("invalid task json: {e}"))
    }

    pub fn offline_task_retry(&self, task_id: &str) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/task");

        let payload = serde_json::json!({
            "type": "offline",
            "create_type": "RETRY",
            "id": task_id,
        });

        let rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        let response = self.send_authed("offline task retry", rb)?;
        self.clear_ls_cache();
        ensure_success(response, "offline task retry")
    }

    pub fn delete_tasks(&self, task_ids: &[&str], delete_files: bool) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/tasks");

        let mut pairs: Vec<(&str, String)> = task_ids
            .iter()
            .map(|id| ("task_ids", id.to_string()))
            .collect();
        pairs.push(("delete_files", delete_files.to_string()));

        let mut rb = self.http.delete(&url).bearer_auth(&token);
        for (k, v) in &pairs {
            rb = rb.query(&[(k, v)]);
        }
        let response = self.send_authed("delete tasks", rb)?;
        if delete_files {
            self.clear_ls_cache();
        }
        ensure_success(response, "delete tasks")
    }
}

impl PikPak {
    pub fn task_status_page(
        &self,
        id: &str,
        page: Option<&str>,
    ) -> Result<(Vec<serde_json::Value>, Option<String>)> {
        anyhow::ensure!(
            !id.is_empty()
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
            "invalid task ID"
        );
        let url = self.drive_url(&format!("drive/v1/task/{id}/statuses"));
        let rb = self
            .http
            .get(url)
            .bearer_auth(self.access_token()?)
            .query(&[
                ("page_token", page.unwrap_or("")),
                ("limit", "100"),
                ("with", "reference_resource"),
                ("thumbnail_size", "SIZE_LARGE"),
                ("filters", "{}"),
            ]);
        let mut data: serde_json::Value =
            json_or_api_error(self.send_authed("task statuses", rb)?, "task statuses")?;
        let next = data["next_page_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let key = if data.get("statuses").is_some() {
            "statuses"
        } else {
            "tasks"
        };
        let rows = match data[key].take() {
            serde_json::Value::Array(rows) => rows,
            serde_json::Value::Null => Vec::new(),
            _ => anyhow::bail!("invalid task status list"),
        };
        Ok((rows, next))
    }
}

impl PikPak {
    /// Associate preview results with their original input even if PikPak reorders or
    /// normalizes the URLs. Older servers omit the association and need a single lookup.
    pub fn preview_resources(&self, urls: &[String]) -> Vec<Result<serde_json::Value>> {
        let mut results = Vec::with_capacity(urls.len());
        for group in urls.chunks(100) {
            if group.len() == 1 {
                results.push(self.parse_resource(&group[0]));
                continue;
            }
            let preview = match self.resource_list(serde_json::json!({
                "urls": group.join("\n"), "page": 100, "thumbnailType": "FROM_HASH"
            })) {
                Ok(data) => data,
                Err(error)
                    if error
                        .downcast_ref::<super::DriveRequestError>()
                        .is_some_and(|e| matches!(e.status.as_u16(), 400 | 413 | 422)) =>
                {
                    results.extend(group.iter().map(|url| self.parse_resource(url)));
                    continue;
                }
                Err(error) => {
                    let message = format!("{error:#}");
                    results.extend(group.iter().map(|_| Err(anyhow!(message.clone()))));
                    continue;
                }
            };
            let mut matched = std::collections::HashMap::<&str, Vec<&serde_json::Value>>::new();
            if let Some(items) = preview.get("list").unwrap_or(&preview)["resources"].as_array() {
                for item in items {
                    let meta = &item["meta"];
                    if let Some(key) = ["originUrl", "url"]
                        .into_iter()
                        .filter_map(|k| meta[k].as_str())
                        .find(|key| group.iter().any(|url| url == key))
                    {
                        matched.entry(key).or_default().push(item);
                    }
                }
            }
            for url in group {
                results.push(match matched.remove(url.as_str()) {
                    Some(items) => Ok(serde_json::json!({"list":{"resources":items}})),
                    None => self.parse_resource(url),
                });
            }
        }
        results
    }
}
