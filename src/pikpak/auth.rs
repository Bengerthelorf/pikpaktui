use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct SigninResponse {
    pub(super) access_token: String,
    pub(super) refresh_token: String,
    pub(super) expires_in: u64,
}

#[derive(Debug, Deserialize)]
pub(super) struct CaptchaInitResponse {
    #[serde(default)]
    pub(super) captcha_token: Option<String>,
    #[serde(default)]
    pub(super) url: Option<String>,
}
