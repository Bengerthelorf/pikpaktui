use std::collections::HashSet;
use std::fs;
use std::io::{Read as _, Seek, SeekFrom, Write as _};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::pikpak::PikPak;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    Downloading,
    Paused,
    Done,
    Failed(String),
}

#[derive(Clone)]
pub struct DownloadTask {
    pub file_id: String,
    pub name: String,
    pub total_size: u64,
    pub downloaded: u64,
    pub dest_path: PathBuf,
    pub status: TaskStatus,
    pub pause_flag: Arc<AtomicBool>,
    pub cancel_flag: Arc<AtomicBool>,
    pub speed: f64, // bytes per second
}

pub enum DownloadMsg {
    Progress {
        index: usize,
        downloaded: u64,
        speed: f64,
    },
    Done {
        index: usize,
    },
    Failed {
        index: usize,
        error: String,
    },
    Started {
        index: usize,
        total_size: u64,
    },
}

pub struct DownloadState {
    pub tasks: Vec<DownloadTask>,
    pub selected: usize,
    pub msg_tx: Sender<DownloadMsg>,
    pub msg_rx: Receiver<DownloadMsg>,
    pub active_ids: HashSet<usize>,
}

impl DownloadState {
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            tasks: Vec::new(),
            selected: 0,
            msg_tx: tx,
            msg_rx: rx,
            active_ids: HashSet::new(),
        }
    }

    pub fn done_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Done)
            .count()
    }

    #[allow(dead_code)]
    pub fn has_active(&self) -> bool {
        self.tasks
            .iter()
            .any(|t| matches!(t.status, TaskStatus::Downloading | TaskStatus::Pending))
    }

    /// Start downloading the next pending task, if any, and no task is currently downloading.
    pub fn start_next(&mut self, client: &Arc<PikPak>) {
        // Only one download at a time
        if self
            .tasks
            .iter()
            .any(|t| t.status == TaskStatus::Downloading)
        {
            return;
        }

        let next = self
            .tasks
            .iter()
            .position(|t| t.status == TaskStatus::Pending);

        if let Some(idx) = next {
            self.tasks[idx].status = TaskStatus::Downloading;
            self.active_ids.insert(idx);
            spawn_download_worker(
                Arc::clone(client),
                idx,
                self.tasks[idx].file_id.clone(),
                self.tasks[idx].dest_path.clone(),
                self.msg_tx.clone(),
                Arc::clone(&self.tasks[idx].pause_flag),
                Arc::clone(&self.tasks[idx].cancel_flag),
            );
        }
    }

    /// Poll messages and update task states. Returns log messages.
    pub fn poll(&mut self, client: &Arc<PikPak>) -> Vec<String> {
        let mut logs = Vec::new();
        while let Ok(msg) = self.msg_rx.try_recv() {
            match msg {
                DownloadMsg::Started { index, total_size } => {
                    if let Some(task) = self.tasks.get_mut(index) {
                        task.total_size = total_size;
                    }
                }
                DownloadMsg::Progress {
                    index,
                    downloaded,
                    speed,
                } => {
                    if let Some(task) = self.tasks.get_mut(index) {
                        task.downloaded = downloaded;
                        task.speed = speed;
                    }
                }
                DownloadMsg::Done { index } => {
                    if let Some(task) = self.tasks.get_mut(index) {
                        task.status = TaskStatus::Done;
                        task.downloaded = task.total_size;
                        logs.push(format!("Downloaded '{}'", task.name));
                    }
                    self.active_ids.remove(&index);
                    self.start_next(client);
                }
                DownloadMsg::Failed { index, error } => {
                    if let Some(task) = self.tasks.get_mut(index) {
                        task.status = TaskStatus::Failed(error.clone());
                        logs.push(format!("Download failed '{}': {}", task.name, error));
                    }
                    self.active_ids.remove(&index);
                    self.start_next(client);
                }
            }
        }
        logs
    }
}

fn spawn_download_worker(
    client: Arc<PikPak>,
    index: usize,
    file_id: String,
    dest: PathBuf,
    msg_tx: Sender<DownloadMsg>,
    pause_flag: Arc<AtomicBool>,
    cancel_flag: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        if let Err(e) = download_worker(
            &client,
            index,
            &file_id,
            &dest,
            &msg_tx,
            &pause_flag,
            &cancel_flag,
        ) {
            let _ = msg_tx.send(DownloadMsg::Failed {
                index,
                error: format!("{e:#}"),
            });
        }
    });
}

fn download_worker(
    client: &PikPak,
    index: usize,
    file_id: &str,
    dest: &PathBuf,
    msg_tx: &Sender<DownloadMsg>,
    pause_flag: &Arc<AtomicBool>,
    cancel_flag: &Arc<AtomicBool>,
) -> anyhow::Result<()> {
    // Get fresh download URL
    let (url, total_size) = client.download_url(file_id)?;

    let _ = msg_tx.send(DownloadMsg::Started { index, total_size });

    // Create parent directories
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    // Check existing file size for resume
    let existing_size = dest.metadata().map(|m| m.len()).unwrap_or(0);

    if existing_size >= total_size && total_size > 0 {
        let _ = msg_tx.send(DownloadMsg::Done { index });
        return Ok(());
    }

    let mut rb = client.http().get(&url);
    if existing_size > 0 {
        rb = rb.header("Range", format!("bytes={}-", existing_size));
    }

    let response = rb.send()?;
    let status = response.status();
    if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
        anyhow::bail!("HTTP {}", status);
    }

    let mut file = if existing_size > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT {
        let mut f = fs::OpenOptions::new().write(true).open(dest)?;
        f.seek(SeekFrom::Start(existing_size))?;
        f
    } else {
        fs::File::create(dest)?
    };

    let start_offset = if status == reqwest::StatusCode::PARTIAL_CONTENT {
        existing_size
    } else {
        0
    };

    let mut reader = response;
    let mut downloaded = start_offset;
    let mut buf = [0u8; 65536]; // 64KB chunks
    let mut last_report = Instant::now();
    let mut last_report_bytes = downloaded;
    let speed_interval = std::time::Duration::from_millis(500);

    loop {
        // Check cancel
        if cancel_flag.load(Ordering::Relaxed) {
            return Ok(());
        }

        // Check pause — spin-wait
        while pause_flag.load(Ordering::Relaxed) {
            if cancel_flag.load(Ordering::Relaxed) {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }

        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }

        file.write_all(&buf[..n])?;
        downloaded += n as u64;

        // Report progress periodically
        let elapsed = last_report.elapsed();
        if elapsed >= speed_interval {
            let speed = (downloaded - last_report_bytes) as f64 / elapsed.as_secs_f64();
            let _ = msg_tx.send(DownloadMsg::Progress {
                index,
                downloaded,
                speed,
            });
            last_report = Instant::now();
            last_report_bytes = downloaded;
        }
    }

    let _ = msg_tx.send(DownloadMsg::Done { index });
    Ok(())
}

// --- Persistence ---

#[derive(Serialize, Deserialize)]
struct PersistedTask {
    file_id: String,
    name: String,
    total_size: u64,
    downloaded: u64,
    dest_path: String,
    status: String, // "pending", "paused", "done", "failed"
}

fn persist_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("pikpaktui").join("downloads.json"))
}

pub fn save_download_state(tasks: &[DownloadTask]) {
    let Some(path) = persist_path() else {
        return;
    };
    let persisted: Vec<PersistedTask> = tasks
        .iter()
        .filter(|t| !matches!(t.status, TaskStatus::Done))
        .map(|t| PersistedTask {
            file_id: t.file_id.clone(),
            name: t.name.clone(),
            total_size: t.total_size,
            downloaded: t.downloaded,
            dest_path: t.dest_path.to_string_lossy().to_string(),
            status: match &t.status {
                TaskStatus::Pending => "pending".into(),
                TaskStatus::Downloading => "paused".into(), // save as paused
                TaskStatus::Paused => "paused".into(),
                TaskStatus::Done => "done".into(),
                TaskStatus::Failed(_) => "failed".into(),
            },
        })
        .collect();

    if persisted.is_empty() {
        let _ = fs::remove_file(&path);
        return;
    }

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(&persisted) {
        let _ = fs::write(&path, json);
    }
}

pub fn load_download_state() -> Vec<DownloadTask> {
    let Some(path) = persist_path() else {
        return Vec::new();
    };
    let Ok(data) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(persisted): Result<Vec<PersistedTask>, _> = serde_json::from_str(&data) else {
        return Vec::new();
    };

    persisted
        .into_iter()
        .map(|p| {
            let status = match p.status.as_str() {
                "pending" => TaskStatus::Paused, // load as paused, user can resume
                "paused" => TaskStatus::Paused,
                "done" => TaskStatus::Done,
                _ => TaskStatus::Paused,
            };
            let is_paused = status == TaskStatus::Paused;
            DownloadTask {
                file_id: p.file_id,
                name: p.name,
                total_size: p.total_size,
                downloaded: p.downloaded,
                dest_path: PathBuf::from(p.dest_path),
                status,
                pause_flag: Arc::new(AtomicBool::new(is_paused)),
                cancel_flag: Arc::new(AtomicBool::new(false)),
                speed: 0.0,
            }
        })
        .collect()
}
