use anyhow::{Context, Result, anyhow};

use super::{
    CreateShareResponse, MyShare, PikPak, ShareInfoResponse, ShareListResponse, ensure_success,
    sanitize,
};

impl PikPak {
    pub fn share_info(&self, share_id: &str, pass_code: &str) -> Result<ShareInfoResponse> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/share");

        let mut rb = self.http.get(&url).bearer_auth(&token).query(&[
            ("share_id", share_id),
            ("pass_code", pass_code),
            ("thumbnail_size", "SIZE_MEDIUM"),
        ]);
        rb = self.authed_headers(rb);

        let response = rb.send().context("share info request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!(
                "share info failed ({}): {}",
                status,
                sanitize(&body)
            ));
        }

        let info: ShareInfoResponse = response.json().context("invalid share info json")?;
        if info.share_status != "OK" {
            return Err(anyhow!(
                "share is not available (status: {})",
                info.share_status
            ));
        }
        Ok(info)
    }

    pub fn save_share(
        &self,
        share_id: &str,
        pass_code_token: &str,
        file_ids: &[&str],
        to_parent_id: &str,
    ) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/share/restore");

        let payload = serde_json::json!({
            "share_id": share_id,
            "pass_code_token": pass_code_token,
            "file_ids": file_ids,
            "to": { "parent_id": to_parent_id },
        });

        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("save share request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            if body.contains("file_restore_own") {
                return Err(anyhow!(
                    "cannot save: these files already belong to your account"
                ));
            }
            return Err(anyhow!(
                "save share failed ({}): {}",
                status,
                sanitize(&body)
            ));
        }
        self.clear_ls_cache();
        Ok(())
    }

    pub fn create_share(
        &self,
        file_ids: &[&str],
        need_password: bool,
        expiration_days: i64,
    ) -> Result<CreateShareResponse> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/share");

        let payload = serde_json::json!({
            "file_ids": file_ids,
            "share_to": if need_password { "encryptedlink" } else { "publiclink" },
            "expiration_days": expiration_days,
            "pass_code_option": if need_password { "REQUIRED" } else { "NOT_REQUIRED" },
        });

        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("create share request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!(
                "create share failed ({}): {}",
                status,
                sanitize(&body)
            ));
        }
        response.json().context("invalid create share response")
    }

    pub fn list_shares(&self) -> Result<Vec<MyShare>> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/share/list");

        let mut rb = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .query(&[("limit", "100"), ("thumbnail_size", "SIZE_SMALL")]);
        rb = self.authed_headers(rb);

        let response = rb.send().context("list shares request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!(
                "list shares failed ({}): {}",
                status,
                sanitize(&body)
            ));
        }
        let resp: ShareListResponse = response.json().context("invalid share list json")?;
        Ok(resp.data)
    }

    pub fn delete_shares(&self, share_ids: &[&str]) -> Result<()> {
        let token = self.access_token()?;
        let url = self.drive_url("drive/v1/share:batchDelete");

        let payload = serde_json::json!({ "ids": share_ids });

        let mut rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        rb = self.authed_headers(rb);

        let response = rb.send().context("delete shares request failed")?;
        ensure_success(response, "delete shares")
    }
}
