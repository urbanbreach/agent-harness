use super::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Copy, Default)]
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<AttachmentMetadata>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ProjectedMessageRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Copy, Default)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Copy)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleEventKind {
    RunStarted,
    RunFinished,
    RunFailed,
    AgentSpawned,
    AgentStopped,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProvenanceRange {
    pub first_seq: u64,
    pub last_seq: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_ids: Vec<String>,
}
