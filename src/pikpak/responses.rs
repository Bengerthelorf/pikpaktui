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
