use anyhow::{Result, anyhow};

use super::{PikPak, QuotaInfo, TransferQuotaResponse, VipInfoResponse, json_or_api_error};

impl PikPak {
    pub fn quota(&self) -> Result<QuotaInfo> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/about");

        let rb = self.http.get(&url).bearer_auth(&token);
        let response = self.send_authed("quota", rb)?;
        json_or_api_error(response, "quota")
    }

    pub fn vip_info(&self) -> Result<VipInfoResponse> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/privilege/vip");

        let rb = self.http.get(&url).bearer_auth(&token);
        let response = self.send_authed("vip info", rb)?;
        json_or_api_error(response, "vip info")
    }

    pub fn invite_code(&self) -> Result<String> {
        let token = self.access_token()?;
        let url = self.drive_url("vip/v1/activity/inviteCode");

        let rb = self.http.get(&url).bearer_auth(&token);
        let response = self.send_authed("invite code", rb)?;
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

        let rb = self.http.get(&url).bearer_auth(&token);
        let response = self.send_authed("user info", rb)?;
        json_or_api_error(response, "user info")
    }

    pub fn transfer_quota(&self) -> Result<TransferQuotaResponse> {
        let token = self.access_token()?;
        let url = self.drive_url("vip/v1/quantity/list");

        let rb = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .query(&[("type", "transfer")]);
        let response = self.send_authed("transfer quota", rb)?;
        json_or_api_error(response, "transfer quota")
    }
}
