use super::{files, *};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileSnapshotEntry {
    pub path: String,
    pub content: String,
}
#[derive(Debug, Serialize)]
pub struct SavedSessionRewindReport {
    pub cutoff_seq: u64,
    pub retained_event_count: usize,
    pub discarded_event_count: usize,
    pub conversation_message_count: usize,
    pub files_restored: usize,
    pub files_unchanged: usize,
    pub events_append_only: bool,
}
/// A historical view only; the live conversation cursor and source history stay unchanged.
pub fn plan_saved_session_rewind(
    events: &[EventEnvelopeV1],
    cutoff: u64,
) -> Result<SavedSessionRewindReport, CoordinatorError> {
    crate::proj::checked_history(events).map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
    if cutoff == 0 || events.last().is_none_or(|last| cutoff > last.seq) {
        return Err(CoordinatorError::Invalid(
            "cutoff must be within the stored history".into(),
        ));
    }
    let retained = events.partition_point(|e| e.seq <= cutoff);
    let transcript = crate::transcript_projection::project_transcript(&events[..retained])
        .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
    Ok(SavedSessionRewindReport {
        cutoff_seq: cutoff,
        retained_event_count: retained,
        discarded_event_count: events.len() - retained,
        conversation_message_count: transcript
            .messages
            .iter()
            .filter(|message| {
                message.role != crate::transcript_projection::ProjectedMessageRole::System
            })
            .count(),
        files_restored: 0,
        files_unchanged: 0,
        events_append_only: true,
    })
}
impl CoordinatorHandle {
    /// Restores an operator-supplied snapshot after obtaining exclusive access to saved history.
    pub async fn restore_saved_session_snapshot(
        &self,
        id: String,
        cutoff: u64,
        workspace: PathBuf,
        snapshot: Vec<FileSnapshotEntry>,
    ) -> Result<SavedSessionRewindReport, CoordinatorError> {
        self.call(move |runtime| runtime.restore_saved_snapshot(&id, cutoff, &workspace, snapshot))
            .await
    }
}
impl Runtime {
    fn restore_saved_snapshot(
        &self,
        id: &str,
        cutoff: u64,
        workspace: &Path,
        snapshot: Vec<FileSnapshotEntry>,
    ) -> Result<SavedSessionRewindReport, CoordinatorError> {
        if self.info.is_some() {
            return Err(CoordinatorError::RunAlreadyStarted);
        }
        if snapshot.len() > 1024 {
            return Err(CoordinatorError::Invalid(
                "snapshot exceeds 1024 files".into(),
            ));
        }
        let journal = crate::store::Journal::open_existing(&self.config.session_dir, id, false)?;
        let length = fs::metadata(journal.file_path())?.len();
        if length > 64 * 1024 * 1024 {
            return Err(CoordinatorError::Invalid(
                "session history exceeds 64 MiB".into(),
            ));
        }
        let mut report =
            plan_saved_session_rewind(&crate::store::read_events(journal.file_path())?, cutoff)?;
        let workspace = workspace.canonicalize()?;
        if !workspace.is_dir() {
            return Err(CoordinatorError::Invalid(
                "workspace is not a directory".into(),
            ));
        }
        let sessions = self.config.session_dir.canonicalize()?;
        let mut paths = std::collections::BTreeSet::new();
        let mut changes = Vec::new();
        let mut total = 0usize;
        for entry in snapshot {
            let path = workspace.join(files::relative_path(&entry.path)?);
            if path.starts_with(&sessions)
                || path == workspace.join(super::super::grants::GRANTS_FILE)
            {
                return Err(CoordinatorError::PermissionDenied(
                    "snapshot cannot replace managed history or permission grants".into(),
                ));
            }
            if !paths.insert(path.clone()) || entry.content.len() > 8 * 1024 * 1024 {
                return Err(CoordinatorError::Invalid(
                    "snapshot has a duplicate path or file larger than 8 MiB".into(),
                ));
            }
            let current = files::read(&path)?;
            if current.as_ref().map(|file| file.bytes.as_slice()) == Some(entry.content.as_bytes())
            {
                report.files_unchanged += 1;
                continue;
            }
            total = total
                .saturating_add(entry.content.len())
                .saturating_add(current.as_ref().map_or(0, |file| file.bytes.len()));
            if total > 64 * 1024 * 1024 {
                return Err(CoordinatorError::Invalid(
                    "snapshot restore exceeds 64 MiB".into(),
                ));
            }
            changes.push(files::Change {
                relative: entry.path,
                path,
                before: Some(entry.content.into_bytes()),
                mode: current.as_ref().map_or(0o600, |file| file.mode),
                current,
            });
        }
        for (index, change) in changes.iter().enumerate() {
            if let Err(error) = change.restore() {
                return Err(files::rollback(&changes[..=index], error));
            }
        }
        report.files_restored = changes.len();
        Ok(report)
    }
}
