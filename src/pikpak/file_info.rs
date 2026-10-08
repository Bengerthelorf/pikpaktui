use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaLink {
    #[serde(default)]
    pub expire: Option<String>,
    #[serde(default)]
    pub fallbacks: Vec<serde_json::Value>,
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
    pub media_id: Option<String>,
    #[serde(default)]
    pub is_visible: Option<bool>,
    #[serde(default)]
    pub need_more_quota: Option<bool>,
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
    pub phase: Option<String>,
    #[serde(default)]
    pub audit: Option<serde_json::Value>,
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

#[derive(Debug, Clone)]
pub struct PlayOption {
    pub label: String,
    pub url: String,
    pub available: bool,
    pub unavailable_reason: Option<&'static str>,
    pub is_original: bool,
    pub height: Option<u32>,
}

impl PlayOption {
    pub fn display_label(&self) -> String {
        match (self.is_original, self.height) {
            (true, Some(height)) => format!("{} · {height}p", self.label),
            _ => self.label.clone(),
        }
    }

    /// Choose only permitted links. A resolution preference uses the highest
    /// available transcode at/below the target, then the lowest above it,
    /// then the original. Explicit resolutions prefer transcodes even when
    /// the source has the same height.
    /// Original preference uses the source, then the highest known rendition.
    pub fn preferred_index(options: &[Self], target_height: Option<u32>) -> Option<usize> {
        let available: Vec<_> = options
            .iter()
            .enumerate()
            .filter(|(_, o)| o.available)
            .collect();
        let original = available
            .iter()
            .find(|(_, o)| o.is_original)
            .map(|(i, _)| *i);
        if let Some(target) = target_height {
            available
                .iter()
                .filter(|(_, o)| !o.is_original && o.height.is_some_and(|h| h <= target))
                .min_by_key(|(i, o)| (std::cmp::Reverse(o.height), *i))
                .map(|(i, _)| *i)
                .or_else(|| {
                    available
                        .iter()
                        .filter(|(_, o)| !o.is_original && o.height.is_some_and(|h| h > target))
                        .min_by_key(|(i, o)| (o.height, *i))
                        .map(|(i, _)| *i)
                })
                .or(original)
                .or_else(|| available.first().map(|(i, _)| *i))
        } else {
            original
                .or_else(|| {
                    available
                        .iter()
                        .filter(|(_, o)| o.height.is_some())
                        .min_by_key(|(i, o)| (std::cmp::Reverse(o.height), *i))
                        .map(|(i, _)| *i)
                })
                .or_else(|| available.first().map(|(i, _)| *i))
        }
    }
}

fn named_height(label: &str) -> Option<u32> {
    let label = label.trim().to_ascii_lowercase();
    if label == "4k" {
        return Some(2160);
    }
    label
        .strip_suffix('p')?
        .parse::<u32>()
        .ok()
        .filter(|h| *h > 0)
}

impl FileInfoResponse {
    /// The website hands the visible media URL directly to external players.
    /// A slow first response is not evidence that the stream is unavailable.
    pub fn play_options(&self) -> Vec<PlayOption> {
        let mut options: Vec<_> = self
            .medias
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|m| m.is_visible != Some(false))
            .map(|m| {
                let url = m
                    .link
                    .as_ref()
                    .and_then(|l| l.url.as_deref())
                    .unwrap_or("")
                    .to_owned();
                let reason = if m.need_more_quota == Some(true) {
                    Some("additional playback quota required")
                } else if url.is_empty() {
                    Some("media link not ready; refresh to retry")
                } else {
                    None
                };
                let label = if m.is_origin == Some(true) {
                    "Original".to_owned()
                } else {
                    m.media_name.as_deref().unwrap_or("Unknown").to_owned()
                };
                let height = m
                    .video
                    .as_ref()
                    .and_then(|v| v.height)
                    .and_then(|h| u32::try_from(h).ok())
                    .filter(|h| *h > 0)
                    .or_else(|| named_height(&label));
                (
                    m.is_origin == Some(true),
                    PlayOption {
                        label,
                        url,
                        available: reason.is_none(),
                        unavailable_reason: reason,
                        is_original: m.is_origin == Some(true),
                        height,
                    },
                )
            })
            .collect();
        options.sort_by_key(|(origin, _)| !*origin);
        // Older responses and non-video files can lack media metadata entirely.
        // Do not bypass a hidden or quota-restricted media with a download URL.
        if self.medias.as_ref().is_none_or(|m| m.is_empty())
            && let Some(url) = self.download_url().filter(|u| !u.is_empty())
        {
            options.push((
                true,
                PlayOption {
                    label: "Original".into(),
                    url: url.into(),
                    available: true,
                    unavailable_reason: None,
                    is_original: true,
                    height: None,
                },
            ));
        }
        options.into_iter().map(|(_, option)| option).collect()
    }

    pub fn download_url(&self) -> Option<&str> {
        self.medias
            .as_ref()
            .and_then(|medias| {
                medias.iter().find_map(|media| {
                    (media.is_origin == Some(true)
                        && media.is_visible != Some(false)
                        && media.need_more_quota != Some(true))
                    .then_some(media.link.as_ref()?.url.as_deref())
                    .flatten()
                    .filter(|url| !url.is_empty())
                })
            })
            .or(self
                .web_content_link
                .as_deref()
                .filter(|url| !url.is_empty()))
            .or(self.links.as_ref().and_then(|l| {
                l.get("application/octet-stream")
                    .and_then(|v| v.url.as_deref())
                    .filter(|url| !url.is_empty())
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

#[cfg(test)]
mod tests {
    use super::{FileInfoResponse, PlayOption};

    fn quality_options() -> Vec<PlayOption> {
        serde_json::from_value::<FileInfoResponse>(serde_json::json!({
            "name": "sample-video.mp4",
            "medias": [
                {"is_origin":true,"video":{"height":1080},"link":{"url":"https://example.invalid/original"}},
                {"media_name":"2160P","need_more_quota":true,"link":{"url":"https://example.invalid/restricted"}},
                {"media_name":"Hidden","is_visible":false,"video":{"height":1440},"link":{"url":"https://example.invalid/hidden"}},
                {"media_name":"1080P","link":{"url":"https://example.invalid/1080"}},
                {"media_name":"HD","video":{"height":720},"link":{"url":"https://example.invalid/720"}},
                {"media_name":"480p"},
                {"media_name":"360p","link":{"url":"https://example.invalid/360"}}
            ]
        })).unwrap().play_options()
    }

    #[test]
    fn resolution_preference_uses_metadata_and_names_without_bypassing_permissions() {
        let options = quality_options();
        assert_eq!(options.len(), 6);
        assert_eq!(options[0].display_label(), "Original · 1080p");
        assert_eq!(options[3].height, Some(720));
        assert_eq!(PlayOption::preferred_index(&options, None), Some(0));
        // An explicit 1080p preference selects the transcode, even if source is 1080p.
        assert_eq!(PlayOption::preferred_index(&options, Some(1080)), Some(2));
        // No permitted 2160p; do not use hidden or quota-restricted streams.
        assert_eq!(PlayOption::preferred_index(&options, Some(2160)), Some(2));
        assert_eq!(PlayOption::preferred_index(&options, Some(480)), Some(5));
        assert_eq!(PlayOption::preferred_index(&options, Some(240)), Some(5));
    }

    #[test]
    fn default_quality_falls_back_only_to_available_streams() {
        let mut options = quality_options();
        options[0].available = false;
        assert_eq!(PlayOption::preferred_index(&options, None), Some(2));
        options.retain(|o| o.is_original);
        assert_eq!(PlayOption::preferred_index(&options, Some(720)), None);
        options[0].available = true;
        assert_eq!(PlayOption::preferred_index(&options, Some(720)), Some(0));
        assert_eq!(PlayOption::preferred_index(&[], None), None);
    }

    #[test]
    fn original_media_link_is_preferred_for_download() {
        let info: FileInfoResponse = serde_json::from_str(
            r#"{
                "name": "movie.mkv",
                "size": "6110009965",
                "web_content_link": "https://slow.example/download",
                "medias": [
                    {
                        "media_name": "Original",
                        "is_origin": true,
                        "link": {"url": "https://fast.example/original"}
                    },
                    {
                        "media_name": "1080P",
                        "is_origin": false,
                        "link": {"url": "https://fast.example/transcoded"}
                    }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(info.download_url(), Some("https://fast.example/original"));
    }
}

#[cfg(test)]
mod playback_parity_tests {
    use super::FileInfoResponse;

    #[test]
    fn playback_uses_original_media_and_respects_server_permissions() {
        let info: FileInfoResponse = serde_json::from_value(serde_json::json!({
            "name":"fixture.mkv", "web_content_link":"https://slow.invalid/download",
            "medias":[
                {"media_name":"720p","is_visible":true,"link":{"url":"https://media.invalid/720"}},
                {"media_id":"original","is_origin":true,"is_visible":true,"link":{"url":"https://media.invalid/original","expire":"later","fallbacks":[]}},
                {"media_name":"Hidden","is_visible":false,"link":{"url":"https://media.invalid/hidden"}},
                {"media_name":"Quota","is_visible":true,"need_more_quota":true,"link":{"url":"https://media.invalid/quota"}},
                {"media_name":"Preparing","is_visible":true,"link":{"url":""}}
            ]
        })).unwrap();
        let options = info.play_options();
        assert_eq!(options.len(), 4);
        assert_eq!(options[0].url, "https://media.invalid/original");
        assert!(options[0].available && options[1].available);
        assert!(options[2].unavailable_reason.unwrap().contains("quota"));
        assert!(options[3].unavailable_reason.unwrap().contains("not ready"));
        assert_eq!(
            info.medias.unwrap()[1].media_id.as_deref(),
            Some("original")
        );
    }

    #[test]
    fn playback_cannot_bypass_hidden_original_with_download_link() {
        let info: FileInfoResponse = serde_json::from_str(r#"{"name":"fixture","web_content_link":"https://download.invalid/file","medias":[{"is_origin":true,"is_visible":false,"link":{"url":"https://media.invalid/original"}}]}"#).unwrap();
        assert!(info.play_options().is_empty());
    }

    #[test]
    fn empty_download_link_does_not_hide_legacy_link_fallback() {
        let info: FileInfoResponse = serde_json::from_str(r#"{"name":"fixture","web_content_link":"","links":{"application/octet-stream":{"url":"https://download.invalid/fallback"}}}"#).unwrap();
        assert_eq!(
            info.play_options()[0].url,
            "https://download.invalid/fallback"
        );
    }

    #[test]
    fn legacy_response_without_medias_can_still_play() {
        let info: FileInfoResponse = serde_json::from_str(
            r#"{"name":"fixture","web_content_link":"https://download.invalid/file"}"#,
        )
        .unwrap();
        assert_eq!(info.play_options()[0].url, "https://download.invalid/file");
    }
}
