use anyhow::{Context, Result, anyhow};
use std::fs;
use std::io;
use std::io::Read as _;
use std::io::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{Entry, EntryKind, PikPak, sanitize_filename};

const DEFAULT_DOWNLOAD_CONNECTIONS: usize = 4;
const DEFAULT_DOWNLOAD_CHUNK_SIZE: u64 = 8 * 1024 * 1024;

/// Sidecar path for in-progress data: "name.ext" downloads as "name.ext.part"
/// and is renamed only after the byte count checks out.
pub(crate) fn part_path(dest: &Path) -> std::path::PathBuf {
    let mut os = dest.as_os_str().to_owned();
    os.push(".part");
    std::path::PathBuf::from(os)
}

fn part_identity_path(dest: &Path) -> std::path::PathBuf {
    let mut os = part_path(dest).into_os_string();
    os.push(".meta");
    std::path::PathBuf::from(os)
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
struct PartialIdentity {
    file_id: String,
    expected_size: u64,
}

fn parse_content_range(value: &str) -> Result<(u64, u64)> {
    let value = value
        .strip_prefix("bytes ")
        .ok_or_else(|| anyhow!("invalid Content-Range unit: '{value}'"))?;
    let (bounds, _) = value
        .split_once('/')
        .ok_or_else(|| anyhow!("invalid Content-Range: '{value}'"))?;
    let (start, end) = bounds
        .split_once('-')
        .ok_or_else(|| anyhow!("invalid Content-Range bounds: '{bounds}'"))?;
    let start = start
        .parse::<u64>()
        .with_context(|| format!("invalid Content-Range start: '{start}'"))?;
    let end = end
        .parse::<u64>()
        .with_context(|| format!("invalid Content-Range end: '{end}'"))?;
    if end < start {
        return Err(anyhow!(
            "invalid Content-Range bounds: end {end} precedes start {start}"
        ));
    }
    Ok((start, end))
}

fn plan_parallel_ranges(
    existing_size: u64,
    total_size: u64,
    chunk_size: u64,
    connections: usize,
) -> Vec<(u64, u64)> {
    if existing_size >= total_size || chunk_size == 0 || connections == 0 {
        return Vec::new();
    }

    (0..connections)
        .scan(existing_size, |start, _| {
            if *start >= total_size {
                return None;
            }
            let end = start.saturating_add(chunk_size - 1).min(total_size - 1);
            let range = (*start, end);
            *start = end + 1;
            Some(range)
        })
        .collect()
}

/// Prepare a resumable sidecar that is owned by exactly one remote file.
///
/// Unknown or mismatched sidecars are never deleted: `.part` and
/// `.part.meta` are ordinary names a user may already own. Automatic callers
/// can select another destination, while an explicit destination gets a clear
/// conflict instead of silent local data loss.
pub(crate) fn prepare_partial_download(
    dest: &Path,
    file_id: &str,
    expected_size: u64,
) -> Result<(std::path::PathBuf, u64)> {
    let part = part_path(dest);
    let identity_path = part_identity_path(dest);
    let wanted = PartialIdentity {
        file_id: file_id.to_string(),
        expected_size,
    };
    let part_metadata = match fs::symlink_metadata(&part) {
        Ok(metadata) => {
            if !metadata.file_type().is_file() {
                return Err(anyhow!(
                    "partial download path '{}' is not a regular file; move or remove it first",
                    part.display()
                ));
            }
            Some(metadata)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(e)
                .with_context(|| format!("cannot inspect partial download '{}'", part.display()));
        }
    };

    let identity = match fs::symlink_metadata(&identity_path) {
        Ok(metadata) => {
            if !metadata.file_type().is_file() {
                return Err(anyhow!(
                    "partial download identity '{}' is not a regular file; move or remove it first",
                    identity_path.display()
                ));
            }
            let raw = fs::read(&identity_path).with_context(|| {
                format!(
                    "cannot read partial download identity '{}'",
                    identity_path.display()
                )
            })?;
            Some(
                serde_json::from_slice::<PartialIdentity>(&raw).with_context(|| {
                    format!(
                        "partial download identity '{}' is malformed; move or remove it first",
                        identity_path.display()
                    )
                })?,
            )
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(e).with_context(|| {
                format!(
                    "cannot inspect partial download identity '{}'",
                    identity_path.display()
                )
            });
        }
    };

    match identity {
        Some(existing) if existing != wanted => {
            return Err(anyhow!(
                "partial download '{}' belongs to remote file '{}' (expected '{}'); move or remove its .part/.meta files first",
                part.display(),
                existing.file_id,
                file_id
            ));
        }
        Some(_) => {}
        None if part_metadata.is_some() => {
            return Err(anyhow!(
                "partial download '{}' has no valid identity; move or remove it first",
                part.display()
            ));
        }
        None => {
            let encoded = serde_json::to_vec(&wanted)
                .context("failed to encode partial download identity")?;
            let mut identity_file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&identity_path)
                .with_context(|| {
                    format!(
                        "cannot claim partial download identity '{}'",
                        identity_path.display()
                    )
                })?;
            if let Err(e) = identity_file.write_all(&encoded) {
                drop(identity_file);
                let _ = fs::remove_file(&identity_path);
                return Err(e).with_context(|| {
                    format!(
                        "cannot write partial download identity '{}'",
                        identity_path.display()
                    )
                });
            }
        }
    }

    let size = part_metadata.map(|metadata| metadata.len()).unwrap_or(0);
    if expected_size > 0 && size > expected_size {
        return Err(anyhow!(
            "partial download '{}' is larger than the expected {} bytes; move or remove it first",
            part.display(),
            expected_size
        ));
    }
    Ok((part, size))
}

/// Move a complete partial into place and remove its resumable identity.
pub(crate) fn finish_partial_download(dest: &Path, part: &Path) -> Result<()> {
    let identity_path = part_identity_path(dest);
    match fs::rename(part, dest) {
        Ok(()) => {}
        Err(first) if dest.exists() => {
            fs::remove_file(dest).with_context(|| {
                format!("cannot replace existing download '{}'", dest.display())
            })?;
            fs::rename(part, dest).with_context(|| {
                format!(
                    "downloaded but could not move '{}' into place after: {}",
                    part.display(),
                    first
                )
            })?;
        }
        Err(e) => {
            return Err(e).with_context(|| {
                format!(
                    "downloaded but could not move '{}' into place",
                    part.display()
                )
            });
        }
    }

    match fs::remove_file(&identity_path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| {
            format!(
                "cannot remove partial download identity '{}'",
                identity_path.display()
            )
        }),
    }
}

impl PikPak {
    /// Returns (download_url, total_size) for a file.
    pub fn download_url(&self, file_id: &str) -> Result<(String, u64)> {
        let info = self.file_info(file_id)?;
        let url = info
            .download_url()
            .ok_or_else(|| anyhow!("no download link for file {}", file_id))?
            .to_string();
        Ok((url, info.file_size()))
    }

    pub fn check_stream_available(&self, url: &str) -> bool {
        // Reuse the pooled client (keep-alive + user-agent); just override the
        // timeout for this quick probe.
        match self
            .http
            .get(url)
            .timeout(std::time::Duration::from_secs(5))
            .header("Range", "bytes=0-0")
            .send()
        {
            Ok(resp) => {
                resp.headers().contains_key("content-range")
                    && resp.content_length().unwrap_or(0) > 0
            }
            Err(_) => false,
        }
    }

    /// Issue a ranged GET for a download URL, resuming from `existing_size`.
    /// Returns the response and the byte offset its body starts at (0 for a
    /// fresh 200, `existing_size` for a 206 — some CDNs ignore Range and reply
    /// 200, in which case the caller must restart from 0). This is the single
    /// place the CLI and TUI downloads agree on the range/resume contract.
    pub fn download_stream(
        &self,
        url: &str,
        existing_size: u64,
    ) -> Result<(reqwest::blocking::Response, u64)> {
        let mut rb = self.http.get(url);
        if existing_size > 0 {
            rb = rb.header("Range", format!("bytes={}-", existing_size));
        }

        let response = rb.send().context("download request failed")?;
        let status = response.status();
        if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(anyhow!("download failed ({})", status));
        }

        let start_offset = if status == reqwest::StatusCode::PARTIAL_CONTENT {
            let value = response
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .ok_or_else(|| anyhow!("partial download response is missing Content-Range"))?
                .to_str()
                .context("partial download Content-Range is not valid text")?;
            let (actual_start, _) = parse_content_range(value)?;
            if actual_start != existing_size {
                return Err(anyhow!(
                    "partial download started at byte {actual_start}, expected {existing_size}"
                ));
            }
            actual_start
        } else {
            0
        };
        Ok((response, start_offset))
    }

    pub fn download_to(&self, file_id: &str, dest: &std::path::Path) -> Result<u64> {
        self.download_to_with_connections_and_chunk_size(
            file_id,
            dest,
            DEFAULT_DOWNLOAD_CONNECTIONS,
            DEFAULT_DOWNLOAD_CHUNK_SIZE,
        )
    }

    fn download_bounded_range(
        &self,
        url: &str,
        start: u64,
        end: u64,
        total_size: u64,
    ) -> Result<Option<Vec<u8>>> {
        let response = self
            .http
            .get(url)
            .header("Range", format!("bytes={start}-{end}"))
            .send()
            .context("download range request failed")?;

        if response.status() == reqwest::StatusCode::OK {
            return Ok(None);
        }
        if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(anyhow!("download range failed ({})", response.status()));
        }

        let value = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .ok_or_else(|| anyhow!("partial download response is missing Content-Range"))?
            .to_str()
            .context("partial download Content-Range is not valid text")?;
        let (actual_start, actual_end) = parse_content_range(value)?;
        let actual_total = value
            .split_once('/')
            .and_then(|(_, total)| total.parse::<u64>().ok())
            .ok_or_else(|| anyhow!("invalid Content-Range total: '{value}'"))?;
        if (actual_start, actual_end, actual_total) != (start, end, total_size) {
            return Err(anyhow!(
                "partial download returned bytes {actual_start}-{actual_end}/{actual_total}, expected {start}-{end}/{total_size}"
            ));
        }

        let expected_len = end - start + 1;
        let mut bytes = Vec::with_capacity(expected_len as usize);
        response
            .take(expected_len + 1)
            .read_to_end(&mut bytes)
            .context("download range body failed")?;
        if bytes.len() as u64 != expected_len {
            return Err(anyhow!(
                "download range {start}-{end} returned {} bytes, expected {expected_len}",
                bytes.len()
            ));
        }
        Ok(Some(bytes))
    }

    fn download_parallel_attempt(
        &self,
        url: &str,
        part: &Path,
        existing_size: u64,
        total_size: u64,
        connections: usize,
        chunk_size: u64,
    ) -> Result<Option<u64>> {
        let mut committed = existing_size;
        while committed < total_size {
            let ranges =
                plan_parallel_ranges(committed, total_size, chunk_size, connections.max(1));
            let results = std::thread::scope(|scope| {
                ranges
                    .iter()
                    .map(|&(start, end)| {
                        scope
                            .spawn(move || self.download_bounded_range(url, start, end, total_size))
                    })
                    .collect::<Vec<_>>()
                    .into_iter()
                    .map(|handle| {
                        handle
                            .join()
                            .map_err(|_| anyhow!("download range worker panicked"))?
                    })
                    .collect::<Result<Vec<_>>>()
            })?;

            if results.iter().any(Option::is_none) {
                return Ok(None);
            }

            let mut file = if committed > 0 {
                fs::OpenOptions::new().append(true).open(part)?
            } else {
                fs::File::create(part)?
            };
            for bytes in results.into_iter().flatten() {
                file.write_all(&bytes).context("download write failed")?;
                committed += bytes.len() as u64;
            }
        }
        Ok(Some(committed))
    }

    fn download_sequential_attempt(
        &self,
        url: &str,
        part: &Path,
        existing_size: u64,
    ) -> Result<u64> {
        let (response, start_offset) = self.download_stream(url, existing_size)?;
        let mut file = if start_offset > 0 {
            fs::OpenOptions::new().append(true).open(part)?
        } else {
            fs::File::create(part)?
        };
        let mut reader: Box<dyn io::Read> = Box::new(response);
        let bytes = io::copy(&mut reader, &mut file).context("download write failed")?;
        Ok(start_offset + bytes)
    }

    pub(crate) fn download_to_with_connections_and_chunk_size(
        &self,
        file_id: &str,
        dest: &std::path::Path,
        connections: usize,
        chunk_size: u64,
    ) -> Result<u64> {
        let info = self.file_info(file_id)?;
        let mut download_url = info
            .download_url()
            .ok_or_else(|| anyhow!("no download link for file {}", file_id))?
            .to_string();
        let total_size = info.file_size();

        let (part, initial_part_size) = prepare_partial_download(dest, file_id, total_size)?;
        if total_size > 0 && initial_part_size == total_size {
            finish_partial_download(dest, &part)?;
            return Ok(total_size);
        }

        let mut renewed = false;
        let written = loop {
            let mut part_size = part.metadata().map(|m| m.len()).unwrap_or(0);
            if total_size > 0 && part_size > total_size {
                part_size = 0;
            }

            let attempt = if total_size > 0 && connections > 1 && chunk_size > 0 {
                match self.download_parallel_attempt(
                    &download_url,
                    &part,
                    part_size,
                    total_size,
                    connections,
                    chunk_size,
                ) {
                    Ok(Some(written)) => Ok(written),
                    Ok(None) => self.download_sequential_attempt(&download_url, &part, part_size),
                    Err(e) => Err(e),
                }
            } else {
                self.download_sequential_attempt(&download_url, &part, part_size)
            };

            match attempt {
                Ok(w) if total_size == 0 || w == total_size => break w,
                // Short body or dropped connection: the .part keeps what
                // arrived, and one fresh link (they expire mid-transfer on
                // long downloads) resumes from that offset.
                Ok(_) | Err(_) if !renewed => {
                    renewed = true;
                    download_url = self
                        .file_info(file_id)?
                        .download_url()
                        .ok_or_else(|| anyhow!("no download link for file {}", file_id))?
                        .to_string();
                }
                Ok(w) => {
                    return Err(anyhow!(
                        "incomplete download: got {} of {} bytes",
                        w,
                        total_size
                    ));
                }
                Err(e) => return Err(e),
            }
        };

        finish_partial_download(dest, &part)?;
        Ok(written)
    }

    pub fn fetch_text_preview(
        &self,
        file_id: &str,
        max_bytes: u64,
    ) -> Result<(String, String, u64, bool)> {
        let info = self.file_info(file_id)?;
        let url = info
            .download_url()
            .ok_or_else(|| anyhow!("no download link for file {}", file_id))?;
        let file_size = info.file_size();

        let response = self
            .http
            .get(url)
            .header("Range", format!("bytes=0-{}", max_bytes.saturating_sub(1)))
            .send()
            .context("text preview request failed")?;

        let status = response.status();
        if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(anyhow!("text preview failed ({})", status));
        }

        // Cap the read ourselves: a CDN may ignore Range and answer 200 with
        // the full body, which for a "preview" could buffer gigabytes.
        let mut limited = response.take(max_bytes);
        let mut bytes = Vec::new();
        limited
            .read_to_end(&mut bytes)
            .context("text preview read failed")?;
        let truncated = file_size > bytes.len() as u64;
        let content = String::from_utf8_lossy(&bytes).into_owned();

        Ok((info.name, content, file_size, truncated))
    }

    pub fn download_dir(
        &self,
        folder_id: &str,
        folder_name: &str,
        local_dest: &Path,
        workers: usize,
    ) -> Result<(usize, usize)> {
        let dir = local_dest.join(sanitize_filename(folder_name));
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("cannot create dir '{}'", dir.display()))?;
        self.download_dir_inner(folder_id, &dir, workers)
    }

    fn download_dir_inner(
        &self,
        folder_id: &str,
        local_dir: &Path,
        workers: usize,
    ) -> Result<(usize, usize)> {
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        };

        let workers = workers.max(1);
        let connections_per_file = if workers < DEFAULT_DOWNLOAD_CONNECTIONS {
            DEFAULT_DOWNLOAD_CONNECTIONS / workers
        } else {
            1
        };

        let entries = match self.ls(folder_id) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("  [error] listing '{}': {}", folder_id, e);
                return Ok((0, 1));
            }
        };

        let mut files: Vec<Entry> = Vec::new();
        let mut folders: Vec<Entry> = Vec::new();
        for entry in entries {
            match entry.kind {
                EntryKind::File => files.push(entry),
                EntryKind::Folder => folders.push(entry),
            }
        }

        // One namespace per directory level: PikPak allows duplicate names in
        // a folder (and sanitization can collapse distinct names), so every
        // entry reserves a unique local name up front — otherwise two workers
        // interleave writes into the same file.
        let mut taken = std::collections::HashSet::new();

        let mut failed_count = 0usize;
        let mut folder_dests = Vec::with_capacity(folders.len());
        for folder in folders {
            let dest = local_dir.join(super::unique_local_name(
                &mut taken,
                &sanitize_filename(&folder.name),
            ));
            if let Err(e) = std::fs::create_dir_all(&dest) {
                eprintln!("  [error] mkdir '{}': {}", folder.name, e);
                failed_count += 1;
            }
            folder_dests.push((folder, dest));
        }

        let ok = Arc::new(AtomicUsize::new(0));
        let failed = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = std::sync::mpsc::channel::<(Entry, std::path::PathBuf)>();
        for entry in files {
            let dest = local_dir.join(super::unique_local_name(
                &mut taken,
                &sanitize_filename(&entry.name),
            ));
            tx.send((entry, dest)).ok();
        }
        drop(tx);
        let rx = Arc::new(Mutex::new(rx));

        std::thread::scope(|s| {
            for _ in 0..workers {
                let rx = Arc::clone(&rx);
                let ok = Arc::clone(&ok);
                let failed = Arc::clone(&failed);
                s.spawn(move || {
                    loop {
                        // Take one item and release the lock before downloading;
                        // a `while let` scrutinee would hold the guard through
                        // the whole body and serialize every worker.
                        let msg = rx.lock().unwrap_or_else(|e| e.into_inner()).recv();
                        let Ok((entry, dest)) = msg else { break };
                        println!("  {}", dest.display());
                        match self.download_to_with_connections_and_chunk_size(
                            &entry.id,
                            &dest,
                            connections_per_file,
                            DEFAULT_DOWNLOAD_CHUNK_SIZE,
                        ) {
                            Ok(_) => {
                                ok.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(e) => {
                                eprintln!("  [error] '{}': {}", entry.name, e);
                                failed.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                });
            }
        });

        let mut total_ok = ok.load(Ordering::Relaxed);
        let mut total_failed = failed.load(Ordering::Relaxed) + failed_count;

        for (folder, sub_dir) in folder_dests {
            match self.download_dir_inner(&folder.id, &sub_dir, workers) {
                Ok((sub_ok, sub_fail)) => {
                    total_ok += sub_ok;
                    total_failed += sub_fail;
                }
                Err(e) => {
                    eprintln!("  [error] {}: {}", folder.name, e);
                    total_failed += 1;
                }
            }
        }

        Ok((total_ok, total_failed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("pikpaktui-{label}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn unowned_partial_is_never_deleted_or_claimed() {
        let dir = temp_dir("unowned-partial");
        let dest = dir.join("movie");
        let part = part_path(&dest);
        fs::write(&part, b"user data").unwrap();

        let err = prepare_partial_download(&dest, "remote", 100).unwrap_err();

        assert!(format!("{err:#}").contains("has no valid identity"));
        assert_eq!(fs::read(&part).unwrap(), b"user data");
        assert!(!part_identity_path(&dest).exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn non_file_identity_cannot_cause_partial_deletion() {
        let dir = temp_dir("identity-directory");
        let dest = dir.join("movie");
        let part = part_path(&dest);
        let identity = part_identity_path(&dest);
        fs::write(&part, b"user data").unwrap();
        fs::create_dir(&identity).unwrap();

        let err = prepare_partial_download(&dest, "remote", 100).unwrap_err();

        assert!(format!("{err:#}").contains("is not a regular file"));
        assert_eq!(fs::read(&part).unwrap(), b"user data");
        assert!(identity.is_dir());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn content_range_must_report_the_requested_start() {
        assert_eq!(parse_content_range("bytes 42-99/100").unwrap(), (42, 99));
        assert!(parse_content_range("items 42-99/100").is_err());
        assert!(parse_content_range("bytes 100-42/101").is_err());
        assert!(parse_content_range("bytes nope-99/100").is_err());
    }

    #[test]
    fn parallel_range_plan_is_bounded_contiguous_and_resumable() {
        assert_eq!(
            plan_parallel_ranges(5, 14, 4, 4),
            vec![(5, 8), (9, 12), (13, 13)]
        );
        assert!(plan_parallel_ranges(14, 14, 4, 4).is_empty());
    }

    #[test]
    fn failed_final_move_keeps_partial_identity_for_retry() {
        let dir = temp_dir("partial-finalize");
        let dest = dir.join("occupied");
        fs::create_dir(&dest).unwrap();
        let (part, _) = prepare_partial_download(&dest, "file-id", 5).unwrap();
        fs::write(&part, b"hello").unwrap();
        let identity = part_identity_path(&dest);

        let result = finish_partial_download(&dest, &part);

        assert!(result.is_err());
        assert!(part.exists(), "complete partial should remain retryable");
        assert!(
            identity.exists(),
            "partial identity should remain for retry"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
