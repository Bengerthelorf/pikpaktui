use super::download::{DownloadTask, TaskStatus};
use super::{App, OpResult};
use crate::pikpak::{Entry, EntryKind, Verification, sanitize_filename, unique_local_name};
use anyhow::{Context, Result, ensure};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::AtomicBool};

pub(super) struct CartPlan {
    files: Vec<(Entry, PathBuf)>,
    directories: Vec<PathBuf>,
    selected_ids: HashSet<String>,
    verify: bool,
}

impl CartPlan {
    fn create_directories(&self, root: &Path) -> Result<()> {
        fs::create_dir_all(root).with_context(|| format!("cannot create {}", root.display()))?;
        // Parents precede children. Reject an existing symlink in the remote
        // subtree so a server-supplied path cannot leave the chosen directory.
        for path in &self.directories {
            match fs::create_dir(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    ensure!(
                        fs::symlink_metadata(path)?.is_dir(),
                        "not a directory: {}",
                        path.display()
                    );
                }
                Err(e) => {
                    return Err(e).with_context(|| format!("cannot create {}", path.display()));
                }
            }
        }
        Ok(())
    }
}

fn plan_cart(
    cart: Vec<Entry>,
    dest: &Path,
    reserved: HashSet<String>,
    verify: bool,
    mut list: impl FnMut(&str) -> Result<Vec<Entry>>,
) -> Result<CartPlan> {
    let selected_ids: HashSet<_> = cart.iter().map(|item| item.id.clone()).collect();
    let mut children = HashMap::new();
    let mut nested = HashSet::new();
    let mut folders: Vec<_> = cart
        .iter()
        .filter(|e| e.kind == EntryKind::Folder)
        .map(|e| (e.id.clone(), e.name.clone()))
        .collect();
    while let Some((id, name)) = folders.pop() {
        if children.contains_key(&id) {
            continue;
        }
        let entries = list(&id).with_context(|| format!("cannot list '{name}'"))?;
        for item in &entries {
            if selected_ids.contains(&item.id) {
                nested.insert(item.id.clone());
            }
            if item.kind == EntryKind::Folder {
                folders.push((item.id.clone(), item.name.clone()));
            }
        }
        children.insert(id, entries);
    }

    // Resolve overlap by remote ID before assigning local names. A child
    // selected before its parent still lands under the parent's local path.
    let roots: Vec<_> = cart
        .into_iter()
        .filter(|item| !nested.contains(&item.id))
        .collect();
    let mut pending = vec![(dest.to_owned(), roots, reserved)];
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    let mut directories = Vec::new();
    while let Some((dir, entries, mut taken)) = pending.pop() {
        for item in entries {
            if !seen.insert(item.id.clone()) {
                continue;
            }
            let path = dir.join(unique_local_name(
                &mut taken,
                &sanitize_filename(&item.name),
            ));
            if item.kind == EntryKind::Folder {
                directories.push(path.clone());
                let entries = children
                    .remove(&item.id)
                    .context("missing folder listing")?;
                pending.push((path, entries, HashSet::new()));
            } else {
                files.push((item, path));
            }
        }
    }
    ensure!(
        selected_ids.is_subset(&seen),
        "conflicting folder hierarchy in listing"
    );
    Ok(CartPlan {
        files,
        directories,
        selected_ids,
        verify,
    })
}

impl App {
    pub(super) fn start_cart_download(&mut self, dest_dir: &str) {
        if self.download_state.planning {
            self.push_log("Preparing the previous cart download".into());
            return;
        }
        let root = PathBuf::from(dest_dir);
        let reserved = self.download_state.reserved_names_in(&root);
        let verify = self.download_state.verify_new;
        let cart = self.cart.clone();
        let client = Arc::clone(&self.client);
        let tx = self.result_tx.clone();
        self.download_state.planning = true;
        self.push_log("Preparing cart download".into());
        std::thread::spawn(move || {
            let result =
                plan_cart(cart, &root, reserved, verify, |id| client.ls(id)).and_then(|plan| {
                    plan.create_directories(&root)?;
                    Ok(plan)
                });
            let _ = tx.send(OpResult::CartDownload(result));
        });
    }

    pub(super) fn finish_cart_download(&mut self, result: Result<CartPlan>) {
        self.download_state.planning = false;
        let plan = match result {
            Ok(plan) => plan,
            Err(e) => {
                self.push_log(format!("Could not queue cart download: {e:#}"));
                return;
            }
        };
        self.cart
            .retain(|item| !plan.selected_ids.contains(&item.id));
        self.cart_ids.retain(|id| !plan.selected_ids.contains(id));
        self.cart_selected = self.cart_selected.min(self.cart.len().saturating_sub(1));
        let count = plan.files.len();
        let folders = plan.directories.len();
        for (item, dest_path) in plan.files {
            let id = self.download_state.alloc_id();
            self.download_state.tasks.push(DownloadTask {
                id,
                file_id: item.id,
                name: item.name,
                total_size: item.size,
                downloaded: 0,
                dest_path,
                verify: plan.verify,
                verification: Verification::Unchecked,
                status: if self.download_state.queue_paused {
                    TaskStatus::Paused
                } else {
                    TaskStatus::Pending
                },
                cancel_flag: Arc::new(AtomicBool::new(false)),
                speed: 0.0,
            });
        }
        self.push_log(format!(
            "Queued {count} files for download ({folders} folders)"
        ));
        self.download_state.start_next(&self.client);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, name: &str, kind: EntryKind) -> Entry {
        Entry {
            id: id.into(),
            name: name.into(),
            kind,
            size: 0,
            created_time: String::new(),
            modified_time: String::new(),
            starred: false,
            thumbnail_link: None,
            phase: None,
            audit: None,
        }
    }

    #[test]
    fn download_plan_deduplicates_children_keeps_empty_folders_and_reserves_sidecars() {
        let root = item("root", "photos", EntryKind::Folder);
        let nested = item("nested", "2026", EntryKind::Folder);
        let preview = item("one", "a.jpg", EntryKind::File);
        let collision = item("two", "a.jpg.part", EntryKind::File);
        let mut tree = HashMap::from([
            (
                root.id.clone(),
                vec![nested.clone(), item("empty", "empty", EntryKind::Folder)],
            ),
            (nested.id.clone(), vec![preview.clone(), collision]),
            ("empty".into(), vec![]),
        ]);
        let plan = plan_cart(
            vec![preview, nested, root],
            Path::new("out"),
            HashSet::new(),
            true,
            |id| Ok(tree.remove(id).expect("list each folder once")),
        )
        .unwrap();
        assert!(tree.is_empty());
        let paths: Vec<_> = plan.files.into_iter().map(|(_, path)| path).collect();
        assert_eq!(
            paths,
            [
                PathBuf::from("out/photos/2026/a.jpg"),
                PathBuf::from("out/photos/2026/a.jpg (1).part")
            ]
        );
        assert!(
            plan.directories
                .contains(&PathBuf::from("out/photos/empty"))
        );
    }
}
