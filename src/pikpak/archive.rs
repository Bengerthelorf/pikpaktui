use super::{PikPak, json_or_api_error};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ArchiveMember {
    pub filename: String,
    pub path: String,
    pub kind: String,
    #[serde(flatten)]
    fields: Map<String, Value>,
}

impl ArchiveMember {
    pub fn is_folder(&self) -> bool {
        self.kind == "drive#folder"
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ArchiveListing {
    pub status: String,
    #[serde(default)]
    pub status_text: String,
    #[serde(default)]
    pub files: Vec<ArchiveMember>,
    #[serde(default)]
    pub task_id: String,
}

impl ArchiveListing {
    pub fn needs_password(&self) -> bool {
        matches!(self.status.as_str(), "PASS_WORD_EMPTY" | "PASS_WORD_ERROR")
    }
    pub fn check(&self) -> Result<()> {
        ensure!(
            self.status == "OK",
            "archive listing {}: {}",
            self.status,
            self.status_text
        );
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ArchiveProgress {
    #[serde(default)]
    pub phase: String,
    pub progress: u64,
    #[serde(default)]
    pub file_id: String,
    #[serde(default)]
    pub error_description: String,
    pub expires_in: Option<u64>,
}

impl ArchiveProgress {
    pub fn failed(&self) -> bool {
        self.phase == "PHASE_TYPE_ERROR"
    }
    pub fn complete(&self) -> bool {
        !self.failed() && (self.progress >= 100 || self.phase == "PHASE_TYPE_COMPLETE")
    }
}

fn selected_files(members: &[ArchiveMember]) -> Result<Vec<Value>> {
    members
        .iter()
        .map(|member| {
            let mut value = serde_json::to_value(member)?;
            value["checked"] = json!(true);
            Ok(value)
        })
        .collect()
}

impl PikPak {
    pub(crate) fn archive_gcid(&self, file_id: &str) -> Result<String> {
        let token = self.access_token()?;
        let url = format!("{}/{}", self.drive_url("drive/v1/files"), file_id);
        let rb = self
            .http
            .get(url)
            .bearer_auth(token)
            .query(&[("usage", "ALL")]);
        let info: Value = json_or_api_error(self.send_authed("archive info", rb)?, "archive info")?;
        let link = info["apps"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|app| app["id"] == "decompress" && app["is_default"] == true)
            .and_then(|app| app["link"].as_str())
            .context("PikPak did not offer cloud extraction for this file")?;
        reqwest::Url::parse(link)?
            .query_pairs()
            .find(|(key, val)| key == "gcid" && !val.is_empty())
            .map(|(_, value)| value.into_owned())
            .context("archive link has no GCID")
    }

    pub(crate) fn archive_list(
        &self,
        id: &str,
        gcid: &str,
        path: &str,
        password: &str,
    ) -> Result<ArchiveListing> {
        let rb = self
            .http
            .post(self.drive_url("decompress/v1/list"))
            .bearer_auth(self.access_token()?)
            .json(&json!({"file_id": id, "gcid": gcid, "path": path, "password": password}));
        json_or_api_error(self.send_authed("list archive", rb)?, "list archive")
    }

    pub(crate) fn archive_extract(
        &self,
        id: &str,
        gcid: &str,
        password: &str,
        members: &[ArchiveMember],
    ) -> Result<String> {
        let rb = self.http.post(self.drive_url("decompress/v1/decompress")).bearer_auth(self.access_token()?)
            .json(&json!({"file_id": id, "gcid": gcid, "password": password, "files": selected_files(members)?, "default_parent":true}));
        let response: Value =
            json_or_api_error(self.send_authed("extract archive", rb)?, "extract archive")?;
        let status = response["status"].as_str().unwrap_or("");
        ensure!(
            status == "OK",
            "extract archive {status}: {}",
            response["status_text"].as_str().unwrap_or("")
        );
        let task = response["task_id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .context("extraction returned no task ID")?;
        self.clear_ls_cache();
        Ok(task.to_owned())
    }

    pub(crate) fn archive_progress(&self, task: &str) -> Result<ArchiveProgress> {
        let rb = self
            .http
            .get(self.drive_url("decompress/v1/progress"))
            .bearer_auth(self.access_token()?)
            .query(&[("task_id", task)]);
        let progress: ArchiveProgress = json_or_api_error(
            self.send_authed("archive progress", rb)?,
            "archive progress",
        )?;
        if progress.complete() {
            self.clear_ls_cache();
        }
        Ok(progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_workflow_uses_cloud_ids_and_selected_member_metadata() {
        use super::super::parity_tests::scripted_server;
        use super::super::tests::{temp_test_dir, test_client};
        let (base, requests, handle) = scripted_server(vec![
            (
                200,
                r#"{"apps":[{"id":"decompress","is_default":true,"link":"https://example.invalid/open?gcid=abc"}]}"#,
            ),
            (
                200,
                r#"{"status":"PASS_WORD_EMPTY","status_text":"","files":[]}"#,
            ),
            (
                200,
                r#"{"status":"OK","files":[{"filename":"name","path":"sub/name","kind":"drive#file","index":23}]}"#,
            ),
            (200, r#"{"status":"OK","task_id":"t /?"}"#),
            (
                200,
                r#"{"phase":"PHASE_TYPE_COMPLETE","progress":100,"file_id":"new"}"#,
            ),
        ]);
        let dir = temp_test_dir("archive-workflow");
        let client = test_client(base, dir.join("session.json"));
        let gcid = client.archive_gcid("source").unwrap();
        assert_eq!(gcid, "abc");
        assert!(
            client
                .archive_list("source", &gcid, "", "")
                .unwrap()
                .needs_password()
        );
        let listing = client
            .archive_list("source", &gcid, "sub", "secret")
            .unwrap();
        listing.check().unwrap();
        let task = client
            .archive_extract("source", &gcid, "secret", &listing.files)
            .unwrap();
        assert!(client.archive_progress(&task).unwrap().complete());
        handle.join().unwrap();
        let requests = requests.lock().unwrap();
        assert!(requests[0].starts_with("GET /drive/v1/files/source?usage=ALL "));
        assert!(requests[1].starts_with("POST /decompress/v1/list "));
        let body = |idx: usize| {
            serde_json::from_str::<Value>(requests[idx].split_once("\r\n\r\n").unwrap().1).unwrap()
        };
        assert_eq!(
            body(1),
            json!({"gcid":"abc","file_id":"source","path":"","password":""})
        );
        assert_eq!(body(2)["path"], "sub");
        assert!(requests[3].starts_with("POST /decompress/v1/decompress "));
        assert_eq!(
            body(3),
            json!({"gcid":"abc","file_id":"source","password":"secret","default_parent":true,"files":[{"filename":"name","path":"sub/name","kind":"drive#file","index":23,"checked":true}]})
        );
        assert!(requests[4].starts_with("GET /decompress/v1/progress?task_id=t+%2F%3F "));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn extraction_sends_the_server_member_with_selection_and_preserves_opaque_fields() {
        let file = json!({"filename":"x", "path":"sub/x", "kind":"drive#file", "index":7, "filesize":"452", "future_field":[1]});
        let mut expected = file.clone();
        expected["checked"] = json!(true);
        assert_eq!(
            selected_files(&[serde_json::from_value(file).unwrap()]).unwrap(),
            vec![expected]
        );
        assert!(selected_files(&[]).unwrap().is_empty());
    }
}
