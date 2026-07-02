use anyhow::{Context, Result, anyhow};
use std::fs;
use std::io;
use std::io::Read as _;
use std::path::Path;

use super::{Entry, EntryKind, PikPak, sanitize_filename};

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
            existing_size
        } else {
            0
        };
        Ok((response, start_offset))
    }

    pub fn download_to(&self, file_id: &str, dest: &std::path::Path) -> Result<u64> {
        let info = self.file_info(file_id)?;
        let download_url = info
            .download_url()
            .ok_or_else(|| anyhow!("no download link for file {}", file_id))?;
        let total_size = info.file_size();

        let mut existing_size = dest.metadata().map(|m| m.len()).unwrap_or(0);
        if total_size > 0 {
            if existing_size == total_size {
                return Ok(existing_size);
            }
            if existing_size > total_size {
                // Not a partial of this file (stale or foreign content);
                // appending to it would corrupt silently. Start over.
                existing_size = 0;
            }
        }

        let (response, start_offset) = self.download_stream(download_url, existing_size)?;
        let mut file = if start_offset > 0 {
            fs::OpenOptions::new().append(true).open(dest)?
        } else {
            fs::File::create(dest)?
        };

        let mut reader: Box<dyn io::Read> = Box::new(response);
        let bytes = io::copy(&mut reader, &mut file).context("download write failed")?;
        let written = start_offset + bytes;
        // A well-formed but short body would otherwise count as success.
        if total_size > 0 && written != total_size {
            return Err(anyhow!(
                "incomplete download: got {} of {} bytes",
                written,
                total_size
            ));
        }
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
                        let local_size = dest.metadata().map(|m| m.len()).unwrap_or(0);
                        if local_size > 0 && local_size == entry.size {
                            println!("  skipping '{}' (already complete)", dest.display());
                            ok.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        println!("  {}", dest.display());
                        match self.download_to(&entry.id, &dest) {
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
