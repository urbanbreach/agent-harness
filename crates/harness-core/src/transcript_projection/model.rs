// allow: SIZE_OK — transcript projection (pure replay state derivation)
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::event::{EventEnvelopeV1, PermissionDecision, ToolCallMetadata, ToolCallStatus};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TranscriptProjectionError {
    #[error("events must be strictly increasing by seq: previous={previous_seq}, current={seq}")]
    EventsOutOfOrder { previous_seq: u64, seq: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TranscriptProjection {
    pub session: TranscriptSessionProjection,
    pub messages: Vec<ProjectedMessage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compaction_checkpoints: Vec<CompactionCheckpointProjection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<TranscriptArtifactRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session_lineage: Vec<SessionLineageProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TranscriptSessionProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    pub status: TranscriptRunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub agent_profiles: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptRunStatus {
    #[default]
    NotStarted,
    Running,
    Finished,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectedMessage {
    pub message_id: String,
    pub role: ProjectedMessageRole,
    pub state: ProjectedMessageState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<crate::ids::RequestId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProjectedProviderMessageMetadata>,
    pub provenance: ProvenanceRange,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<ProjectedPart>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProjectedProviderMessageMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_text_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_reasoning_digest: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectedMessageRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectedMessageState {
    #[default]
    Complete,
    Streaming,
    Incomplete,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "part_type", rename_all = "snake_case")]
pub enum ProjectedPart {
    Text(ProjectedTextPart),
    Reasoning(ProjectedTextPart),
    ToolCall(Box<ProjectedToolCallPart>),
    Permission(ProjectedPermissionPart),
    Compaction(ProjectedCompactionPart),
    Artifact(ProjectedArtifactPart),
    Lifecycle(ProjectedLifecyclePart),
    Task(ProjectedTaskPart),
    PolicyViolation(ProjectedPolicyViolationPart),
    UiIntent(ProjectedUiIntentPart),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectedTextPart {
    pub text: String,
    pub provenance: ProvenanceRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectedPermissionState {
    #[default]
    Pending,
    Resolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    pub provenance: ProvenanceRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectedArtifactPart {
    pub artifact: TranscriptArtifactRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectedLifecyclePart {
    pub event: LifecycleEventKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub provenance: ProvenanceRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleEventKind {
    RunStarted,
    RunFinished,
    RunFailed,
    AgentSpawned,
    AgentStopped,
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
    pub provenance: ProvenanceRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectedTaskState {
    Queued,
    Started,
    Cancelled,
    Completed,
    LateResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectedPolicyViolationPart {
    pub policy: String,
    pub detail: String,
    pub provenance: ProvenanceRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectedUiIntentPart {
    pub intent: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
    pub provenance: ProvenanceRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionCheckpointStatus {
    Requested,
    Written,
    Applied,
    Failed,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactProjectionSource {
    ArtifactWritten,
    ToolCallMetadata,
    CompactionWritten,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvenanceRange {
    pub first_seq: u64,
    pub last_seq: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_ids: Vec<String>,
}

impl ProvenanceRange {
    pub fn from_event(event: &EventEnvelopeV1) -> Self {
        Self {
            first_seq: event.seq,
            last_seq: event.seq,
            event_ids: vec![event.event_id.clone()],
        }
    }

    pub(super) fn extend(&mut self, event: &EventEnvelopeV1) {
        self.first_seq = self.first_seq.min(event.seq);
        self.last_seq = self.last_seq.max(event.seq);
        if !self.event_ids.iter().any(|id| id == &event.event_id) {
            self.event_ids.push(event.event_id.clone());
        }
    }
}
