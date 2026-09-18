use crate::UnwrapOrAbort;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tokio::fs;

use crate::digest::digest12;
use crate::event::{EventV1, WorkspaceRevertFailure, WorkspaceRevertedEvent};

use super::{
    append_payload_event_with_correlation, system_actor, Coordinator, CoordinatorError,
    WorkspaceRevertSummary,
};

const SNAPSHOTS_DIR: &str = "snapshots";

use super::snapshot::{
    normalize_relative_path, should_ignore_path, workspace_files, SnapshotEntry,
};

impl Coordinator {
    pub(in crate::coord) async fn revert_workspace_internal(
        &mut self,
        snapshot_request_id: String,
    ) -> Result<WorkspaceRevertSummary, CoordinatorError> {
        let run_state = self
            .run_state
            .as_ref()
            .ok_or(CoordinatorError::RunNotStarted)?;
        let workspace_root = run_state.info.workspace_root.clone();
        let artifacts_dir = run_state.info.artifacts_dir.clone();
        let request_id = format!("rev_{}", snapshot_request_id);

        let snapshot_path = artifacts_dir
            .join(SNAPSHOTS_DIR)
            .join(format!("{snapshot_request_id}.json"));
        if !snapshot_path.is_file() {
            return Err(CoordinatorError::SnapshotNotFound(
                snapshot_request_id.clone(),
            ));
        }

        let snapshot_bytes = fs::read(&snapshot_path)
            .await
            .map_err(|err| CoordinatorError::RevertFailed(format!("read snapshot: {err}")))?;
        let snapshot: BTreeMap<String, SnapshotEntry> = serde_json::from_slice(&snapshot_bytes)
            .map_err(|err| CoordinatorError::RevertFailed(format!("parse snapshot: {err}")))?;

        let mut restored_paths = Vec::new();
        let mut removed_paths = Vec::new();
        let mut failed_paths = Vec::new();

        let current_entries = current_workspace_entries(&workspace_root)
            .await
            .map_err(|err| CoordinatorError::RevertFailed(format!("scan workspace: {err}")))?;

        let snapshot_paths: BTreeSet<&str> = snapshot
            .keys()
            .map(String::as_str)
            .filter(|path| !should_ignore_path(path))
            .collect();
        let current_paths: BTreeSet<&str> = current_entries.keys().map(String::as_str).collect();

        // Paths present in the snapshot must be restored to their captured state.
        for path in snapshot_paths.iter() {
            let entry = snapshot.get(*path).unwrap_or_abort();
            let Some(content) = entry
                .content
                .as_deref()
                .filter(|content| digest12(content.as_bytes()) == entry.digest)
            else {
                if current_entries.get(*path) != Some(&entry.digest) {
                    failed_paths.push((
                        path.to_string(),
                        "binary or redacted content cannot be restored; file left unchanged"
                            .to_string(),
                    ));
                }
                continue;
            };
            match apply_restore(&workspace_root, path, Some(content)).await {
                Ok(true) => restored_paths.push(path.to_string()),
                Ok(false) => {}
                Err(reason) => failed_paths.push((path.to_string(), reason)),
            }
        }

        // Paths that exist now but were not present in the snapshot were created after the
        // snapshot and should be removed.
        for path in current_paths.difference(&snapshot_paths) {
            match apply_restore(&workspace_root, path, None).await {
                Ok(true) => removed_paths.push(path.to_string()),
                Ok(false) => {}
                Err(reason) => failed_paths.push((path.to_string(), reason)),
            }
        }

        let failed_event_failures: Vec<WorkspaceRevertFailure> = failed_paths
            .iter()
            .map(|(path, reason)| WorkspaceRevertFailure {
                path: path.clone(),
                reason: reason.clone(),
            })
            .collect();

        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            system_actor(),
            Some(format!("revert:{request_id}")),
            Some(request_id.clone()),
            EventV1::WorkspaceReverted(WorkspaceRevertedEvent {
                request_id: request_id.clone().into(),
                snapshot_request_id,
                restored_paths: restored_paths.clone(),
                removed_paths: removed_paths.clone(),
                failed_paths: failed_event_failures,
            }),
        )?;

        Ok(WorkspaceRevertSummary {
            request_id: request_id.into(),
            restored_paths,
            removed_paths,
            failed_paths,
        })
    }
}

async fn current_workspace_entries(
    workspace_root: &Path,
) -> Result<BTreeMap<String, String>, std::io::Error> {
    let mut entries = BTreeMap::new();
    for path in workspace_files(workspace_root).await? {
        let relative = normalize_relative_path(path.strip_prefix(workspace_root).unwrap_or(&path));
        entries.insert(relative, digest12(&fs::read(&path).await?));
    }
    Ok(entries)
}

async fn apply_restore(
    workspace_root: &Path,
    relative_path: &str,
    content: Option<&str>,
) -> Result<bool, String> {
    let normalized = normalize_relative_path(Path::new(relative_path));
    let target = resolve_within_workspace(workspace_root, &normalized)?;

    let exists = target.is_file();
    match content {
        Some(expected) => {
            if exists {
                let current = fs::read(&target)
                    .await
                    .map_err(|err| format!("read failed: {err}"))?;
                if current == expected.as_bytes() {
                    return Ok(false);
                }
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .await
                    .map_err(|err| format!("create parent dir failed: {err}"))?;
            }
            fs::write(&target, expected)
                .await
                .map_err(|err| format!("write failed: {err}"))?;
            Ok(true)
        }
        None => {
            if exists {
                fs::remove_file(&target)
                    .await
                    .map_err(|err| format!("remove failed: {err}"))?;
                Ok(true)
            } else {
                Ok(false)
            }
        }
    }
}

fn resolve_within_workspace(workspace_root: &Path, relative: &str) -> Result<PathBuf, String> {
    let candidate = workspace_root.join(relative);
    let canonical = candidate
        .canonicalize()
        .unwrap_or_else(|_| candidate.clone());
    let canonical_root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    if !canonical.starts_with(&canonical_root) {
        return Err(format!("path escapes workspace root: {relative}"));
    }
    Ok(candidate)
}
