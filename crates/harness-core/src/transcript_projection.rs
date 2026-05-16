use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::event::{
    ArtifactWrittenEvent, EventEnvelopeV1, EventV1, PermissionDecision,
    ProviderAssistantMessageMetadata, TaskLineageMetadata, TaskScheduleState, ToolCallMetadata,
    ToolCallStatus,
};
use crate::text::non_empty_trimmed;

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
    pub request_id: Option<String>,
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
    Team(ProjectedTeamPart),
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
    pub tool_call_id: String,
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
    pub tool_call_id: Option<String>,
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
    pub task_id: String,
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
pub struct ProjectedTeamPart {
    pub team_run_id: String,
    pub event: ProjectedTeamEventKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub participant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub provenance: ProvenanceRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectedTeamEventKind {
    Created,
    MemberSpawned,
    MessageSent,
    TaskCreated,
    TaskUpdated,
    ShutdownRequested,
    ShutdownApproved,
    ShutdownRejected,
    Deleted,
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
    pub tool_call_id: Option<String>,
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

    fn extend(&mut self, event: &EventEnvelopeV1) {
        self.first_seq = self.first_seq.min(event.seq);
        self.last_seq = self.last_seq.max(event.seq);
        if !self.event_ids.iter().any(|id| id == &event.event_id) {
            self.event_ids.push(event.event_id.clone());
        }
    }
}

#[derive(Debug, Clone, Default)]
struct RequestLocations {
    user_message_index: Option<usize>,
    assistant_message_index: Option<usize>,
    assistant_text_part_index: Option<usize>,
    assistant_reasoning_part_index: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
struct PartLocation {
    message_index: usize,
    part_index: usize,
}

pub fn project_transcript(
    events: &[EventEnvelopeV1],
) -> Result<TranscriptProjection, TranscriptProjectionError> {
    ensure_strict_seq_order(events)?;

    let mut projection = TranscriptProjection::default();
    let mut request_locations = BTreeMap::<String, RequestLocations>::new();
    let mut tool_locations = BTreeMap::<String, PartLocation>::new();
    let mut permission_locations = BTreeMap::<String, PartLocation>::new();
    let mut compaction_locations = BTreeMap::<String, usize>::new();

    for event in events {
        projection
            .session
            .run_id
            .get_or_insert(event.run_id.clone());
        projection.session.max_seq = Some(event.seq);

        match &event.payload {
            EventV1::RunStarted(payload) => {
                projection.session.run_id = Some(event.run_id.clone());
                projection.session.run_name = Some(payload.run_name.clone());
                projection.session.workspace_root = Some(payload.workspace_root.clone());
                projection.session.status = TranscriptRunStatus::Running;
                projection.session.status_reason = None;
                projection.session.started_seq = Some(event.seq);
                projection.session.terminal_seq = None;
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::Lifecycle(ProjectedLifecyclePart {
                        event: LifecycleEventKind::RunStarted,
                        agent_id: event.actor.agent_id.clone(),
                        profile: None,
                        parent_agent_id: None,
                        summary: Some(payload.run_name.clone()),
                        error: None,
                        reason: None,
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::SessionTitleUpdated(payload) => {
                projection.session.run_name = Some(payload.title.clone());
            }
            EventV1::RunFinished(payload) => {
                projection.session.status = TranscriptRunStatus::Finished;
                projection.session.status_reason = Some(payload.summary.clone());
                projection.session.terminal_seq = Some(event.seq);
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::Lifecycle(ProjectedLifecyclePart {
                        event: LifecycleEventKind::RunFinished,
                        agent_id: event.actor.agent_id.clone(),
                        profile: None,
                        parent_agent_id: None,
                        summary: Some(payload.summary.clone()),
                        error: None,
                        reason: None,
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::RunFailed(payload) => {
                projection.session.status = TranscriptRunStatus::Failed;
                projection.session.status_reason = Some(payload.error.clone());
                projection.session.terminal_seq = Some(event.seq);
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::Lifecycle(ProjectedLifecyclePart {
                        event: LifecycleEventKind::RunFailed,
                        agent_id: event.actor.agent_id.clone(),
                        profile: None,
                        parent_agent_id: None,
                        summary: None,
                        error: Some(payload.error.clone()),
                        reason: None,
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::AgentSpawned(payload) => {
                projection
                    .session
                    .agent_profiles
                    .insert(payload.agent_id.clone(), payload.profile.clone());
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::Lifecycle(ProjectedLifecyclePart {
                        event: LifecycleEventKind::AgentSpawned,
                        agent_id: Some(payload.agent_id.clone()),
                        profile: Some(payload.profile.clone()),
                        parent_agent_id: payload.parent_agent_id.clone(),
                        summary: None,
                        error: None,
                        reason: None,
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::AgentStopped(payload) => {
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::Lifecycle(ProjectedLifecyclePart {
                        event: LifecycleEventKind::AgentStopped,
                        agent_id: Some(payload.agent_id.clone()),
                        profile: None,
                        parent_agent_id: None,
                        summary: None,
                        error: None,
                        reason: Some(payload.reason.clone()),
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::TaskScheduled(payload) => append_task_part(
                &mut projection,
                event,
                ProjectedTaskPart {
                    task_id: payload.task_id.clone(),
                    state: match payload.state {
                        TaskScheduleState::Queued => ProjectedTaskState::Queued,
                        TaskScheduleState::Started => ProjectedTaskState::Started,
                    },
                    queue_key: payload.queue_key.clone(),
                    reason: None,
                    result_summary: None,
                    result_digest: None,
                    lineage: None,
                    provenance: ProvenanceRange::from_event(event),
                },
            ),
            EventV1::TaskCancelled(payload) => append_task_part(
                &mut projection,
                event,
                ProjectedTaskPart {
                    task_id: payload.task_id.clone(),
                    state: ProjectedTaskState::Cancelled,
                    queue_key: None,
                    reason: Some(payload.reason.clone()),
                    result_summary: None,
                    result_digest: None,
                    lineage: None,
                    provenance: ProvenanceRange::from_event(event),
                },
            ),
            EventV1::TaskCompleted(payload) => {
                let lineage = payload
                    .metadata
                    .as_ref()
                    .and_then(|metadata| lineage_projection(metadata.lineage.as_ref(), event));
                if let Some(lineage) = lineage.as_ref() {
                    push_unique_lineage(&mut projection.session_lineage, lineage.clone());
                }
                append_task_part(
                    &mut projection,
                    event,
                    ProjectedTaskPart {
                        task_id: payload.task_id.clone(),
                        state: ProjectedTaskState::Completed,
                        queue_key: None,
                        reason: None,
                        result_summary: Some(payload.result_summary.clone()),
                        result_digest: Some(payload.result_digest.clone()),
                        lineage,
                        provenance: ProvenanceRange::from_event(event),
                    },
                );
            }
            EventV1::TaskResultLate(payload) => append_task_part(
                &mut projection,
                event,
                ProjectedTaskPart {
                    task_id: payload.task_id.clone(),
                    state: ProjectedTaskState::LateResult,
                    queue_key: None,
                    reason: None,
                    result_summary: None,
                    result_digest: Some(payload.result_digest.clone()),
                    lineage: None,
                    provenance: ProvenanceRange::from_event(event),
                },
            ),
            EventV1::UserMessageSubmitted(payload) => {
                let message = ProjectedMessage {
                    message_id: format!("user:{}:{}", payload.request_id, event.seq),
                    role: ProjectedMessageRole::User,
                    state: ProjectedMessageState::Complete,
                    request_id: Some(payload.request_id.clone()),
                    agent_id: event.actor.agent_id.clone(),
                    provider: None,
                    provenance: ProvenanceRange::from_event(event),
                    parts: vec![ProjectedPart::Text(ProjectedTextPart {
                        text: payload.text.clone(),
                        provenance: ProvenanceRange::from_event(event),
                    })],
                };
                let index = projection.messages.len();
                projection.messages.push(message);
                request_locations
                    .entry(payload.request_id.clone())
                    .or_default()
                    .user_message_index = Some(index);
            }
            EventV1::ProviderRequestStarted(payload) => {
                let request_id = provider_turn_request_id(event, &payload.request_id);
                let message_index = ensure_assistant_message(
                    &mut projection,
                    &mut request_locations,
                    event,
                    &request_id,
                );
                let message = &mut projection.messages[message_index];
                message.state = ProjectedMessageState::Streaming;
                let provider = message.provider.get_or_insert_with(Default::default);
                provider.provider_request_id = Some(payload.request_id.clone());
                provider.provider_id = Some(payload.provider_id.clone());
                provider.model_id = Some(payload.model_id.clone());
                provider.prompt_summary = Some(payload.prompt_summary.clone());
                provider.request_digest = Some(payload.request_digest.clone());
                message.provenance.extend(event);
            }
            EventV1::ProviderStreamDelta(payload) => {
                let request_id = provider_turn_request_id(event, &payload.request_id);
                append_or_extend_assistant_text(
                    &mut projection,
                    &mut request_locations,
                    event,
                    &request_id,
                    &payload.delta,
                    AssistantTextKind::Text,
                );
            }
            EventV1::ProviderReasoningDelta(payload) => {
                let request_id = provider_turn_request_id(event, &payload.request_id);
                append_or_extend_assistant_text(
                    &mut projection,
                    &mut request_locations,
                    event,
                    &request_id,
                    &payload.delta,
                    AssistantTextKind::Reasoning,
                );
            }
            EventV1::ProviderRequestFinished(payload) => {
                let request_id = provider_turn_request_id(event, &payload.request_id);
                let message_index = ensure_assistant_message(
                    &mut projection,
                    &mut request_locations,
                    event,
                    &request_id,
                );
                let message = &mut projection.messages[message_index];
                message.state = if payload.finish_reason.eq_ignore_ascii_case("error") {
                    ProjectedMessageState::Failed
                } else {
                    ProjectedMessageState::Complete
                };
                let provider = message.provider.get_or_insert_with(Default::default);
                provider.provider_request_id = Some(payload.request_id.clone());
                provider.finish_reason = Some(payload.finish_reason.clone());
                provider.output_digest = payload.output_digest.clone();
                if let Some(metadata) = payload.metadata.as_ref() {
                    if let Some(assistant_message) = metadata.assistant_message.as_ref() {
                        apply_assistant_message_metadata(provider, assistant_message);
                    }
                }
                message.provenance.extend(event);
            }
            EventV1::AssistantMessageFinished(payload) => {
                let request_id = provider_turn_request_id(event, &payload.request_id);
                let message_index = ensure_assistant_message(
                    &mut projection,
                    &mut request_locations,
                    event,
                    &request_id,
                );
                let message = &mut projection.messages[message_index];
                message.state = ProjectedMessageState::Complete;
                if let Some(assistant_message) = payload.assistant_message.as_ref() {
                    let provider = message.provider.get_or_insert_with(Default::default);
                    apply_assistant_message_metadata(provider, assistant_message);
                }
                message.provenance.extend(event);
            }
            EventV1::CompactionRequested(payload) => {
                let checkpoint = CompactionCheckpointProjection {
                    checkpoint_id: Some(payload.checkpoint_id.clone()),
                    agent_id: payload.agent_id.clone(),
                    status: CompactionCheckpointStatus::Requested,
                    trigger_reason: Some(payload.trigger_reason.clone()),
                    reason: None,
                    through_seq: Some(payload.through_seq),
                    through_request_id: payload.through_request_id.clone(),
                    provider_id: payload.provider_id.clone(),
                    model_id: payload.model_id.clone(),
                    tokens_before: payload.tokens_before,
                    tokens_before_estimate: payload.tokens_before_estimate,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                    preserved_turns: None,
                    artifact: None,
                    provenance: ProvenanceRange::from_event(event),
                };
                upsert_compaction_checkpoint(
                    &mut projection,
                    &mut compaction_locations,
                    event,
                    checkpoint,
                );
            }
            EventV1::CompactionWritten(payload) => {
                let artifact = TranscriptArtifactRef {
                    path: payload.artifact_path.clone(),
                    digest: payload.artifact_digest.clone(),
                    bytes: Some(payload.artifact_bytes),
                    tool_call_id: None,
                    source: ArtifactProjectionSource::CompactionWritten,
                    metadata: BTreeMap::from([
                        ("checkpoint_id".to_string(), payload.checkpoint_id.clone()),
                        ("agent_id".to_string(), payload.agent_id.clone()),
                    ]),
                    provenance: ProvenanceRange::from_event(event),
                };
                push_unique_artifact(&mut projection.artifacts, artifact.clone());
                let checkpoint = CompactionCheckpointProjection {
                    checkpoint_id: Some(payload.checkpoint_id.clone()),
                    agent_id: payload.agent_id.clone(),
                    status: CompactionCheckpointStatus::Written,
                    trigger_reason: Some(payload.trigger_reason.clone()),
                    reason: None,
                    through_seq: Some(payload.through_seq),
                    through_request_id: payload.through_request_id.clone(),
                    provider_id: payload.provider_id.clone(),
                    model_id: payload.model_id.clone(),
                    tokens_before: payload.tokens_before,
                    tokens_before_estimate: payload.tokens_before_estimate,
                    tokens_after_estimate: payload.tokens_after_estimate,
                    summary_tokens_estimate: payload.summary_tokens_estimate,
                    compacted_turns: payload.compacted_turns,
                    reduction_tokens_estimate: payload.reduction_tokens_estimate,
                    reduction_percent_estimate: payload.reduction_percent_estimate,
                    preserved_turns: Some(payload.preserved_turns),
                    artifact: Some(artifact),
                    provenance: ProvenanceRange::from_event(event),
                };
                upsert_compaction_checkpoint(
                    &mut projection,
                    &mut compaction_locations,
                    event,
                    checkpoint,
                );
            }
            EventV1::CompactionApplied(payload) => {
                let checkpoint = CompactionCheckpointProjection {
                    checkpoint_id: Some(payload.checkpoint_id.clone()),
                    agent_id: payload.agent_id.clone(),
                    status: CompactionCheckpointStatus::Applied,
                    trigger_reason: None,
                    reason: None,
                    through_seq: Some(payload.through_seq),
                    through_request_id: payload.through_request_id.clone(),
                    provider_id: None,
                    model_id: None,
                    tokens_before: None,
                    tokens_before_estimate: payload.tokens_before_estimate,
                    tokens_after_estimate: payload.tokens_after_estimate,
                    summary_tokens_estimate: payload.summary_tokens_estimate,
                    compacted_turns: payload.compacted_turns,
                    reduction_tokens_estimate: payload.reduction_tokens_estimate,
                    reduction_percent_estimate: payload.reduction_percent_estimate,
                    preserved_turns: payload.preserved_turns,
                    artifact: None,
                    provenance: ProvenanceRange::from_event(event),
                };
                upsert_compaction_checkpoint(
                    &mut projection,
                    &mut compaction_locations,
                    event,
                    checkpoint,
                );
            }
            EventV1::CompactionFailed(payload) => {
                let checkpoint = CompactionCheckpointProjection {
                    checkpoint_id: payload.checkpoint_id.clone(),
                    agent_id: payload.agent_id.clone(),
                    status: CompactionCheckpointStatus::Failed,
                    trigger_reason: Some(payload.trigger_reason.clone()),
                    reason: Some(payload.reason.clone()),
                    through_seq: payload.through_seq,
                    through_request_id: payload.through_request_id.clone(),
                    provider_id: None,
                    model_id: None,
                    tokens_before: None,
                    tokens_before_estimate: None,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                    preserved_turns: None,
                    artifact: None,
                    provenance: ProvenanceRange::from_event(event),
                };
                upsert_compaction_checkpoint(
                    &mut projection,
                    &mut compaction_locations,
                    event,
                    checkpoint,
                );
            }
            EventV1::ToolCallRequested(payload) => {
                let lineage = payload
                    .metadata
                    .as_ref()
                    .and_then(|metadata| lineage_projection(metadata.lineage.as_ref(), event));
                if let Some(lineage) = lineage.as_ref() {
                    push_unique_lineage(&mut projection.session_lineage, lineage.clone());
                }
                let metadata_artifacts = artifacts_from_tool_metadata(
                    &payload.tool_call_id,
                    payload.metadata.as_ref(),
                    event,
                );
                for artifact in metadata_artifacts.iter().cloned() {
                    push_unique_artifact(&mut projection.artifacts, artifact);
                }

                let tool_part = ProjectedToolCallPart {
                    tool_call_id: payload.tool_call_id.clone(),
                    tool_id: payload.tool_id.clone(),
                    args_summary: payload.args_summary.clone(),
                    args_digest: payload.args_digest.clone(),
                    state: ProjectedToolCallState::Pending,
                    status: None,
                    output_summary: None,
                    output_digest: None,
                    output_json: None,
                    requested_seq: Some(event.seq),
                    started_seq: None,
                    finished_seq: None,
                    metadata: payload.metadata.clone(),
                    permissions: Vec::new(),
                    artifacts: metadata_artifacts,
                    lineage,
                    provenance: ProvenanceRange::from_event(event),
                };

                if let Some(request_id) = event.correlation_id.as_deref() {
                    let message_index = ensure_assistant_message(
                        &mut projection,
                        &mut request_locations,
                        event,
                        request_id,
                    );
                    let part_index = append_part_to_message(
                        &mut projection,
                        message_index,
                        ProjectedPart::ToolCall(Box::new(tool_part)),
                        event,
                    );
                    tool_locations.insert(
                        payload.tool_call_id.clone(),
                        PartLocation {
                            message_index,
                            part_index,
                        },
                    );
                } else {
                    let message_index = append_system_part(
                        &mut projection,
                        event,
                        ProjectedPart::ToolCall(Box::new(tool_part)),
                    );
                    tool_locations.insert(
                        payload.tool_call_id.clone(),
                        PartLocation {
                            message_index,
                            part_index: 0,
                        },
                    );
                }
            }
            EventV1::ToolCallStarted(payload) => {
                if let Some(location) = tool_locations.get(&payload.tool_call_id).copied() {
                    if let Some(tool_call) = tool_call_part_mut(&mut projection, location) {
                        tool_call.state = ProjectedToolCallState::Running;
                        tool_call.started_seq = Some(event.seq);
                        tool_call.provenance.extend(event);
                    }
                    projection.messages[location.message_index]
                        .provenance
                        .extend(event);
                } else {
                    let message_index = append_system_part(
                        &mut projection,
                        event,
                        ProjectedPart::ToolCall(Box::new(placeholder_tool_call_part(
                            &payload.tool_call_id,
                            ProjectedToolCallState::Running,
                            event,
                        ))),
                    );
                    tool_locations.insert(
                        payload.tool_call_id.clone(),
                        PartLocation {
                            message_index,
                            part_index: 0,
                        },
                    );
                }
            }
            EventV1::ToolCallFinished(payload) => {
                let metadata_artifacts = artifacts_from_tool_metadata(
                    &payload.tool_call_id,
                    payload.metadata.as_ref(),
                    event,
                );
                for artifact in metadata_artifacts.iter().cloned() {
                    push_unique_artifact(&mut projection.artifacts, artifact);
                }
                if let Some(lineage) = payload
                    .metadata
                    .as_ref()
                    .and_then(|metadata| lineage_projection(metadata.lineage.as_ref(), event))
                {
                    push_unique_lineage(&mut projection.session_lineage, lineage.clone());
                }

                if let Some(location) = tool_locations.get(&payload.tool_call_id).copied() {
                    if let Some(tool_call) = tool_call_part_mut(&mut projection, location) {
                        tool_call.state = match payload.status {
                            ToolCallStatus::Succeeded => ProjectedToolCallState::Succeeded,
                            ToolCallStatus::Failed => ProjectedToolCallState::Failed,
                        };
                        tool_call.status = Some(payload.status);
                        tool_call.output_summary = payload.output_summary.clone();
                        tool_call.output_digest = payload.output_digest.clone();
                        tool_call.output_json = payload.output_json.clone();
                        tool_call.finished_seq = Some(event.seq);
                        if tool_call.metadata.is_none() {
                            tool_call.metadata = payload.metadata.clone();
                        }
                        if tool_call.lineage.is_none() {
                            tool_call.lineage = payload.metadata.as_ref().and_then(|metadata| {
                                lineage_projection(metadata.lineage.as_ref(), event)
                            });
                        }
                        for artifact in metadata_artifacts {
                            push_unique_artifact(&mut tool_call.artifacts, artifact);
                        }
                        tool_call.provenance.extend(event);
                    }
                    projection.messages[location.message_index]
                        .provenance
                        .extend(event);
                } else {
                    let mut tool_part = placeholder_tool_call_part(
                        &payload.tool_call_id,
                        match payload.status {
                            ToolCallStatus::Succeeded => ProjectedToolCallState::Succeeded,
                            ToolCallStatus::Failed => ProjectedToolCallState::Failed,
                        },
                        event,
                    );
                    tool_part.status = Some(payload.status);
                    tool_part.output_summary = payload.output_summary.clone();
                    tool_part.output_digest = payload.output_digest.clone();
                    tool_part.output_json = payload.output_json.clone();
                    tool_part.finished_seq = Some(event.seq);
                    tool_part.metadata = payload.metadata.clone();
                    tool_part.lineage = payload
                        .metadata
                        .as_ref()
                        .and_then(|metadata| lineage_projection(metadata.lineage.as_ref(), event));
                    tool_part.artifacts = metadata_artifacts;
                    let message_index = append_system_part(
                        &mut projection,
                        event,
                        ProjectedPart::ToolCall(Box::new(tool_part)),
                    );
                    tool_locations.insert(
                        payload.tool_call_id.clone(),
                        PartLocation {
                            message_index,
                            part_index: 0,
                        },
                    );
                }
            }
            EventV1::PermissionRequested(payload) => {
                let part = ProjectedPermissionPart {
                    permission_id: payload.permission_id.clone(),
                    kind: payload.kind.clone(),
                    tool_call_id: payload.tool_call_id.clone(),
                    summary: payload.summary.clone(),
                    request_digest: payload.request_digest.clone(),
                    timeout_ms: payload.timeout_ms,
                    default_decision: payload.default_decision,
                    state: ProjectedPermissionState::Pending,
                    decision: None,
                    reason: None,
                    provenance: ProvenanceRange::from_event(event),
                };
                let message_index = append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::Permission(part.clone()),
                );
                permission_locations.insert(
                    payload.permission_id.clone(),
                    PartLocation {
                        message_index,
                        part_index: 0,
                    },
                );
                if let Some(tool_call_id) = payload.tool_call_id.as_ref() {
                    if let Some(tool_location) = tool_locations.get(tool_call_id).copied() {
                        if let Some(tool_call) = tool_call_part_mut(&mut projection, tool_location)
                        {
                            tool_call.permissions.push(part);
                            tool_call.provenance.extend(event);
                        }
                    }
                }
            }
            EventV1::PermissionResolved(payload) => {
                if let Some(location) = permission_locations.get(&payload.permission_id).copied() {
                    if let Some(permission) = permission_part_mut(&mut projection, location) {
                        permission.state = ProjectedPermissionState::Resolved;
                        permission.decision = Some(payload.decision);
                        permission.reason = payload.reason.clone();
                        permission.provenance.extend(event);
                    }
                    projection.messages[location.message_index]
                        .provenance
                        .extend(event);
                } else {
                    append_system_part(
                        &mut projection,
                        event,
                        ProjectedPart::Permission(ProjectedPermissionPart {
                            permission_id: payload.permission_id.clone(),
                            kind: String::new(),
                            tool_call_id: None,
                            summary: String::new(),
                            request_digest: String::new(),
                            timeout_ms: 0,
                            default_decision: payload.decision,
                            state: ProjectedPermissionState::Resolved,
                            decision: Some(payload.decision),
                            reason: payload.reason.clone(),
                            provenance: ProvenanceRange::from_event(event),
                        }),
                    );
                }
                update_tool_permission_resolution(
                    &mut projection,
                    &payload.permission_id,
                    payload.decision,
                    payload.reason.clone(),
                    event,
                );
            }
            EventV1::ArtifactWritten(payload) => {
                let artifact = artifact_from_written(payload, event);
                push_unique_artifact(&mut projection.artifacts, artifact.clone());
                if let Some(tool_call_id) = payload.tool_call_id.as_ref() {
                    if let Some(location) = tool_locations.get(tool_call_id).copied() {
                        if let Some(tool_call) = tool_call_part_mut(&mut projection, location) {
                            push_unique_artifact(&mut tool_call.artifacts, artifact.clone());
                            tool_call.provenance.extend(event);
                        }
                    }
                }
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::Artifact(ProjectedArtifactPart { artifact }),
                );
            }
            EventV1::PolicyViolationDetected(payload) => {
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::PolicyViolation(ProjectedPolicyViolationPart {
                        policy: payload.policy.clone(),
                        detail: payload.detail.clone(),
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::UiIntentReceived(payload) => {
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::UiIntent(ProjectedUiIntentPart {
                        intent: payload.intent.clone(),
                        params: payload.params.clone(),
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::TeamCreated(payload) => {
                append_team_part(
                    &mut projection,
                    event,
                    ProjectedTeamPart {
                        team_run_id: payload.team_run_id.clone(),
                        event: ProjectedTeamEventKind::Created,
                        participant: None,
                        target: None,
                        task_id: None,
                        message_id: None,
                        status: Some("active".to_string()),
                        summary: Some(payload.spec.name.clone()),
                        provenance: ProvenanceRange::from_event(event),
                    },
                );
            }
            EventV1::TeamMemberSpawned(payload) => {
                append_team_part(
                    &mut projection,
                    event,
                    ProjectedTeamPart {
                        team_run_id: payload.team_run_id.clone(),
                        event: ProjectedTeamEventKind::MemberSpawned,
                        participant: Some(payload.member_name.clone()),
                        target: Some(payload.agent_id.clone()),
                        task_id: None,
                        message_id: None,
                        status: Some("running".to_string()),
                        summary: Some(payload.profile.clone()),
                        provenance: ProvenanceRange::from_event(event),
                    },
                );
            }
            EventV1::TeamMessageSent(payload) => {
                append_team_part(
                    &mut projection,
                    event,
                    ProjectedTeamPart {
                        team_run_id: payload.team_run_id.clone(),
                        event: ProjectedTeamEventKind::MessageSent,
                        participant: Some(payload.message.from.clone()),
                        target: Some(payload.message.to.clone()),
                        task_id: None,
                        message_id: Some(payload.message.message_id.clone()),
                        status: Some(format!("{:?}", payload.message.kind)),
                        summary: payload.message.summary.clone().or_else(|| {
                            non_empty_trimmed(&payload.message.body).map(str::to_string)
                        }),
                        provenance: ProvenanceRange::from_event(event),
                    },
                );
            }
            EventV1::TeamTaskCreated(payload) => {
                append_team_part(
                    &mut projection,
                    event,
                    ProjectedTeamPart {
                        team_run_id: payload.team_run_id.clone(),
                        event: ProjectedTeamEventKind::TaskCreated,
                        participant: payload.task.owner.clone(),
                        target: None,
                        task_id: Some(payload.task.task_id.clone()),
                        message_id: None,
                        status: Some(payload.task.status.as_str().to_string()),
                        summary: Some(payload.task.subject.clone()),
                        provenance: ProvenanceRange::from_event(event),
                    },
                );
            }
            EventV1::TeamTaskUpdated(payload) => {
                append_team_part(
                    &mut projection,
                    event,
                    ProjectedTeamPart {
                        team_run_id: payload.team_run_id.clone(),
                        event: ProjectedTeamEventKind::TaskUpdated,
                        participant: payload.owner.clone(),
                        target: None,
                        task_id: Some(payload.task_id.clone()),
                        message_id: None,
                        status: Some(payload.status.as_str().to_string()),
                        summary: None,
                        provenance: ProvenanceRange::from_event(event),
                    },
                );
            }
            EventV1::PersistentTaskCreated(payload) => {
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::UiIntent(ProjectedUiIntentPart {
                        intent: "persistent_task_created".to_string(),
                        params: BTreeMap::from([
                            ("task_id".to_string(), payload.task.task_id.clone()),
                            (
                                "status".to_string(),
                                payload.task.status.as_str().to_string(),
                            ),
                            ("subject".to_string(), payload.task.subject.clone()),
                        ]),
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::PersistentTaskUpdated(payload) => {
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::UiIntent(ProjectedUiIntentPart {
                        intent: "persistent_task_updated".to_string(),
                        params: BTreeMap::from([
                            ("task_id".to_string(), payload.task_id.clone()),
                            ("status".to_string(), payload.status.as_str().to_string()),
                        ]),
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::TeamShutdownRequested(payload) => {
                append_team_part(
                    &mut projection,
                    event,
                    ProjectedTeamPart {
                        team_run_id: payload.team_run_id.clone(),
                        event: ProjectedTeamEventKind::ShutdownRequested,
                        participant: Some(payload.requester.clone()),
                        target: Some(payload.member_name.clone()),
                        task_id: None,
                        message_id: None,
                        status: Some("shutdown_requested".to_string()),
                        summary: None,
                        provenance: ProvenanceRange::from_event(event),
                    },
                );
            }
            EventV1::TeamShutdownApproved(payload) => {
                append_team_part(
                    &mut projection,
                    event,
                    ProjectedTeamPart {
                        team_run_id: payload.team_run_id.clone(),
                        event: ProjectedTeamEventKind::ShutdownApproved,
                        participant: Some(payload.approver.clone()),
                        target: Some(payload.member_name.clone()),
                        task_id: None,
                        message_id: None,
                        status: Some("shutdown_approved".to_string()),
                        summary: None,
                        provenance: ProvenanceRange::from_event(event),
                    },
                );
            }
            EventV1::TeamShutdownRejected(payload) => {
                append_team_part(
                    &mut projection,
                    event,
                    ProjectedTeamPart {
                        team_run_id: payload.team_run_id.clone(),
                        event: ProjectedTeamEventKind::ShutdownRejected,
                        participant: Some(payload.rejecter.clone()),
                        target: Some(payload.member_name.clone()),
                        task_id: None,
                        message_id: None,
                        status: Some("running".to_string()),
                        summary: Some(payload.reason.clone()),
                        provenance: ProvenanceRange::from_event(event),
                    },
                );
            }
            EventV1::TeamDeleted(payload) => {
                append_team_part(
                    &mut projection,
                    event,
                    ProjectedTeamPart {
                        team_run_id: payload.team_run_id.clone(),
                        event: ProjectedTeamEventKind::Deleted,
                        participant: None,
                        target: None,
                        task_id: None,
                        message_id: None,
                        status: Some("deleted".to_string()),
                        summary: None,
                        provenance: ProvenanceRange::from_event(event),
                    },
                );
            }
            EventV1::ContinuationStarted(payload) => {
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::UiIntent(ProjectedUiIntentPart {
                        intent: "continuation_started".to_string(),
                        params: BTreeMap::from([
                            (
                                "continuation_id".to_string(),
                                payload.continuation_id.clone(),
                            ),
                            ("mode".to_string(), payload.mode.clone()),
                            ("command".to_string(), payload.command.clone()),
                        ]),
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::ContinuationReminderQueued(payload) => {
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::UiIntent(ProjectedUiIntentPart {
                        intent: "continuation_reminder_queued".to_string(),
                        params: BTreeMap::from([
                            (
                                "continuation_id".to_string(),
                                payload.continuation_id.clone(),
                            ),
                            ("iteration".to_string(), payload.iteration.to_string()),
                            ("reason".to_string(), payload.reason.clone()),
                        ]),
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::ContinuationStopped(payload) => {
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::UiIntent(ProjectedUiIntentPart {
                        intent: "continuation_stopped".to_string(),
                        params: BTreeMap::from([
                            (
                                "continuation_id".to_string(),
                                payload.continuation_id.clone(),
                            ),
                            ("reason".to_string(), payload.reason.clone()),
                        ]),
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::ContinuationLimitReached(payload) => {
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::UiIntent(ProjectedUiIntentPart {
                        intent: "continuation_limit_reached".to_string(),
                        params: BTreeMap::from([
                            (
                                "continuation_id".to_string(),
                                payload.continuation_id.clone(),
                            ),
                            ("limit".to_string(), payload.limit.clone()),
                            ("iteration".to_string(), payload.iteration.to_string()),
                        ]),
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::StaleDetected(_)
            | EventV1::BackgroundTaskNotification(_)
            | EventV1::PermissionGrantRecorded(_)
            | EventV1::EditProposed(_)
            | EventV1::EditApplied(_)
            | EventV1::EditRejected(_)
            | EventV1::WorkflowStarted(_)
            | EventV1::WorkflowTransitionRecorded(_)
            | EventV1::WorkflowTransitionDenied(_)
            | EventV1::WorkflowEvidenceRecorded(_)
            | EventV1::WorkflowOperatorDecisionRecorded(_)
            | EventV1::WorkflowCompleted(_) => {}
        }
    }

    Ok(projection)
}

fn ensure_strict_seq_order(events: &[EventEnvelopeV1]) -> Result<(), TranscriptProjectionError> {
    let mut previous_seq = None;
    for event in events {
        if let Some(previous_seq) = previous_seq {
            if event.seq <= previous_seq {
                return Err(TranscriptProjectionError::EventsOutOfOrder {
                    previous_seq,
                    seq: event.seq,
                });
            }
        }
        previous_seq = Some(event.seq);
    }
    Ok(())
}

fn append_system_part(
    projection: &mut TranscriptProjection,
    event: &EventEnvelopeV1,
    part: ProjectedPart,
) -> usize {
    let index = projection.messages.len();
    projection.messages.push(ProjectedMessage {
        message_id: format!("system:{}", event.seq),
        role: ProjectedMessageRole::System,
        state: ProjectedMessageState::Complete,
        request_id: event.correlation_id.clone(),
        agent_id: event.actor.agent_id.clone(),
        provider: None,
        provenance: ProvenanceRange::from_event(event),
        parts: vec![part],
    });
    index
}

fn append_task_part(
    projection: &mut TranscriptProjection,
    event: &EventEnvelopeV1,
    part: ProjectedTaskPart,
) {
    append_system_part(projection, event, ProjectedPart::Task(part));
}

fn append_team_part(
    projection: &mut TranscriptProjection,
    event: &EventEnvelopeV1,
    part: ProjectedTeamPart,
) {
    append_system_part(projection, event, ProjectedPart::Team(part));
}

fn append_part_to_message(
    projection: &mut TranscriptProjection,
    message_index: usize,
    part: ProjectedPart,
    event: &EventEnvelopeV1,
) -> usize {
    let message = &mut projection.messages[message_index];
    message.provenance.extend(event);
    let part_index = message.parts.len();
    message.parts.push(part);
    part_index
}

fn ensure_assistant_message(
    projection: &mut TranscriptProjection,
    request_locations: &mut BTreeMap<String, RequestLocations>,
    event: &EventEnvelopeV1,
    request_id: &str,
) -> usize {
    if let Some(index) = request_locations
        .get(request_id)
        .and_then(|locations| locations.assistant_message_index)
    {
        return index;
    }

    let index = projection.messages.len();
    projection.messages.push(ProjectedMessage {
        message_id: format!("assistant:{request_id}"),
        role: ProjectedMessageRole::Assistant,
        state: ProjectedMessageState::Incomplete,
        request_id: Some(request_id.to_string()),
        agent_id: event.actor.agent_id.clone(),
        provider: None,
        provenance: ProvenanceRange::from_event(event),
        parts: Vec::new(),
    });
    request_locations
        .entry(request_id.to_string())
        .or_default()
        .assistant_message_index = Some(index);
    index
}

#[derive(Debug, Clone, Copy)]
enum AssistantTextKind {
    Text,
    Reasoning,
}

fn append_or_extend_assistant_text(
    projection: &mut TranscriptProjection,
    request_locations: &mut BTreeMap<String, RequestLocations>,
    event: &EventEnvelopeV1,
    request_id: &str,
    delta: &str,
    kind: AssistantTextKind,
) {
    let message_index = ensure_assistant_message(projection, request_locations, event, request_id);
    let existing_part_index = match kind {
        AssistantTextKind::Text => request_locations
            .get(request_id)
            .and_then(|locations| locations.assistant_text_part_index),
        AssistantTextKind::Reasoning => request_locations
            .get(request_id)
            .and_then(|locations| locations.assistant_reasoning_part_index),
    };

    projection.messages[message_index].state = ProjectedMessageState::Streaming;
    projection.messages[message_index].provenance.extend(event);

    if let Some(part_index) = existing_part_index {
        if let Some(text_part) = text_part_mut(&mut projection.messages[message_index], part_index)
        {
            text_part.text.push_str(delta);
            text_part.provenance.extend(event);
            return;
        }
    }

    let part = ProjectedTextPart {
        text: delta.to_string(),
        provenance: ProvenanceRange::from_event(event),
    };
    let part_index = projection.messages[message_index].parts.len();
    projection.messages[message_index].parts.push(match kind {
        AssistantTextKind::Text => ProjectedPart::Text(part),
        AssistantTextKind::Reasoning => ProjectedPart::Reasoning(part),
    });
    let locations = request_locations.entry(request_id.to_string()).or_default();
    match kind {
        AssistantTextKind::Text => locations.assistant_text_part_index = Some(part_index),
        AssistantTextKind::Reasoning => locations.assistant_reasoning_part_index = Some(part_index),
    }
}

fn provider_turn_request_id(event: &EventEnvelopeV1, provider_request_id: &str) -> String {
    event
        .correlation_id
        .as_deref()
        .and_then(non_empty_trimmed)
        .unwrap_or(provider_request_id)
        .to_string()
}

fn apply_assistant_message_metadata(
    provider: &mut ProjectedProviderMessageMetadata,
    metadata: &ProviderAssistantMessageMetadata,
) {
    provider.assistant_message_id = metadata.message_id.clone();
    provider.assistant_text_digest = metadata.text_digest.clone();
    provider.assistant_reasoning_digest = metadata.reasoning_digest.clone();
}

fn text_part_mut(
    message: &mut ProjectedMessage,
    part_index: usize,
) -> Option<&mut ProjectedTextPart> {
    match message.parts.get_mut(part_index) {
        Some(ProjectedPart::Text(part)) | Some(ProjectedPart::Reasoning(part)) => Some(part),
        _ => None,
    }
}

fn tool_call_part_mut(
    projection: &mut TranscriptProjection,
    location: PartLocation,
) -> Option<&mut ProjectedToolCallPart> {
    match projection
        .messages
        .get_mut(location.message_index)?
        .parts
        .get_mut(location.part_index)?
    {
        ProjectedPart::ToolCall(part) => Some(part),
        _ => None,
    }
}

fn permission_part_mut(
    projection: &mut TranscriptProjection,
    location: PartLocation,
) -> Option<&mut ProjectedPermissionPart> {
    match projection
        .messages
        .get_mut(location.message_index)?
        .parts
        .get_mut(location.part_index)?
    {
        ProjectedPart::Permission(part) => Some(part),
        _ => None,
    }
}

fn update_tool_permission_resolution(
    projection: &mut TranscriptProjection,
    permission_id: &str,
    decision: PermissionDecision,
    reason: Option<String>,
    event: &EventEnvelopeV1,
) {
    for message in &mut projection.messages {
        let mut updated_message = false;
        for part in &mut message.parts {
            let ProjectedPart::ToolCall(tool_call) = part else {
                continue;
            };
            for permission in &mut tool_call.permissions {
                if permission.permission_id == permission_id {
                    permission.state = ProjectedPermissionState::Resolved;
                    permission.decision = Some(decision);
                    permission.reason = reason.clone();
                    permission.provenance.extend(event);
                    tool_call.provenance.extend(event);
                    updated_message = true;
                }
            }
        }
        if updated_message {
            message.provenance.extend(event);
        }
    }
}

fn placeholder_tool_call_part(
    tool_call_id: &str,
    state: ProjectedToolCallState,
    event: &EventEnvelopeV1,
) -> ProjectedToolCallPart {
    ProjectedToolCallPart {
        tool_call_id: tool_call_id.to_string(),
        tool_id: String::new(),
        args_summary: String::new(),
        args_digest: String::new(),
        state,
        status: None,
        output_summary: None,
        output_digest: None,
        output_json: None,
        requested_seq: None,
        started_seq: matches!(state, ProjectedToolCallState::Running).then_some(event.seq),
        finished_seq: None,
        metadata: None,
        permissions: Vec::new(),
        artifacts: Vec::new(),
        lineage: None,
        provenance: ProvenanceRange::from_event(event),
    }
}

fn upsert_compaction_checkpoint(
    projection: &mut TranscriptProjection,
    compaction_locations: &mut BTreeMap<String, usize>,
    event: &EventEnvelopeV1,
    checkpoint: CompactionCheckpointProjection,
) {
    let key = checkpoint
        .checkpoint_id
        .clone()
        .unwrap_or_else(|| format!("failed:{}:{}", checkpoint.agent_id, event.seq));
    let index = if let Some(index) = compaction_locations.get(&key).copied() {
        merge_compaction_checkpoint(&mut projection.compaction_checkpoints[index], checkpoint);
        index
    } else {
        let index = projection.compaction_checkpoints.len();
        projection.compaction_checkpoints.push(checkpoint);
        compaction_locations.insert(key, index);
        index
    };

    let stored = projection.compaction_checkpoints[index].clone();
    append_system_part(
        projection,
        event,
        ProjectedPart::Compaction(ProjectedCompactionPart {
            checkpoint_id: stored.checkpoint_id,
            agent_id: stored.agent_id,
            status: stored.status,
            trigger_reason: stored.trigger_reason,
            reason: stored.reason,
            through_seq: stored.through_seq,
            through_request_id: stored.through_request_id,
            artifact: stored.artifact,
            provenance: ProvenanceRange::from_event(event),
        }),
    );
}

fn merge_compaction_checkpoint(
    existing: &mut CompactionCheckpointProjection,
    incoming: CompactionCheckpointProjection,
) {
    existing.status = incoming.status;
    existing.provenance.last_seq = incoming.provenance.last_seq;
    existing
        .provenance
        .event_ids
        .extend(incoming.provenance.event_ids);
    if existing.trigger_reason.is_none() {
        existing.trigger_reason = incoming.trigger_reason;
    }
    if incoming.reason.is_some() {
        existing.reason = incoming.reason;
    }
    if incoming.through_seq.is_some() {
        existing.through_seq = incoming.through_seq;
    }
    if incoming.through_request_id.is_some() {
        existing.through_request_id = incoming.through_request_id;
    }
    if incoming.provider_id.is_some() {
        existing.provider_id = incoming.provider_id;
    }
    if incoming.model_id.is_some() {
        existing.model_id = incoming.model_id;
    }
    if incoming.tokens_before.is_some() {
        existing.tokens_before = incoming.tokens_before;
    }
    if incoming.tokens_before_estimate.is_some() {
        existing.tokens_before_estimate = incoming.tokens_before_estimate;
    }
    if incoming.tokens_after_estimate.is_some() {
        existing.tokens_after_estimate = incoming.tokens_after_estimate;
    }
    if incoming.summary_tokens_estimate.is_some() {
        existing.summary_tokens_estimate = incoming.summary_tokens_estimate;
    }
    if incoming.compacted_turns.is_some() {
        existing.compacted_turns = incoming.compacted_turns;
    }
    if incoming.reduction_tokens_estimate.is_some() {
        existing.reduction_tokens_estimate = incoming.reduction_tokens_estimate;
    }
    if incoming.reduction_percent_estimate.is_some() {
        existing.reduction_percent_estimate = incoming.reduction_percent_estimate;
    }
    if incoming.preserved_turns.is_some() {
        existing.preserved_turns = incoming.preserved_turns;
    }
    if incoming.artifact.is_some() {
        existing.artifact = incoming.artifact;
    }
}

fn artifact_from_written(
    payload: &ArtifactWrittenEvent,
    event: &EventEnvelopeV1,
) -> TranscriptArtifactRef {
    TranscriptArtifactRef {
        path: payload.path.clone(),
        digest: Some(payload.digest.clone()),
        bytes: Some(payload.bytes),
        tool_call_id: payload.tool_call_id.clone(),
        source: ArtifactProjectionSource::ArtifactWritten,
        metadata: payload.metadata.clone(),
        provenance: ProvenanceRange::from_event(event),
    }
}

fn artifacts_from_tool_metadata(
    tool_call_id: &str,
    metadata: Option<&ToolCallMetadata>,
    event: &EventEnvelopeV1,
) -> Vec<TranscriptArtifactRef> {
    metadata
        .map(|metadata| {
            metadata
                .artifact_refs
                .iter()
                .map(|artifact| TranscriptArtifactRef {
                    path: artifact.path.clone(),
                    digest: artifact.digest.clone(),
                    bytes: None,
                    tool_call_id: Some(tool_call_id.to_string()),
                    source: ArtifactProjectionSource::ToolCallMetadata,
                    metadata: BTreeMap::new(),
                    provenance: ProvenanceRange::from_event(event),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn push_unique_artifact(
    artifacts: &mut Vec<TranscriptArtifactRef>,
    artifact: TranscriptArtifactRef,
) {
    if artifacts.iter().any(|existing| {
        existing.path == artifact.path
            && existing.digest == artifact.digest
            && existing.tool_call_id == artifact.tool_call_id
    }) {
        return;
    }
    artifacts.push(artifact);
}

fn lineage_projection(
    lineage: Option<&TaskLineageMetadata>,
    event: &EventEnvelopeV1,
) -> Option<SessionLineageProjection> {
    let lineage = lineage?;
    let has_any = lineage.parent_tool_call_id.is_some()
        || lineage.parent_task_id.is_some()
        || lineage.parent_request_id.is_some()
        || lineage.parent_session_id.is_some()
        || lineage.child_session_id.is_some()
        || lineage.child_request_id.is_some()
        || lineage.child_provider_id.is_some()
        || lineage.child_model_id.is_some();
    if !has_any {
        return None;
    }
    Some(SessionLineageProjection {
        parent_tool_call_id: lineage.parent_tool_call_id.clone(),
        parent_task_id: lineage.parent_task_id.clone(),
        parent_request_id: lineage.parent_request_id.clone(),
        parent_session_id: lineage.parent_session_id.clone(),
        child_session_id: lineage.child_session_id.clone(),
        child_request_id: lineage.child_request_id.clone(),
        child_provider_id: lineage.child_provider_id.clone(),
        child_model_id: lineage.child_model_id.clone(),
        provenance: ProvenanceRange::from_event(event),
    })
}

fn push_unique_lineage(
    lineages: &mut Vec<SessionLineageProjection>,
    lineage: SessionLineageProjection,
) {
    if lineages
        .iter()
        .any(|existing| same_lineage(existing, &lineage))
    {
        return;
    }
    lineages.push(lineage);
}

fn same_lineage(left: &SessionLineageProjection, right: &SessionLineageProjection) -> bool {
    left.parent_tool_call_id == right.parent_tool_call_id
        && left.parent_task_id == right.parent_task_id
        && left.parent_request_id == right.parent_request_id
        && left.parent_session_id == right.parent_session_id
        && left.child_session_id == right.child_session_id
        && left.child_request_id == right.child_request_id
        && left.child_provider_id == right.child_provider_id
        && left.child_model_id == right.child_model_id
}
