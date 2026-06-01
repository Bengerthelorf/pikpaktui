use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct ShareListResponse {
    #[serde(default)]
    pub data: Vec<MyShare>,
}

#[derive(Debug, Deserialize)]
pub struct MyShare {
    pub share_id: String,
    pub share_url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub pass_code: String,
    #[serde(default)]
    pub share_to: String,
    #[serde(default)]
    pub create_time: String,
    #[serde(default)]
    pub expiration_days: String,
    #[serde(default)]
    pub view_count: String,
    #[serde(default)]
    pub restore_count: String,
    #[serde(default)]
    pub file_num: String,
    #[serde(default)]
    pub share_status: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateShareResponse {
    pub share_id: String,
    pub share_url: String,
    #[serde(default)]
    pub pass_code: String,
    #[serde(default)]
    pub share_text: String,
}

#[derive(Debug, Deserialize)]
pub struct ShareEntry {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct ShareInfoResponse {
    pub share_status: String,
    #[serde(default)]
    pub pass_code_token: String,
    #[serde(default)]
    pub files: Vec<ShareEntry>,
}

#[derive(Debug, Deserialize)]
pub struct QuotaInfo {
    pub quota: Option<QuotaDetail>,
}

#[derive(Debug, Deserialize)]
pub struct QuotaDetail {
    #[serde(default)]
    pub limit: Option<String>,
    #[serde(default)]
    pub usage: Option<String>,
    #[serde(default)]
    pub usage_in_trash: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TransferQuotaResponse {
    pub base: Option<TransferQuotaBase>,
}

#[derive(Debug, Deserialize)]
pub struct TransferQuotaBase {
    pub offline: Option<TransferBand>,
    pub download: Option<TransferBand>,
    pub upload: Option<TransferBand>,
    #[serde(default)]
    pub expire_time: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TransferBand {
    pub total_assets: Option<u64>,
    pub assets: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct OfflineTaskResponse {
    #[serde(default)]
    pub task: Option<OfflineTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineTask {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub progress: i64,
    #[serde(default)]
    pub file_id: Option<String>,
    #[serde(default)]
    pub file_size: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub created_time: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OfflineListResponse {
    #[serde(default)]
    pub tasks: Vec<OfflineTask>,
}

#[derive(Debug, Deserialize)]
pub struct EventsResponse {
    #[serde(default)]
    pub events: Vec<EventEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEntry {
    #[serde(default, rename = "type")]
    pub event_type: Option<String>,
    #[serde(default)]
    pub type_name: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub created_time: Option<String>,
    #[serde(default)]
    pub reference_resource: Option<EventRefResource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRefResource {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct VipInfoResponse {
    #[serde(default)]
    pub data: Option<VipData>,
}

#[derive(Debug, Deserialize)]
pub struct VipData {
    #[serde(default, rename = "type")]
    pub vip_type: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub expire: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_info_parses_about_response() {
        // drive/v1/about reports byte counts as strings.
        let json = r#"{
            "quota": {
                "limit": "10737418240",
                "usage": "2147483648",
                "usage_in_trash": "1024"
            }
        }"#;
        let resp: QuotaInfo = serde_json::from_str(json).unwrap();
        let detail = resp.quota.expect("quota present");
        assert_eq!(detail.limit.as_deref(), Some("10737418240"));
        assert_eq!(detail.usage.as_deref(), Some("2147483648"));
        assert_eq!(detail.usage_in_trash.as_deref(), Some("1024"));
    }

    #[test]
    fn quota_info_tolerates_missing_quota() {
        let resp: QuotaInfo = serde_json::from_str("{}").unwrap();
        assert!(resp.quota.is_none());
    }

    #[test]
    fn offline_list_parses_tasks() {
        let json = r#"{
            "tasks": [
                {
                    "id": "TASK1",
                    "name": "movie.mkv",
                    "phase": "PHASE_TYPE_RUNNING",
                    "progress": 42,
                    "file_id": "FID1",
                    "file_size": "123456",
                    "message": "downloading"
                },
                { "id": "TASK2", "name": "done.zip", "phase": "PHASE_TYPE_COMPLETE" }
            ]
        }"#;
        let resp: OfflineListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.tasks.len(), 2);
        assert_eq!(resp.tasks[0].progress, 42);
        assert_eq!(resp.tasks[0].file_id.as_deref(), Some("FID1"));
        // Second task omits the optional fields; defaults apply.
        assert_eq!(resp.tasks[1].phase, "PHASE_TYPE_COMPLETE");
        assert_eq!(resp.tasks[1].progress, 0);
        assert!(resp.tasks[1].file_id.is_none());
    }

    #[test]
    fn offline_list_defaults_to_empty() {
        let resp: OfflineListResponse = serde_json::from_str("{}").unwrap();
        assert!(resp.tasks.is_empty());
    }

    #[test]
    fn offline_task_response_optional_task() {
        let resp: OfflineTaskResponse =
            serde_json::from_str(r#"{"task":{"id":"T","name":"n","phase":"P","progress":0}}"#)
                .unwrap();
        assert_eq!(resp.task.unwrap().id, "T");
        let empty: OfflineTaskResponse = serde_json::from_str("{}").unwrap();
        assert!(empty.task.is_none());
    }

    #[test]
    fn share_info_parses_files_and_status() {
        let json = r#"{
            "share_status": "OK",
            "pass_code_token": "PCT123",
            "files": [
                {"id": "F1", "name": "a.txt"},
                {"id": "F2", "name": "b.txt"}
            ]
        }"#;
        let resp: ShareInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.share_status, "OK");
        assert_eq!(resp.pass_code_token, "PCT123");
        assert_eq!(resp.files.len(), 2);
        assert_eq!(resp.files[1].name, "b.txt");
    }

    #[test]
    fn share_info_tolerates_missing_optional_fields() {
        // A restricted share may omit pass_code_token and files.
        let resp: ShareInfoResponse =
            serde_json::from_str(r#"{"share_status":"SENSITIVE_RESOURCE"}"#).unwrap();
        assert_eq!(resp.share_status, "SENSITIVE_RESOURCE");
        assert!(resp.pass_code_token.is_empty());
        assert!(resp.files.is_empty());
    }

    #[test]
    fn share_list_parses_string_typed_counts() {
        // PikPak returns numeric counts as strings, not integers.
        let json = r#"{
            "data": [
                {
                    "share_id": "S1",
                    "share_url": "https://mypikpak.com/s/S1",
                    "title": "my share",
                    "file_num": "3",
                    "view_count": "10",
                    "share_status": "OK"
                }
            ]
        }"#;
        let resp: ShareListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data.len(), 1);
        assert_eq!(resp.data[0].share_id, "S1");
        assert_eq!(resp.data[0].file_num, "3");
        assert_eq!(resp.data[0].view_count, "10");
        // Absent fields fall back to empty strings.
        assert!(resp.data[0].pass_code.is_empty());
    }

    #[test]
    fn vip_info_renames_type_field() {
        // The API field is "type"; we expose it as `vip_type`.
        let json = r#"{"data":{"type":"platinum","status":"ok","expire":"2027-01-01T00:00:00Z"}}"#;
        let resp: VipInfoResponse = serde_json::from_str(json).unwrap();
        let data = resp.data.expect("vip data present");
        assert_eq!(data.vip_type.as_deref(), Some("platinum"));
        assert_eq!(data.status.as_deref(), Some("ok"));
    }

    #[test]
    fn transfer_quota_parses_bands() {
        let json = r#"{
            "base": {
                "offline": {"total_assets": 100, "assets": 40},
                "download": {"total_assets": 50, "assets": 50},
                "expire_time": "2026-12-31T00:00:00Z"
            }
        }"#;
        let resp: TransferQuotaResponse = serde_json::from_str(json).unwrap();
        let base = resp.base.expect("base present");
        let offline = base.offline.expect("offline band");
        assert_eq!(offline.total_assets, Some(100));
        assert_eq!(offline.assets, Some(40));
        // The upload band is absent in this payload.
        assert!(base.upload.is_none());
    }

    #[test]
    fn events_response_renames_type_and_nests_resource() {
        let json = r#"{
            "events": [
                {
                    "type": "file_created",
                    "type_name": "Created",
                    "file_name": "report.pdf",
                    "reference_resource": {
                        "kind": "drive#file",
                        "name": "report.pdf",
                        "mime_type": "application/pdf"
                    }
                }
            ]
        }"#;
        let resp: EventsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.events.len(), 1);
        let ev = &resp.events[0];
        assert_eq!(ev.event_type.as_deref(), Some("file_created"));
        let res = ev.reference_resource.as_ref().expect("ref resource");
        assert_eq!(res.mime_type.as_deref(), Some("application/pdf"));
    }
}
