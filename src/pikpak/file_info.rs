use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaLink {
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaVideo {
    #[serde(default)]
    pub height: Option<i64>,
    #[serde(default)]
    pub width: Option<i64>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub bit_rate: Option<i64>,
    #[serde(default)]
    pub video_codec: Option<String>,
    #[serde(default)]
    pub audio_codec: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInfo {
    #[serde(default)]
    pub media_name: Option<String>,
    #[serde(default)]
    pub link: Option<MediaLink>,
    #[serde(default)]
    pub video: Option<MediaVideo>,
    #[serde(default)]
    pub is_origin: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfoResponse {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub created_time: Option<String>,
    #[serde(default)]
    pub modified_time: Option<String>,
    #[serde(default)]
    pub web_content_link: Option<String>,
    #[serde(default)]
    pub thumbnail_link: Option<String>,
    #[serde(default)]
    pub links: Option<std::collections::HashMap<String, LinkInfo>>,
    #[serde(default)]
    pub medias: Option<Vec<MediaInfo>>,
}

impl FileInfoResponse {
    pub fn download_url(&self) -> Option<&str> {
        self.web_content_link
            .as_deref()
            .or(self.links.as_ref().and_then(|l| {
                l.get("application/octet-stream")
                    .and_then(|v| v.url.as_deref())
            }))
    }

    pub fn file_size(&self) -> u64 {
        self.size
            .as_deref()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkInfo {
    #[serde(default)]
    pub url: Option<String>,
}
