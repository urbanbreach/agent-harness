use super::{
    runtime::{JobKind, Runtime},
    *,
};
use crate::tool::{ArtifactRef, ToolCapability};

#[derive(Clone)]
pub struct EditReceipt {
    pub edit_id: String,
    pub diff: ArtifactRef,
}
pub(super) struct PendingEdit {
    task: String,
    actor: EventActor,
    path: String,
    receipt: EditReceipt,
    snapshot: Option<super::workspace::SnapshotEdit>,
}
impl CoordinatorHandle {
    pub async fn retain_tool_diff(
        &self,
        task: String,
        diff: String,
    ) -> Result<ArtifactRef, CoordinatorError> {
        self.call(move |s| {
            s.check_task(&task)?;
            let job = &s.running[&task];
            if job.join_id.is_none()
                || !matches!(
                    job.kind,
                    JobKind::Tool {
                        capability: ToolCapability::EditFs,
                        ..
                    }
                )
            {
                return Err(CoordinatorError::PermissionDenied(
                    "diff previews require an approved editing tool".into(),
                ));
            }
            let actor = job.actor.clone();
            s.write_artifact(&actor, Some(&task), "diff", &diff)
        })
        .await
    }

    pub async fn begin_tool_edit(
        &self,
        task: impl Into<String>,
        path: PathBuf,
        diff: String,
        expected_digest: Option<String>,
    ) -> Result<EditReceipt, CoordinatorError> {
        let task = task.into();
        self.call(move |s| s.begin_edit(&task, &path, &diff, expected_digest))
            .await
    }
    pub async fn finish_tool_edit(
        &self,
        task: impl Into<String>,
        edit: impl Into<String>,
        result: Result<String, String>,
    ) -> Result<(), CoordinatorError> {
        let (task, edit) = (task.into(), edit.into());
        self.call(move |s| s.finish_edit(&task, &edit, result))
            .await
    }
}
impl Runtime {
    fn begin_edit(
        &mut self,
        task: &str,
        path: &std::path::Path,
        diff: &str,
        expected_digest: Option<String>,
    ) -> Result<EditReceipt, CoordinatorError> {
        self.check_task(task)?;
        let job = self
            .running
            .get(task)
            .ok_or_else(|| CoordinatorError::UnknownTask(task.into()))?;
        let authorized = job.join_id.is_some()
            && matches!(&job.kind, JobKind::Tool { capability: ToolCapability::EditFs, paths, .. } if paths.iter().any(|p| p == path));
        if !authorized {
            return Err(CoordinatorError::PermissionDenied(
                "edit path was not approved for this tool call".into(),
            ));
        }
        let actor = job.actor.clone();
        let snapshot = self.capture_edit(task, path, expected_digest)?;
        let path = path
            .strip_prefix(&self.info()?.workspace_root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        let receipt = EditReceipt {
            edit_id: self.id("edit")?,
            diff: self.write_artifact(&actor, Some(task), "diff", diff)?,
        };
        self.emit(
            actor.clone(),
            Some(task.into()),
            EventV1::EditProposed(EditProposedEvent {
                edit_id: receipt.edit_id.clone(),
                path: path.clone(),
                summary: format!("Update {path}"),
                patch_digest: receipt.diff.digest.clone(),
            }),
        )?;
        self.edits.insert(
            receipt.edit_id.clone(),
            PendingEdit {
                task: task.into(),
                actor,
                path,
                receipt: receipt.clone(),
                snapshot,
            },
        );
        Ok(receipt)
    }
    fn finish_edit(
        &mut self,
        task: &str,
        id: &str,
        result: Result<String, String>,
    ) -> Result<(), CoordinatorError> {
        if self.edits.get(id).is_none_or(|edit| edit.task != task) {
            return Err(CoordinatorError::Invalid(
                "unknown edit for this tool call".into(),
            ));
        }
        let edit = self
            .edits
            .remove(id)
            .ok_or_else(|| CoordinatorError::Invalid("edit disappeared".into()))?;
        if result.is_err()
            && let Some(snapshot) = &edit.snapshot
        {
            self.reject_snapshot_edit(snapshot, &edit.actor)?;
        }
        let payload = match &result {
            Ok(digest) => EventV1::EditApplied(EditAppliedEvent {
                edit_id: id.into(),
                path: edit.path.clone(),
                new_file_digest: digest.chars().take(12).collect(),
                diff_rel_path: Some(edit.receipt.diff.path),
                diff_digest: Some(edit.receipt.diff.digest),
            }),
            Err(reason) => EventV1::EditRejected(EditRejectedEvent {
                edit_id: id.into(),
                path: edit.path.clone(),
                reason: reason.clone(),
            }),
        };
        self.emit(edit.actor.clone(), Some(task.into()), payload)?;
        if let Ok(digest) = result
            && let Err(error) = self.record_edit_attribution(&edit.path, &digest)
        {
            let message = self.redactor.redact_text(&format!(
                "File edit succeeded, but attribution was not recorded: {error}"
            ));
            let _ = self.live(
                edit.actor,
                task.into(),
                LiveEventV1::RuntimeWarning { message },
            );
        }
        Ok(())
    }
    fn record_edit_attribution(&self, path: &str, digest: &str) -> Result<(), CoordinatorError> {
        if std::path::Path::new(path).is_absolute() {
            return Ok(()); // Attribution is scoped to the run workspace.
        }
        let root = &self.info()?.workspace_root;
        let bytes = crate::store::read_private_bytes(&root.join(path), 8 * 1024 * 1024)?
            .unwrap_or_default();
        if blake3::hash(&bytes).to_hex().as_str() != digest {
            return Err(CoordinatorError::Invalid(
                "file changed after the edit".into(),
            ));
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| CoordinatorError::Invalid("attribution requires text".into()))?;
        if self.redactor.redact_text(text) != text || self.redactor.redact_text(path) != path {
            return Err(CoordinatorError::Invalid(
                "snapshot contains a credential".into(),
            ));
        }
        let runtime_dir =
            crate::storage_paths::ProjectPaths::new(&self.config.data_dir, root)?.runtime_dir();
        crate::edit_attribution::EditAttributionJournal::empty(root, &runtime_dir)
            .record_agent_tool_edit(path, &bytes, None)
            .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
        Ok(())
    }
    pub fn reject_unfinished_edits(&mut self, task: &str) -> Result<(), CoordinatorError> {
        let edits: Vec<_> = self
            .edits
            .iter()
            .filter(|(_, edit)| edit.task == task)
            .map(|(id, _)| id.clone())
            .collect();
        for id in edits {
            self.finish_edit(
                task,
                &id,
                Err("edit did not settle; inspect the file before retrying".into()),
            )?;
        }
        Ok(())
    }
}
