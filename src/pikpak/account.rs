use anyhow::{Context, Result, anyhow};

use super::{PikPak, QuotaInfo, TransferQuotaResponse, VipInfoResponse, json_or_api_error};

impl PikPak {
    pub fn quota(&self) -> Result<QuotaInfo> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/about");

        let mut rb = self.http.get(&url).bearer_auth(&token);
        rb = self.authed_headers(rb);

        let response = rb.send().context("quota request failed")?;
        json_or_api_error(response, "quota")
    }

    pub fn vip_info(&self) -> Result<VipInfoResponse> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/privilege/vip");

        let mut rb = self.http.get(&url).bearer_auth(&token);
        rb = self.authed_headers(rb);

        let response = rb.send().context("vip info request failed")?;
        json_or_api_error(response, "vip info")
    }

    pub fn invite_code(&self) -> Result<String> {
        let token = self.access_token()?;
        let url = self.drive_url("vip/v1/activity/inviteCode");

        let mut rb = self.http.get(&url).bearer_auth(&token);
        rb = self.authed_headers(rb);

        let response = rb.send().context("invite code request failed")?;
        let data: serde_json::Value = json_or_api_error(response, "invite code")?;
        data["code"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("no invite code in response"))
    }

    /// Account identity from the auth host (rclone's getUserInfo endpoint).
    pub fn user_info(&self) -> Result<serde_json::Value> {
        let token = self.access_token()?;
        let url = self.auth_url("v1/user/me");

        let mut rb = self.http.get(&url).bearer_auth(&token);
        rb = self.authed_headers(rb);

        let response = rb.send().context("user info request failed")?;
        json_or_api_error(response, "user info")
    }

    pub fn transfer_quota(&self) -> Result<TransferQuotaResponse> {
        let token = self.access_token()?;
        let url = self.drive_url("vip/v1/quantity/list");

        let mut rb = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .query(&[("type", "transfer")]);
        rb = self.authed_headers(rb);

        let response = rb.send().context("transfer quota request failed")?;
        json_or_api_error(response, "transfer quota")
    }
}
