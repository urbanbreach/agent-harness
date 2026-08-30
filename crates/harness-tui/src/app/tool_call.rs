// allow: SIZE_OK — TUI app state (session projection + interaction)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallPresentationStatus {
    Queued,
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolCallPresentation {
    pub status: ToolCallPresentationStatus,
    pub duration_ms: Option<u64>,
    pub result_count: Option<u64>,
}

impl ToolCallPresentation {
    pub const fn from_display_status(status: ToolCallDisplayStatus) -> Self {
        Self {
            status: match status {
                ToolCallDisplayStatus::PendingPermission => ToolCallPresentationStatus::Waiting,
                ToolCallDisplayStatus::Queued => ToolCallPresentationStatus::Queued,
                ToolCallDisplayStatus::Running => ToolCallPresentationStatus::Running,
                ToolCallDisplayStatus::Succeeded => ToolCallPresentationStatus::Succeeded,
                ToolCallDisplayStatus::Failed => ToolCallPresentationStatus::Failed,
            },
            duration_ms: None,
            result_count: None,
        }
    }
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

    pub fn presentation(&self) -> ToolCallPresentation {
        let status = if self.has_correlated_background_cancellation() {
            ToolCallPresentationStatus::Cancelled
        } else {
            ToolCallPresentation::from_display_status(self.status).status
        };
        let terminal = matches!(
            status,
            ToolCallPresentationStatus::Succeeded
                | ToolCallPresentationStatus::Failed
                | ToolCallPresentationStatus::Cancelled
        );

        ToolCallPresentation {
            status,
            duration_ms: terminal.then_some(self.timing_elapsed_ms).flatten(),
            result_count: terminal.then(|| self.structured_result_count()).flatten(),
        }
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

    fn has_correlated_background_cancellation(&self) -> bool {
        if self.status != ToolCallDisplayStatus::Succeeded
            || !matches!(
                self.effective_tool_id(),
                "background_cancel" | "background_output"
            )
        {
            return false;
        }

        let Some(requested_id) = json_string_field_from_text(&self.args_summary, "request_id")
        else {
            return false;
        };
        let Some(output) = self.output_json.as_ref() else {
            return false;
        };
        let Some(output_id) = output.get("request_id").and_then(serde_json::Value::as_str) else {
            return false;
        };
        let cancelled = output
            .get("final_status")
            .or_else(|| output.get("status"))
            .and_then(serde_json::Value::as_str)
            == Some("cancelled");

        cancelled && requested_id == output_id
    }

    fn structured_result_count(&self) -> Option<u64> {
        const RESULT_COUNT_FIELDS: [&str; 6] = [
            "result_count",
            "match_count",
            "file_count",
            "entry_count",
            "processed_call_count",
            "total_count",
        ];

        let output = self.output_json.as_ref()?;
        RESULT_COUNT_FIELDS
            .iter()
            .find_map(|field| output.get(*field).and_then(serde_json::Value::as_u64))
    }
}

fn json_string_field_from_text(text: &str, field: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()?
        .get(field)?
        .as_str()
        .map(str::to_string)
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

pub(in crate::app) fn display_status_for_tool_call(
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

#[cfg(test)]
mod presentation_tests {
    use super::*;

    fn tool_call(tool_id: &str) -> ToolCallEntry {
        ToolCallEntry {
            tool_call_id: "tool-call".to_string(),
            tool_id: tool_id.to_string(),
            canonical_tool_id: None,
            alias_source_tool_id: None,
            resolved_tool_identity: None,
            args_summary: "{}".to_string(),
            args_digest: "digest".to_string(),
            lifecycle_state: Some(ToolCallLifecycleState::Completed),
            status: ToolCallDisplayStatus::Succeeded,
            output_summary: None,
            output_digest: None,
            output_json: None,
            truncated_output: None,
            edit: None,
            lineage: None,
            artifact_refs: Vec::new(),
            timing_elapsed_ms: None,
            permissions: Vec::new(),
            first_seq: 1,
            last_seq: 2,
            first_mono_ms: 100,
            last_mono_ms: 1_350,
            first_timestamp: None,
            last_timestamp: None,
        }
    }

    #[test]
    fn unresolved_permission_is_explicit_waiting_presentation() {
        // arrange
        let mut tool_call = tool_call("bash");
        tool_call.lifecycle_state = Some(ToolCallLifecycleState::Running);
        tool_call.status = ToolCallDisplayStatus::Running;
        tool_call.permissions.push(PermissionEntry {
            permission_id: "permission".to_string(),
            kind: "bash".to_string(),
            tool_call_id: Some(tool_call.tool_call_id.clone()),
            summary: "Run command".to_string(),
            request_digest: "permission-digest".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
            resolved_decision: None,
            resolution_reason: None,
            first_seq: 2,
            last_seq: 2,
        });

        // act
        tool_call.sync_display_status();

        // assert
        assert_eq!(
            tool_call.presentation().status,
            ToolCallPresentationStatus::Waiting
        );
    }

    #[test]
    fn correlated_structured_background_cancellation_is_cancelled_presentation() {
        // arrange
        // act
        for (tool_id, args_summary, output_json) in [
            (
                "background_cancel",
                r#"{"request_id":"req-child"}"#,
                serde_json::json!({
                    "request_id": "req-child",
                    "final_status": "cancelled"
                }),
            ),
            (
                "background_output",
                r#"{"request_id":"req-child","cancel":true}"#,
                serde_json::json!({
                    "request_id": "req-child",
                    "status": "cancelled"
                }),
            ),
        ] {
            let mut tool_call = tool_call(tool_id);
            tool_call.args_summary = args_summary.to_string();
            tool_call.output_json = Some(output_json);

            // assert
            assert_eq!(
                tool_call.presentation().status,
                ToolCallPresentationStatus::Cancelled
            );
        }
    }

    #[test]
    fn cancellation_prose_or_mismatched_request_id_is_not_cancelled_presentation() {
        // arrange
        let mut prose_only = tool_call("background_cancel");
        prose_only.args_summary = r#"{"request_id":"req-child"}"#.to_string();
        prose_only.output_summary = Some("Cancelled background task req-child".to_string());
        assert_eq!(
            prose_only.presentation().status,
            ToolCallPresentationStatus::Succeeded
        );

        // act
        let mut mismatched = tool_call("background_cancel");
        mismatched.args_summary = r#"{"request_id":"req-child"}"#.to_string();
        mismatched.output_json = Some(serde_json::json!({
            "request_id": "another-child",
            "status": "cancelled"
        }));
        // assert
        assert_eq!(
            mismatched.presentation().status,
            ToolCallPresentationStatus::Succeeded
        );
    }

    #[test]
    fn presentation_carries_explicit_duration_and_result_count_metadata() {
        // arrange
        let mut tool_call = tool_call("grep");
        tool_call.timing_elapsed_ms = Some(1_250);
        tool_call.output_json = Some(serde_json::json!({ "match_count": 7 }));

        // act
        let presentation = tool_call.presentation();

        // assert
        assert_eq!(presentation.duration_ms, Some(1_250));
        assert_eq!(presentation.result_count, Some(7));
    }
}
