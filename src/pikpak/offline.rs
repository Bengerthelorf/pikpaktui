use anyhow::{Result, anyhow};

use super::{
    OfflineListResponse, OfflineTask, OfflineTaskResponse, PikPak, ensure_success,
    json_or_api_error,
};

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
        json_or_api_error(response, "offline download")
    }

    pub fn offline_list(&self, limit: u32, phases: &[&str]) -> Result<OfflineListResponse> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/tasks");

        let filters = serde_json::json!({
            "phase": { "in": phases.join(",") }
        });

        let rb = self.http.get(&url).bearer_auth(&token).query(&[
            ("type", "offline"),
            ("thumbnail_size", "SIZE_SMALL"),
            ("limit", &limit.to_string()),
            ("filters", &filters.to_string()),
            ("with", "reference_resource"),
        ]);
        let response = self.send_authed("offline list", rb)?;
        json_or_api_error(response, "offline list")
    }

    /// Ask the server what a magnet/URL contains without adding a task
    /// (the web client's landing preview).
    pub fn parse_resource(&self, resource_url: &str) -> Result<serde_json::Value> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/resource/list");

        let payload = serde_json::json!({
            "urls": resource_url,
            "page_size": 500,
            "thumbnail_type": "FROM_HASH",
        });

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
        ensure_success(response, "delete tasks")
    }
}
