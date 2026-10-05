use super::{handle::system, *};
use crate::tool::ArtifactRef;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
mod files;
mod saved;
pub use saved::{plan_saved_session_rewind, FileSnapshotEntry, SavedSessionRewindReport};

pub struct WorkspaceSnapshotSummary {
    pub request_id: crate::ids::RequestId,
    pub artifact_path: String,
    pub file_count: usize,
}
pub struct WorkspaceRevertSummary {
    pub request_id: crate::ids::RequestId,
    pub restored_paths: Vec<String>,
    pub removed_paths: Vec<String>,
    pub failed_paths: Vec<(String, String)>,
    pub conflicts: Vec<(String, String)>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot {
    request: String,
    files: BTreeMap<String, SnapshotFile>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotFile {
    before: Option<ArtifactRef>,
    mode: u32,
    expected: Option<String>,
    unavailable: Option<String>,
}
pub(super) struct SnapshotEdit {
    request: String,
    path: String,
    previous: Option<String>,
}

impl CoordinatorHandle {
    pub async fn revert_workspace(
        &self,
        request: impl Into<String>,
    ) -> Result<WorkspaceRevertSummary, CoordinatorError> {
        let request = request.into();
        self.call(move |s| s.revert_workspace(&request)).await
    }
}
impl Runtime {
    pub fn capture_edit(
        &mut self,
        task: &str,
        path: &Path,
        expected: Option<String>,
    ) -> Result<Option<SnapshotEdit>, CoordinatorError> {
        if expected.as_deref().is_some_and(|s| !files::valid_digest(s)) {
            return Err(CoordinatorError::Invalid(
                "invalid expected edit digest".into(),
            ));
        }
        let Ok(relative) = path.strip_prefix(&self.info()?.workspace_root) else {
            return Ok(None); // Workspace undo does not reach outside the workspace.
        };
        let relative = relative
            .to_str()
            .ok_or_else(|| CoordinatorError::Invalid("snapshot path is not UTF-8".into()))?
            .to_owned();
        files::relative_path(&relative)?;
        let job = self
            .running
            .get(task)
            .ok_or_else(|| CoordinatorError::UnknownTask(task.into()))?;
        let (request, actor) = (
            job.parent.clone().unwrap_or_else(|| task.into()),
            job.actor.clone(),
        );
        let previous = files::read(path)?;
        let previous_digest = previous.as_ref().map(|file| files::digest(&file.bytes));
        let mut file = if let Some(file) = self
            .snapshots
            .get(&request)
            .and_then(|s| s.files.get(&relative))
        {
            let mut file = file.clone();
            if file.expected != previous_digest {
                file.unavailable = Some("file changed outside this turn's recorded edits".into());
            }
            file
        } else {
            let (before, unavailable) =
                match previous.as_ref().map(|f| std::str::from_utf8(&f.bytes)) {
                    Some(Ok(text)) if self.redactor.redact_text(text) == text => (
                        Some(self.write_artifact(&actor, Some(task), "txt", text)?),
                        None,
                    ),
                    Some(_) => (
                        None,
                        Some("the original file cannot be retained as credential-free text".into()),
                    ),
                    None => (None, None),
                };
            SnapshotFile {
                before,
                mode: previous.as_ref().map_or(0, |file| file.mode),
                expected: previous_digest,
                unavailable,
            }
        };
        let previous = file.expected.clone();
        file.expected = expected;
        let snapshot = self
            .snapshots
            .entry(request.clone())
            .or_insert_with(|| Snapshot {
                request: request.clone(),
                files: BTreeMap::new(),
            });
        if snapshot.files.len() >= 1024 && !snapshot.files.contains_key(&relative) {
            return Err(CoordinatorError::Invalid(
                "a turn snapshot exceeds 1024 files".into(),
            ));
        }
        snapshot.files.insert(relative.clone(), file);
        self.persist_snapshot(&request, &actor)?;
        Ok(Some(SnapshotEdit {
            request,
            path: relative,
            previous,
        }))
    }
    fn persist_snapshot(
        &mut self,
        request: &str,
        actor: &EventActor,
    ) -> Result<(), CoordinatorError> {
        let snapshot = self
            .snapshots
            .get(request)
            .ok_or_else(|| CoordinatorError::Invalid("missing active snapshot".into()))?;
        let count = snapshot.files.len();
        let json = serde_json::to_string(snapshot)?;
        if json.len() > 1024 * 1024 {
            return Err(CoordinatorError::Invalid(
                "snapshot manifest exceeds 1 MiB".into(),
            ));
        }
        let artifact = self.write_artifact(actor, None, "json", &json)?;
        self.emit(
            actor.clone(),
            Some(request.into()),
            EventV1::WorkspaceSnapshot(WorkspaceSnapshotEvent {
                request_id: request.into(),
                artifact_path: artifact.path,
                artifact_digest: artifact.digest,
                file_count: count,
            }),
        )?;
        Ok(())
    }
    pub fn reject_snapshot_edit(
        &mut self,
        edit: &SnapshotEdit,
        actor: &EventActor,
    ) -> Result<(), CoordinatorError> {
        if let Some(file) = self
            .snapshots
            .get_mut(&edit.request)
            .and_then(|s| s.files.get_mut(&edit.path))
        {
            file.expected = edit.previous.clone();
            self.persist_snapshot(&edit.request, actor)?;
        }
        Ok(())
    }
    fn revert_workspace(
        &mut self,
        request: &str,
    ) -> Result<WorkspaceRevertSummary, CoordinatorError> {
        self.accepting()?;
        if !self.running.is_empty() || self.agents.values().any(|a| !a.queue.is_empty()) {
            return Err(CoordinatorError::Invalid(
                "wait for active work to finish before reverting".into(),
            ));
        }
        let info = self.info()?.clone();
        let mut reference = None;
        // ponytail: undo scans the journal on demand; no per-turn snapshot cache survives completion.
        for event in crate::store::JournalReader::open(
            &info.events_path,
            fs::metadata(&info.events_path)?.len(),
        )? {
            if let EventV1::WorkspaceSnapshot(snapshot) = event?.payload
                && snapshot.request_id.as_str() == request
            {
                reference = Some(snapshot);
            }
        }
        let reference = reference
            .ok_or_else(|| CoordinatorError::Invalid("workspace snapshot was not found".into()))?;
        let bytes = files::artifact(
            &info.run_dir,
            &ArtifactRef {
                path: reference.artifact_path,
                digest: reference.artifact_digest,
            },
            "json",
            1024 * 1024,
        )?;
        let snapshot: Snapshot = serde_json::from_slice(&bytes)
            .map_err(|_| CoordinatorError::Invalid("invalid workspace snapshot".into()))?;
        if snapshot.request != request
            || snapshot.files.len() != reference.file_count
            || snapshot.files.len() > 1024
        {
            return Err(CoordinatorError::Invalid(
                "workspace snapshot identity or file count mismatch".into(),
            ));
        }
        let mut changes = Vec::new();
        let mut total = 0usize;
        for (relative, file) in &snapshot.files {
            let path = info.workspace_root.join(files::relative_path(relative)?);
            if let Some(reason) = &file.unavailable {
                return Err(CoordinatorError::Invalid(format!(
                    "cannot revert {relative}: {reason}"
                )));
            }
            let before = file
                .before
                .as_ref()
                .map(|reference| files::artifact(&info.run_dir, reference, "txt", 8 * 1024 * 1024))
                .transpose()?;
            let current = files::read(&path)?;
            let digest = current.as_ref().map(|file| files::digest(&file.bytes));
            if digest == before.as_deref().map(files::digest) {
                continue;
            }
            if digest != file.expected {
                return Err(CoordinatorError::Invalid(format!(
                    "revert conflict: {relative} changed after the recorded edit"
                )));
            }
            total = total
                .saturating_add(before.as_ref().map_or(0, Vec::len))
                .saturating_add(current.as_ref().map_or(0, |file| file.bytes.len()));
            if total > 64 * 1024 * 1024 {
                return Err(CoordinatorError::Invalid(
                    "revert exceeds 64 MiB of file data".into(),
                ));
            }
            changes.push(files::Change {
                relative: relative.clone(),
                path,
                before,
                mode: file.mode,
                current,
            });
        }
        let id = self.id("revert")?;
        self.emit(
            system(),
            Some(id.clone()),
            EventV1::UiIntentReceived(UiIntentReceivedEvent {
                intent: "workspace_revert".into(),
                params: BTreeMap::from([("snapshot_request_id".into(), request.into())]),
            }),
        )?;
        let mut summary = WorkspaceRevertSummary {
            request_id: id.clone().into(),
            restored_paths: Vec::new(),
            removed_paths: Vec::new(),
            failed_paths: Vec::new(),
            conflicts: Vec::new(),
        };
        for (index, change) in changes.iter().enumerate() {
            if let Err(error) = change.restore() {
                return Err(files::rollback(&changes[..=index], error));
            }
            if change.before.is_some() {
                summary.restored_paths.push(change.relative.clone());
            } else {
                summary.removed_paths.push(change.relative.clone());
            }
        }
        if let Err(error) = self.emit(
            system(),
            Some(id),
            EventV1::WorkspaceReverted(WorkspaceRevertedEvent {
                request_id: summary.request_id.clone(),
                snapshot_request_id: request.into(),
                restored_paths: summary.restored_paths.clone(),
                removed_paths: summary.removed_paths.clone(),
                failed_paths: Vec::new(),
                conflicts: Vec::new(),
            }),
        ) {
            return Err(files::rollback(&changes, error));
        }
        self.tool_state = Default::default();
        for agent in self.agents.values_mut() {
            agent.tool_state = self.tool_state.fresh_owner();
        }
        Ok(summary)
    }
}
