use serde::Deserialize;

pub(super) struct OssArgs {
    pub(super) endpoint: String,
    pub(super) access_key_id: String,
    pub(super) access_key_secret: String,
    pub(super) security_token: String,
    pub(super) bucket: String,
    pub(super) key: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct UploadInitResponse {
    #[serde(default)]
    pub(super) upload_type: String,
    pub(super) file: UploadFileInfo,
    #[serde(default)]
    pub(super) resumable: Option<ResumableContext>,
}

#[derive(Debug, Deserialize)]
pub(super) struct UploadFileInfo {
    #[serde(default)]
    pub(super) phase: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ResumableContext {
    #[serde(default)]
    pub(super) kind: String,
    #[serde(default)]
    pub(super) params: ResumableParams,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct ResumableParams {
    #[serde(default)]
    pub(super) endpoint: Option<String>,
    #[serde(default)]
    pub(super) access_key_id: Option<String>,
    #[serde(default)]
    pub(super) access_key_secret: Option<String>,
    #[serde(default)]
    pub(super) security_token: Option<String>,
    #[serde(default)]
    pub(super) bucket: Option<String>,
    #[serde(default)]
    pub(super) key: Option<String>,
}
