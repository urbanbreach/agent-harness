use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tokio::fs;

use crate::digest::digest12;
use crate::event::{EventV1, WorkspaceRevertFailure, WorkspaceRevertedEvent};
use crate::path_selector::{recheck_restore_target, resolve_restore_target};

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

        let preflight = async {
            let workspace_root = workspace_root
                .canonicalize()
                .map_err(|err| (".".to_string(), format!("resolve workspace: {err}")))?;
            let targets = snapshot
                .iter()
                .filter(|(path, _)| !should_ignore_path(path))
                .map(|(path, entry)| {
                    resolve_restore_target(&workspace_root, Path::new(path))
                        .map(|target| (path, entry, target))
                        .map_err(|err| (path.clone(), err.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let current_entries = current_workspace_entries(&workspace_root).await?;
            Ok::<_, (String, String)>((workspace_root, targets, current_entries))
        }
        .await;

        match preflight {
            Err(failure) => failed_paths.push(failure),
            Ok((workspace_root, targets, current_entries)) => {
                // No restore or removal starts until the complete target set is valid.
                for (path, entry, target) in targets {
                    let content = entry
                        .content
                        .as_deref()
                        .filter(|content| digest12(content.as_bytes()) == entry.digest);
                    if content.is_none()
                        && current_entries.get(path).map(|(_, digest)| digest)
                            == Some(&entry.digest)
                    {
                        continue;
                    }
                    let Some(content) = content else {
                        failed_paths.push((
                            path.to_string(),
                            "binary or redacted content cannot be restored; file left unchanged"
                                .to_string(),
                        ));
                        continue;
                    };
                    match apply_restore(&workspace_root, &target, Some(content)).await {
                        Ok(true) => restored_paths.push(path.to_string()),
                        Ok(false) => {}
                        Err(reason) => failed_paths.push((path.to_string(), reason)),
                    }
                }

                // Files created after the snapshot must be removed.
                for (path, (target, _)) in current_entries {
                    if snapshot.contains_key(&path) {
                        continue;
                    }
                    match apply_restore(&workspace_root, &target, None).await {
                        Ok(true) => removed_paths.push(path),
                        Ok(false) => {}
                        Err(reason) => failed_paths.push((path, reason)),
                    }
                }
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
) -> Result<BTreeMap<String, (PathBuf, String)>, (String, String)> {
    let mut entries = BTreeMap::new();
    for path in workspace_files(workspace_root)
        .await
        .map_err(|err| (".".to_string(), format!("scan workspace: {err}")))?
    {
        let relative = path
            .strip_prefix(workspace_root)
            .map_err(|err| (path.display().to_string(), err.to_string()))?;
        let target = resolve_restore_target(workspace_root, relative)
            .map_err(|err| (relative.display().to_string(), err.to_string()))?;
        let relative = normalize_relative_path(relative);
        let bytes = fs::read(&target)
            .await
            .map_err(|err| (relative.clone(), format!("read failed: {err}")))?;
        entries.insert(relative, (target, digest12(&bytes)));
    }
    Ok(entries)
}

async fn apply_restore(
    workspace_root: &Path,
    target: &Path,
    content: Option<&str>,
) -> Result<bool, String> {
    recheck_restore_target(workspace_root, target).map_err(|err| err.to_string())?;

    let exists = target.is_file();
    match content {
        Some(expected) => {
            if exists {
                let current = fs::read(target)
                    .await
                    .map_err(|err| format!("read failed: {err}"))?;
                if current == expected.as_bytes() {
                    return Ok(false);
                }
            }
            if let Some(parent) = target.parent() {
                recheck_restore_target(workspace_root, target).map_err(|err| err.to_string())?;
                fs::create_dir_all(parent)
                    .await
                    .map_err(|err| format!("create parent dir failed: {err}"))?;
            }
            recheck_restore_target(workspace_root, target).map_err(|err| err.to_string())?;
            fs::write(target, expected)
                .await
                .map_err(|err| format!("write failed: {err}"))?;
            Ok(true)
        }
        None => {
            if exists {
                recheck_restore_target(workspace_root, target).map_err(|err| err.to_string())?;
                fs::remove_file(target)
                    .await
                    .map_err(|err| format!("remove failed: {err}"))?;
                Ok(true)
            } else {
                Ok(false)
            }
        }
    }
}
