use std::collections::HashSet;

use anyhow::{Context, Result, bail, ensure};
use reqwest::blocking::RequestBuilder;
use serde::Deserialize;
use serde_json::{Value, json};
use time::format_description::well_known::Rfc3339;

use super::{PikPak, ensure_success, json_or_api_error};

pub(crate) fn share_expiry(input: &str) -> Result<String> {
    if input == "-1" {
        return Ok(input.to_owned());
    }
    time::OffsetDateTime::parse(input, &Rfc3339)
        .context("use an RFC 3339 expiration timestamp or -1 for permanent")?;
    Ok(input.to_owned())
}

pub(crate) fn share_link(url: &str, view: Option<&str>, order: Option<&str>) -> Result<String> {
    if view.is_none() && order.is_none() {
        return Ok(url.to_owned());
    }
    ensure!(
        view.is_none_or(|v| matches!(v, "list" | "grid" | "flow")),
        "view must be list, grid or flow"
    );
    ensure!(
        order.is_none_or(|v| matches!(
            v,
            "name_asc" | "name_desc" | "mod_asc" | "mod_desc" | "size_asc" | "size_desc"
        )),
        "unsupported order (use name/mod/size with _asc or _desc)"
    );
    let mut parsed = reqwest::Url::parse(url).context("invalid share URL")?;
    ensure!(
        matches!(parsed.scheme(), "https" | "http"),
        "expected an HTTP share link"
    );
    let kept: Vec<_> = parsed
        .query_pairs()
        .filter(|(key, _)| !(view.is_some() && key == "view" || order.is_some() && key == "order"))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let mut pairs = parsed.query_pairs_mut();
    pairs.clear().extend_pairs(kept);
    if let Some(v) = view {
        pairs.append_pair("view", v);
    }
    if let Some(v) = order {
        pairs.append_pair("order", v);
    }
    drop(pairs);
    Ok(parsed.to_string())
}

pub(crate) enum ShareFeed<'a> {
    Following,
    Updates(Option<&'a str>),
}

impl PikPak {
    fn share_management_json(&self, op: &str, rb: RequestBuilder) -> Result<Value> {
        json_or_api_error(
            self.send_authed(op, rb.bearer_auth(self.access_token()?))?,
            op,
        )
    }

    pub fn edit_share(
        &self,
        id: &str,
        expiry: Option<&str>,
        password: Option<&str>,
        forbid_follow: Option<bool>,
    ) -> Result<()> {
        ensure!(
            expiry.is_some() || password.is_some() || forbid_follow.is_some(),
            "provide an expiration, password or follow setting"
        );
        let mut body = json!({"share_id": id});
        if let Some(forbid) = forbid_follow {
            body["forbid_follow"] = json!(forbid);
        }
        if let Some(expiry) = expiry {
            body["expiration_at"] = json!(share_expiry(expiry)?);
        }
        if let Some(password) = password {
            if password.is_empty() {
                body["pass_code_option"] = json!("NOT_REQUIRED");
            } else {
                body["pass_code_option"] = json!("REQUIRED");
                body["custom_pass_code"] = json!(password);
            }
        }
        let rb = self
            .http
            .patch(self.drive_url("drive/v1/share"))
            .bearer_auth(self.access_token()?)
            .json(&body);
        ensure_success(self.send_authed("edit share", rb)?, "edit share")
    }

    pub fn share_file_status(&self, ids: &[&str]) -> Result<Value> {
        ensure!(!ids.is_empty(), "select at least one file");
        let body = json!({"file_ids": ids});
        let rb = self
            .http
            .post(self.drive_url("drive/v1/share/status/files"))
            .json(&body);
        self.share_management_json("share file status", rb)?
            .get("file_statuses")
            .cloned()
            .context("missing file_statuses")
    }

    pub fn share_code_info(&self, share_id: &str) -> Result<Value> {
        let rb = self
            .http
            .get(self.drive_url("phrase/v1/content/info/share"))
            .query(&[("id", share_id)]);
        self.share_management_json("share code info", rb)
    }

    pub fn set_share_code(&self, share_id: &str, old: Option<&str>, new: &str) -> Result<()> {
        ensure!(
            old.is_some() || !new.trim().is_empty(),
            "enter a nonempty share code"
        );
        ensure!(
            old.is_none_or(|value| !value.trim().is_empty() && value != new),
            "provide the existing code and a different replacement"
        );
        let url = self.drive_url("phrase/v1/content/share");
        let rb = match old {
            None => self
                .http
                .post(url)
                .json(&json!({"type":"share","content":share_id,"phrase":new})),
            Some(old) => self.http.patch(url).json(
                &json!({"type":"share","content":share_id,"old_phrase":old,"new_phrase":new}),
            ),
        }
        .bearer_auth(self.access_token()?);
        ensure_success(self.send_authed("set share code", rb)?, "set share code")
    }

    pub fn save_update_selection(
        &self,
        share_id: &str,
        record: u64,
        selected: &[&str],
        pass_code: &str,
        parent: &str,
        dry_run: bool,
    ) -> Result<()> {
        ensure!(!selected.is_empty(), "select at least one file");
        let feed = self.share_feed(ShareFeed::Updates(Some(share_id)))?;
        ensure!(
            feed.iter()
                .any(|row| row["share_id"].as_str() == Some(share_id)
                    && (row["auto_id"].as_u64() == Some(record)
                        || row["auto_id"].as_str().and_then(|id| id.parse().ok()) == Some(record))),
            "record does not belong to the selected share"
        );
        let ids = self.update_file_ids(record)?;
        ensure!(
            selected.iter().all(|id| ids.iter().any(|i| i == id)),
            "selection contains a file outside this update"
        );
        let info = self.share_info(share_id, pass_code)?;
        if !dry_run {
            self.save_share(share_id, &info.pass_code_token, selected, parent)?;
        }
        Ok(())
    }

    pub fn resolve_share_code(&self, phrase: &str) -> Result<Vec<String>> {
        #[derive(Deserialize)]
        struct Match {
            content: String,
        }
        #[derive(Deserialize)]
        struct Matches {
            data: Vec<Match>,
        }
        let rb = self
            .http
            .get(self.drive_url("phrase/v1/content/list"))
            .query(&[("type", "share"), ("phrase", &phrase.trim().to_lowercase())]);
        let response: Matches =
            serde_json::from_value(self.share_management_json("resolve share code", rb)?)?;
        let mut links = Vec::new();
        for row in response.data {
            let id = row
                .content
                .trim_start_matches("/s/")
                .split('?')
                .next()
                .unwrap_or("");
            ensure!(
                !id.is_empty()
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
                "invalid share code result"
            );
            links.push(format!(
                "https://mypikpak.com/s/{}",
                row.content.trim_start_matches("/s/")
            ));
        }
        ensure!(!links.is_empty(), "share code has no matches");
        Ok(links)
    }

    pub fn set_share_followed(&self, id: &str, pass_code: &str, follow: bool) -> Result<()> {
        let (path, body) = if follow {
            let info = self.share_info(id, pass_code)?;
            (
                "drive/v1/share/follow",
                json!({"share_id":id,"space":"","pass_code":pass_code,"pass_code_token":info.pass_code_token}),
            )
        } else {
            (
                "drive/v1/share/unfollow",
                json!({"share_ids":[id],"space":""}),
            )
        };
        let rb = self
            .http
            .put(self.drive_url(path))
            .bearer_auth(self.access_token()?)
            .json(&body);
        ensure_success(
            self.send_authed("change followed shares", rb)?,
            "change followed shares",
        )
    }

    pub(crate) fn share_feed(&self, feed: ShareFeed<'_>) -> Result<Vec<Value>> {
        let (path, key, id) = match feed {
            ShareFeed::Following => ("share_updates/v1/list_follow", "follow_record", None),
            ShareFeed::Updates(None) => ("share_updates/v1/review", "update_records", None),
            ShareFeed::Updates(Some(id)) => {
                ("share_updates/v1/review/all", "update_records", Some(id))
            }
        };
        let mut token = 0;
        let mut seen = HashSet::new();
        let mut rows = Vec::new();
        loop {
            let rb = self
                .http
                .get(self.drive_url(path))
                .query(&[("page_token", token)]);
            let rb = if let Some(id) = id {
                rb.query(&[("share_id", id)])
            } else {
                rb
            };
            let mut page = self.share_management_json("share feed", rb)?;
            if let Some(records) = page.get_mut(key) {
                rows.append(
                    records
                        .as_array_mut()
                        .context("invalid share feed records")?,
                );
            }
            let next = match page.get("page_token") {
                Some(Value::String(s)) if !s.is_empty() => {
                    s.parse::<u64>().context("invalid share feed page token")?
                }
                Some(Value::Number(n)) => n.as_u64().context("invalid share feed page token")?,
                None | Some(Value::Null | Value::String(_)) => 0,
                _ => bail!("invalid share feed page token"),
            };
            if next == 0 || next == token {
                break;
            }
            ensure!(seen.insert(next), "repeated share feed page token");
            token = next;
        }
        Ok(rows)
    }

    pub fn update_file_ids(&self, record: u64) -> Result<Vec<String>> {
        let rb = self
            .http
            .get(self.drive_url("share_updates/v1/update_record/file_ids"))
            .query(&[("ids", record)]);
        let response = self.share_management_json("share update files", rb)?;
        let ids = response["dataMap"]
            .get(record.to_string())
            .context("missing file IDs for the selected update")?;
        Ok(serde_json::from_value(ids.clone())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::pikpak::parity_tests::scripted_server;
    use crate::pikpak::tests::{temp_test_dir, test_client};

    #[test]
    fn reads_all_follow_pages_and_resolves_lowercase_codes() {
        let (base, requests, handle) = scripted_server(vec![
            (
                200,
                r#"{"follow_record":[{"share_id":"a"}],"page_token":"9"}"#,
            ),
            (
                200,
                r#"{"follow_record":[{"share_id":"b"}],"page_token":0}"#,
            ),
            (200, r#"{"data":[{"content":"Ab0?pass_code=1"}]}"#),
        ]);
        let dir = temp_test_dir("share-follow-pages");
        let client = test_client(base, dir.join("session.json"));
        let rows = client.share_feed(ShareFeed::Following).unwrap();
        assert_eq!(rows, vec![json!({"share_id":"a"}), json!({"share_id":"b"})]);
        assert_eq!(
            client.resolve_share_code("ExAmPle").unwrap(),
            vec!["https://mypikpak.com/s/Ab0?pass_code=1"]
        );
        handle.join().unwrap();
        let captured = requests.lock().unwrap();
        assert!(captured[0].contains("list_follow?page_token=0"));
        assert!(captured[1].contains("list_follow?page_token=9"));
        assert!(captured[2].contains("phrase/v1/content/list?type=share&phrase=example"));
    }

    #[test]
    fn manages_individual_short_codes_without_replacing_siblings() {
        let (base, requests, handle) = scripted_server(vec![(200, "{}"), (200, "{}"), (200, "{}")]);
        let dir = temp_test_dir("shared-short-code");
        let client = test_client(base, dir.join("session.json"));
        client.set_share_code("one", None, "first").unwrap();
        client
            .set_share_code("one", Some("first"), "second")
            .unwrap();
        client.set_share_code("one", Some("second"), "").unwrap();
        handle.join().unwrap();
        let captured = requests.lock().unwrap();
        let payload =
            |r: &str| serde_json::from_str::<Value>(r.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert!(captured[0].starts_with("POST /phrase/v1/content/share "));
        assert!(
            captured[1..]
                .iter()
                .all(|r| r.starts_with("PATCH /phrase/v1/content/share "))
        );
        assert_eq!(
            payload(&captured[0]),
            json!({"type":"share","content":"one","phrase":"first"})
        );
        assert_eq!(
            payload(&captured[1]),
            json!({"type":"share","content":"one","old_phrase":"first","new_phrase":"second"})
        );
        assert_eq!(payload(&captured[2])["new_phrase"], "");
    }

    #[test]
    fn editing_expiration_does_not_change_password_or_follow_permissions() {
        let (base, requests, handle) = scripted_server(vec![(204, ""), (204, "")]);
        let dir = temp_test_dir("share-patch");
        let client = test_client(base, dir.join("session.json"));
        client.edit_share("mine", Some("-1"), None, None).unwrap();
        client.edit_share("mine", None, Some(""), None).unwrap();
        handle.join().unwrap();
        let captured = requests.lock().unwrap();
        let payload = |request: &str| {
            serde_json::from_str::<Value>(request.split("\r\n\r\n").nth(1).unwrap()).unwrap()
        };
        assert!(captured[0].starts_with("PATCH /drive/v1/share "));
        assert_eq!(
            payload(&captured[0]),
            json!({"share_id":"mine","expiration_at":"-1"})
        );
        assert_eq!(
            payload(&captured[1]),
            json!({"share_id":"mine","pass_code_option":"NOT_REQUIRED"})
        );
    }

    #[test]
    fn update_save_requires_record_and_file_membership_before_a_write() {
        let (base, requests, handle) = scripted_server(vec![
            (
                200,
                r#"{"update_records":[{"share_id":"other","auto_id":17}]}"#,
            ),
            (
                200,
                r#"{"update_records":[{"share_id":"mine","auto_id":"17"}]}"#,
            ),
            (200, r#"{"dataMap":{"17":["member"]}}"#),
        ]);
        let dir = temp_test_dir("share-selected-update");
        let client = test_client(base, dir.join("session.json"));
        assert!(
            client
                .save_update_selection("mine", 17, &["member"], "", "", false)
                .unwrap_err()
                .to_string()
                .contains("record")
        );
        assert!(
            client
                .save_update_selection("mine", 17, &["other"], "", "", false)
                .unwrap_err()
                .to_string()
                .contains("outside")
        );
        handle.join().unwrap();
        assert!(
            requests
                .lock()
                .unwrap()
                .iter()
                .all(|request| request.starts_with("GET "))
        );
    }

    #[test]
    fn link_preserves_access_code_and_replaces_only_requested_settings() {
        let link = share_link(
            "https://mypikpak.com/s/ABC?pass_code=1%202&order=name_desc&view=list#root",
            Some("grid"),
            None,
        )
        .unwrap();
        assert_eq!(
            link,
            "https://mypikpak.com/s/ABC?pass_code=1+2&order=name_desc&view=grid#root"
        );
        assert!(share_link(&link, None, Some("bad")).is_err());
        assert!(share_expiry("tomorrow").is_err());
    }
}
