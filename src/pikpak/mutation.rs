use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::time::Duration;

use super::{OfflineListResponse, PikPak, json_or_api_error};

const BATCH_SIZE: usize = 500;
const POLL_ATTEMPTS: usize = 10;
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Progress remains available to UI callers even when a batch ends in an error.
#[derive(Debug, Clone, Copy, Default)]
pub struct MutationProgress {
    pub completed_items: usize,
    pub accepted: bool,
    pub pending: bool,
}

#[derive(Debug)]
struct PendingMutation(String);
impl std::fmt::Display for PendingMutation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for PendingMutation {}

#[derive(Debug)]
struct BatchMutationError {
    op: String,
    total: usize,
    progress: MutationProgress,
    cause: anyhow::Error,
}
impl std::fmt::Display for BatchMutationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {}/{} items completed before this batch",
            self.op, self.progress.completed_items, self.total
        )
    }
}
impl std::error::Error for BatchMutationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.cause.as_ref())
    }
}

pub fn mutation_progress(error: &anyhow::Error) -> MutationProgress {
    if let Some(batch) = error.downcast_ref::<BatchMutationError>() {
        return batch.progress;
    }
    MutationProgress {
        accepted: error.downcast_ref::<PendingMutation>().is_some(),
        pending: error.downcast_ref::<PendingMutation>().is_some(),
        ..Default::default()
    }
}

fn pending(message: impl Into<String>) -> anyhow::Error {
    PendingMutation(message.into()).into()
}

#[cfg(test)]
pub(crate) fn test_batch_failure(completed: usize, accepted: bool, pending: bool) -> anyhow::Error {
    BatchMutationError {
        op: "fixture".into(),
        total: 3,
        progress: MutationProgress {
            completed_items: completed,
            accepted,
            pending,
        },
        cause: anyhow!("fixture failure"),
    }
    .into()
}

impl PikPak {
    /// Keep the same per-request batch boundary as the web client. Completed
    /// batches are not replayed if a later submission or task fails.
    pub(super) fn batch_mutation(
        &self,
        op: &str,
        endpoint: &str,
        ids: &[&str],
        ids_field: &str,
        base: Value,
    ) -> Result<()> {
        let mut completed = 0;
        for chunk in ids.chunks(BATCH_SIZE) {
            let mut payload = base.clone();
            payload[ids_field] = json!(chunk);
            let mut accepted = false;
            let result = (|| {
                let rb = self
                    .http
                    .post(self.drive_url(endpoint))
                    .bearer_auth(self.access_token()?)
                    .json(&payload);
                let response = self.send_authed(op, rb)?;
                // The accepted mutation may already have changed the listing,
                // even if its task later fails or remains pending.
                accepted = true;
                self.clear_ls_cache();
                self.finish_mutation(response, op)
            })();
            if let Err(cause) = result {
                let pending = mutation_progress(&cause).pending;
                return Err(BatchMutationError {
                    op: op.into(),
                    total: ids.len(),
                    progress: MutationProgress {
                        completed_items: completed,
                        accepted,
                        pending,
                    },
                    cause,
                }
                .into());
            }
            completed += chunk.len();
        }
        Ok(())
    }

    pub(super) fn finish_mutation(
        &self,
        response: reqwest::blocking::Response,
        op: &str,
    ) -> Result<()> {
        let body = response.text().map_err(|e| {
            pending(format!(
                "{op}: cannot read accepted response; completion unknown: {e}"
            ))
        })?;
        // Some synchronous endpoints legitimately return 204/empty content.
        if body.trim().is_empty() {
            return Ok(());
        }
        let value: Value = serde_json::from_str(&body).map_err(|e| {
            pending(format!(
                "{op}: invalid accepted response; completion unknown: {e}"
            ))
        })?;
        if let Some(task) = value.get("task") {
            match task["phase"].as_str() {
                Some("PHASE_TYPE_COMPLETE") => return Ok(()),
                Some("PHASE_TYPE_ERROR") => {
                    return Err(anyhow!(
                        "{op} task failed: {}",
                        task["message"].as_str().unwrap_or("task did not complete")
                    ));
                }
                _ => {}
            }
        }
        let task_id = value["task_id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .or_else(|| value["task"]["id"].as_str().filter(|id| !id.is_empty()));
        if let Some(id) = task_id {
            self.wait_mutation_task(id, op)?;
        } else if value.get("task").is_some_and(|task| !task.is_null())
            || value
                .get("task_id")
                .is_some_and(|id| !id.is_null() && id != "")
        {
            return Err(pending(format!(
                "{op} accepted but task identity is missing or invalid; completion unknown"
            )));
        }
        Ok(())
    }

    fn wait_mutation_task(&self, task_id: &str, op: &str) -> Result<()> {
        let filters = json!({"id": {"in": task_id}}).to_string();
        for attempt in 0..POLL_ATTEMPTS {
            if attempt > 0 {
                std::thread::sleep(POLL_INTERVAL);
            }
            let page: OfflineListResponse = (|| {
                let rb = self
                    .http
                    .get(self.drive_url("drive/v1/tasks"))
                    .bearer_auth(self.access_token()?)
                    .timeout(Duration::from_secs(10))
                    .query(&[
                        ("type", ""),
                        ("filters", filters.as_str()),
                        ("limit", "500"),
                    ]);
                self.send_authed("mutation task poll", rb)
                    .and_then(|response| json_or_api_error(response, "mutation task poll"))
            })()
            .map_err(|error| {
                pending(format!(
                    "{op} accepted as task {task_id}; completion check failed: {error:#}"
                ))
            })?;
            if let Some(task) = page.tasks.iter().find(|t| t.id == task_id) {
                match task.phase.as_str() {
                    "PHASE_TYPE_COMPLETE" => return Ok(()),
                    "PHASE_TYPE_ERROR" => {
                        return Err(anyhow!(
                            "{op} task {task_id}: {} ({})",
                            task.phase,
                            task.message.as_deref().unwrap_or("task did not complete")
                        ));
                    }
                    "PHASE_TYPE_PAUSED" => {
                        return Err(pending(format!(
                            "{op} accepted as task {task_id}, paused; completion has not been confirmed"
                        )));
                    }
                    _ => {}
                }
            }
        }
        Err(pending(format!(
            "{op} accepted as task {task_id}, still processing; completion has not been confirmed. Check this task before resubmitting"
        )))
    }

    pub(super) fn wait_uploaded_file(&self, file_id: &str) -> Result<()> {
        for attempt in 0..POLL_ATTEMPTS {
            if attempt > 0 {
                std::thread::sleep(POLL_INTERVAL);
            }
            let info = self.file_info(file_id).map_err(|error| {
                pending(format!(
                    "upload transferred for file {file_id}; finalization check failed: {error:#}"
                ))
            })?;
            match info.phase.as_deref() {
                Some("PHASE_TYPE_COMPLETE") => return Ok(()),
                Some("PHASE_TYPE_ERROR") => {
                    return Err(anyhow!("upload finalization failed for file {file_id}"));
                }
                _ => {}
            }
        }
        Err(pending(format!(
            "upload transferred for file {file_id}, server finalization still pending; completion has not been confirmed"
        )))
    }
}
