use super::*;

const TOOL_TRANSCRIPT_SUMMARY_MAX_CHARS: usize = 72;
const TOOL_TRANSCRIPT_SUMMARY_MAX_FIELDS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallDisplayStatus {
    PendingPermission,
    Queued,
    Running,
    Succeeded,
    Failed,
}

impl std::fmt::Display for ToolCallDisplayStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolCallDisplayStatus::PendingPermission => write!(f, "pending permission"),
            ToolCallDisplayStatus::Queued => write!(f, "queued"),
            ToolCallDisplayStatus::Running => write!(f, "running"),
            ToolCallDisplayStatus::Succeeded => write!(f, "succeeded"),
            ToolCallDisplayStatus::Failed => write!(f, "failed"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolCallEntry {
    pub tool_call_id: String,
    pub tool_id: String,
    pub canonical_tool_id: Option<String>,
    pub alias_source_tool_id: Option<String>,
    pub resolved_tool_identity: Option<ResolvedToolIdentity>,
    pub args_summary: String,
    pub args_digest: String,
    pub lifecycle_state: Option<ToolCallLifecycleState>,
    pub status: ToolCallDisplayStatus,
    pub output_summary: Option<String>,
    pub output_digest: Option<String>,
    pub output_json: Option<serde_json::Value>,
    pub truncated_output: Option<String>,
    pub edit: Option<EditEntry>,
    pub lineage: Option<TaskLineageEntry>,
    pub artifact_refs: Vec<ToolArtifactEntry>,
    pub timing_elapsed_ms: Option<u64>,
    pub permissions: Vec<PermissionEntry>,
    pub first_seq: u64,
    pub last_seq: u64,
    pub first_mono_ms: u64,
    pub last_mono_ms: u64,
    pub first_timestamp: Option<String>,
    pub last_timestamp: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLineageEntry {
    pub parent_tool_call_id: Option<String>,
    pub parent_task_id: Option<String>,
    pub parent_request_id: Option<String>,
    pub child_session_id: Option<String>,
    pub child_request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolArtifactEntry {
    pub path: String,
    pub digest: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditDisplayStatus {
    Proposed,
    Applied,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditEntry {
    pub edit_id: String,
    pub path: String,
    pub status: EditDisplayStatus,
    pub summary: Option<String>,
    pub patch_digest: Option<String>,
    pub new_file_digest: Option<String>,
    pub diff_rel_path: Option<String>,
    pub diff_digest: Option<String>,
    pub rejection_reason: Option<String>,
}

impl ToolCallEntry {
    pub fn duration_ms(&self) -> Option<u64> {
        self.timing_elapsed_ms.or_else(|| {
            (self.last_mono_ms >= self.first_mono_ms)
                .then_some(self.last_mono_ms.saturating_sub(self.first_mono_ms))
        })
    }

    pub fn invoked_tool_id(&self) -> &str {
        self.resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.invoked_tool_id.as_deref())
            .unwrap_or(&self.tool_id)
    }

    pub fn effective_tool_id(&self) -> &str {
        self.resolved_tool_identity
            .as_ref()
            .and_then(|identity| {
                identity
                    .effective_tool_id
                    .as_deref()
                    .or(identity.canonical_tool_id.as_deref())
            })
            .or(self.canonical_tool_id.as_deref())
            .unwrap_or(&self.tool_id)
    }

    pub fn resolved_canonical_tool_id(&self) -> Option<&str> {
        if let Some(identity) = self.resolved_tool_identity.as_ref() {
            identity.canonical_tool_id.as_deref()
        } else {
            self.canonical_tool_id.as_deref()
        }
    }

    pub fn resolved_alias_source_tool_id(&self) -> Option<&str> {
        if let Some(identity) = self.resolved_tool_identity.as_ref() {
            identity
                .alias_source_tool_id
                .as_deref()
                .or(self.alias_source_tool_id.as_deref())
        } else {
            self.alias_source_tool_id.as_deref()
        }
    }

    pub fn canonical_tool_id(&self) -> &str {
        self.resolved_canonical_tool_id()
            .unwrap_or_else(|| self.effective_tool_id())
    }

    pub fn lifecycle_state(&self) -> ToolCallLifecycleState {
        self.lifecycle_state.unwrap_or(match self.status {
            ToolCallDisplayStatus::PendingPermission | ToolCallDisplayStatus::Queued => {
                ToolCallLifecycleState::Pending
            }
            ToolCallDisplayStatus::Running => ToolCallLifecycleState::Running,
            ToolCallDisplayStatus::Succeeded => ToolCallLifecycleState::Completed,
            ToolCallDisplayStatus::Failed => ToolCallLifecycleState::Error,
        })
    }

    pub fn is_compat_alias(&self) -> bool {
        self.resolved_alias_source_tool_id()
            .is_some_and(|alias_source| alias_source != self.effective_tool_id())
    }

    pub(in crate::app) fn sync_display_status(&mut self) {
        self.status = display_status_for_tool_call(self.lifecycle_state(), &self.permissions);
    }

    pub fn transcript_timestamp(&self) -> Option<&str> {
        self.last_timestamp
            .as_deref()
            .or(self.first_timestamp.as_deref())
    }

    pub fn transcript_summary(&self) -> Option<String> {
        match self.status {
            ToolCallDisplayStatus::Succeeded | ToolCallDisplayStatus::Failed => self
                .output_summary
                .as_deref()
                .and_then(compact_tool_payload_for_transcript)
                .or_else(|| compact_tool_payload_for_transcript(&self.args_summary)),
            ToolCallDisplayStatus::PendingPermission
            | ToolCallDisplayStatus::Queued
            | ToolCallDisplayStatus::Running => {
                compact_tool_payload_for_transcript(&self.args_summary)
            }
        }
    }

    pub fn edit_path_display(&self) -> Option<String> {
        self.edit
            .as_ref()
            .map(|edit| edit.path.clone())
            .or_else(|| tool_path_summary(&self.args_summary))
    }
}

fn tool_path_summary(args_summary: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(args_summary).ok()?;
    trimmed_json_string_field(Some(&value), &["path", "filePath"])
}

fn compact_tool_payload_for_transcript(payload: &str) -> Option<String> {
    crate::text::compact_payload(
        payload,
        TOOL_TRANSCRIPT_SUMMARY_MAX_FIELDS,
        TOOL_TRANSCRIPT_SUMMARY_MAX_CHARS,
    )
}

fn task_lineage_entry_from_metadata(metadata: &TaskLineageMetadata) -> TaskLineageEntry {
    TaskLineageEntry {
        parent_tool_call_id: metadata.parent_tool_call_id.clone(),
        parent_task_id: metadata.parent_task_id.clone(),
        parent_request_id: metadata.parent_request_id.clone(),
        child_session_id: metadata.child_session_id.clone(),
        child_request_id: metadata.child_request_id.clone(),
    }
}

fn tool_artifact_entry_from_metadata(artifact: &EventArtifactRef) -> ToolArtifactEntry {
    ToolArtifactEntry {
        path: artifact.path.clone(),
        digest: artifact.digest.clone(),
    }
}

pub(in crate::app) fn merge_tool_call_metadata(
    entry: &mut ToolCallEntry,
    metadata: Option<&ToolCallMetadata>,
) {
    let Some(metadata) = metadata else {
        return;
    };

    if entry.canonical_tool_id.is_none() {
        entry.canonical_tool_id = metadata.canonical_tool_id.clone();
    }
    if entry.alias_source_tool_id.is_none() {
        entry.alias_source_tool_id = metadata.alias_source_tool_id.clone();
    }
    if entry.lineage.is_none() {
        entry.lineage = metadata
            .lineage
            .as_ref()
            .map(task_lineage_entry_from_metadata);
    }
    if entry.timing_elapsed_ms.is_none() {
        entry.timing_elapsed_ms = metadata
            .timing
            .as_ref()
            .and_then(execution_timing_elapsed_ms);
    }

    for artifact in &metadata.artifact_refs {
        let artifact = tool_artifact_entry_from_metadata(artifact);
        if !entry
            .artifact_refs
            .iter()
            .any(|existing| existing.path == artifact.path && existing.digest == artifact.digest)
        {
            entry.artifact_refs.push(artifact);
        }
    }
}

pub(in crate::app) fn merge_resolved_tool_identity(
    entry: &mut ToolCallEntry,
    incoming: ResolvedToolIdentity,
) {
    if incoming.is_empty() {
        return;
    }

    let identity = entry
        .resolved_tool_identity
        .get_or_insert_with(ResolvedToolIdentity::default);
    if identity.invoked_tool_id.is_none() {
        identity.invoked_tool_id = incoming.invoked_tool_id;
    }
    if identity.effective_tool_id.is_none() {
        identity.effective_tool_id = incoming.effective_tool_id;
    }
    if identity.canonical_tool_id.is_none() {
        identity.canonical_tool_id = incoming.canonical_tool_id;
    }
    if identity.alias_source_tool_id.is_none() {
        identity.alias_source_tool_id = incoming.alias_source_tool_id;
    }
}

fn display_status_for_tool_call(
    lifecycle_state: ToolCallLifecycleState,
    permissions: &[PermissionEntry],
) -> ToolCallDisplayStatus {
    if permissions
        .iter()
        .any(|permission| permission.resolved_decision.is_none())
    {
        return ToolCallDisplayStatus::PendingPermission;
    }

    match lifecycle_state {
        ToolCallLifecycleState::Pending => ToolCallDisplayStatus::Queued,
        ToolCallLifecycleState::Running => ToolCallDisplayStatus::Running,
        ToolCallLifecycleState::Completed => ToolCallDisplayStatus::Succeeded,
        ToolCallLifecycleState::Error => ToolCallDisplayStatus::Failed,
    }
}

pub(in crate::app) fn execution_timing_elapsed_ms(timing: &ExecutionTimingMetadata) -> Option<u64> {
    timing
        .elapsed_ms
        .or_else(|| match (timing.started_mono_ms, timing.finished_mono_ms) {
            (Some(started), Some(finished)) if finished >= started => {
                Some(finished.saturating_sub(started))
            }
            _ => None,
        })
}
