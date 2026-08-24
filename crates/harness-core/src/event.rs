// allow: SIZE_OK — event schema v1 (30+ event variants + metadata structs + identity resolution)
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use harness_providers::{CompletionUsage, ProviderErrorCategory};

use crate::agent::ProviderCompactionSummarySource;
use crate::perm::PermissionGrant;

pub const SCHEMA_VERSION: u16 = 1;

mod builder;
#[cfg(test)]
mod tests;

pub use builder::{EventBuildError, EventBuilder};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelopeV1 {
    pub schema_version: u16,
    pub event_id: String,
    pub seq: u64,
    pub run_id: crate::ids::RunId,
    pub mono_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
    pub actor: EventActor,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_key: Option<String>,
    pub payload: EventV1,
}

impl EventEnvelopeV1 {
    pub fn lineage_parent_session_id(&self) -> Option<&str> {
        self.payload.lineage_parent_session_id()
    }
}

pub fn first_lineage_parent_session_id<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
) -> Option<&'a str> {
    events
        .into_iter()
        .find_map(EventEnvelopeV1::lineage_parent_session_id)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventActor {
    pub kind: ActorKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}

impl EventActor {
    pub fn new(kind: ActorKind, agent_id: Option<String>) -> Self {
        Self { kind, agent_id }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    Supervisor,
    Worker,
    User,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventContext {
    pub seq: u64,
    pub event_id: Option<String>,
    pub actor: EventActor,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub stream_key: Option<String>,
}

impl EventContext {
    pub fn new(seq: u64, actor: EventActor) -> Self {
        Self {
            seq,
            event_id: None,
            actor,
            correlation_id: None,
            causation_id: None,
            stream_key: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event_type", content = "data", rename_all = "snake_case")]
pub enum EventV1 {
    RunStarted(RunStartedEvent),
    SessionTitleUpdated(SessionTitleUpdatedEvent),
    RunFinished(RunFinishedEvent),
    RunFailed(RunFailedEvent),
    AgentSpawned(AgentSpawnedEvent),
    AgentStopped(AgentStoppedEvent),
    TaskScheduled(TaskScheduledEvent),
    TaskCancelled(TaskCancelledEvent),
    TaskCompleted(TaskCompletedEvent),
    TaskResultLate(TaskResultLateEvent),
    BackgroundTaskNotification(BackgroundTaskNotificationEvent),
    StaleDetected(StaleDetectedEvent),
    UserMessageSubmitted(UserMessageSubmittedEvent),
    PromptAttachmentsSubmitted(PromptAttachmentsSubmittedEvent),
    ProviderRequestStarted(ProviderRequestStartedEvent),
    ProviderStreamDelta(ProviderStreamDeltaEvent),
    ProviderReasoningDelta(ProviderReasoningDeltaEvent),
    ProviderRequestFinished(ProviderRequestFinishedEvent),
    AssistantMessageFinished(AssistantMessageFinishedEvent),
    /// Deprecated: replaced by [`EventV1::SessionCompaction`].
    #[deprecated(
        note = "replaced by `SessionCompaction`; will be removed after compaction migration"
    )]
    CompactionRequested(CompactionRequestedEvent),
    /// Deprecated: replaced by [`EventV1::SessionCompaction`].
    #[deprecated(
        note = "replaced by `SessionCompaction`; will be removed after compaction migration"
    )]
    CompactionWritten(CompactionWrittenEvent),
    /// Deprecated: replaced by [`EventV1::SessionCompaction`].
    #[deprecated(
        note = "replaced by `SessionCompaction`; will be removed after compaction migration"
    )]
    CompactionApplied(CompactionAppliedEvent),
    /// Deprecated: replaced by [`EventV1::SessionCompaction`].
    #[deprecated(
        note = "replaced by `SessionCompaction`; will be removed after compaction migration"
    )]
    CompactionFailed(CompactionFailedEvent),
    SessionCompaction(SessionCompactionEvent),
    BranchSummary(BranchSummaryEvent),
    ToolCallRequested(ToolCallRequestedEvent),
    ToolCallStarted(ToolCallStartedEvent),
    ToolCallFinished(ToolCallFinishedEvent),
    PermissionRequested(PermissionRequestedEvent),
    PermissionGrantRecorded(PermissionGrantRecordedEvent),
    PermissionResolved(PermissionResolvedEvent),
    EditProposed(EditProposedEvent),
    EditApplied(EditAppliedEvent),
    EditRejected(EditRejectedEvent),
    ArtifactWritten(ArtifactWrittenEvent),
    PolicyViolationDetected(PolicyViolationDetectedEvent),
    UiIntentReceived(UiIntentReceivedEvent),
    WorkspaceSnapshot(WorkspaceSnapshotEvent),
    WorkspaceReverted(WorkspaceRevertedEvent),
}

impl EventV1 {
    pub fn lineage_parent_session_id(&self) -> Option<&str> {
        match self {
            Self::TaskCompleted(payload) => payload
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.lineage.as_ref()),
            Self::ToolCallRequested(payload) => payload
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.lineage.as_ref()),
            Self::ToolCallFinished(payload) => payload
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.lineage.as_ref()),
            _ => None,
        }
        .and_then(TaskLineageMetadata::non_empty_parent_session_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunStartedEvent {
    pub run_name: crate::ids::RunName,
    pub workspace_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTitleUpdatedEvent {
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFinishedEvent {
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFailedEvent {
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSpawnedEvent {
    pub agent_id: String,
    pub profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_agent_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStoppedEvent {
    pub agent_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskScheduledEvent {
    pub task_id: crate::ids::TaskId,
    pub state: TaskScheduleState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<TaskScheduleMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TaskScheduleMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage: Option<TaskLineageMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskScheduleState {
    Queued,
    Started,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskTerminalScope {
    AgentTurn,
    ToolCall,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ToolIdentityMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias_source_tool_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ResolvedToolIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invoked_tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias_source_tool_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TaskLineageMetadata {
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
}

impl TaskLineageMetadata {
    pub fn non_empty_parent_session_id(&self) -> Option<&str> {
        let parent_session_id = self.parent_session_id.as_deref()?.trim();
        (!parent_session_id.is_empty()).then_some(parent_session_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExecutionTimingMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_mono_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_mono_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HookExecutionStatus {
    Succeeded,
    Failed,
    Skipped,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookExecutionMetadata {
    pub hook_name: String,
    pub status: HookExecutionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook_event: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventArtifactRef {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ToolCallMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias_source_tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage: Option<TaskLineageMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_refs: Vec<EventArtifactRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<ExecutionTimingMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hook_executions: Vec<HookExecutionMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TaskCompletionMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage: Option<TaskLineageMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_scope: Option<TaskTerminalScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<ExecutionTimingMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hook_executions: Vec<HookExecutionMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCancelledEvent {
    pub task_id: crate::ids::TaskId,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_scope: Option<TaskTerminalScope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCompletedEvent {
    pub task_id: crate::ids::TaskId,
    pub result_summary: String,
    pub result_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<TaskCompletionMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskResultLateEvent {
    pub task_id: crate::ids::TaskId,
    pub result_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundTaskNotificationStatus {
    Completed,
    Cancelled,
    Failed,
    TimedOut,
}

impl BackgroundTaskNotificationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundTaskNotificationEvent {
    pub parent_session_id: crate::ids::SessionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_agent_id: Option<String>,
    pub child_session_id: crate::ids::SessionId,
    pub child_request_id: String,
    pub task_id: crate::ids::TaskId,
    pub description: String,
    pub status: BackgroundTaskNotificationStatus,
    pub summary: String,
    pub terminal_event_id: String,
    pub terminal_task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_turn_request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaleDetectedEvent {
    pub task_id: crate::ids::TaskId,
    pub stale_for_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserMessageSubmittedEvent {
    pub request_id: crate::ids::RequestId,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptAttachmentsSubmittedEvent {
    pub request_id: crate::ids::RequestId,
    pub attachments: Vec<crate::attachment_transport::AttachmentMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderRequestStartedMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_cache_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<ProviderRequestRetryMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_budget: Option<crate::context_budget::RequestBudgetSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRequestRetryMetadata {
    pub attempt: u32,
    pub max_attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<ProviderErrorCategory>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderAssistantMessageMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderThinkingMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderRequestFinishedMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_response_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_cache_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_message: Option<ProviderAssistantMessageMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ProviderThinkingMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_error_category: Option<ProviderErrorCategory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_error_remediation: Option<String>,
}

/// Durable provider request start barrier.
///
/// `request_id`, `provider_id`, `model_id`, `prompt_summary`, and `request_digest` are the stable
/// replay-visible contract. `metadata` carries only optional, redacted, non-semantic provider
/// correlation hints: stable turn/request correlation, provider-call identity, provider
/// session/cache ids, and a redacted context-budget snapshot. Raw provider payloads, unredacted
/// thinking text, secrets, and
/// provider-specific control hints must not be persisted in this event. Provider stream chunk
/// boundaries remain presentation details derived from following delta events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRequestStartedEvent {
    pub request_id: crate::ids::RequestId,
    pub provider_id: String,
    pub model_id: String,
    pub prompt_summary: String,
    pub request_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ProviderRequestStartedMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStreamDeltaEvent {
    pub request_id: crate::ids::RequestId,
    pub delta: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderReasoningDeltaEvent {
    pub request_id: crate::ids::RequestId,
    pub delta: String,
}

/// Durable provider request finish barrier.
///
/// `request_id`, `finish_reason`, `output_digest`, and redacted aggregate `usage` are the stable
/// replay-visible contract. `metadata` describes the completed provider exchange without changing
/// replay semantics: provider stop reason, cache read/write token counts, compatibility
/// assistant-message ids/digests, summarized or signed thinking metadata, and optional provider ids
/// mirrored from the started event for easier inspection. `AssistantMessageFinished` is the explicit
/// assistant-message barrier for new logs. Tool-call readiness and loop continuation state are
/// derived by the coordinator from the ordered event stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRequestFinishedEvent {
    pub request_id: crate::ids::RequestId,
    pub finish_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<CompletionUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ProviderRequestFinishedMetadata>,
}

/// Durable assistant message finish barrier.
///
/// This event is appended after the coordinator has committed the completed assistant response to
/// its provider-visible message state, and before tool preflight or execution begins. It separates
/// provider transport completion (`ProviderRequestFinished`) from the assistant-message boundary
/// that replay/debugging tools can observe deterministically in JSONL order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantMessageFinishedEvent {
    pub request_id: crate::ids::RequestId,
    pub tool_call_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_message: Option<ProviderAssistantMessageMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionRequestedEvent {
    pub checkpoint_id: String,
    pub agent_id: String,
    pub trigger_reason: String,
    pub through_seq: u64,
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
    pub estimate_source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionWrittenEvent {
    pub checkpoint_id: String,
    pub agent_id: String,
    pub artifact_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_digest: Option<String>,
    pub artifact_bytes: u64,
    pub trigger_reason: String,
    pub through_seq: u64,
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
    pub estimate_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_source: Option<ProviderCompactionSummarySource>,
    pub preserved_turns: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionAppliedEvent {
    pub checkpoint_id: String,
    pub agent_id: String,
    pub through_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_before_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_after_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_tokens_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compacted_turns: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserved_turns: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduction_tokens_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduction_percent_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimate_source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionFailedEvent {
    pub agent_id: String,
    pub trigger_reason: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through_request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCompactionEvent {
    pub agent_id: String,
    pub summary: String,
    pub first_kept_event_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_kept_request_id: Option<String>,
    pub tokens_before: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified_files: Vec<String>,
    pub trigger_reason: String,
    pub from_hook: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchSummaryEvent {
    pub agent_id: String,
    pub summary: String,
    pub from_event_seq: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified_files: Vec<String>,
    pub from_hook: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallRequestedEvent {
    pub tool_call_id: crate::ids::ToolCallId,
    pub tool_id: String,
    pub args_summary: String,
    pub args_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ToolCallMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallStartedEvent {
    pub tool_call_id: crate::ids::ToolCallId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallFinishedEvent {
    pub tool_call_id: crate::ids::ToolCallId,
    pub status: ToolCallStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_json: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ToolCallMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallLifecycleState {
    #[default]
    Pending,
    Running,
    Completed,
    Error,
}

impl ToolCallLifecycleState {
    pub fn from_finish_status(status: ToolCallStatus) -> Self {
        match status {
            ToolCallStatus::Succeeded => Self::Completed,
            ToolCallStatus::Failed => Self::Error,
        }
    }
}

impl ResolvedToolIdentity {
    pub fn from_tool_call(
        invoked_tool_id: Option<&str>,
        metadata: Option<&ToolCallMetadata>,
    ) -> Self {
        Self::from_parts(
            invoked_tool_id,
            metadata.and_then(|metadata| metadata.canonical_tool_id.as_deref()),
            metadata.and_then(|metadata| metadata.alias_source_tool_id.as_deref()),
        )
    }

    pub fn from_tool_artifact(
        invoked_tool_id: Option<&str>,
        metadata: Option<&ToolIdentityMetadata>,
    ) -> Self {
        Self::from_parts(
            invoked_tool_id,
            metadata.and_then(|metadata| metadata.canonical_tool_id.as_deref()),
            metadata.and_then(|metadata| metadata.alias_source_tool_id.as_deref()),
        )
    }

    pub fn is_empty(&self) -> bool {
        self.invoked_tool_id.is_none()
            && self.effective_tool_id.is_none()
            && self.canonical_tool_id.is_none()
            && self.alias_source_tool_id.is_none()
    }

    fn from_parts(
        invoked_tool_id: Option<&str>,
        persisted_canonical_tool_id: Option<&str>,
        alias_source_tool_id: Option<&str>,
    ) -> Self {
        let invoked_tool_id = normalized_tool_id(invoked_tool_id);
        let persisted_canonical_tool_id = normalized_tool_id(persisted_canonical_tool_id);
        let alias_source_tool_id = normalized_tool_id(alias_source_tool_id);
        let is_mcp_invocation = invoked_tool_id
            .as_deref()
            .is_some_and(|tool_id| tool_id.starts_with("mcp."));

        let effective_tool_id = persisted_canonical_tool_id
            .clone()
            .or_else(|| invoked_tool_id.clone());
        let canonical_tool_id = if is_mcp_invocation {
            None
        } else {
            persisted_canonical_tool_id
        };

        Self {
            invoked_tool_id,
            effective_tool_id,
            canonical_tool_id,
            alias_source_tool_id,
        }
    }
}

fn normalized_tool_id(tool_id: Option<&str>) -> Option<String> {
    tool_id
        .map(str::trim)
        .filter(|tool_id| !tool_id.is_empty())
        .map(str::to_string)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRequestedEvent {
    pub permission_id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<crate::ids::ToolCallId>,
    pub summary: String,
    pub request_digest: String,
    pub timeout_ms: u64,
    pub default_decision: PermissionDecision,
}

pub struct PermissionRequestedArgs {
    pub permission_id: String,
    pub kind: String,
    pub tool_call_id: Option<crate::ids::ToolCallId>,
    pub summary: String,
    pub request_digest: String,
    pub timeout_ms: u64,
    pub default_decision: PermissionDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionResolvedEvent {
    pub permission_id: String,
    pub decision: PermissionDecision,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionGrantRecordedEvent {
    pub grant: PermissionGrant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditProposedEvent {
    pub edit_id: String,
    pub path: String,
    pub summary: String,
    pub patch_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditAppliedEvent {
    pub edit_id: String,
    pub path: String,
    pub new_file_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_rel_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditRejectedEvent {
    pub edit_id: String,
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactWrittenEvent {
    pub path: String,
    pub digest: String,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<crate::ids::ToolCallId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_metadata: Option<ToolIdentityMetadata>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyViolationDetectedEvent {
    pub policy: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiIntentReceivedEvent {
    pub intent: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSnapshotEvent {
    pub request_id: crate::ids::RequestId,
    pub artifact_path: String,
    pub artifact_digest: String,
    pub file_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRevertedEvent {
    pub request_id: crate::ids::RequestId,
    pub snapshot_request_id: String,
    pub restored_paths: Vec<String>,
    pub removed_paths: Vec<String>,
    pub failed_paths: Vec<WorkspaceRevertFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRevertFailure {
    pub path: String,
    pub reason: String,
}
