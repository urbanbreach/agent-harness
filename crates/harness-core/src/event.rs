use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use harness_providers::CompletionUsage;

use crate::clock::Clock;
use crate::digest::digest12_json;
use crate::perm::PermissionGrant;
use crate::redact::{redact_value, Redactor};
use crate::text::truncate_with_ellipsis;

pub const SCHEMA_VERSION: u16 = 1;
const DEFAULT_EVENT_ID_PREFIX: &str = "evt";
const MAX_SUMMARY_CHARS: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelopeV1 {
    pub schema_version: u16,
    pub event_id: String,
    pub seq: u64,
    pub run_id: String,
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
    ProviderRequestStarted(ProviderRequestStartedEvent),
    ProviderStreamDelta(ProviderStreamDeltaEvent),
    ProviderReasoningDelta(ProviderReasoningDeltaEvent),
    ProviderRequestFinished(ProviderRequestFinishedEvent),
    AssistantMessageFinished(AssistantMessageFinishedEvent),
    CompactionRequested(CompactionRequestedEvent),
    CompactionWritten(CompactionWrittenEvent),
    CompactionApplied(CompactionAppliedEvent),
    CompactionFailed(CompactionFailedEvent),
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
    TeamCreated(TeamCreatedEvent),
    TeamMemberSpawned(TeamMemberSpawnedEvent),
    TeamMessageSent(TeamMessageSentEvent),
    TeamTaskCreated(TeamTaskCreatedEvent),
    TeamTaskUpdated(TeamTaskUpdatedEvent),
    PersistentTaskCreated(PersistentTaskCreatedEvent),
    PersistentTaskUpdated(PersistentTaskUpdatedEvent),
    TeamShutdownRequested(TeamShutdownRequestedEvent),
    TeamShutdownApproved(TeamShutdownApprovedEvent),
    TeamShutdownRejected(TeamShutdownRejectedEvent),
    TeamDeleted(TeamDeletedEvent),
    WorkflowStarted(WorkflowStartedEvent),
    WorkflowTransitionRecorded(WorkflowTransitionRecordedEvent),
    WorkflowTransitionDenied(WorkflowTransitionDeniedEvent),
    WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent),
    WorkflowOperatorDecisionRecorded(WorkflowOperatorDecisionRecordedEvent),
    WorkflowCompleted(WorkflowCompletedEvent),
    ContinuationStarted(ContinuationStartedEvent),
    ContinuationReminderQueued(ContinuationReminderQueuedEvent),
    ContinuationStopped(ContinuationStoppedEvent),
    ContinuationLimitReached(ContinuationLimitReachedEvent),
    UiIntentReceived(UiIntentReceivedEvent),
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
    pub run_name: String,
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
    pub task_id: String,
    pub state: TaskScheduleState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_key: Option<String>,
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
pub struct TaskRouteMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_catalog_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_category_binding: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_display_order: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_redelegate: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_fallback_applied: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explicit_subagent: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loaded_skills: Vec<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEffectKind {
    Allow,
    Deny,
    TransformContext,
    RequestReminder,
    WriteArtifact,
    AddDiagnostic,
    TruncateOutput,
    Recover,
    Notify,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookEffectMetadata {
    pub kind: HookEffectKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_ref: Option<EventArtifactRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookExecutionMetadata {
    pub hook_name: String,
    pub status: HookExecutionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook_event: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook_phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<HookEffectMetadata>,
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
    pub route: Option<TaskRouteMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_scope: Option<TaskTerminalScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<ExecutionTimingMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hook_executions: Vec<HookExecutionMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCancelledEvent {
    pub task_id: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_scope: Option<TaskTerminalScope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCompletedEvent {
    pub task_id: String,
    pub result_summary: String,
    pub result_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<TaskCompletionMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskResultLateEvent {
    pub task_id: String,
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
    pub parent_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_agent_id: Option<String>,
    pub child_session_id: String,
    pub child_request_id: String,
    pub task_id: String,
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
    pub task_id: String,
    pub stale_for_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserMessageSubmittedEvent {
    pub request_id: String,
    pub text: String,
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
    pub fallback_attempt: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_from_model_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_retryable: Option<bool>,
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
    pub provider_error_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_error_retryable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_attempt: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_from_model_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_retryable: Option<bool>,
}

/// Durable provider request start barrier.
///
/// `request_id`, `provider_id`, `model_id`, `prompt_summary`, and `request_digest` are the stable
/// replay-visible contract. `metadata` carries only optional, redacted, non-semantic provider
/// correlation hints: stable turn/request correlation, provider-call identity, and provider
/// session/cache ids. Raw provider payloads, unredacted thinking text, secrets, and
/// provider-specific control hints must not be persisted in this event. Provider stream chunk
/// boundaries remain presentation details derived from following delta events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRequestStartedEvent {
    pub request_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub prompt_summary: String,
    pub request_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ProviderRequestStartedMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStreamDeltaEvent {
    pub request_id: String,
    pub delta: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderReasoningDeltaEvent {
    pub request_id: String,
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
    pub request_id: String,
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
    pub request_id: String,
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
pub struct ToolCallRequestedEvent {
    pub tool_call_id: String,
    pub tool_id: String,
    pub args_summary: String,
    pub args_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ToolCallMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallStartedEvent {
    pub tool_call_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallFinishedEvent {
    pub tool_call_id: String,
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
    pub tool_call_id: Option<String>,
    pub summary: String,
    pub request_digest: String,
    pub timeout_ms: u64,
    pub default_decision: PermissionDecision,
}

pub struct PermissionRequestedArgs {
    pub permission_id: String,
    pub kind: String,
    pub tool_call_id: Option<String>,
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
    pub tool_call_id: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamCreatedEvent {
    pub team_run_id: String,
    pub spec: TeamSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<WorkflowEventMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamSpec {
    pub version: u16,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<TeamMemberSelector>,
    pub members: Vec<TeamMemberSpec>,
    pub bounds: TeamBounds,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TeamMemberSelector {
    Category { category: String },
    SubagentType { subagent_type: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamMemberSpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "TeamMemberRole::is_default_member")]
    pub role: TeamMemberRole,
    pub selector: TeamMemberSelector,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum TeamMemberRole {
    #[default]
    Member,
    Research,
}

impl TeamMemberRole {
    pub fn is_default_member(&self) -> bool {
        matches!(self, Self::Member)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Member => "member",
            Self::Research => "research",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamBounds {
    pub max_members: u32,
    pub max_parallel_members: u32,
    pub max_messages_per_run: u32,
    pub max_wall_clock_minutes: u32,
    pub max_member_turns: u32,
}

impl Default for TeamBounds {
    fn default() -> Self {
        Self {
            max_members: 8,
            max_parallel_members: 4,
            max_messages_per_run: 10_000,
            max_wall_clock_minutes: 120,
            max_member_turns: 500,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamMemberSpawnedEvent {
    pub team_run_id: String,
    pub member_name: String,
    pub agent_id: String,
    pub profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<WorkflowEventMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamReference {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TeamMessageKind {
    Message,
    Announcement,
    ShutdownRequest,
    ShutdownApproved,
    ShutdownRejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamMessage {
    pub version: u16,
    pub message_id: String,
    pub from: String,
    pub to: String,
    pub kind: TeamMessageKind,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<TeamReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamMessageSentEvent {
    pub team_run_id: String,
    pub message: TeamMessage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<WorkflowEventMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TeamTaskStatus {
    Pending,
    Claimed,
    InProgress,
    Completed,
    Deleted,
}

impl TeamTaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Claimed => "claimed",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Deleted => "deleted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamTask {
    pub version: u16,
    pub task_id: String,
    pub subject: String,
    pub description: String,
    pub status: TeamTaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamTaskCreatedEvent {
    pub team_run_id: String,
    pub task: TeamTask,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<WorkflowEventMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamTaskUpdatedEvent {
    pub team_run_id: String,
    pub task_id: String,
    pub status: TeamTaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<WorkflowEventMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PersistentTaskStatus {
    Pending,
    Claimed,
    InProgress,
    Completed,
    Cancelled,
}

impl PersistentTaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Claimed => "claimed",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PersistentTask {
    pub version: u16,
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub subject: String,
    pub description: String,
    pub status: PersistentTaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_form: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PersistentTaskCreatedEvent {
    pub task: PersistentTask,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PersistentTaskUpdatedEvent {
    pub task_id: String,
    pub status: PersistentTaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_form: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_by: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamShutdownRequestedEvent {
    pub team_run_id: String,
    pub member_name: String,
    pub requester: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<WorkflowEventMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamShutdownApprovedEvent {
    pub team_run_id: String,
    pub member_name: String,
    pub approver: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<WorkflowEventMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamShutdownRejectedEvent {
    pub team_run_id: String,
    pub member_name: String,
    pub rejecter: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<WorkflowEventMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamDeletedEvent {
    pub team_run_id: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<WorkflowEventMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowStartedEvent {
    pub workflow_id: String,
    pub mode: String,
    pub owner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowTransitionRecordedEvent {
    pub workflow_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_status: Option<String>,
    pub to_status: String,
    pub reason: String,
    pub owner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowTransitionDeniedEvent {
    pub workflow_id: String,
    pub requested_status: String,
    pub reason: String,
    pub owner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_status: Option<String>,
    pub policy_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowEvidenceRecordedEvent {
    pub workflow_id: String,
    pub category: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptance_ref: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowOperatorDecisionRecordedEvent {
    pub workflow_id: String,
    pub decision: String,
    pub operator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowCompletedEvent {
    pub workflow_id: String,
    pub outcome: String,
    pub reason: String,
    pub owner: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowEventMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iteration: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContinuationStartedEvent {
    pub continuation_id: String,
    pub mode: String,
    pub command: String,
    pub max_iterations: u32,
    pub max_wall_clock_ms: u64,
    pub max_provider_calls: u32,
    pub max_tool_calls: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<WorkflowEventMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContinuationReminderQueuedEvent {
    pub continuation_id: String,
    pub iteration: u32,
    pub reminder: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<WorkflowEventMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContinuationStoppedEvent {
    pub continuation_id: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<WorkflowEventMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContinuationLimitReachedEvent {
    pub continuation_id: String,
    pub limit: String,
    pub iteration: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<WorkflowEventMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiIntentReceivedEvent {
    pub intent: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
}

#[derive(Debug, Error)]
pub enum EventBuildError {
    #[error("failed to serialize event envelope for redaction: {0}")]
    SerializeEnvelope(#[source] serde_json::Error),
    #[error("failed to deserialize redacted event envelope: {0}")]
    DeserializeEnvelope(#[source] serde_json::Error),
}

pub struct EventBuilder<'a, C: Clock + ?Sized, R: Redactor + ?Sized> {
    clock: &'a C,
    redactor: &'a R,
    run_id: String,
}

impl<'a, C: Clock + ?Sized, R: Redactor + ?Sized> EventBuilder<'a, C, R> {
    pub fn new(clock: &'a C, redactor: &'a R, run_id: impl Into<String>) -> Self {
        Self {
            clock,
            redactor,
            run_id: run_id.into(),
        }
    }

    pub fn build(
        &self,
        context: EventContext,
        payload: EventV1,
    ) -> Result<EventEnvelopeV1, EventBuildError> {
        let envelope = EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: context
                .event_id
                .unwrap_or_else(|| default_event_id(context.seq)),
            seq: context.seq,
            run_id: self.run_id.clone(),
            mono_ms: self.clock.mono_ms(),
            ts: self.clock.system_time_rfc3339(),
            actor: context.actor,
            correlation_id: context.correlation_id,
            causation_id: context.causation_id,
            stream_key: context.stream_key,
            payload,
        };

        self.redact_envelope(envelope)
    }

    pub fn run_started(
        &self,
        context: EventContext,
        run_name: impl Into<String>,
        workspace_root: impl Into<String>,
    ) -> Result<EventEnvelopeV1, EventBuildError> {
        let payload = EventV1::RunStarted(RunStartedEvent {
            run_name: run_name.into(),
            workspace_root: workspace_root.into(),
        });
        self.build(context, payload)
    }

    pub fn permission_requested(
        &self,
        context: EventContext,
        args: PermissionRequestedArgs,
    ) -> Result<EventEnvelopeV1, EventBuildError> {
        let PermissionRequestedArgs {
            permission_id,
            kind,
            tool_call_id,
            summary,
            request_digest,
            timeout_ms,
            default_decision,
        } = args;
        let payload = EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id,
            kind,
            tool_call_id,
            summary,
            request_digest,
            timeout_ms,
            default_decision,
        });
        self.build(context, payload)
    }

    pub fn tool_call_requested(
        &self,
        context: EventContext,
        tool_call_id: impl Into<String>,
        tool_id: impl Into<String>,
        raw_args: &Value,
        metadata: Option<ToolCallMetadata>,
    ) -> Result<EventEnvelopeV1, EventBuildError> {
        let args_summary = self.summarize_and_redact(raw_args);
        let args_digest = value_digest(raw_args);
        let payload = EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.into(),
            tool_id: tool_id.into(),
            args_summary,
            args_digest,
            metadata,
        });

        self.build(context, payload)
    }

    fn summarize_and_redact(&self, value: &Value) -> String {
        let redacted = redact_value(self.redactor, value);
        let as_text = serde_json::to_string(&redacted).unwrap_or_else(|_| "null".to_string());
        truncate_with_ellipsis(&as_text, MAX_SUMMARY_CHARS)
    }

    fn redact_envelope(
        &self,
        envelope: EventEnvelopeV1,
    ) -> Result<EventEnvelopeV1, EventBuildError> {
        let value = serde_json::to_value(&envelope).map_err(EventBuildError::SerializeEnvelope)?;
        let redacted = redact_value(self.redactor, &value);
        serde_json::from_value(redacted).map_err(EventBuildError::DeserializeEnvelope)
    }
}

fn default_event_id(seq: u64) -> String {
    format!("{DEFAULT_EVENT_ID_PREFIX}-{seq:020}")
}

fn value_digest(value: &Value) -> String {
    let canonical = canonicalize_json(value);
    digest12_json(&canonical)
}

fn canonicalize_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut ordered = serde_json::Map::new();
            for (key, value) in map.iter().collect::<BTreeMap<_, _>>() {
                ordered.insert(key.clone(), canonicalize_json(value));
            }
            Value::Object(ordered)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonicalize_json).collect()),
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActorKind, EventActor, EventBuilder, EventContext, EventV1, PermissionDecision,
        PermissionRequestedArgs, ToolCallRequestedEvent,
    };
    use crate::clock::FakeClock;
    use crate::redact::DefaultRedactor;
    use serde_json::json;

    #[test]
    fn run_started_snapshot_is_stable_in_deterministic_mode() {
        let clock = FakeClock::new();
        clock.advance(42);
        let redactor = DefaultRedactor::default();
        let builder = EventBuilder::new(&clock, &redactor, "run_123");

        let mut context = EventContext::new(
            1,
            EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string())),
        );
        context.stream_key = Some("run:run_123".to_string());

        let envelope = builder
            .run_started(context, "golden_path", "/workspace/project")
            .expect("build run started envelope");

        insta::assert_json_snapshot!("run_started_envelope_v1", envelope);
    }

    #[test]
    fn permission_requested_snapshot_is_stable_in_deterministic_mode() {
        let clock = FakeClock::new();
        clock.advance(128);
        let redactor = DefaultRedactor::default();
        let builder = EventBuilder::new(&clock, &redactor, "run_123");

        let mut context = EventContext::new(
            2,
            EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        );
        context.correlation_id = Some("toolcall_001".to_string());
        context.stream_key = Some("permission:perm_001".to_string());

        let envelope = builder
            .permission_requested(
                context,
                PermissionRequestedArgs {
                    permission_id: "perm_001".to_string(),
                    kind: "edit".to_string(),
                    tool_call_id: Some("toolcall_001".to_string()),
                    summary: "Apply patch to file with Bearer abc.def".to_string(),
                    request_digest: "req_90ac2e1e".to_string(),
                    timeout_ms: 30_000,
                    default_decision: PermissionDecision::Deny,
                },
            )
            .expect("build permission requested envelope");

        insta::assert_json_snapshot!("permission_requested_envelope_v1", envelope);
    }

    #[test]
    fn tool_call_requested_uses_redacted_summary_and_digest() {
        let clock = FakeClock::new();
        let redactor = DefaultRedactor::default();
        let builder = EventBuilder::new(&clock, &redactor, "run_123");

        let args = json!({
            "cmd": "curl https://example.invalid",
            "auth": "Bearer secret.value",
            "api_key": "sk-ABCDE12345ABCDE",
        });

        let envelope = builder
            .tool_call_requested(
                EventContext::new(
                    3,
                    EventActor::new(ActorKind::Worker, Some("agent-worker".to_string())),
                ),
                "toolcall_002",
                "shell.run",
                &args,
                None,
            )
            .expect("build tool call requested envelope");

        let EventV1::ToolCallRequested(ToolCallRequestedEvent {
            args_summary,
            args_digest,
            ..
        }) = envelope.payload
        else {
            panic!("expected tool call requested payload")
        };

        assert!(!args_summary.contains("Bearer secret.value"));
        assert!(!args_summary.contains("sk-ABCDE12345ABCDE"));
        assert!(args_summary.contains("Bearer [REDACTED]"));
        assert!(args_summary.contains("[REDACTED_API_KEY]"));
        assert_eq!(args_digest.len(), 12);
    }
}
