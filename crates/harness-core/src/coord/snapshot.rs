use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::digest::digest12;
use crate::event::{EventV1, WorkspaceSnapshotEvent};

use super::{
    append_payload_event_with_correlation, system_actor, Coordinator, CoordinatorError,
    WorkspaceSnapshotSummary,
};

const SNAPSHOTS_DIR: &str = "snapshots";
const DEFAULT_IGNORED_DIRS: &[&str] = &[".git", ".agent-harness", "target"];
const DEFAULT_IGNORED_FILES: &[&str] = &[".envrc"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SnapshotEntry {
    pub(super) digest: String,
    pub(super) content: Option<String>,
}

impl Coordinator {
    pub(in crate::coord) async fn snapshot_workspace_internal(
        &mut self,
        request_id: String,
    ) -> Result<WorkspaceSnapshotSummary, CoordinatorError> {
        let run_state = self
            .run_state
            .as_ref()
            .ok_or(CoordinatorError::RunNotStarted)?;
        let workspace_root = run_state.info.workspace_root.clone();
        let artifacts_dir = run_state.info.artifacts_dir.clone();

        let snapshot_dir = artifacts_dir.join(SNAPSHOTS_DIR);
        fs::create_dir_all(&snapshot_dir)
            .await
            .map_err(|err| CoordinatorError::SnapshotFailed(format!("create dir: {err}")))?;

        let entries = collect_workspace_entries(&workspace_root, self.redactor.as_ref())
            .await
            .map_err(|err| CoordinatorError::SnapshotFailed(err.to_string()))?;
        let file_count = entries.len();
        let payload: BTreeMap<String, SnapshotEntry> = entries;

        let mut value = serde_json::to_value(&payload)
            .map_err(|err| CoordinatorError::SnapshotFailed(err.to_string()))?;
        value = crate::redact::redact_value(self.redactor.as_ref(), &value);

        let artifact_path = Path::new(SNAPSHOTS_DIR)
            .join(format!("{request_id}.json"))
            .to_string_lossy()
            .to_string();
        let absolute_path = artifacts_dir.join(&artifact_path);
        let bytes = serde_json::to_vec(&value)
            .map_err(|err| CoordinatorError::SnapshotFailed(err.to_string()))?;
        let artifact_digest = digest12(&bytes);

        fs::write(&absolute_path, &bytes)
            .await
            .map_err(|err| CoordinatorError::SnapshotFailed(format!("write artifact: {err}")))?;

        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            system_actor(),
            Some(format!("snapshot:{request_id}")),
            Some(request_id.clone()),
            EventV1::WorkspaceSnapshot(WorkspaceSnapshotEvent {
                request_id: request_id.clone().into(),
                artifact_path: artifact_path.clone(),
                artifact_digest,
                file_count,
            }),
        )?;

        Ok(WorkspaceSnapshotSummary {
            request_id: request_id.into(),
            artifact_path,
            file_count,
        })
    }
}

async fn collect_workspace_entries(
    workspace_root: &Path,
    redactor: &(impl crate::redact::Redactor + ?Sized),
) -> Result<BTreeMap<String, SnapshotEntry>, std::io::Error> {
    let mut entries = BTreeMap::new();
    for path in workspace_files(workspace_root).await? {
        let relative = normalize_relative_path(path.strip_prefix(workspace_root).unwrap_or(&path));
        let bytes = fs::read(&path).await?;
        let digest = digest12(&bytes);
        let content = String::from_utf8(bytes)
            .ok()
            .filter(|text| !text.contains('\0'))
            .filter(|text| redactor.redact_text(text) == *text)
            .filter(|text| {
                // Structured config credentials must not hide inside a JSON string field.
                json5::from_str::<serde_json::Value>(text)
                    .ok()
                    .is_none_or(|value| crate::redact::redact_value(redactor, &value) == value)
            });
        entries.insert(relative, SnapshotEntry { digest, content });
    }
    Ok(entries)
}

pub(super) async fn workspace_files(workspace_root: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut files = Vec::new();
    // Use Git's own exclusions for repository snapshots. This also excludes generated
    // session histories and snapshot artifacts instead of recursively capturing them.
    if workspace_root.join(".git").exists() {
        let root = workspace_root.to_path_buf();
        let paths = tokio::task::spawn_blocking(move || {
            crate::vcs::git_output(
                &root,
                &[
                    "ls-files",
                    "--cached",
                    "--others",
                    "--exclude-standard",
                    "-z",
                ],
            )
        })
        .await
        .map_err(std::io::Error::other)?
        .map_err(std::io::Error::other)?;
        for relative in paths
            .split('\0')
            .filter(|path| !path.is_empty() && !should_ignore_path(path))
        {
            let path = workspace_root.join(relative);
            match fs::symlink_metadata(&path).await {
                Ok(metadata) if metadata.is_file() => files.push(path),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        files.sort();
        files.dedup();
        return Ok(files);
    }
    let mut stack = vec![workspace_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut read_dir = fs::read_dir(&dir).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let path = entry.path();
            let relative =
                normalize_relative_path(path.strip_prefix(workspace_root).unwrap_or(&path));
            if should_ignore_path(&relative) {
                continue;
            }
            let file_type = entry.file_type().await?;
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                files.push(path);
            }
        }
    }
    Ok(files)
}

pub(super) fn should_ignore_path(relative: &str) -> bool {
    if relative.is_empty() {
        return false;
    }
    let normalized = relative.replace('\\', "/");
    let segments: Vec<&str> = normalized.split('/').filter(|s| !s.is_empty()).collect();
    for ignored in DEFAULT_IGNORED_DIRS {
        if segments.iter().any(|segment| segment == ignored) {
            return true;
        }
    }
    if let Some(file_name) = segments.last() {
        if DEFAULT_IGNORED_FILES.contains(file_name)
            || *file_name == ".env"
            || file_name.starts_with(".env.")
            || file_name.ends_with(".env")
        {
            return true;
        }
    }
    false
}

pub(super) fn normalize_relative_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
