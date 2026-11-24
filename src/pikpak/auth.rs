use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct SigninResponse {
    pub(super) access_token: String,
    pub(super) refresh_token: String,
    pub(super) expires_in: u64,
    /// The account's user id; the captcha refresh meta wants it.
    #[serde(default)]
    pub(super) sub: String,
}

#[cfg(pikpak_android_jni)]
#[derive(Deserialize)]
pub(super) struct RefreshResponse {
    pub(super) access_token: String,
    pub(super) refresh_token: Option<String>,
    pub(super) expires_in: u64,
    #[serde(default)]
    pub(super) sub: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct CaptchaInitResponse {
    #[serde(default)]
    pub(super) captcha_token: Option<String>,
    #[serde(default)]
    pub(super) expires_in: u64,
    #[serde(default)]
    pub(super) url: Option<String>,
}

impl CaptchaInitResponse {
    #[cfg(not(pikpak_android_jni))]
    pub(super) fn login_token(self) -> anyhow::Result<String> {
        self.captcha_token
            // An empty token would sail through and fail signin with an opaque
            // 4xx, and it would shadow the documented env escape hatch.
            .filter(|t| !t.is_empty())
            .or_else(|| {
                std::env::var("PIKPAK_CAPTCHA_TOKEN")
                    .ok()
                    .filter(|t| !t.is_empty())
            })
            .ok_or_else(|| {
                let hint = self.url.as_deref().unwrap_or("<no challenge url>");
                anyhow::anyhow!(
                    "captcha token unavailable; set PIKPAK_CAPTCHA_TOKEN. url={}",
                    super::sanitize(hint)
                )
            })
    }

    #[cfg(pikpak_android_jni)]
    pub(super) fn login_token(self) -> anyhow::Result<String> {
        self.captcha_token
            .filter(|value| !value.is_empty())
            .ok_or_else(|| match self.url {
                Some(url) => anyhow::anyhow!(super::LoginChallenge(url)),
                None => anyhow::anyhow!("captcha token is unavailable"),
            })
    }
}

#[cfg(all(test, pikpak_android_jni))]
#[test]
fn jni_captcha_reply_keeps_the_verification_url_only_for_a_challenge() {
    let decode = |body| serde_json::from_str::<CaptchaInitResponse>(body).unwrap();
    let challenge = decode(r#"{"captcha_token":"","url":"https://example.invalid/v"}"#);
    assert_eq!(
        crate::error_payload(challenge.login_token().unwrap_err()),
        serde_json::json!({
            "code":"challenge", "error":"PikPak requires verification",
            "challenge":"https://example.invalid/v"
        })
    );
    let ready = decode(r#"{"captcha_token":"ready","url":"https://example.invalid/v"}"#);
    assert_eq!(ready.login_token().unwrap(), "ready");
    let missing = crate::error_payload(decode("{}").login_token().unwrap_err());
    assert_eq!(missing["code"], "restricted");
    assert!(missing.get("challenge").is_none());
}
