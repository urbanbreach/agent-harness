use super::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProjectedToolCallPart {
    pub tool_call_id: crate::ids::ToolCallId,
    pub tool_id: String,
    pub args_summary: String,
    pub args_digest: String,
    pub state: ProjectedToolCallState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ToolCallStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_json: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ToolCallMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<ProjectedPermissionPart>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<TranscriptArtifactRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage: Option<SessionLineageProjection>,
    pub provenance: ProvenanceRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Copy, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectedToolCallState {
    #[default]
    Pending,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectedPermissionPart {
    pub permission_id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<crate::ids::ToolCallId>,
    pub summary: String,
    pub request_digest: String,
    pub timeout_ms: u64,
    pub default_decision: PermissionDecision,
    pub state: ProjectedPermissionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<PermissionDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub provenance: ProvenanceRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Copy, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectedPermissionState {
    #[default]
    Pending,
    Resolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProjectedCompactionPart {
    pub checkpoint_id: Option<String>,
    pub agent_id: String,
    pub status: CompactionCheckpointStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<TranscriptArtifactRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_before: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_hook: Option<bool>,
    pub provenance: ProvenanceRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectedTaskPart {
    pub task_id: crate::ids::TaskId,
    pub state: ProjectedTaskState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage: Option<SessionLineageProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_scope: Option<TaskTerminalScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing_elapsed_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_mono_ms: Option<u64>,
    pub provenance: ProvenanceRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ProjectedTaskState {
    Queued,
    Started,
    Cancelled,
    Failed,
    Completed,
    LateResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CompactionCheckpointProjection {
    pub checkpoint_id: Option<String>,
    pub agent_id: String,
    pub status: CompactionCheckpointStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_before: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_before_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_after_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_tokens_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compacted_turns: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduction_tokens_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduction_percent_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserved_turns: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<TranscriptArtifactRef>,
    pub provenance: ProvenanceRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Copy, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompactionCheckpointStatus {
    #[default]
    Requested,
    Written,
    Applied,
    Failed,
    SessionCompacted,
    BranchSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptArtifactRef {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<crate::ids::ToolCallId>,
    pub source: ArtifactProjectionSource,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
    pub provenance: ProvenanceRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactProjectionSource {
    ArtifactWritten,
    ToolCallMetadata,
    CompactionWritten,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SessionLineageProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_model_id: Option<String>,
    pub provenance: ProvenanceRange,
}
