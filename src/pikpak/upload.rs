use anyhow::{Context, Result, anyhow};
use base64::Engine as _;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha1::Sha1;
use std::fmt::Write as _;
use std::fs;
use std::io::Read as _;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{PikPak, sanitize};

impl PikPak {
    pub fn upload_file(
        &self,
        parent_id: Option<&str>,
        local_path: &Path,
    ) -> Result<(String, bool)> {
        let file_name = local_path
            .file_name()
            .ok_or_else(|| anyhow!("invalid file path"))?
            .to_string_lossy()
            .to_string();

        let meta = fs::symlink_metadata(local_path)
            .with_context(|| format!("cannot stat '{}'", local_path.display()))?;
        if meta.file_type().is_symlink() {
            return Err(anyhow!(
                "refusing to upload symbolic link file '{}'",
                local_path.display()
            ));
        }
        if !meta.is_file() {
            return Err(anyhow!("not a regular file: '{}'", local_path.display()));
        }
        let file_size = meta.len();

        let hash = pikpak_hash(local_path)?;

        let url = self.drive_url("drive/v1/files");
        let mut payload = serde_json::json!({
            "kind": "drive#file",
            "name": file_name,
            "size": file_size.to_string(),
            "hash": hash,
            "upload_type": "UPLOAD_TYPE_RESUMABLE",
            "objProvider": { "provider": "UPLOAD_TYPE_UNKNOWN" },
        });
        if let Some(pid) = parent_id {
            payload["parent_id"] = serde_json::json!(pid);
        }

        let token = self.access_token()?;
        let rb = self.http.post(&url).bearer_auth(&token).json(&payload);
        let response = self.send_authed("upload init", rb)?;
        let init: UploadInitResponse = response.json().context("invalid upload init json")?;

        // Instant completion (hash dedup): the server already had this content,
        // so there's nothing to upload.
        if init.file.phase.as_deref() == Some("PHASE_TYPE_COMPLETE") {
            self.clear_ls_cache();
            return Ok((file_name, true));
        }

        let resumable = init
            .resumable
            .ok_or_else(|| anyhow!("no resumable context in upload init response"))?;

        let oss_args = OssArgs {
            endpoint: resumable
                .params
                .endpoint
                .clone()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("upload init response missing OSS endpoint"))?,
            access_key_id: resumable.params.access_key_id.clone().unwrap_or_default(),
            access_key_secret: resumable
                .params
                .access_key_secret
                .clone()
                .unwrap_or_default(),
            security_token: resumable.params.security_token.clone().unwrap_or_default(),
            bucket: resumable
                .params
                .bucket
                .clone()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("upload init response missing OSS bucket"))?,
            key: resumable
                .params
                .key
                .clone()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("upload init response missing OSS key"))?,
        };

        let upload_id = self.oss_initiate_multipart(&oss_args)?;
        let etags = self.oss_upload_chunks(&oss_args, &upload_id, local_path, file_size)?;
        self.oss_complete_multipart(&oss_args, &upload_id, &etags)?;

        self.clear_ls_cache();
        Ok((file_name, false))
    }

    pub fn upload_dir(&self, parent_id: &str, local_dir: &Path) -> Result<(usize, usize)> {
        let local_meta = fs::symlink_metadata(local_dir)
            .with_context(|| format!("cannot stat '{}'", local_dir.display()))?;
        if local_meta.file_type().is_symlink() {
            return Err(anyhow!(
                "refusing to upload symbolic link directory '{}'",
                local_dir.display()
            ));
        }
        if !local_meta.is_dir() {
            return Err(anyhow!("not a directory: '{}'", local_dir.display()));
        }
        let name = local_dir
            .file_name()
            .ok_or_else(|| anyhow!("directory has no name"))?
            .to_string_lossy();
        let folder = self.mkdir(parent_id, &name)?;
        self.upload_dir_inner(&folder.id, local_dir)
    }

    fn upload_dir_inner(&self, parent_id: &str, local_dir: &Path) -> Result<(usize, usize)> {
        let mut ok = 0usize;
        let mut failed = 0usize;
        let entries = std::fs::read_dir(local_dir)
            .with_context(|| format!("cannot read dir: {}", local_dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(e) => {
                    eprintln!("  [error] cannot inspect '{}': {e}", path.display());
                    failed += 1;
                    continue;
                }
            };
            if file_type.is_symlink() {
                eprintln!("  [skip] symbolic link '{}'", path.display());
                failed += 1;
            } else if file_type.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                match self.mkdir(parent_id, &name) {
                    Ok(sub) => {
                        let (sub_ok, sub_fail) = self.upload_dir_inner(&sub.id, &path)?;
                        ok += sub_ok;
                        failed += sub_fail;
                    }
                    Err(e) => {
                        // The whole subtree is skipped, so count its files —
                        // "1 failed" for a folder of 200 would misreport badly.
                        eprintln!("  [error] mkdir '{}': {:#}", name, e);
                        failed += Self::count_files_in(&path).max(1);
                    }
                }
            } else if file_type.is_file() {
                match self.upload_file(Some(parent_id), &path) {
                    Ok(_) => ok += 1,
                    Err(e) => {
                        eprintln!("  [error] '{}': {:#}", path.display(), e);
                        failed += 1;
                    }
                }
            }
        }
        Ok((ok, failed))
    }

    fn count_files_in(dir: &Path) -> usize {
        std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| match e.file_type() {
                        Ok(file_type) if file_type.is_dir() => Self::count_files_in(&e.path()),
                        Ok(file_type) if file_type.is_file() => 1,
                        _ => 0,
                    })
                    .sum()
            })
            .unwrap_or(0)
    }

    fn oss_initiate_multipart(&self, oss: &OssArgs) -> Result<String> {
        let date = httpdate_now();
        let auth = oss_hmac_auth(
            "POST",
            &date,
            &oss.security_token,
            &oss.access_key_id,
            &oss.access_key_secret,
            &format!("/{}/{}", oss.bucket, oss.key),
            "application/octet-stream",
            "?uploads",
        );

        let url = format!(
            "https://{}/{}?uploads",
            oss.endpoint.trim_end_matches('/'),
            oss.key
        );
        let response = self
            .http
            .post(&url)
            .header("Authorization", auth)
            .header("Date", &date)
            .header("Content-Type", "application/octet-stream")
            .header("x-oss-security-token", &oss.security_token)
            .send()
            .context("OSS initiate multipart failed")?;

        let status = response.status();
        let body = response.text().unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "OSS initiate multipart failed ({}): {}",
                status,
                sanitize(&body)
            ));
        }

        extract_xml_tag(&body, "UploadId")
            .ok_or_else(|| anyhow!("no UploadId in initiate multipart response"))
    }

    fn oss_upload_chunks(
        &self,
        oss: &OssArgs,
        upload_id: &str,
        local_path: &Path,
        file_size: u64,
    ) -> Result<Vec<String>> {
        const CHUNK_SIZE: u64 = 10 * 1024 * 1024;

        let mut file = fs::File::open(local_path)
            .with_context(|| format!("cannot open '{}'", local_path.display()))?;

        let num_parts = if file_size == 0 {
            1
        } else {
            file_size.div_ceil(CHUNK_SIZE)
        };

        let mut etags = Vec::new();

        for part_num in 1..=num_parts {
            let remaining = if file_size == 0 {
                0
            } else {
                std::cmp::min(CHUNK_SIZE, file_size - (part_num - 1) * CHUNK_SIZE)
            };

            let mut buf = vec![0u8; remaining as usize];
            file.read_exact(&mut buf)?;

            let date = httpdate_now();
            let auth = oss_hmac_auth(
                "PUT",
                &date,
                &oss.security_token,
                &oss.access_key_id,
                &oss.access_key_secret,
                &format!("/{}/{}", oss.bucket, oss.key),
                "application/octet-stream",
                &format!("?partNumber={}&uploadId={}", part_num, upload_id),
            );

            let url = format!(
                "https://{}/{}?partNumber={}&uploadId={}",
                oss.endpoint.trim_end_matches('/'),
                oss.key,
                part_num,
                upload_id
            );
            let response = self
                .http
                .put(&url)
                .header("Authorization", auth)
                .header("Date", &date)
                .header("Content-Type", "application/octet-stream")
                .header("x-oss-security-token", &oss.security_token)
                .body(buf)
                .send()
                .with_context(|| format!("OSS upload part {} failed", part_num))?;

            let status = response.status();
            if !status.is_success() {
                let body = response.text().unwrap_or_default();
                return Err(anyhow!(
                    "OSS upload part {} failed ({}): {}",
                    part_num,
                    status,
                    sanitize(&body)
                ));
            }

            // Fail at the part that lost its ETag: an empty one silently
            // recorded here only surfaces later as a confusing
            // CompleteMultipartUpload error (or a corrupt accepted object).
            let etag = response
                .headers()
                .get("ETag")
                .and_then(|v| v.to_str().ok())
                .filter(|t| !t.is_empty())
                .ok_or_else(|| anyhow!("OSS upload part {} returned no ETag", part_num))?
                .to_string();

            etags.push(etag);
        }

        Ok(etags)
    }

    fn oss_complete_multipart(
        &self,
        oss: &OssArgs,
        upload_id: &str,
        etags: &[String],
    ) -> Result<()> {
        let mut xml = String::from("<CompleteMultipartUpload>");
        for (i, etag) in etags.iter().enumerate() {
            xml.push_str(&format!(
                "<Part><PartNumber>{}</PartNumber><ETag>{}</ETag></Part>",
                i + 1,
                etag
            ));
        }
        xml.push_str("</CompleteMultipartUpload>");

        let date = httpdate_now();
        let auth = oss_hmac_auth(
            "POST",
            &date,
            &oss.security_token,
            &oss.access_key_id,
            &oss.access_key_secret,
            &format!("/{}/{}", oss.bucket, oss.key),
            "application/octet-stream",
            &format!("?uploadId={}", upload_id),
        );

        let url = format!(
            "https://{}/{}?uploadId={}",
            oss.endpoint.trim_end_matches('/'),
            oss.key,
            upload_id
        );
        let response = self
            .http
            .post(&url)
            .header("Authorization", auth)
            .header("Date", &date)
            .header("Content-Type", "application/octet-stream")
            .header("x-oss-security-token", &oss.security_token)
            .body(xml)
            .send()
            .context("OSS complete multipart failed")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!(
                "OSS complete multipart failed ({}): {}",
                status,
                sanitize(&body)
            ));
        }
        Ok(())
    }
}

/// Compute the PikPak proprietary file hash for upload deduplication.
/// Algorithm: chunk the file, SHA1 each chunk, concatenate hex hashes, SHA1 the result.
/// Chunk sizes follow PikPak's server-side spec (reverse-engineered from the Android client):
///   < 128 MB  -> 256 KB chunks
///   < 256 MB  -> 512 KB chunks
///   < 512 MB  -> 1 MB chunks
///   >= 512 MB -> 2 MB chunks
pub fn pikpak_hash(path: &Path) -> Result<String> {
    use sha1::Digest;

    let meta = fs::metadata(path).with_context(|| format!("cannot stat '{}'", path.display()))?;
    let file_size = meta.len();

    let chunk_size: u64 = if file_size < 128 * 1024 * 1024 {
        256 * 1024
    } else if file_size < 256 * 1024 * 1024 {
        512 * 1024
    } else if file_size < 512 * 1024 * 1024 {
        1024 * 1024
    } else {
        2 * 1024 * 1024
    };

    let mut file =
        fs::File::open(path).with_context(|| format!("cannot open '{}'", path.display()))?;

    let mut all_hashes = String::new();
    let mut remaining = file_size;

    while remaining > 0 {
        let to_read = std::cmp::min(chunk_size, remaining) as usize;
        let mut buf = vec![0u8; to_read];
        file.read_exact(&mut buf)?;

        let mut hasher = Sha1::new();
        hasher.update(&buf);
        let hash = hasher.finalize();
        for b in hash.iter() {
            write!(all_hashes, "{:02X}", b).unwrap();
        }

        remaining -= to_read as u64;
    }

    if file_size == 0 {
        let mut hasher = Sha1::new();
        hasher.update(b"");
        let hash = hasher.finalize();
        for b in hash.iter() {
            write!(all_hashes, "{:02X}", b).unwrap();
        }
    }

    let mut final_hasher = Sha1::new();
    final_hasher.update(all_hashes.as_bytes());
    let final_hash = final_hasher.finalize();
    let mut hex = String::with_capacity(40);
    for b in final_hash.iter() {
        write!(hex, "{:02X}", b).unwrap();
    }

    Ok(hex)
}

#[allow(clippy::too_many_arguments)]
fn oss_hmac_auth(
    method: &str,
    date: &str,
    security_token: &str,
    access_key_id: &str,
    access_key_secret: &str,
    canonicalized_resource: &str,
    content_type: &str,
    query_string: &str,
) -> String {
    type HmacSha1 = Hmac<Sha1>;

    let canonicalized_headers = format!("x-oss-security-token:{}", security_token);
    let resource = format!("{}{}", canonicalized_resource, query_string);

    let string_to_sign = format!(
        "{}\n\n{}\n{}\n{}\n{}",
        method, content_type, date, canonicalized_headers, resource
    );

    let mut mac = HmacSha1::new_from_slice(access_key_secret.as_bytes()).expect("HMAC key length");
    mac.update(string_to_sign.as_bytes());
    let result = mac.finalize();
    let signature = base64::engine::general_purpose::STANDARD.encode(result.into_bytes());

    format!("OSS {}:{}", access_key_id, signature)
}

fn httpdate_now() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();

    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = days_to_ymd(days);

    let wday = ((days + 4) % 7) as usize;
    let wday_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let month_names = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        wday_names[wday],
        day,
        month_names[(month - 1) as usize],
        year,
        hours,
        minutes,
        seconds
    )
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let mut y = 1970;
    let mut remaining = days;

    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }

    let month_days = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut m = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining < md {
            m = i;
            break;
        }
        remaining -= md;
    }

    (y, (m + 1) as u64, remaining + 1)
}

fn is_leap(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].to_string())
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pikpak::SessionToken;
    use std::io::Write as _;
    use std::net::TcpListener;
    use std::sync::Mutex;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "pikpaktui-upload-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn upload_test_client(base_url: String, session_path: &Path) -> PikPak {
        let mut client = PikPak::new().unwrap();
        client.drive_base_url = base_url.clone();
        client.auth_base_url = base_url;
        client.session_path = session_path.to_path_buf();
        client.device_id = "test-device".into();
        client.user_id = "test-user".into();
        client.captcha_token = Mutex::new("stale-captcha".into());
        let captcha_expires_at_unix = crate::pikpak::now_unix() + 300;
        client.captcha_expires_at_unix = Mutex::new(captcha_expires_at_unix);
        client
            .save_session(&SessionToken {
                access_token: "test-access".into(),
                refresh_token: "test-refresh".into(),
                expires_at_unix: crate::pikpak::now_unix() + 3600,
                device_id: "test-device".into(),
                user_id: "test-user".into(),
                captcha_token: "stale-captcha".into(),
                captcha_expires_at_unix,
            })
            .unwrap();
        client
    }

    fn write_response(stream: &mut std::net::TcpStream, code: u16, reason: &str, body: &[u8]) {
        let header = format!(
            "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(header.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
    }

    fn start_upload_captcha_server(
        refresh_succeeds: bool,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());

        let handle = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            let mut request_index = 0usize;
            while std::time::Instant::now() < deadline {
                let mut stream = match crate::pikpak::accept_test_connection(&listener) {
                    Ok(stream) => stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        continue;
                    }
                    Err(e) => panic!("accept failed: {e}"),
                };
                let request =
                    crate::pikpak::read_test_http_request(&mut stream).unwrap_or_default();
                request_index += 1;

                match request_index {
                    1 => {
                        assert!(request.starts_with("POST /drive/v1/files "));
                        write_response(
                            &mut stream,
                            400,
                            "Bad Request",
                            br#"{"error_code":9,"error":"riskLimited"}"#,
                        );
                    }
                    2 => {
                        assert!(request.starts_with("POST /v1/shield/captcha/init?"));
                        assert!(
                            request.contains(r#""action":"POST:/drive/v1/files""#),
                            "wrong captcha action: {request}"
                        );
                        if refresh_succeeds {
                            write_response(
                                &mut stream,
                                200,
                                "OK",
                                br#"{"captcha_token":"fresh-captcha","expires_in":300}"#,
                            );
                        } else {
                            write_response(
                                &mut stream,
                                500,
                                "Internal Server Error",
                                br#"{"error":"refresh failed"}"#,
                            );
                            break;
                        }
                    }
                    3 => {
                        assert!(request.starts_with("POST /drive/v1/files "));
                        assert!(
                            request
                                .to_ascii_lowercase()
                                .contains("x-captcha-token: fresh-captcha"),
                            "retry did not use refreshed captcha: {request}"
                        );
                        write_response(
                            &mut stream,
                            200,
                            "OK",
                            br#"{"file":{"phase":"PHASE_TYPE_COMPLETE"}}"#,
                        );
                        break;
                    }
                    _ => panic!("unexpected request: {request}"),
                }
            }
        });

        (base_url, handle)
    }

    #[cfg(unix)]
    #[test]
    fn count_files_does_not_follow_directory_symlinks() {
        use std::os::unix::fs::symlink;

        let root = temp_dir("count-symlink");
        let outside = temp_dir("count-symlink-outside");
        fs::write(root.join("inside.txt"), b"inside").unwrap();
        fs::write(outside.join("outside.txt"), b"outside").unwrap();
        symlink(&outside, root.join("escape")).unwrap();

        assert_eq!(PikPak::count_files_in(&root), 1);

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn directory_upload_rejects_symlink_root_before_remote_request() {
        use std::os::unix::fs::symlink;

        let root = temp_dir("root-symlink");
        let target = root.join("target");
        fs::create_dir(&target).unwrap();
        let link = root.join("link");
        symlink(&target, &link).unwrap();

        let mut client = PikPak::new().unwrap();
        client.session_path = root.join("missing-session.json");
        let err = client.upload_dir("parent", &link).unwrap_err();

        assert!(
            err.to_string().contains("symbolic link"),
            "unexpected error: {err:#}"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn file_upload_rejects_symlink_before_remote_request() {
        use std::os::unix::fs::symlink;

        let root = temp_dir("file-symlink");
        let target = root.join("target.txt");
        fs::write(&target, b"secret").unwrap();
        let link = root.join("link.txt");
        symlink(&target, &link).unwrap();

        let mut client = PikPak::new().unwrap();
        client.session_path = root.join("missing-session.json");
        let err = client.upload_file(Some("parent"), &link).unwrap_err();

        assert!(
            err.to_string().contains("symbolic link"),
            "unexpected error: {err:#}"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn directory_upload_skips_symlink_entry_as_one_failure() {
        use std::os::unix::fs::symlink;

        let root = temp_dir("entry-symlink");
        let outside = temp_dir("entry-symlink-outside");
        fs::write(outside.join("one.txt"), b"one").unwrap();
        fs::write(outside.join("two.txt"), b"two").unwrap();
        symlink(&outside, root.join("escape")).unwrap();

        let mut client = PikPak::new().unwrap();
        client.session_path = root.join("missing-session.json");
        let result = client.upload_dir_inner("parent", &root).unwrap();

        assert_eq!(result, (0, 1));
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn upload_init_refreshes_captcha_code_nine_once_and_retries() {
        let dir = temp_dir("captcha-retry");
        let file = dir.join("file.txt");
        fs::write(&file, b"hello").unwrap();
        let (base_url, handle) = start_upload_captcha_server(true);
        let client = upload_test_client(base_url, &dir.join("session.json"));

        let result = client.upload_file(Some("parent"), &file);

        handle.join().unwrap();
        fs::remove_dir_all(dir).unwrap();
        assert_eq!(result.unwrap(), ("file.txt".into(), true));
    }

    #[test]
    fn upload_init_surfaces_captcha_refresh_failure() {
        let dir = temp_dir("captcha-refresh-failure");
        let file = dir.join("file.txt");
        fs::write(&file, b"hello").unwrap();
        let (base_url, handle) = start_upload_captcha_server(false);
        let client = upload_test_client(base_url, &dir.join("session.json"));

        let result = client.upload_file(Some("parent"), &file);

        handle.join().unwrap();
        fs::remove_dir_all(dir).unwrap();
        let err = result.unwrap_err();
        assert!(
            format!("{err:#}").contains("captcha refresh failed"),
            "unexpected error: {err:#}"
        );
    }
}
