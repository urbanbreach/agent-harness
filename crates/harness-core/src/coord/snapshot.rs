use std::collections::BTreeMap;
use std::path::Path;

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
struct SnapshotEntry {
    digest: String,
    content: String,
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

        let entries = collect_workspace_entries(&workspace_root)
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
) -> Result<BTreeMap<String, SnapshotEntry>, std::io::Error> {
    let mut entries = BTreeMap::new();
    let mut stack: Vec<std::path::PathBuf> = vec![workspace_root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let mut read_dir = fs::read_dir(&dir).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let path = entry.path();
            let relative = path.strip_prefix(workspace_root).unwrap_or(&path);
            let relative_str = normalize_relative_path(relative);

            if should_ignore_path(&relative_str) {
                continue;
            }

            let file_type = entry.file_type().await?;
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                let content = fs::read_to_string(&path).await?;
                let digest = digest12(content.as_bytes());
                entries.insert(relative_str, SnapshotEntry { digest, content });
            }
        }
    }

    Ok(entries)
}

fn should_ignore_path(relative: &str) -> bool {
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

fn normalize_relative_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
