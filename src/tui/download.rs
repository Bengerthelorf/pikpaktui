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
    /// Stable routing id for worker messages; survives cancel/remove (a Vec
    /// position would not).
    pub id: u64,
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
        id: u64,
        downloaded: u64,
        speed: f64,
    },
    Done {
        id: u64,
    },
    Failed {
        id: u64,
        error: String,
    },
    Started {
        id: u64,
        total_size: u64,
    },
}

pub struct DownloadState {
    pub tasks: Vec<DownloadTask>,
    pub selected: usize,
    pub msg_tx: Sender<DownloadMsg>,
    pub msg_rx: Receiver<DownloadMsg>,
    /// Task ids that currently have a live (running or parked-paused) worker.
    pub active_ids: HashSet<u64>,
    pub max_concurrent: usize,
    next_id: u64,
}

impl DownloadState {
    pub fn new(max_concurrent: usize) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            tasks: Vec::new(),
            selected: 0,
            msg_tx: tx,
            msg_rx: rx,
            active_ids: HashSet::new(),
            max_concurrent: max_concurrent.max(1),
            next_id: 0,
        }
    }

    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        id
    }

    /// Replace the task list (e.g. from persisted state), assigning fresh ids.
    pub fn load_tasks(&mut self, mut tasks: Vec<DownloadTask>) {
        for (i, t) in tasks.iter_mut().enumerate() {
            t.id = i as u64;
        }
        self.next_id = tasks.len() as u64;
        self.tasks = tasks;
    }

    pub fn done_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Done)
            .count()
    }

    pub fn has_active(&self) -> bool {
        self.tasks
            .iter()
            .any(|t| matches!(t.status, TaskStatus::Downloading | TaskStatus::Pending))
    }

    /// Start pending tasks up to max_concurrent slots.
    pub fn start_next(&mut self, client: &Arc<PikPak>) {
        loop {
            let active = self
                .tasks
                .iter()
                .filter(|t| t.status == TaskStatus::Downloading)
                .count();
            if active >= self.max_concurrent {
                break;
            }
            let active_ids = &self.active_ids;
            let next = self
                .tasks
                .iter()
                .position(|t| t.status == TaskStatus::Pending && !active_ids.contains(&t.id));
            match next {
                Some(idx) => {
                    self.tasks[idx].status = TaskStatus::Downloading;
                    let id = self.tasks[idx].id;
                    self.active_ids.insert(id);
                    spawn_download_worker(
                        Arc::clone(client),
                        id,
                        self.tasks[idx].file_id.clone(),
                        self.tasks[idx].dest_path.clone(),
                        self.msg_tx.clone(),
                        Arc::clone(&self.tasks[idx].pause_flag),
                        Arc::clone(&self.tasks[idx].cancel_flag),
                    );
                }
                None => break,
            }
        }
    }

    /// Poll messages and update task states. Returns log messages.
    pub fn poll(&mut self, client: &Arc<PikPak>) -> Vec<String> {
        let mut logs = Vec::new();
        while let Ok(msg) = self.msg_rx.try_recv() {
            match msg {
                DownloadMsg::Started { id, total_size } => {
                    if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
                        task.total_size = total_size;
                    }
                }
                DownloadMsg::Progress {
                    id,
                    downloaded,
                    speed,
                } => {
                    if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
                        task.downloaded = downloaded;
                        task.speed = speed;
                    }
                }
                DownloadMsg::Done { id } => {
                    if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
                        task.status = TaskStatus::Done;
                        task.downloaded = task.total_size;
                        logs.push(format!("Downloaded '{}'", task.name));
                    }
                    self.active_ids.remove(&id);
                    self.start_next(client);
                }
                DownloadMsg::Failed { id, error } => {
                    if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
                        task.status = TaskStatus::Failed(error.clone());
                        logs.push(format!("Download failed '{}': {}", task.name, error));
                    }
                    self.active_ids.remove(&id);
                    self.start_next(client);
                }
            }
        }
        logs
    }
}

fn spawn_download_worker(
    client: Arc<PikPak>,
    id: u64,
    file_id: String,
    dest: PathBuf,
    msg_tx: Sender<DownloadMsg>,
    pause_flag: Arc<AtomicBool>,
    cancel_flag: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        if let Err(e) = download_worker(
            &client,
            id,
            &file_id,
            &dest,
            &msg_tx,
            &pause_flag,
            &cancel_flag,
        ) {
            let _ = msg_tx.send(DownloadMsg::Failed {
                id,
                error: format!("{e:#}"),
            });
        }
    });
}

fn download_worker(
    client: &PikPak,
    id: u64,
    file_id: &str,
    dest: &PathBuf,
    msg_tx: &Sender<DownloadMsg>,
    pause_flag: &Arc<AtomicBool>,
    cancel_flag: &Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let (url, total_size) = client.download_url(file_id)?;

    let _ = msg_tx.send(DownloadMsg::Started { id, total_size });

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    let existing_size = dest.metadata().map(|m| m.len()).unwrap_or(0);

    if existing_size >= total_size && total_size > 0 {
        let _ = msg_tx.send(DownloadMsg::Done { id });
        return Ok(());
    }

    // Shared range/resume contract with the CLI download (see download_stream).
    let (response, start_offset) = client.download_stream(&url, existing_size)?;

    let mut file = if start_offset > 0 {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(dest)?;
        f.seek(SeekFrom::Start(start_offset))?;
        f
    } else {
        fs::File::create(dest)?
    };

    let mut reader = response;
    let mut downloaded = start_offset;
    let mut buf = [0u8; 65536]; // 64KB chunks
    let mut last_report = Instant::now();
    let mut last_report_bytes = downloaded;
    let speed_interval = std::time::Duration::from_millis(500);

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            return Ok(());
        }

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

        let elapsed = last_report.elapsed();
        if elapsed >= speed_interval {
            let speed = (downloaded - last_report_bytes) as f64 / elapsed.as_secs_f64();
            let _ = msg_tx.send(DownloadMsg::Progress {
                id,
                downloaded,
                speed,
            });
            last_report = Instant::now();
            last_report_bytes = downloaded;
        }
    }

    let _ = msg_tx.send(DownloadMsg::Done { id });
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct PersistedTask {
    file_id: String,
    name: String,
    total_size: u64,
    downloaded: u64,
    dest_path: String,
    status: String, // "pending", "paused", "failed" (Done tasks aren't persisted)
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
                // Exiting mid-download: persist as paused so it reloads parked,
                // then resumes from the partial file via Range on next launch
                // (no worker survives the restart).
                TaskStatus::Downloading => "paused".into(),
                TaskStatus::Paused => "paused".into(),
                // Done tasks are filtered out above, so this is never reached.
                TaskStatus::Done => unreachable!("Done tasks are not persisted"),
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
        let tmp_path = path.with_extension("tmp");
        if fs::write(&tmp_path, &json).is_ok() {
            let _ = fs::rename(&tmp_path, &path);
        }
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
            // Everything reloads as Paused (no live worker survives a restart);
            // the user resumes from the partial file.
            let status = TaskStatus::Paused;
            DownloadTask {
                id: 0, // reassigned by DownloadState::load_tasks
                file_id: p.file_id,
                name: p.name,
                total_size: p.total_size,
                downloaded: p.downloaded,
                dest_path: PathBuf::from(p.dest_path),
                pause_flag: Arc::new(AtomicBool::new(status == TaskStatus::Paused)),
                status,
                cancel_flag: Arc::new(AtomicBool::new(false)),
                speed: 0.0,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn downloading_task(id: u64, name: &str) -> DownloadTask {
        DownloadTask {
            id,
            file_id: name.into(),
            name: name.into(),
            total_size: 100,
            downloaded: 0,
            dest_path: PathBuf::from(name),
            status: TaskStatus::Downloading,
            pause_flag: Arc::new(AtomicBool::new(false)),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            speed: 0.0,
        }
    }

    // Cancelling a task removes it from the Vec, shifting later positions. A
    // worker's message must still reach the right task by its stable id, not by
    // the now-stale position.
    #[test]
    fn progress_routes_by_id_after_remove() {
        let client = Arc::new(PikPak::new().unwrap());
        let mut state = DownloadState::new(2);
        for name in ["a", "b", "c"] {
            let id = state.alloc_id();
            state.tasks.push(downloading_task(id, name));
        }

        // Remove the middle task (id 1); "c" shifts from position 2 to 1.
        state.tasks.remove(1);

        // The worker for "c" reports progress under its stable id (2).
        state
            .msg_tx
            .send(DownloadMsg::Progress {
                id: 2,
                downloaded: 42,
                speed: 1.0,
            })
            .unwrap();
        state.poll(&client);

        assert_eq!(
            state.tasks.iter().find(|t| t.id == 2).unwrap().downloaded,
            42
        );
        assert_eq!(
            state.tasks.iter().find(|t| t.id == 0).unwrap().downloaded,
            0
        );
    }

    // Resuming a still-parked worker must not spawn a second one. The backstop:
    // start_next skips any Pending task whose id already has a live worker, so it
    // never starts a duplicate (which would write the same file twice).
    #[test]
    fn start_next_skips_ids_with_a_live_worker() {
        let client = Arc::new(PikPak::new().unwrap());
        let mut state = DownloadState::new(1);
        let id = state.alloc_id();
        let mut task = downloading_task(id, "a");
        task.status = TaskStatus::Pending;
        state.tasks.push(task);
        state.active_ids.insert(id); // a worker already exists for this id

        state.start_next(&client);

        // No second worker: the task is left Pending, unspawned.
        assert_eq!(state.tasks[0].status, TaskStatus::Pending);
    }
}
