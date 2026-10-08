use serde::{Deserialize, Serialize};
mod compaction;
mod provider;
mod reminder;
mod subagent_progress;
mod task;
mod tool;
mod workspace;
pub use compaction::*;
pub use provider::*;
pub use reminder::*;
pub use subagent_progress::*;
pub use task::*;
pub use tool::*;
pub use workspace::*;

pub const SCHEMA_VERSION: u16 = 1;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveEventEnvelope {
    pub event_id: String,
    pub run_id: crate::ids::RunId,
    pub mono_ms: u64,
    pub ts: Option<String>,
    pub actor: EventActor,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub stream_key: Option<String>,
    pub payload: LiveEventV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event_type", content = "data", rename_all = "snake_case")]
pub enum LiveEventV1 {
    EvalProgress {
        tool_call_id: crate::ids::ToolCallId,
        output: String,
        details: serde_json::Value,
    },
    SubagentProgress(SubagentProgressEvent),
    RuntimeWarning {
        message: String,
    },
    CompactionProgress {
        agent_id: String,
        generation: u64,
        trigger_reason: String,
        preview: Option<String>,
    },
    ProviderRetrying {
        turn_id: crate::ids::RequestId,
        retry: ProviderRequestRetryMetadata,
    },
    ProviderTextDelta {
        request_id: crate::ids::ProviderRequestId,
        delta: String,
    },
    ProviderReasoningDelta {
        request_id: crate::ids::ProviderRequestId,
        delta: String,
    },
    ProviderToolInputDelta {
        request_id: crate::ids::ProviderRequestId,
        tool_call_id: crate::ids::ToolCallId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_name: Option<String>,
        delta: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "delivery", content = "event", rename_all = "snake_case")]
pub enum RuntimeEvent {
    Durable(Box<EventEnvelopeV1>),
    Live(Box<LiveEventEnvelope>),
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event_type", content = "data", rename_all = "snake_case")]
pub enum EventV1 {
    EvalCellFinished(ToolCallFinishedEvent),
    RunStarted(RunStartedEvent),
    RunFinished(RunFinishedEvent),
    UserMessageSubmitted(UserMessageSubmittedEvent),
    SessionTitleUpdated(SessionTitleUpdatedEvent),
    RunFailed(RunFailedEvent),
    AgentSpawned(AgentSpawnedEvent),
    AgentStopped(AgentStoppedEvent),
    TaskScheduled(TaskScheduledEvent),
    TaskCancelled(TaskCancelledEvent),
    TaskCompleted(TaskCompletedEvent),
    TaskResultLate(TaskResultLateEvent),
    BackgroundTaskNotification(BackgroundTaskNotificationEvent),
    StaleDetected(StaleDetectedEvent),
    PromptAttachmentsSubmitted(PromptAttachmentsSubmittedEvent),
    ProviderRequestStarted(ProviderRequestStartedEvent),
    ProviderStreamDelta(ProviderStreamDeltaEvent),
    ProviderReasoningDelta(ProviderReasoningDeltaEvent),
    ProviderRequestFinished(ProviderRequestFinishedEvent),
    AssistantMessageFinished(AssistantMessageFinishedEvent),
    CompactionRequested(CompactionRequestedEvent),
    CompactionWritten(CompactionWrittenEvent),
    CompactionApplied(CompactionAppliedEvent),
    CompactionFailed(CompactionFailedEvent),
    SessionCompaction(SessionCompactionEvent),
    BranchSummary(BranchSummaryEvent),
    ToolCallRequested(ToolCallRequestedEvent),
    ToolCallStarted(ToolCallStartedEvent),
    ToolCallFinished(ToolCallFinishedEvent),
    PermissionRequested(PermissionRequestedEvent),
    PermissionGrantRecorded(PermissionGrantRecordedEvent),
    PermissionResolved(PermissionResolvedEvent),
    YoloModeChanged { enabled: bool },
    EditProposed(EditProposedEvent),
    EditApplied(EditAppliedEvent),
    EditRejected(EditRejectedEvent),
    ArtifactWritten(ArtifactWrittenEvent),
    PolicyViolationDetected(PolicyViolationDetectedEvent),
    UiIntentReceived(UiIntentReceivedEvent),
    ConversationRewound(crate::conversation_rewind::ConversationRewoundEvent),
    WorkspaceSnapshot(WorkspaceSnapshotEvent),
    WorkspaceReverted(WorkspaceRevertedEvent),
    SubagentTransition(Box<crate::subagent::SubagentTransitionV1>),
    FinalizedAgentState(crate::subagent::FinalizedAgentStateReferenceV1),
    AgentContextInitialized(crate::subagent::AgentContextInitializedV1),
    AgentExecutionContextChanged(crate::subagent::AgentExecutionContextChangedV1),
    SubagentCancelRequested(crate::subagent::SubagentCancelIntentV1),
    NativeSubagentRegistered(Box<crate::coord::NativeSubagentRegistration>),
    NativeSubagentMessage(Box<crate::coord::NativeMessageReceipt>),
    NativeSubagentReceipt(Box<crate::coord::NativeSubagentReceipt>),
    NativeSubagentWorkspace(Box<crate::coord::NativeWorkspaceReceipt>),
    RuntimeReminder(RuntimeReminderEvent),
    CommandNotice(CommandNoticeEvent),
    SteeringAccepted(SteeringAcceptedEvent),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFailedEvent {
    pub error: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTitleUpdatedEvent {
    pub title: String,
}

impl EventV1 {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::RunStarted(..) => "run_started",
            Self::RunFinished(..) => "run_finished",
            Self::UserMessageSubmitted(..) => "user_message_submitted",
            Self::SessionTitleUpdated(..) => "session_title_updated",
            Self::RunFailed(..) => "run_failed",
            Self::AgentSpawned(..) => "agent_spawned",
            Self::AgentStopped(..) => "agent_stopped",
            Self::TaskScheduled(..) => "task_scheduled",
            Self::TaskCancelled(..) => "task_cancelled",
            Self::TaskCompleted(..) => "task_completed",
            Self::TaskResultLate(..) => "task_result_late",
            Self::BackgroundTaskNotification(..) => "background_task_notification",
            Self::StaleDetected(..) => "stale_detected",
            Self::PromptAttachmentsSubmitted(..) => "prompt_attachments_submitted",
            Self::ProviderRequestStarted(..) => "provider_request_started",
            Self::ProviderStreamDelta(..) => "provider_stream_delta",
            Self::ProviderReasoningDelta(..) => "provider_reasoning_delta",
            Self::ProviderRequestFinished(..) => "provider_request_finished",
            Self::AssistantMessageFinished(..) => "assistant_message_finished",
            Self::CompactionRequested(..) => "compaction_requested",
            Self::CompactionWritten(..) => "compaction_written",
            Self::CompactionApplied(..) => "compaction_applied",
            Self::CompactionFailed(..) => "compaction_failed",
            Self::SessionCompaction(..) => "session_compaction",
            Self::BranchSummary(..) => "branch_summary",
            Self::ToolCallRequested(..) => "tool_call_requested",
            Self::ToolCallStarted(..) => "tool_call_started",
            Self::ToolCallFinished(..) => "tool_call_finished",
            Self::EvalCellFinished(..) => "eval_cell_finished",
            Self::PermissionRequested(..) => "permission_requested",
            Self::PermissionGrantRecorded(..) => "permission_grant_recorded",
            Self::PermissionResolved(..) => "permission_resolved",
            Self::YoloModeChanged { .. } => "yolo_mode_changed",
            Self::EditProposed(..) => "edit_proposed",
            Self::EditApplied(..) => "edit_applied",
            Self::EditRejected(..) => "edit_rejected",
            Self::ArtifactWritten(..) => "artifact_written",
            Self::PolicyViolationDetected(..) => "policy_violation_detected",
            Self::UiIntentReceived(..) => "ui_intent_received",
            Self::ConversationRewound(..) => "conversation_rewound",
            Self::WorkspaceSnapshot(..) => "workspace_snapshot",
            Self::WorkspaceReverted(..) => "workspace_reverted",
            Self::SubagentTransition(..) => "subagent_transition",
            Self::FinalizedAgentState(..) => "finalized_agent_state",
            Self::AgentContextInitialized(..) => "agent_context_initialized",
            Self::AgentExecutionContextChanged(..) => "agent_execution_context_changed",
            Self::SubagentCancelRequested(..) => "subagent_cancel_requested",
            Self::NativeSubagentRegistered(..) => "native_subagent_registered",
            Self::NativeSubagentMessage(..) => "native_subagent_message",
            Self::NativeSubagentReceipt(..) => "native_subagent_receipt",
            Self::NativeSubagentWorkspace(..) => "native_subagent_workspace",
            Self::RuntimeReminder(..) => "runtime_reminder",
            Self::CommandNotice(..) => "command_notice",
            Self::SteeringAccepted(..) => "steering_accepted",
        }
    }

    pub fn lineage_parent_session_id(&self) -> Option<&str> {
        match self {
            Self::TaskCompleted(e) => e.metadata.as_ref()?.lineage.as_ref(),
            Self::ToolCallRequested(e) => e.metadata.as_ref()?.lineage.as_ref(),
            Self::ToolCallFinished(e) | Self::EvalCellFinished(e) => {
                e.metadata.as_ref()?.lineage.as_ref()
            }
            _ => None,
        }?
        .non_empty_parent_session_id()
    }
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
pub struct RunStartedEvent {
    pub run_name: crate::ids::RunName,
    pub workspace_root: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFinishedEvent {
    pub summary: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserMessageSubmittedEvent {
    pub request_id: crate::ids::RequestId,
    pub text: String,
}
