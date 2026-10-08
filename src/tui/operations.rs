//! File mutation workers and UI bookkeeping share one completion path.
use super::{App, InputMode, OpResult, StatusKind};
use crate::pikpak::{PikPak, mutation_progress};
use anyhow::Result;
use std::{collections::HashSet, sync::Arc};

pub(super) enum Destination {
    Folder { id: String, path: String },
    Path(String),
}
impl Destination {
    pub fn display(&self) -> &str {
        match self {
            Self::Folder { path, .. } | Self::Path(path) => path,
        }
    }
    fn resolve(self, client: &PikPak) -> Result<String> {
        match self {
            Self::Folder { id, .. } => Ok(id),
            Self::Path(path) => client.resolve_folder(&path),
        }
    }
}

pub(super) enum FileAction {
    Move(Destination),
    Copy(Destination),
    Trash,
    Delete,
    Star(bool),
}
impl FileAction {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Move(_) => "Move",
            Self::Copy(_) => "Copy",
            Self::Trash => "Trash",
            Self::Delete => "Delete",
            Self::Star(true) => "Star",
            Self::Star(false) => "Unstar",
        }
    }
    pub fn completed_label(&self) -> &'static str {
        match self {
            Self::Move(_) => "Moved",
            Self::Copy(_) => "Copied",
            Self::Trash => "Trashed",
            Self::Delete => "Permanently deleted",
            Self::Star(true) => "Starred",
            Self::Star(false) => "Unstarred",
        }
    }
    fn run(self, client: &PikPak, ids: &[String]) -> Result<()> {
        let ids: Vec<_> = ids.iter().map(String::as_str).collect();
        match self {
            Self::Move(dest) => client.mv(&ids, &dest.resolve(client)?),
            Self::Copy(dest) => client.cp(&ids, &dest.resolve(client)?),
            Self::Trash => client.remove(&ids),
            Self::Delete => client.delete_permanent(&ids),
            Self::Star(true) => client.star(&ids),
            Self::Star(false) => client.unstar(&ids),
        }
    }
}

pub(super) struct MutationReport {
    pub op: &'static str,
    pub success: String,
    pub result: Result<()>,
    pub cart_ids: Option<Vec<String>>,
}

impl App {
    pub(super) fn spawn_file_mutation(
        &mut self,
        action: FileAction,
        ids: Vec<String>,
        success: String,
        from_cart: bool,
    ) {
        if from_cart && self.cart_mutation_in_flight {
            self.push_status_log(
                "Cart operation still in progress".into(),
                StatusKind::Warning,
            );
            self.input = InputMode::CartView;
            return;
        }
        if ids.is_empty() {
            return;
        }
        if from_cart {
            self.cart_mutation_in_flight = true;
        }
        self.loading = true;
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        let op = action.label();
        std::thread::spawn(move || {
            let result = action.run(&client, &ids);
            let _ = tx.send(OpResult::Mutation(MutationReport {
                op,
                success,
                result,
                cart_ids: from_cart.then_some(ids),
            }));
        });
    }

    /// Return whether an accepted/completed operation requires fresh listings.
    /// Keeping this part free of network work makes UI transitions testable.
    pub(super) fn report_mutation_error(&mut self, op: &str, error: &anyhow::Error) -> bool {
        let progress = mutation_progress(error);
        self.push_status_log(
            format!(
                "{} {}: {error:#}",
                op,
                if progress.pending {
                    "pending"
                } else {
                    "failed"
                }
            ),
            if progress.pending {
                StatusKind::Warning
            } else {
                StatusKind::Error
            },
        );
        progress.accepted || progress.completed_items > 0
    }

    pub(super) fn record_mutation_result(&mut self, report: MutationReport) -> bool {
        let progress = report
            .result
            .as_ref()
            .err()
            .map(mutation_progress)
            .unwrap_or_default();
        if let Some(ids) = report.cart_ids {
            self.cart_mutation_in_flight = false;
            let completed = if report.result.is_ok() {
                ids.len()
            } else {
                progress.completed_items
            };
            let removed: HashSet<_> = ids.iter().take(completed).collect();
            self.cart.retain(|e| !removed.contains(&e.id));
            self.cart_ids.retain(|id| !removed.contains(id));
            self.cart_selected = self.cart_selected.min(self.cart.len().saturating_sub(1));
        }
        self.finish_background_loading();
        match report.result {
            Ok(()) => {
                self.push_status_log(report.success, StatusKind::Info);
                true
            }
            Err(error) => self.report_mutation_error(report.op, &error),
        }
    }
}
