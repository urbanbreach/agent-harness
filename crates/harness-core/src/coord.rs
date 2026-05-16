use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{sleep, MissedTickBehavior};
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::agent::{
    build_provider_context_messages, build_provider_tool_defs, default_model_settings_for_profile,
    default_provider, stream_assistant_response_once, tool_result_to_message_content,
    AgentModelRef, AgentModelSettings, AgentProfile, AgentRequest, AgentRuntimeEvent,
    AgentTurnFailure, AgentTurnOutcome, AssistantResponse, AssistantToolIntent,
    ProviderBoundaryContext, ProviderCompactionFacts, ProviderCompactionSummarySource,
    ProviderCompactionTailBoundary, ProviderCompactionTimelineEntry, ProviderCompactionTurnFact,
    ProviderContext, ProviderContextCheckpoint, ProviderContextCheckpointMetadata,
    ProviderConversationTurn, ProviderConversationTurnStatus, ProviderFileOperationFact,
    StreamAssistantResponseOnceRequest, MAX_TOOL_CALLS_TOTAL,
};
use crate::clock::Clock;
use crate::config::{
    registered_hook_runtime_config, registered_mcp_server_first_class_tool_id,
    CompactionRuntimeConfig, HookLifecycleEvent, HookRuntimeConfig, LifecycleHookConfig,
    ShellAllowlist, ToolFailureMode,
};
use crate::context_snapshot::{
    build_context_snapshot, snapshot_write_result, write_context_snapshot_artifact,
    ContextSnapshotInput, ContextSnapshotOptions, ContextSnapshotWriteResult,
    CONTEXT_SNAPSHOT_ARTIFACT_KIND, CONTEXT_SNAPSHOT_EVIDENCE_CATEGORY,
};
use crate::continuation::{ContinuationBounds, ContinuationController, ContinuationDecision};
use crate::conversation::{
    ConversationAssistantMessage, ConversationMessage, ConversationToolCall,
    ConversationToolResultMessage, ConversationUserMessage,
};
use crate::counter_id::parse_prefixed_counter;
use crate::digest::{digest12, digest12_json};
use crate::edit::hashline::HashlinePatch;
use crate::event::{
    ActorKind, AgentSpawnedEvent, ArtifactWrittenEvent, BackgroundTaskNotificationEvent,
    BackgroundTaskNotificationStatus, CompactionAppliedEvent, CompactionFailedEvent,
    CompactionRequestedEvent, CompactionWrittenEvent, ContinuationLimitReachedEvent,
    ContinuationReminderQueuedEvent, ContinuationStartedEvent, ContinuationStoppedEvent,
    EditAppliedEvent, EditProposedEvent, EditRejectedEvent, EventActor, EventArtifactRef,
    EventBuildError, EventBuilder, EventContext, EventEnvelopeV1, EventV1, ExecutionTimingMetadata,
    HookEffectKind, HookEffectMetadata, HookExecutionMetadata, HookExecutionStatus,
    PermissionDecision as EventPermissionDecision, PermissionGrantRecordedEvent,
    PermissionRequestedArgs, PermissionResolvedEvent, PersistentTask, PersistentTaskCreatedEvent,
    PersistentTaskStatus, PersistentTaskUpdatedEvent, PolicyViolationDetectedEvent,
    ProviderAssistantMessageMetadata, ProviderReasoningDeltaEvent, ProviderRequestFinishedMetadata,
    ProviderRequestStartedMetadata, ResolvedToolIdentity, RunFinishedEvent, RunStartedEvent,
    StaleDetectedEvent, TaskCancelledEvent, TaskCompletedEvent, TaskCompletionMetadata,
    TaskLineageMetadata, TaskResultLateEvent, TaskRouteMetadata, TaskScheduleState,
    TaskScheduledEvent, TaskTerminalScope, TeamBounds, TeamCreatedEvent, TeamDeletedEvent,
    TeamMemberRole, TeamMemberSelector, TeamMemberSpawnedEvent, TeamMemberSpec, TeamMessage,
    TeamMessageKind, TeamMessageSentEvent, TeamShutdownApprovedEvent, TeamShutdownRejectedEvent,
    TeamShutdownRequestedEvent, TeamSpec, TeamTask, TeamTaskCreatedEvent, TeamTaskStatus,
    TeamTaskUpdatedEvent, ToolCallFinishedEvent, ToolCallMetadata, ToolCallStartedEvent,
    ToolCallStatus, ToolIdentityMetadata, UserMessageSubmittedEvent, WorkflowCompletedEvent,
    WorkflowEventMetadata, WorkflowEvidenceRecordedEvent, WorkflowOperatorDecisionRecordedEvent,
    WorkflowTransitionDeniedEvent, WorkflowTransitionRecordedEvent,
};
use crate::path_selector::workspace_relative_path_from_maybe_absolute;
use crate::perm::{
    permission_kind_for_tool, permission_kind_for_tool_call, PermissionDecision, PermissionGrant,
    PermissionGrantMatcher, PermissionGrantRequest, PermissionGrantScope, PermissionGrantSet,
    PermissionKind, PermissionPolicy, PermissionRuleRequest, PermissionToolSelector,
    PolicyDecision,
};
use crate::persistent_task::{
    apply_persistent_task_update, blocked_by_incomplete, has_persistent_task_dependency_path,
    project_persistent_tasks, PersistentTaskProjection,
};
use crate::proj::{
    inspect_resume_plan, project_background_request, project_team_state,
    resolve_background_request_ref, BackgroundRequestProjection, BackgroundRequestProjectionError,
    RecordedRuntimeContext, RunMetadata, SessionModeSource, TeamProjection, TeamRunProjection,
};
use crate::provider_args::provider_tool_arguments_json;
use crate::provider_recovery::{classify_provider_error, is_provider_context_overflow_reason};
use crate::question_answers::{validate_question_answers, QuestionAnswerPrompt};
use crate::redact::{redact_value, DefaultRedactor, Redactor};
use crate::sched::{
    ConcurrencyKey, ScheduleDecision, Scheduler, SchedulerLimits, TaskProgressSnapshot,
};
use crate::session_paths::{ARTIFACTS_DIR_NAME, EVENTS_FILE_NAME, META_FILE_NAME};
use crate::session_title::{
    clean_generated_title, create_default_title, is_parent_default_title, TITLE_AGENT_NAME,
    TITLE_GENERATION_USER_PROMPT,
};
use crate::store::{EventEnvelopeWithoutSeqV1, EventStore, EventStoreError, JsonlFileEventStore};
use crate::text::{non_empty_trimmed, truncate_with_ellipsis};
use crate::tool::{
    canonical_tool_id_for, sanitize_mcp_tool_segment, ToolContext, ToolRegistry, ToolResult,
};
use crate::workflow::{
    project_workflows, WorkflowCompletionReadiness, WorkflowEvidenceRequest, WorkflowSignoffPolicy,
    WorkflowStartDecision, WorkflowStartRequest, WorkflowStartResult, WorkflowTransitionPolicy,
    WorkflowTransitionRequest,
};
use harness_providers::{
    AssistantToolCall, CompletionMessage, CompletionRequest, MessageRole, Provider,
    ProviderStreamEvent, ToolDef,
};

const DEFAULT_COMMAND_BUFFER: usize = 64;
const DEFAULT_TOOL_CONCURRENCY: usize = 1;
const DEFAULT_PROVIDER_MODEL_CONCURRENCY: usize = 1;
const DEFAULT_STALE_TIMEOUT_MS: u64 = 15_000;
const DEFAULT_WATCHDOG_TICK_MS: u64 = 100;
const DEFAULT_SIMULATED_JOB_DURATION_MS: u64 = 10;
const DEFAULT_QUESTION_TIMEOUT_MS: u64 = 0;
const COORDINATOR_AGENT_ID: &str = "coordinator";
const HASHLINE_APPLY_TOOL_ID: &str = "edit.hashline_apply";
const PROVIDER_CONTEXT_COMPACTION_RESERVE_TOKENS: u32 = 1_024;
const PROVIDER_CONTEXT_COMPACTION_KEEP_RECENT_MAX_TOKENS: u32 = 8_000;
const PROVIDER_CONTEXT_COMPACTION_KEEP_RECENT_MIN_TOKENS: u32 = 2_000;
const PROVIDER_CONTEXT_COMPACTION_SUMMARY_MAX_CHARS: usize = 6_000;
const PROVIDER_CONTEXT_COMPACTION_TURN_EXCERPT_MAX_CHARS: usize = 240;
const PROVIDER_CONTEXT_SPLIT_PREFIX_SUMMARY_MAX_CHARS: usize = 1_200;
const PROVIDER_CONTEXT_FILE_OPERATION_FACT_LIMIT: usize = 50;
const PROVIDER_CONTEXT_OPERATION_FACT_LIMIT: usize = 20;
const PROVIDER_CONTEXT_SUMMARY_CONTRACT_VERSION: u32 = 2;
const BACKGROUND_TASK_NOTIFICATION_SUMMARY_MAX_CHARS: usize = 511;
const BACKGROUND_TASK_NOTIFICATION_DESCRIPTION_MAX_CHARS: usize = 160;
const TEAM_MESSAGE_BODY_MAX_BYTES: usize = 32 * 1024;
const TEAM_TEXT_FIELD_MAX_CHARS: usize = 512;
const TEAM_TASK_METADATA_MAX_ENTRIES: usize = 32;
const TEAM_TASK_METADATA_MAX_CHARS: usize = 256;
const TEAM_REFERENCE_LIMIT: usize = 32;
const TEAM_MAX_MEMBERS: usize = 8;
const PROVIDER_CONTEXT_SPLIT_PREFIX_SUMMARY_HEADINGS: &[&str] = &[
    "## Original Request",
    "## Early Progress",
    "## Context for Suffix",
];
const PROVIDER_CONTEXT_HARNESS_SUMMARY_HEADINGS: &[&str] = &[
    "## Goal",
    "## Constraints",
    "## Progress",
    "## Key Decisions",
    "## Next Steps",
    "## Critical Context",
];
const PROVIDER_CONTEXT_LEGACY_SUMMARY_HEADINGS: &[&str] = &[
    "## Goal",
    "## Constraints & Preferences",
    "## Progress",
    "### Done",
    "### In Progress",
    "### Blocked",
    "## Key Decisions",
    "## Next Steps",
    "## Critical Context",
    "## Source Facts",
    "## Relevant Files / Artifacts",
];

fn provider_context_summary_required_headings(
    config: &CompactionRuntimeConfig,
) -> &'static [&'static str] {
    if config.structured_summary_contract {
        PROVIDER_CONTEXT_HARNESS_SUMMARY_HEADINGS
    } else {
        PROVIDER_CONTEXT_LEGACY_SUMMARY_HEADINGS
    }
}

fn warn_oneshot_send_failure<T>(result: Result<(), T>, operation: &str) {
    if result.is_err() {
        tracing::warn!(
            operation,
            "coordinator response receiver dropped before result delivery"
        );
    }
}

fn warn_command_send_failure(result: Result<(), mpsc::error::SendError<Command>>, operation: &str) {
    if result.is_err() {
        tracing::warn!(operation, "coordinator background command channel closed");
    }
}

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub struct CoordinatorConfig {
    pub session_dir: PathBuf,
    pub run_id_override: Option<String>,
    pub deterministic_store: bool,
    pub command_buffer: usize,
    pub permission_policy: PermissionPolicy,
    pub tool_concurrency: usize,
    pub provider_model_concurrency: usize,
    pub stale_timeout_ms: u64,
    pub watchdog_tick_ms: u64,
    pub simulated_job_duration_ms: u64,
    pub tool_registry: Arc<ToolRegistry>,
    pub provider: Arc<dyn Provider>,
    pub agent_profiles: BTreeMap<String, AgentProfile>,
    pub hook_runtime_config: HookRuntimeConfig,
    pub compaction: CompactionRuntimeConfig,
    pub config_digest: String,
    pub harness_version: String,
    pub session_mode_source: Option<SessionModeSource>,
}

impl CoordinatorConfig {
    pub fn new(session_dir: impl Into<PathBuf>) -> Self {
        Self {
            session_dir: session_dir.into(),
            run_id_override: None,
            deterministic_store: false,
            command_buffer: DEFAULT_COMMAND_BUFFER,
            permission_policy: PermissionPolicy::default(),
            tool_concurrency: DEFAULT_TOOL_CONCURRENCY,
            provider_model_concurrency: DEFAULT_PROVIDER_MODEL_CONCURRENCY,
            stale_timeout_ms: DEFAULT_STALE_TIMEOUT_MS,
            watchdog_tick_ms: DEFAULT_WATCHDOG_TICK_MS,
            simulated_job_duration_ms: DEFAULT_SIMULATED_JOB_DURATION_MS,
            tool_registry: Arc::new(ToolRegistry::new()),
            provider: default_provider(),
            agent_profiles: BTreeMap::new(),
            hook_runtime_config: registered_hook_runtime_config(),
            compaction: CompactionRuntimeConfig::default(),
            config_digest: "none".to_string(),
            harness_version: env!("CARGO_PKG_VERSION").to_string(),
            session_mode_source: None,
        }
    }

    pub fn with_tool_registry(mut self, tool_registry: Arc<ToolRegistry>) -> Self {
        self.tool_registry = tool_registry;
        self
    }
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self::new(".agent-harness/sessions")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunInfo {
    pub run_id: String,
    pub run_name: String,
    pub workspace_root: PathBuf,
    pub run_dir: PathBuf,
    pub artifacts_dir: PathBuf,
    pub events_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobProgressKind {
    Heartbeat,
    OutputChunk,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JobOutcome {
    Succeeded { result: ToolResult },
    Failed { error: String },
    Cancelled { reason: String },
}

#[derive(Debug)]
pub enum Command {
    StartRun {
        run_name: String,
        workspace_root: PathBuf,
        respond_to: oneshot::Sender<Result<RunInfo, CoordinatorError>>,
    },
    ResumeRun {
        run_id: String,
        run_name: String,
        respond_to: oneshot::Sender<Result<RunInfo, CoordinatorError>>,
    },
    StopRun {
        respond_to: oneshot::Sender<Result<(), CoordinatorError>>,
    },
    GetEventStore {
        respond_to: oneshot::Sender<Result<Arc<JsonlFileEventStore>, CoordinatorError>>,
    },
    GetAgentRuntimeInfo {
        agent_id: String,
        respond_to: oneshot::Sender<Result<AgentRuntimeInfo, CoordinatorError>>,
    },
    SpawnAgent {
        actor: EventActor,
        profile: String,
        parent_agent_id: Option<String>,
        child_session_title: Option<String>,
        respond_to: oneshot::Sender<Result<String, CoordinatorError>>,
    },
    SpawnAgentIdle {
        actor: EventActor,
        profile: String,
        parent_agent_id: Option<String>,
        child_session_title: Option<String>,
        respond_to: oneshot::Sender<Result<String, CoordinatorError>>,
    },
    RequestAgentTurn {
        actor: EventActor,
        agent_id: String,
        prompt: String,
        selected_file_tags: Vec<crate::file_tag::SelectedFileTag>,
        selected_agent_tags: Vec<crate::file_tag::SelectedAgentTag>,
        selected_resource_tags: Vec<crate::file_tag::SelectedResourceTag>,
        model_ref_override: Option<String>,
        model_settings_override: Option<AgentModelSettings>,
        child_task_metadata: Option<ChildTaskRequestMetadata>,
        respond_to: oneshot::Sender<Result<String, CoordinatorError>>,
    },
    AgentProviderReasoningDelta {
        task_id: String,
        agent_id: String,
        request_id: String,
        delta: String,
    },
    RequestToolCall {
        actor: EventActor,
        category: Option<String>,
        tool_id: String,
        args_json: Value,
        respond_to: oneshot::Sender<Result<String, CoordinatorError>>,
    },
    ExecuteAgentToolCall {
        actor: EventActor,
        category: Option<String>,
        tool_id: String,
        args_json: Value,
        respond_to: oneshot::Sender<Result<ToolResult, String>>,
    },
    RequestQuestion {
        actor: EventActor,
        tool_call_id: String,
        request_json: Value,
        respond_to: oneshot::Sender<Result<Vec<Vec<String>>, String>>,
    },
    WriteContextSnapshot {
        actor: EventActor,
        workflow_id: Option<String>,
        input: ContextSnapshotInput,
        options: ContextSnapshotOptions,
        respond_to: oneshot::Sender<Result<ContextSnapshotWriteResult, CoordinatorError>>,
    },
    StartWorkflow {
        actor: EventActor,
        request: WorkflowStartRequest,
        respond_to: oneshot::Sender<Result<WorkflowStartResult, CoordinatorError>>,
    },
    RecordWorkflowTransition {
        actor: EventActor,
        request: WorkflowTransitionRequest,
        respond_to: oneshot::Sender<Result<(), CoordinatorError>>,
    },
    RecordWorkflowEvidence {
        actor: EventActor,
        request: WorkflowEvidenceRequest,
        respond_to: oneshot::Sender<Result<(), CoordinatorError>>,
    },
    RecordWorkflowOperatorDecision {
        actor: EventActor,
        workflow_id: String,
        decision: String,
        operator: String,
        reason: Option<String>,
        correlation_id: Option<String>,
        respond_to: oneshot::Sender<Result<(), CoordinatorError>>,
    },
    CompleteWorkflow {
        actor: EventActor,
        workflow_id: String,
        outcome: String,
        reason: String,
        owner: String,
        respond_to: oneshot::Sender<Result<(), CoordinatorError>>,
    },
    CompleteWorkflowWithSignoffPolicy {
        actor: EventActor,
        workflow_id: String,
        outcome: String,
        reason: String,
        owner: String,
        signoff_policy: WorkflowSignoffPolicy,
        respond_to: oneshot::Sender<Result<(), CoordinatorError>>,
    },
    ResolvePermission {
        permission_id: String,
        decision: PermissionDecision,
        reason: Option<String>,
        grant_scope: Option<PermissionGrantScope>,
        respond_to: oneshot::Sender<Result<(), CoordinatorError>>,
    },
    PermissionTimedOut {
        permission_id: String,
    },
    JobProgress {
        task_id: String,
        kind: JobProgressKind,
    },
    CancelTask {
        task_id: String,
        reason: String,
        respond_to: oneshot::Sender<Result<(), CoordinatorError>>,
    },
    GetBackgroundRequestProjection {
        actor: EventActor,
        request_id: Option<String>,
        selector_hint: Option<String>,
        respond_to: oneshot::Sender<Result<BackgroundRequestProjection, CoordinatorError>>,
    },
    CancelBackgroundRequest {
        actor: EventActor,
        request_id: Option<String>,
        selector_hint: Option<String>,
        reason: String,
        respond_to: oneshot::Sender<Result<BackgroundRequestProjection, CoordinatorError>>,
    },
    CreateTeam {
        actor: EventActor,
        spec: TeamSpec,
        team_run_id: Option<String>,
        respond_to: oneshot::Sender<Result<TeamRunProjection, CoordinatorError>>,
    },
    GetTeamProjection {
        respond_to: oneshot::Sender<Result<TeamProjection, CoordinatorError>>,
    },
    SendTeamMessage {
        actor: EventActor,
        team_run_id: String,
        message: TeamMessage,
        respond_to: oneshot::Sender<Result<TeamRunProjection, CoordinatorError>>,
    },
    CreateTeamTask {
        actor: EventActor,
        team_run_id: String,
        task: TeamTask,
        respond_to: oneshot::Sender<Result<TeamRunProjection, CoordinatorError>>,
    },
    UpdateTeamTask {
        actor: EventActor,
        team_run_id: String,
        task_id: String,
        status: TeamTaskStatus,
        owner: Option<String>,
        metadata: BTreeMap<String, String>,
        respond_to: oneshot::Sender<Result<TeamRunProjection, CoordinatorError>>,
    },
    GetPersistentTaskProjection {
        respond_to: oneshot::Sender<Result<PersistentTaskProjection, CoordinatorError>>,
    },
    CreatePersistentTask {
        actor: EventActor,
        task: PersistentTask,
        respond_to: oneshot::Sender<Result<PersistentTaskProjection, CoordinatorError>>,
    },
    UpdatePersistentTask {
        actor: EventActor,
        update: PersistentTaskUpdatedEvent,
        respond_to: oneshot::Sender<Result<PersistentTaskProjection, CoordinatorError>>,
    },
    RequestTeamShutdown {
        actor: EventActor,
        team_run_id: String,
        member_name: String,
        requester: String,
        respond_to: oneshot::Sender<Result<TeamRunProjection, CoordinatorError>>,
    },
    ApproveTeamShutdown {
        actor: EventActor,
        team_run_id: String,
        member_name: String,
        approver: String,
        respond_to: oneshot::Sender<Result<TeamRunProjection, CoordinatorError>>,
    },
    RejectTeamShutdown {
        actor: EventActor,
        team_run_id: String,
        member_name: String,
        rejecter: String,
        reason: String,
        respond_to: oneshot::Sender<Result<TeamRunProjection, CoordinatorError>>,
    },
    DeleteTeam {
        actor: EventActor,
        team_run_id: String,
        respond_to: oneshot::Sender<Result<TeamRunProjection, CoordinatorError>>,
    },
    JobFinished {
        task_id: String,
        outcome: JobOutcome,
    },
    AgentProviderRequestStarted {
        task_id: String,
        agent_id: String,
        request_id: String,
        provider_id: String,
        model_id: String,
        prompt_summary: String,
        request_digest: String,
        metadata: Option<ProviderRequestStartedMetadata>,
    },
    AgentProviderStreamDelta {
        task_id: String,
        agent_id: String,
        request_id: String,
        delta: String,
    },
    AgentProviderRequestFinished {
        task_id: String,
        agent_id: String,
        request_id: String,
        finish_reason: String,
        output_digest: Option<String>,
        usage: Option<harness_providers::CompletionUsage>,
        metadata: Option<ProviderRequestFinishedMetadata>,
        respond_to: Option<oneshot::Sender<Result<(), CoordinatorError>>>,
    },
    AgentAssistantMessageFinished {
        task_id: String,
        agent_id: String,
        request_id: String,
        assistant_output: String,
        tool_call_count: usize,
        assistant_message: Option<ProviderAssistantMessageMetadata>,
        respond_to: oneshot::Sender<Result<(), CoordinatorError>>,
    },
    AllocateProviderRequestId {
        respond_to: oneshot::Sender<Result<String, CoordinatorError>>,
    },
    SwitchAgentTurnProviderModelSlot {
        task_id: String,
        agent_id: String,
        model_ref: String,
        model_settings: AgentModelSettings,
        respond_to: oneshot::Sender<Result<bool, CoordinatorError>>,
    },
    CompactAgentContext {
        task_id: String,
        agent_id: String,
        request_id: String,
        trigger_reason: String,
        usage: Option<harness_providers::CompletionUsage>,
        respond_to: oneshot::Sender<Result<ProviderContext, CoordinatorError>>,
    },
    ManualCompactAgentContext {
        agent_id: String,
        through_request_id: Option<String>,
        trigger_reason: String,
        respond_to: oneshot::Sender<Result<ManualCompactionOutcome, CoordinatorError>>,
    },
    StartContinuation {
        actor: EventActor,
        mode: String,
        command: String,
        bounds: ContinuationBounds,
        workflow: Option<WorkflowEventMetadata>,
        respond_to: oneshot::Sender<Result<String, CoordinatorError>>,
    },
    StopContinuation {
        actor: EventActor,
        reason: String,
        respond_to: oneshot::Sender<Result<(), CoordinatorError>>,
    },
    TriggerContinuationReminder {
        actor: EventActor,
        agent_id: String,
        reason: String,
        done_marker_seen: bool,
        incomplete_todos: Option<bool>,
        provider_calls: u32,
        tool_calls: u32,
        respond_to: oneshot::Sender<Result<Option<String>, CoordinatorError>>,
    },
    QueueContinuationReminder {
        actor: EventActor,
        continuation_id: String,
        iteration: u32,
        reminder: String,
        reason: String,
        respond_to: oneshot::Sender<Result<(), CoordinatorError>>,
    },
    ReachContinuationLimit {
        actor: EventActor,
        continuation_id: String,
        limit: String,
        iteration: u32,
        respond_to: oneshot::Sender<Result<(), CoordinatorError>>,
    },
    AgentTurnFinished {
        task_id: String,
        agent_id: String,
        request_id: String,
        outcome: AgentTurnTaskOutcome,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRuntimeInfo {
    pub agent_id: String,
    pub profile_name: String,
    pub profile_category: String,
    pub model_ref: String,
    pub model_ref_explicit: bool,
    pub toolset: Vec<String>,
    pub parent_agent_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildTaskRequestMetadata {
    pub parent_tool_call_id: String,
    pub parent_session_id: String,
    pub parent_agent_id: Option<String>,
    pub child_session_id: String,
    pub task_id: String,
    pub description: String,
    pub run_in_background: bool,
    pub route: Option<TaskRouteMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentTurnTaskOutcome {
    Succeeded {
        output: String,
        messages: Vec<ConversationMessage>,
    },
    Failed {
        reason: String,
        memory: Option<AgentTurnFailureMemory>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTurnFailureMemory {
    pub status: ProviderConversationTurnStatus,
    pub failure_stage: String,
    pub failure_reason: String,
    pub partial_assistant_output: String,
    pub provider_request_id: Option<String>,
}

impl AgentTurnFailureMemory {
    fn new(
        status: ProviderConversationTurnStatus,
        failure_stage: impl Into<String>,
        failure_reason: impl Into<String>,
        partial_assistant_output: impl Into<String>,
        provider_request_id: Option<String>,
    ) -> Self {
        Self {
            status,
            failure_stage: failure_stage.into(),
            failure_reason: failure_reason.into(),
            partial_assistant_output: partial_assistant_output.into(),
            provider_request_id,
        }
    }

    fn failed(
        failure_stage: impl Into<String>,
        failure_reason: impl Into<String>,
        partial_assistant_output: impl Into<String>,
        provider_request_id: Option<String>,
    ) -> Self {
        Self::new(
            ProviderConversationTurnStatus::Failed,
            failure_stage,
            failure_reason,
            partial_assistant_output,
            provider_request_id,
        )
    }

    fn aborted(
        failure_stage: impl Into<String>,
        failure_reason: impl Into<String>,
        partial_assistant_output: impl Into<String>,
        provider_request_id: Option<String>,
    ) -> Self {
        Self::new(
            ProviderConversationTurnStatus::Aborted,
            failure_stage,
            failure_reason,
            partial_assistant_output,
            provider_request_id,
        )
    }
}

impl From<AgentTurnFailure> for AgentTurnFailureMemory {
    fn from(failure: AgentTurnFailure) -> Self {
        Self::new(
            failure.status,
            failure.failure_stage,
            failure.reason,
            failure.partial_assistant_output,
            failure.provider_request_id,
        )
    }
}

#[derive(Debug, Error)]
pub enum CoordinatorError {
    #[error("coordinator command channel is closed")]
    CommandChannelClosed,
    #[error("coordinator response channel dropped before reply")]
    ResponseChannelClosed,
    #[error("run already started")]
    RunAlreadyStarted,
    #[error("run is not started")]
    RunNotStarted,
    #[error("failed to create session directory {path}: {source}")]
    CreateSessionDirectory {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to serialize run metadata: {0}")]
    SerializeRunMetadata(#[from] serde_json::Error),
    #[error("failed to write run metadata {path}: {source}")]
    WriteRunMetadata {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("event build failed: {0}")]
    EventBuild(#[from] EventBuildError),
    #[error("event store failed: {0}")]
    EventStore(#[from] EventStoreError),
    #[error("event sequence mismatch: expected {expected}, got {actual}")]
    EventSequenceMismatch { expected: u64, actual: u64 },
    #[error("policy violation: {0}")]
    PolicyViolation(String),
    #[error("unknown pending permission: {0}")]
    UnknownPermission(String),
    #[error("unknown task: {0}")]
    UnknownTask(String),
    #[error("unknown agent: {0}")]
    UnknownAgent(String),
    #[error("permission denied for tool call: {0}")]
    PermissionDenied(String),
    #[error("resume is disabled for run `{run_id}`: {reason}")]
    ResumeDisabled { run_id: String, reason: String },
    #[error("resume restoration failed for run `{run_id}`: {reason}")]
    ResumeRestoreFailed { run_id: String, reason: String },
    #[error("provider context compaction failed: {0}")]
    CompactionFailed(String),
    #[error("context snapshot failed: {0}")]
    ContextSnapshotFailed(String),
    #[error("lifecycle hook failed: {0}")]
    LifecycleHookFailed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualCompactionOutcome {
    CheckpointWritten {
        checkpoint_id: String,
        tokens_before_estimate: Option<u32>,
        tokens_after_estimate: Option<u32>,
    },
    NoOp,
}

#[derive(Debug, Clone)]
pub struct CoordinatorHandle {
    tx: mpsc::Sender<Command>,
}

impl CoordinatorHandle {
    async fn request<T>(
        &self,
        build_command: impl FnOnce(oneshot::Sender<Result<T, CoordinatorError>>) -> Command,
    ) -> Result<T, CoordinatorError> {
        let (respond_to, response_rx) = oneshot::channel();
        self.tx
            .send(build_command(respond_to))
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed)?;

        response_rx
            .await
            .map_err(|_| CoordinatorError::ResponseChannelClosed)?
    }

    async fn request_string_error<T>(
        &self,
        build_command: impl FnOnce(oneshot::Sender<Result<T, String>>) -> Command,
    ) -> Result<T, String> {
        let (respond_to, response_rx) = oneshot::channel();
        self.tx
            .send(build_command(respond_to))
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed.to_string())?;

        response_rx
            .await
            .map_err(|_| CoordinatorError::ResponseChannelClosed.to_string())?
    }

    async fn send_command(&self, command: Command) -> Result<(), CoordinatorError> {
        self.tx
            .send(command)
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed)
    }

    pub async fn start_run(
        &self,
        run_name: impl Into<String>,
        workspace_root: impl Into<PathBuf>,
    ) -> Result<RunInfo, CoordinatorError> {
        self.request(|respond_to| Command::StartRun {
            run_name: run_name.into(),
            workspace_root: workspace_root.into(),
            respond_to,
        })
        .await
    }

    pub async fn resume_run(
        &self,
        run_id: impl Into<String>,
        run_name: impl Into<String>,
    ) -> Result<RunInfo, CoordinatorError> {
        self.request(|respond_to| Command::ResumeRun {
            run_id: run_id.into(),
            run_name: run_name.into(),
            respond_to,
        })
        .await
    }

    pub async fn stop_run(&self) -> Result<(), CoordinatorError> {
        self.request(|respond_to| Command::StopRun { respond_to })
            .await
    }

    pub async fn event_store(&self) -> Result<Arc<dyn EventStore>, CoordinatorError> {
        let store = self
            .request(|respond_to| Command::GetEventStore { respond_to })
            .await?;
        let store: Arc<dyn EventStore> = store;
        Ok(store)
    }

    pub async fn agent_runtime_info(
        &self,
        agent_id: impl Into<String>,
    ) -> Result<AgentRuntimeInfo, CoordinatorError> {
        self.request(|respond_to| Command::GetAgentRuntimeInfo {
            agent_id: agent_id.into(),
            respond_to,
        })
        .await
    }

    pub async fn spawn_agent(
        &self,
        actor: EventActor,
        profile: impl Into<String>,
        parent_agent_id: Option<String>,
    ) -> Result<String, CoordinatorError> {
        self.request(|respond_to| Command::SpawnAgent {
            actor,
            profile: profile.into(),
            parent_agent_id,
            child_session_title: None,
            respond_to,
        })
        .await
    }

    pub async fn spawn_agent_idle(
        &self,
        actor: EventActor,
        profile: impl Into<String>,
        parent_agent_id: Option<String>,
    ) -> Result<String, CoordinatorError> {
        self.request(|respond_to| Command::SpawnAgentIdle {
            actor,
            profile: profile.into(),
            parent_agent_id,
            child_session_title: None,
            respond_to,
        })
        .await
    }

    pub async fn spawn_agent_idle_with_child_title(
        &self,
        actor: EventActor,
        profile: impl Into<String>,
        parent_agent_id: Option<String>,
        child_session_title: impl Into<String>,
    ) -> Result<String, CoordinatorError> {
        self.request(|respond_to| Command::SpawnAgentIdle {
            actor,
            profile: profile.into(),
            parent_agent_id,
            child_session_title: Some(child_session_title.into()),
            respond_to,
        })
        .await
    }

    pub async fn request_tool_call(
        &self,
        actor: EventActor,
        category: Option<String>,
        tool_id: impl Into<String>,
        args_json: Value,
    ) -> Result<String, CoordinatorError> {
        self.request(|respond_to| Command::RequestToolCall {
            actor,
            category,
            tool_id: tool_id.into(),
            args_json,
            respond_to,
        })
        .await
    }

    pub async fn execute_agent_tool_call(
        &self,
        actor: EventActor,
        category: Option<String>,
        tool_id: impl Into<String>,
        args_json: Value,
    ) -> Result<ToolResult, String> {
        self.request_string_error(|respond_to| Command::ExecuteAgentToolCall {
            actor,
            category,
            tool_id: tool_id.into(),
            args_json,
            respond_to,
        })
        .await
    }

    pub async fn request_agent_turn(
        &self,
        actor: EventActor,
        agent_id: impl Into<String>,
        prompt: impl Into<String>,
    ) -> Result<String, CoordinatorError> {
        self.request_agent_turn_with_model(actor, agent_id, prompt, None, None)
            .await
    }

    pub async fn request_agent_turn_with_model(
        &self,
        actor: EventActor,
        agent_id: impl Into<String>,
        prompt: impl Into<String>,
        model_ref_override: Option<String>,
        model_settings_override: Option<AgentModelSettings>,
    ) -> Result<String, CoordinatorError> {
        self.request_agent_turn_with_model_and_selected_tags(
            actor,
            agent_id,
            prompt,
            crate::file_tag::SelectedPromptTags::default(),
            model_ref_override,
            model_settings_override,
        )
        .await
    }

    pub async fn request_agent_turn_with_model_and_selected_tags(
        &self,
        actor: EventActor,
        agent_id: impl Into<String>,
        prompt: impl Into<String>,
        selected_tags: crate::file_tag::SelectedPromptTags,
        model_ref_override: Option<String>,
        model_settings_override: Option<AgentModelSettings>,
    ) -> Result<String, CoordinatorError> {
        self.request(|respond_to| Command::RequestAgentTurn {
            actor,
            agent_id: agent_id.into(),
            prompt: prompt.into(),
            selected_file_tags: selected_tags.files,
            selected_agent_tags: selected_tags.agents,
            selected_resource_tags: selected_tags.resources,
            model_ref_override,
            model_settings_override,
            child_task_metadata: None,
            respond_to,
        })
        .await
    }

    pub async fn request_child_agent_turn_with_model(
        &self,
        actor: EventActor,
        agent_id: impl Into<String>,
        prompt: impl Into<String>,
        model_ref_override: Option<String>,
        model_settings_override: Option<AgentModelSettings>,
        child_task_metadata: ChildTaskRequestMetadata,
    ) -> Result<String, CoordinatorError> {
        self.request(|respond_to| Command::RequestAgentTurn {
            actor,
            agent_id: agent_id.into(),
            prompt: prompt.into(),
            selected_file_tags: Vec::new(),
            selected_agent_tags: Vec::new(),
            selected_resource_tags: Vec::new(),
            model_ref_override,
            model_settings_override,
            child_task_metadata: Some(child_task_metadata),
            respond_to,
        })
        .await
    }

    pub async fn compact_agent_context(
        &self,
        agent_id: impl Into<String>,
        through_request_id: Option<String>,
        trigger_reason: impl Into<String>,
    ) -> Result<ManualCompactionOutcome, CoordinatorError> {
        self.request(|respond_to| Command::ManualCompactAgentContext {
            agent_id: agent_id.into(),
            through_request_id,
            trigger_reason: trigger_reason.into(),
            respond_to,
        })
        .await
    }

    pub async fn start_continuation(
        &self,
        actor: EventActor,
        mode: impl Into<String>,
        command: impl Into<String>,
        bounds: ContinuationBounds,
    ) -> Result<String, CoordinatorError> {
        self.request(|respond_to| Command::StartContinuation {
            actor,
            mode: mode.into(),
            command: command.into(),
            bounds,
            workflow: None,
            respond_to,
        })
        .await
    }

    pub async fn start_workflow_continuation(
        &self,
        actor: EventActor,
        mode: impl Into<String>,
        command: impl Into<String>,
        bounds: ContinuationBounds,
        workflow: WorkflowEventMetadata,
    ) -> Result<String, CoordinatorError> {
        self.request(|respond_to| Command::StartContinuation {
            actor,
            mode: mode.into(),
            command: command.into(),
            bounds,
            workflow: Some(workflow),
            respond_to,
        })
        .await
    }

    pub async fn stop_continuation(
        &self,
        actor: EventActor,
        reason: impl Into<String>,
    ) -> Result<(), CoordinatorError> {
        self.request(|respond_to| Command::StopContinuation {
            actor,
            reason: reason.into(),
            respond_to,
        })
        .await
    }

    pub async fn trigger_continuation_reminder(
        &self,
        actor: EventActor,
        agent_id: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<Option<String>, CoordinatorError> {
        self.request(|respond_to| Command::TriggerContinuationReminder {
            actor,
            agent_id: agent_id.into(),
            reason: reason.into(),
            done_marker_seen: false,
            incomplete_todos: Some(true),
            provider_calls: 0,
            tool_calls: 0,
            respond_to,
        })
        .await
    }

    pub async fn resolve_permission(
        &self,
        permission_id: impl Into<String>,
        decision: PermissionDecision,
        reason: Option<String>,
    ) -> Result<(), CoordinatorError> {
        self.resolve_permission_with_grant_scope(permission_id, decision, reason, None)
            .await
    }

    pub async fn resolve_permission_with_grant_scope(
        &self,
        permission_id: impl Into<String>,
        decision: PermissionDecision,
        reason: Option<String>,
        grant_scope: Option<PermissionGrantScope>,
    ) -> Result<(), CoordinatorError> {
        self.request(|respond_to| Command::ResolvePermission {
            permission_id: permission_id.into(),
            decision,
            reason,
            grant_scope,
            respond_to,
        })
        .await
    }

    pub async fn request_question(
        &self,
        actor: EventActor,
        tool_call_id: impl Into<String>,
        request_json: Value,
    ) -> Result<Vec<Vec<String>>, String> {
        self.request_string_error(|respond_to| Command::RequestQuestion {
            actor,
            tool_call_id: tool_call_id.into(),
            request_json,
            respond_to,
        })
        .await
    }

    pub async fn write_context_snapshot(
        &self,
        actor: EventActor,
        workflow_id: Option<String>,
        input: ContextSnapshotInput,
        options: ContextSnapshotOptions,
    ) -> Result<ContextSnapshotWriteResult, CoordinatorError> {
        self.request(|respond_to| Command::WriteContextSnapshot {
            actor,
            workflow_id,
            input,
            options,
            respond_to,
        })
        .await
    }

    pub async fn start_workflow(
        &self,
        actor: EventActor,
        request: WorkflowStartRequest,
    ) -> Result<WorkflowStartResult, CoordinatorError> {
        self.request(|respond_to| Command::StartWorkflow {
            actor,
            request,
            respond_to,
        })
        .await
    }

    pub async fn record_workflow_transition(
        &self,
        actor: EventActor,
        request: WorkflowTransitionRequest,
    ) -> Result<(), CoordinatorError> {
        self.request(|respond_to| Command::RecordWorkflowTransition {
            actor,
            request,
            respond_to,
        })
        .await
    }

    pub async fn record_workflow_evidence(
        &self,
        actor: EventActor,
        request: WorkflowEvidenceRequest,
    ) -> Result<(), CoordinatorError> {
        self.request(|respond_to| Command::RecordWorkflowEvidence {
            actor,
            request,
            respond_to,
        })
        .await
    }

    pub async fn record_workflow_operator_decision(
        &self,
        actor: EventActor,
        workflow_id: impl Into<String>,
        decision: impl Into<String>,
        operator: impl Into<String>,
        reason: Option<String>,
        correlation_id: Option<String>,
    ) -> Result<(), CoordinatorError> {
        self.request(|respond_to| Command::RecordWorkflowOperatorDecision {
            actor,
            workflow_id: workflow_id.into(),
            decision: decision.into(),
            operator: operator.into(),
            reason,
            correlation_id,
            respond_to,
        })
        .await
    }

    pub async fn complete_workflow(
        &self,
        actor: EventActor,
        workflow_id: impl Into<String>,
        outcome: impl Into<String>,
        reason: impl Into<String>,
        owner: impl Into<String>,
    ) -> Result<(), CoordinatorError> {
        self.request(|respond_to| Command::CompleteWorkflow {
            actor,
            workflow_id: workflow_id.into(),
            outcome: outcome.into(),
            reason: reason.into(),
            owner: owner.into(),
            respond_to,
        })
        .await
    }

    pub async fn complete_workflow_with_signoff_policy(
        &self,
        actor: EventActor,
        workflow_id: impl Into<String>,
        outcome: impl Into<String>,
        reason: impl Into<String>,
        owner: impl Into<String>,
        signoff_policy: WorkflowSignoffPolicy,
    ) -> Result<(), CoordinatorError> {
        self.request(|respond_to| Command::CompleteWorkflowWithSignoffPolicy {
            actor,
            workflow_id: workflow_id.into(),
            outcome: outcome.into(),
            reason: reason.into(),
            owner: owner.into(),
            signoff_policy,
            respond_to,
        })
        .await
    }

    pub async fn job_progress(
        &self,
        task_id: impl Into<String>,
        kind: JobProgressKind,
    ) -> Result<(), CoordinatorError> {
        self.send_command(Command::JobProgress {
            task_id: task_id.into(),
            kind,
        })
        .await
    }

    pub async fn cancel_task(
        &self,
        task_id: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<(), CoordinatorError> {
        self.request(|respond_to| Command::CancelTask {
            task_id: task_id.into(),
            reason: reason.into(),
            respond_to,
        })
        .await
    }

    pub async fn background_request_projection(
        &self,
        actor: EventActor,
        request_id: Option<String>,
        selector_hint: Option<String>,
    ) -> Result<BackgroundRequestProjection, CoordinatorError> {
        self.request(|respond_to| Command::GetBackgroundRequestProjection {
            actor,
            request_id,
            selector_hint,
            respond_to,
        })
        .await
    }

    pub async fn cancel_background_request(
        &self,
        actor: EventActor,
        request_id: Option<String>,
        selector_hint: Option<String>,
        reason: impl Into<String>,
    ) -> Result<BackgroundRequestProjection, CoordinatorError> {
        self.request(|respond_to| Command::CancelBackgroundRequest {
            actor,
            request_id,
            selector_hint,
            reason: reason.into(),
            respond_to,
        })
        .await
    }

    pub async fn create_team(
        &self,
        actor: EventActor,
        spec: TeamSpec,
        team_run_id: Option<String>,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        self.request(|respond_to| Command::CreateTeam {
            actor,
            spec,
            team_run_id,
            respond_to,
        })
        .await
    }

    pub async fn team_projection(&self) -> Result<TeamProjection, CoordinatorError> {
        self.request(|respond_to| Command::GetTeamProjection { respond_to })
            .await
    }

    pub async fn send_team_message(
        &self,
        actor: EventActor,
        team_run_id: impl Into<String>,
        message: TeamMessage,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        self.request(|respond_to| Command::SendTeamMessage {
            actor,
            team_run_id: team_run_id.into(),
            message,
            respond_to,
        })
        .await
    }

    pub async fn create_team_task(
        &self,
        actor: EventActor,
        team_run_id: impl Into<String>,
        task: TeamTask,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        self.request(|respond_to| Command::CreateTeamTask {
            actor,
            team_run_id: team_run_id.into(),
            task,
            respond_to,
        })
        .await
    }

    pub async fn update_team_task(
        &self,
        actor: EventActor,
        team_run_id: impl Into<String>,
        task_id: impl Into<String>,
        status: TeamTaskStatus,
        owner: Option<String>,
        metadata: BTreeMap<String, String>,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        self.request(|respond_to| Command::UpdateTeamTask {
            actor,
            team_run_id: team_run_id.into(),
            task_id: task_id.into(),
            status,
            owner,
            metadata,
            respond_to,
        })
        .await
    }

    pub async fn persistent_task_projection(
        &self,
    ) -> Result<PersistentTaskProjection, CoordinatorError> {
        self.request(|respond_to| Command::GetPersistentTaskProjection { respond_to })
            .await
    }

    pub async fn create_persistent_task(
        &self,
        actor: EventActor,
        task: PersistentTask,
    ) -> Result<PersistentTaskProjection, CoordinatorError> {
        self.request(|respond_to| Command::CreatePersistentTask {
            actor,
            task,
            respond_to,
        })
        .await
    }

    pub async fn update_persistent_task(
        &self,
        actor: EventActor,
        update: PersistentTaskUpdatedEvent,
    ) -> Result<PersistentTaskProjection, CoordinatorError> {
        self.request(|respond_to| Command::UpdatePersistentTask {
            actor,
            update,
            respond_to,
        })
        .await
    }

    pub async fn request_team_shutdown(
        &self,
        actor: EventActor,
        team_run_id: impl Into<String>,
        member_name: impl Into<String>,
        requester: impl Into<String>,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        self.request(|respond_to| Command::RequestTeamShutdown {
            actor,
            team_run_id: team_run_id.into(),
            member_name: member_name.into(),
            requester: requester.into(),
            respond_to,
        })
        .await
    }

    pub async fn approve_team_shutdown(
        &self,
        actor: EventActor,
        team_run_id: impl Into<String>,
        member_name: impl Into<String>,
        approver: impl Into<String>,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        self.request(|respond_to| Command::ApproveTeamShutdown {
            actor,
            team_run_id: team_run_id.into(),
            member_name: member_name.into(),
            approver: approver.into(),
            respond_to,
        })
        .await
    }

    pub async fn reject_team_shutdown(
        &self,
        actor: EventActor,
        team_run_id: impl Into<String>,
        member_name: impl Into<String>,
        rejecter: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        self.request(|respond_to| Command::RejectTeamShutdown {
            actor,
            team_run_id: team_run_id.into(),
            member_name: member_name.into(),
            rejecter: rejecter.into(),
            reason: reason.into(),
            respond_to,
        })
        .await
    }

    pub async fn delete_team(
        &self,
        actor: EventActor,
        team_run_id: impl Into<String>,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        self.request(|respond_to| Command::DeleteTeam {
            actor,
            team_run_id: team_run_id.into(),
            respond_to,
        })
        .await
    }

    pub async fn wait_background_request_terminal(
        &self,
        request_id: impl Into<String>,
        scheduler_task_id: impl Into<String>,
        timeout_ms: u64,
    ) -> Result<bool, CoordinatorError> {
        let request_id = request_id.into();
        let scheduler_task_id = scheduler_task_id.into();
        let store = self.event_store().await?;
        let mut stream = store.subscribe(1)?;
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);

        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let next =
                tokio::time::timeout(remaining.min(Duration::from_millis(250)), stream.next())
                    .await;
            match next {
                Ok(Some(Ok(event))) => {
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && background_terminal_event_matches_task(&event, &scheduler_task_id)
                    {
                        return Ok(true);
                    }
                }
                Ok(Some(Err(err))) => return Err(CoordinatorError::EventStore(err)),
                Ok(None) | Err(_) => sleep(Duration::from_millis(10)).await,
            }
        }

        Ok(false)
    }

    pub async fn job_finished(
        &self,
        task_id: impl Into<String>,
        outcome: JobOutcome,
    ) -> Result<(), CoordinatorError> {
        self.send_command(Command::JobFinished {
            task_id: task_id.into(),
            outcome,
        })
        .await
    }
}

pub fn spawn_coordinator(
    config: CoordinatorConfig,
    clock: Arc<dyn Clock + Send + Sync>,
    redactor: Arc<dyn Redactor + Send + Sync>,
) -> CoordinatorHandle {
    let (command_tx, command_rx) = mpsc::channel(config.command_buffer.max(1));
    let (job_tx, job_rx) = mpsc::channel(config.command_buffer.max(1));

    let coordinator = Coordinator::new(config, clock, redactor, command_rx, job_tx, job_rx);
    tokio::spawn(async move {
        coordinator.run().await;
    });

    CoordinatorHandle { tx: command_tx }
}

struct Coordinator {
    config: CoordinatorConfig,
    clock: Arc<dyn Clock + Send + Sync>,
    redactor: Arc<dyn Redactor + Send + Sync>,
    command_rx: mpsc::Receiver<Command>,
    job_tx: mpsc::Sender<Command>,
    job_rx: mpsc::Receiver<Command>,
    run_state: Option<RunState>,
    next_run_id: u64,
}

impl Coordinator {
    fn new(
        config: CoordinatorConfig,
        clock: Arc<dyn Clock + Send + Sync>,
        redactor: Arc<dyn Redactor + Send + Sync>,
        command_rx: mpsc::Receiver<Command>,
        job_tx: mpsc::Sender<Command>,
        job_rx: mpsc::Receiver<Command>,
    ) -> Self {
        Self {
            config,
            clock,
            redactor,
            command_rx,
            job_tx,
            job_rx,
            run_state: None,
            next_run_id: 1,
        }
    }

    async fn run(mut self) {
        let mut command_channel_closed = false;
        let mut watchdog =
            tokio::time::interval(Duration::from_millis(self.config.watchdog_tick_ms.max(1)));
        watchdog.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            if command_channel_closed {
                if self.run_state.is_some() {
                    let _ = self
                        .stop_run_internal("coordinator command channel closed".to_string())
                        .await;
                } else {
                    break;
                }
            }

            tokio::select! {
                command = self.command_rx.recv(), if !command_channel_closed => {
                    match command {
                        Some(command) => self.handle_command(command).await,
                        None => command_channel_closed = true,
                    }
                }
                command = self.job_rx.recv() => {
                    if let Some(command) = command {
                        self.handle_command(command).await;
                    }
                }
                _ = watchdog.tick(), if self.has_running_tasks() => {
                    if let Err(err) = self.watchdog_tick_internal() {
                        tracing::warn!(error = %err, "coordinator watchdog tick failed");
                    }
                }
            }
        }
    }

    fn has_running_tasks(&self) -> bool {
        self.run_state.as_ref().is_some_and(|run_state| {
            run_state
                .tasks
                .values()
                .any(|task| task.state == TaskExecutionState::Running)
        })
    }

    fn agent_runtime_info_internal(
        &self,
        agent_id: String,
    ) -> Result<AgentRuntimeInfo, CoordinatorError> {
        let run_state = self
            .run_state
            .as_ref()
            .ok_or(CoordinatorError::RunNotStarted)?;
        let profile = run_state
            .agents
            .get(&agent_id)
            .ok_or_else(|| CoordinatorError::UnknownAgent(agent_id.clone()))?;
        Ok(AgentRuntimeInfo {
            agent_id: agent_id.clone(),
            profile_name: profile.name.clone(),
            profile_category: profile.category.clone(),
            model_ref: profile.model_ref.clone(),
            model_ref_explicit: profile.model_ref_explicit,
            toolset: profile.toolset.clone(),
            parent_agent_id: run_state.subagent_parent_by_id.get(&agent_id).cloned(),
        })
    }

    async fn handle_command(&mut self, command: Command) {
        match command {
            Command::StartRun {
                run_name,
                workspace_root,
                respond_to,
            } => {
                let result = self
                    .start_run_internal_async(run_name, workspace_root)
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "start_run");
            }
            Command::ResumeRun {
                run_id,
                run_name,
                respond_to,
            } => {
                let result = self.resume_run_internal(run_id, run_name);
                warn_oneshot_send_failure(respond_to.send(result), "resume_run");
            }
            Command::StopRun { respond_to } => {
                let result = self.stop_run_internal("run stopped".to_string()).await;
                warn_oneshot_send_failure(respond_to.send(result), "stop_run");
            }
            Command::GetEventStore { respond_to } => {
                let result = self.get_event_store_internal();
                warn_oneshot_send_failure(respond_to.send(result), "get_event_store");
            }
            Command::GetAgentRuntimeInfo {
                agent_id,
                respond_to,
            } => {
                let result = self.agent_runtime_info_internal(agent_id);
                warn_oneshot_send_failure(respond_to.send(result), "get_agent_runtime_info");
            }
            Command::SpawnAgent {
                actor,
                profile,
                parent_agent_id,
                child_session_title,
                respond_to,
            } => {
                let result = self
                    .spawn_agent_internal(
                        actor,
                        profile,
                        parent_agent_id,
                        child_session_title,
                        true,
                    )
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "spawn_agent");
            }
            Command::SpawnAgentIdle {
                actor,
                profile,
                parent_agent_id,
                child_session_title,
                respond_to,
            } => {
                let result = self
                    .spawn_agent_internal(
                        actor,
                        profile,
                        parent_agent_id,
                        child_session_title,
                        false,
                    )
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "spawn_agent_idle");
            }
            Command::RequestAgentTurn {
                actor,
                agent_id,
                prompt,
                selected_file_tags,
                selected_agent_tags,
                selected_resource_tags,
                model_ref_override,
                model_settings_override,
                child_task_metadata,
                respond_to,
            } => {
                let result = self
                    .request_agent_turn_internal(
                        actor,
                        agent_id,
                        prompt,
                        crate::file_tag::SelectedPromptTags {
                            files: selected_file_tags,
                            agents: selected_agent_tags,
                            resources: selected_resource_tags,
                        },
                        model_ref_override,
                        model_settings_override,
                        child_task_metadata,
                    )
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "request_agent_turn");
            }
            Command::RequestToolCall {
                actor,
                category,
                tool_id,
                args_json,
                respond_to,
            } => {
                let result = self
                    .request_tool_call_internal(actor, category, tool_id, args_json, None)
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "request_tool_call");
            }
            Command::ExecuteAgentToolCall {
                actor,
                category,
                tool_id,
                args_json,
                respond_to,
            } => {
                let _ = self
                    .request_tool_call_internal(
                        actor,
                        category,
                        tool_id,
                        args_json,
                        Some(respond_to),
                    )
                    .await;
            }
            Command::RequestQuestion {
                actor,
                tool_call_id,
                request_json,
                respond_to,
            } => {
                let _ = self
                    .request_question_internal(actor, tool_call_id, request_json, respond_to)
                    .await;
            }
            Command::WriteContextSnapshot {
                actor,
                workflow_id,
                input,
                options,
                respond_to,
            } => {
                let result =
                    self.write_context_snapshot_internal(actor, workflow_id, input, options);
                warn_oneshot_send_failure(respond_to.send(result), "write_context_snapshot");
            }
            Command::StartWorkflow {
                actor,
                request,
                respond_to,
            } => {
                let result = self.start_workflow_internal(actor, request);
                warn_oneshot_send_failure(respond_to.send(result), "start_workflow");
            }
            Command::RecordWorkflowTransition {
                actor,
                request,
                respond_to,
            } => {
                let result = self.record_workflow_transition_internal(actor, request);
                warn_oneshot_send_failure(respond_to.send(result), "record_workflow_transition");
            }
            Command::RecordWorkflowEvidence {
                actor,
                request,
                respond_to,
            } => {
                let result = self.record_workflow_evidence_internal(actor, request);
                warn_oneshot_send_failure(respond_to.send(result), "record_workflow_evidence");
            }
            Command::RecordWorkflowOperatorDecision {
                actor,
                workflow_id,
                decision,
                operator,
                reason,
                correlation_id,
                respond_to,
            } => {
                let result = self.record_workflow_operator_decision_internal(
                    actor,
                    workflow_id,
                    decision,
                    operator,
                    reason,
                    correlation_id,
                );
                warn_oneshot_send_failure(
                    respond_to.send(result),
                    "record_workflow_operator_decision",
                );
            }
            Command::CompleteWorkflow {
                actor,
                workflow_id,
                outcome,
                reason,
                owner,
                respond_to,
            } => {
                let result =
                    self.complete_workflow_internal(actor, workflow_id, outcome, reason, owner);
                warn_oneshot_send_failure(respond_to.send(result), "complete_workflow");
            }
            Command::CompleteWorkflowWithSignoffPolicy {
                actor,
                workflow_id,
                outcome,
                reason,
                owner,
                signoff_policy,
                respond_to,
            } => {
                let result = self.complete_workflow_with_signoff_policy_internal(
                    actor,
                    workflow_id,
                    outcome,
                    reason,
                    owner,
                    signoff_policy,
                );
                warn_oneshot_send_failure(
                    respond_to.send(result),
                    "complete_workflow_with_signoff_policy",
                );
            }
            Command::ResolvePermission {
                permission_id,
                decision,
                reason,
                grant_scope,
                respond_to,
            } => {
                let result = self
                    .resolve_permission_internal(permission_id, decision, reason, grant_scope)
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "resolve_permission");
            }
            Command::PermissionTimedOut { permission_id } => {
                self.resolve_permission_timeout_internal(permission_id)
                    .await;
            }
            Command::JobProgress { task_id, kind } => {
                self.job_progress_internal(task_id, kind);
            }
            Command::CancelTask {
                task_id,
                reason,
                respond_to,
            } => {
                let result = self.cancel_task_internal(task_id, reason).await;
                warn_oneshot_send_failure(respond_to.send(result), "cancel_task");
            }
            Command::GetBackgroundRequestProjection {
                actor,
                request_id,
                selector_hint,
                respond_to,
            } => {
                let result = self
                    .background_request_projection_internal(actor, request_id, selector_hint)
                    .await;
                warn_oneshot_send_failure(
                    respond_to.send(result),
                    "get_background_request_projection",
                );
            }
            Command::CancelBackgroundRequest {
                actor,
                request_id,
                selector_hint,
                reason,
                respond_to,
            } => {
                let result = self
                    .cancel_background_request_internal(actor, request_id, selector_hint, reason)
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "cancel_background_request");
            }
            Command::CreateTeam {
                actor,
                spec,
                team_run_id,
                respond_to,
            } => {
                let result = self.create_team_internal(actor, spec, team_run_id).await;
                warn_oneshot_send_failure(respond_to.send(result), "create_team");
            }
            Command::GetTeamProjection { respond_to } => {
                let result = self.team_projection_internal().await;
                warn_oneshot_send_failure(respond_to.send(result), "get_team_projection");
            }
            Command::SendTeamMessage {
                actor,
                team_run_id,
                message,
                respond_to,
            } => {
                let result = self
                    .send_team_message_internal(actor, team_run_id, message)
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "send_team_message");
            }
            Command::CreateTeamTask {
                actor,
                team_run_id,
                task,
                respond_to,
            } => {
                let result = self
                    .create_team_task_internal(actor, team_run_id, task)
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "create_team_task");
            }
            Command::UpdateTeamTask {
                actor,
                team_run_id,
                task_id,
                status,
                owner,
                metadata,
                respond_to,
            } => {
                let result = self
                    .update_team_task_internal(actor, team_run_id, task_id, status, owner, metadata)
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "update_team_task");
            }
            Command::GetPersistentTaskProjection { respond_to } => {
                let result = self.persistent_task_projection_internal().await;
                warn_oneshot_send_failure(
                    respond_to.send(result),
                    "get_persistent_task_projection",
                );
            }
            Command::CreatePersistentTask {
                actor,
                task,
                respond_to,
            } => {
                let result = self.create_persistent_task_internal(actor, task).await;
                warn_oneshot_send_failure(respond_to.send(result), "create_persistent_task");
            }
            Command::UpdatePersistentTask {
                actor,
                update,
                respond_to,
            } => {
                let result = self.update_persistent_task_internal(actor, update).await;
                warn_oneshot_send_failure(respond_to.send(result), "update_persistent_task");
            }
            Command::RequestTeamShutdown {
                actor,
                team_run_id,
                member_name,
                requester,
                respond_to,
            } => {
                let result = self
                    .request_team_shutdown_internal(actor, team_run_id, member_name, requester)
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "request_team_shutdown");
            }
            Command::ApproveTeamShutdown {
                actor,
                team_run_id,
                member_name,
                approver,
                respond_to,
            } => {
                let result = self
                    .approve_team_shutdown_internal(actor, team_run_id, member_name, approver)
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "approve_team_shutdown");
            }
            Command::RejectTeamShutdown {
                actor,
                team_run_id,
                member_name,
                rejecter,
                reason,
                respond_to,
            } => {
                let result = self
                    .reject_team_shutdown_internal(
                        actor,
                        team_run_id,
                        member_name,
                        rejecter,
                        reason,
                    )
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "reject_team_shutdown");
            }
            Command::DeleteTeam {
                actor,
                team_run_id,
                respond_to,
            } => {
                let result = self.delete_team_internal(actor, team_run_id).await;
                warn_oneshot_send_failure(respond_to.send(result), "delete_team");
            }
            Command::JobFinished { task_id, outcome } => {
                let _ = self.job_finished_internal_async(task_id, outcome).await;
            }
            Command::AgentProviderRequestStarted {
                task_id,
                agent_id,
                request_id,
                provider_id,
                model_id,
                prompt_summary,
                request_digest,
                metadata,
            } => {
                let _ = self
                    .agent_provider_request_started_internal(AgentProviderRequestStartedArgs {
                        task_id,
                        agent_id,
                        request_id,
                        provider_id,
                        model_id,
                        prompt_summary,
                        request_digest,
                        metadata,
                    })
                    .await;
            }
            Command::AgentProviderStreamDelta {
                task_id,
                agent_id,
                request_id,
                delta,
            } => {
                let _ =
                    self.agent_provider_stream_delta_internal(task_id, agent_id, request_id, delta);
            }
            Command::AgentProviderReasoningDelta {
                task_id,
                agent_id,
                request_id,
                delta,
            } => {
                let _ = self
                    .agent_provider_reasoning_delta_internal(task_id, agent_id, request_id, delta);
            }
            Command::AgentProviderRequestFinished {
                task_id,
                agent_id,
                request_id,
                finish_reason,
                output_digest,
                usage,
                metadata,
                respond_to,
            } => {
                let result = self
                    .agent_provider_request_finished_internal(AgentProviderRequestFinishedArgs {
                        task_id,
                        agent_id,
                        request_id,
                        finish_reason,
                        output_digest,
                        usage,
                        metadata,
                    })
                    .await;
                if let Some(respond_to) = respond_to {
                    warn_oneshot_send_failure(
                        respond_to.send(result),
                        "agent_provider_request_finished",
                    );
                }
            }
            Command::AgentAssistantMessageFinished {
                task_id,
                agent_id,
                request_id,
                assistant_output,
                tool_call_count,
                assistant_message,
                respond_to,
            } => {
                let result = self.agent_assistant_message_finished_internal(
                    task_id,
                    agent_id,
                    request_id,
                    assistant_output,
                    tool_call_count,
                    assistant_message,
                );
                warn_oneshot_send_failure(
                    respond_to.send(result),
                    "agent_assistant_message_finished",
                );
            }
            Command::AllocateProviderRequestId { respond_to } => {
                let result = self.allocate_provider_request_id_internal();
                warn_oneshot_send_failure(respond_to.send(result), "allocate_provider_request_id");
            }
            Command::SwitchAgentTurnProviderModelSlot {
                task_id,
                agent_id,
                model_ref,
                model_settings,
                respond_to,
            } => {
                let result = self
                    .switch_agent_turn_provider_model_slot_internal(
                        task_id,
                        agent_id,
                        model_ref,
                        model_settings,
                    )
                    .await;
                warn_oneshot_send_failure(
                    respond_to.send(result),
                    "switch_agent_turn_provider_model_slot",
                );
            }
            Command::CompactAgentContext {
                task_id,
                agent_id,
                request_id,
                trigger_reason,
                usage,
                respond_to,
            } => {
                let result = self
                    .compact_agent_context_internal(
                        Some(&task_id),
                        &agent_id,
                        Some(request_id),
                        &trigger_reason,
                        usage,
                    )
                    .await
                    .map(CompactAgentContextResult::into_context);
                warn_oneshot_send_failure(respond_to.send(result), "compact_agent_context");
            }
            Command::ManualCompactAgentContext {
                agent_id,
                through_request_id,
                trigger_reason,
                respond_to,
            } => {
                let result = self
                    .compact_agent_context_internal(
                        None,
                        &agent_id,
                        through_request_id,
                        &trigger_reason,
                        None,
                    )
                    .await
                    .map(CompactAgentContextResult::into_manual_outcome);
                warn_oneshot_send_failure(respond_to.send(result), "manual_compact_agent_context");
            }
            Command::StartContinuation {
                actor,
                mode,
                command,
                bounds,
                workflow,
                respond_to,
            } => {
                let result =
                    self.start_continuation_internal(actor, mode, command, bounds, workflow);
                warn_oneshot_send_failure(respond_to.send(result), "start_continuation");
            }
            Command::StopContinuation {
                actor,
                reason,
                respond_to,
            } => {
                let result = self.stop_continuation_internal(actor, reason);
                warn_oneshot_send_failure(respond_to.send(result), "stop_continuation");
            }
            Command::TriggerContinuationReminder {
                actor,
                agent_id,
                reason,
                done_marker_seen,
                incomplete_todos,
                provider_calls,
                tool_calls,
                respond_to,
            } => {
                let result = self
                    .trigger_continuation_reminder_internal(ContinuationReminderTrigger {
                        actor,
                        agent_id,
                        reason,
                        done_marker_seen,
                        incomplete_todos,
                        provider_calls,
                        tool_calls,
                    })
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "trigger_continuation_reminder");
            }
            Command::QueueContinuationReminder {
                actor,
                continuation_id,
                iteration,
                reminder,
                reason,
                respond_to,
            } => {
                let result = self.queue_continuation_reminder_internal(
                    actor,
                    continuation_id,
                    iteration,
                    reminder,
                    reason,
                );
                warn_oneshot_send_failure(respond_to.send(result), "queue_continuation_reminder");
            }
            Command::ReachContinuationLimit {
                actor,
                continuation_id,
                limit,
                iteration,
                respond_to,
            } => {
                let result = self.reach_continuation_limit_internal(
                    actor,
                    continuation_id,
                    limit,
                    iteration,
                );
                warn_oneshot_send_failure(respond_to.send(result), "reach_continuation_limit");
            }
            Command::AgentTurnFinished {
                task_id,
                agent_id,
                request_id,
                outcome,
            } => {
                let _ = self
                    .agent_turn_finished_internal(task_id, agent_id, request_id, outcome)
                    .await;
            }
        }
    }

    #[cfg(test)]
    fn start_run_internal(
        &mut self,
        run_name: String,
        workspace_root: PathBuf,
    ) -> Result<RunInfo, CoordinatorError> {
        block_on_coordinator_future(self.start_run_internal_async(run_name, workspace_root))
    }

    async fn start_run_internal_async(
        &mut self,
        run_name: String,
        workspace_root: PathBuf,
    ) -> Result<RunInfo, CoordinatorError> {
        if self.run_state.is_some() {
            return Err(CoordinatorError::RunAlreadyStarted);
        }

        let run_id = if let Some(run_id) = self.config.run_id_override.clone() {
            run_id
        } else {
            let run_id = format!("run_{:06}", self.next_run_id);
            self.next_run_id += 1;
            run_id
        };

        let run_dir = self.config.session_dir.join(&run_id);
        let artifacts_dir = run_dir.join(ARTIFACTS_DIR_NAME);
        fs::create_dir_all(&artifacts_dir).map_err(|source| {
            CoordinatorError::CreateSessionDirectory {
                path: artifacts_dir.display().to_string(),
                source,
            }
        })?;

        let event_store = JsonlFileEventStore::open(
            &self.config.session_dir,
            &run_id,
            self.config.deterministic_store,
        )?;
        let event_store = Arc::new(event_store);
        let events_path = event_store.file_path().to_path_buf();

        let run_info = RunInfo {
            run_id: run_id.clone(),
            run_name: run_name.clone(),
            workspace_root: workspace_root.clone(),
            run_dir,
            artifacts_dir,
            events_path,
        };

        let next_agent_id = next_agent_counter_for_run(&self.config.session_dir, &run_id, 0)?;

        let mut run_state = RunState {
            info: run_info.clone(),
            event_store,
            next_event_seq: 1,
            next_agent_id,
            next_tool_call_id: 1,
            next_task_id: 1,
            next_provider_request_id: 1,
            next_permission_id: 1,
            agents: BTreeMap::new(),
            provider_context_by_agent: BTreeMap::new(),
            tasks: BTreeMap::new(),
            task_hook_state: BTreeMap::new(),
            agent_hook_state: BTreeMap::new(),
            subagent_parent_by_id: BTreeMap::new(),
            child_session_mirrors: BTreeMap::new(),
            child_request_session_by_id: BTreeMap::new(),
            background_notification_child_requests: BTreeSet::new(),
            pending_agent_wakeups: BTreeMap::new(),
            pending_permissions: BTreeMap::new(),
            active_permission_grants: PermissionGrantSet::default(),
            cancelled_running_tasks: BTreeSet::new(),
            queued_agent_turns: BTreeMap::new(),
            running_agent_turns: BTreeMap::new(),
            failed_terminal_compaction_attempts: BTreeSet::new(),
            overflow_retry_compacted_context_by_attempt: BTreeMap::new(),
            active_continuation_id: None,
            active_continuation_workflow: None,
            continuation_controller: ContinuationController::default(),
            scheduler: Scheduler::new(SchedulerLimits {
                provider_model: self.config.provider_model_concurrency,
                tool: self.config.tool_concurrency,
            }),
            recorded_runtime_context: None,
            allow_initial_runtime_context_recording: true,
            shutdown_token: CancellationToken::new(),
        };

        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            &mut run_state,
            system_actor(),
            Some(format!("run:{run_id}")),
            EventV1::RunStarted(RunStartedEvent {
                run_name,
                workspace_root: workspace_root.display().to_string(),
            }),
        )?;

        write_run_metadata(&run_state, &self.config, self.clock.as_ref())?;

        let hook_batch = run_lifecycle_hooks(
            self.clock.as_ref(),
            &self.config.hook_runtime_config,
            HookInvocationContext {
                event: HookLifecycleEvent::RunStarted,
                run_id: run_state.info.run_id.clone(),
                workspace_root: run_state.info.workspace_root.clone(),
                artifacts_dir: run_state.info.artifacts_dir.clone(),
                actor: Some(system_actor()),
                agent_id: None,
                request_id: None,
                permission_id: None,
                task_id: None,
                tool_call_id: None,
                tool_id: None,
                provider_id: None,
                model_id: None,
                parent_agent_id: None,
                category: None,
                outcome: Some("started".to_string()),
                output_summary: Some(run_state.info.run_name.clone()),
                failure_reason: None,
            },
        )
        .await;
        if let Some(reason) = hook_batch.critical_failure {
            return Err(CoordinatorError::LifecycleHookFailed(reason));
        }

        self.run_state = Some(run_state);
        Ok(run_info)
    }

    fn resume_run_internal(
        &mut self,
        run_id: String,
        run_name: String,
    ) -> Result<RunInfo, CoordinatorError> {
        if self.run_state.is_some() {
            return Err(CoordinatorError::RunAlreadyStarted);
        }

        let run_dir = self.config.session_dir.join(&run_id);
        let event_store = JsonlFileEventStore::open_existing(
            &self.config.session_dir,
            &run_id,
            self.config.deterministic_store,
        )?;
        let event_store = Arc::new(event_store);

        let resume_plan = inspect_resume_plan(&run_dir);
        if !resume_plan.is_resumable {
            let reason = resume_plan
                .resume_disabled_reason
                .unwrap_or_else(|| "resume disabled without reason".to_string());
            return Err(CoordinatorError::ResumeDisabled { run_id, reason });
        }

        if resume_plan.run_id != run_id {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.clone(),
                reason: format!(
                    "resume plan run_id mismatch: expected `{}`, actual `{}`",
                    run_id, resume_plan.run_id
                ),
            });
        }

        let workspace_root = resume_plan
            .workspace_root
            .as_deref()
            .and_then(non_empty_trimmed)
            .map(PathBuf::from)
            .ok_or_else(|| CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.clone(),
                reason: "workspace root is missing from resume plan".to_string(),
            })?;

        let next_event_seq = checked_next_counter(resume_plan.max_seq, &run_id, "event sequence")?;
        let store_next_seq = event_store.next_seq()?;
        if store_next_seq != next_event_seq {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id,
                reason: format!(
                    "event-store sequence mismatch: resume plan expects {next_event_seq}, store reports {store_next_seq}"
                ),
            });
        }

        let mut agents = BTreeMap::new();
        let mut restored_agent_bindings = Vec::new();
        let mut restored_subagent_parent_by_id = BTreeMap::new();
        let mut max_agent_id = 0_u64;
        for (agent_id, profile_name) in &resume_plan.known_agents {
            let parsed_agent_id = parse_prefixed_counter(agent_id, "agent_").ok_or_else(|| {
                CoordinatorError::ResumeRestoreFailed {
                    run_id: run_id.clone(),
                    reason: format!("invalid agent id in resume plan: `{agent_id}`"),
                }
            })?;
            max_agent_id = max_agent_id.max(parsed_agent_id);

            let profile_cfg = self
                .config
                .agent_profiles
                .get(profile_name)
                .cloned()
                .ok_or_else(|| CoordinatorError::ResumeRestoreFailed {
                    run_id: run_id.clone(),
                    reason: format!(
                        "historical agent `{agent_id}` references missing profile binding `{profile_name}`"
                    ),
                })?;
            let parent_agent_id = resume_plan
                .child_sessions
                .get(agent_id)
                .and_then(|child| child.parent_session_id.as_deref())
                .and_then(non_empty_trimmed)
                .map(str::to_string);

            if let Some(parent_agent_id) = parent_agent_id.as_ref() {
                restored_subagent_parent_by_id.insert(agent_id.clone(), parent_agent_id.clone());
            }

            agents.insert(agent_id.clone(), profile_cfg);
            restored_agent_bindings.push((agent_id.clone(), profile_name.clone(), parent_agent_id));
        }

        let provider_context_by_agent =
            restore_provider_context_from_history(&self.config.session_dir, &run_id)?;
        let continuation_controller =
            restore_continuation_controller_from_history(&self.config.session_dir, &run_id)?;

        let next_agent_id =
            next_agent_counter_for_run(&self.config.session_dir, &run_id, max_agent_id)?;
        let next_tool_call_id = checked_next_counter(
            resume_plan.id_watermarks.max_tool_call_id,
            &run_id,
            "tool call id",
        )?;
        let next_task_id =
            checked_next_counter(resume_plan.id_watermarks.max_task_id, &run_id, "task id")?;
        let next_provider_request_id = checked_next_counter(
            resume_plan.id_watermarks.max_request_id,
            &run_id,
            "provider request id",
        )?;
        let next_permission_id = checked_next_counter(
            resume_plan.id_watermarks.max_permission_id,
            &run_id,
            "permission id",
        )?;

        let artifacts_dir = run_dir.join(ARTIFACTS_DIR_NAME);
        fs::create_dir_all(&artifacts_dir).map_err(|source| {
            CoordinatorError::CreateSessionDirectory {
                path: artifacts_dir.display().to_string(),
                source,
            }
        })?;

        let events_path = event_store.file_path().to_path_buf();
        let run_info = RunInfo {
            run_id: run_id.clone(),
            run_name: run_name.clone(),
            workspace_root: workspace_root.clone(),
            run_dir,
            artifacts_dir,
            events_path,
        };

        let mut run_state = RunState {
            info: run_info.clone(),
            event_store,
            next_event_seq,
            next_agent_id,
            next_tool_call_id,
            next_task_id,
            next_provider_request_id,
            next_permission_id,
            agents,
            provider_context_by_agent,
            tasks: BTreeMap::new(),
            task_hook_state: BTreeMap::new(),
            agent_hook_state: BTreeMap::new(),
            subagent_parent_by_id: restored_subagent_parent_by_id,
            child_session_mirrors: BTreeMap::new(),
            child_request_session_by_id: BTreeMap::new(),
            background_notification_child_requests: BTreeSet::new(),
            pending_agent_wakeups: BTreeMap::new(),
            pending_permissions: BTreeMap::new(),
            active_permission_grants: resume_plan.active_permission_grants,
            cancelled_running_tasks: BTreeSet::new(),
            queued_agent_turns: BTreeMap::new(),
            running_agent_turns: BTreeMap::new(),
            failed_terminal_compaction_attempts: BTreeSet::new(),
            overflow_retry_compacted_context_by_attempt: BTreeMap::new(),
            active_continuation_id: resume_plan.active_continuation_id.clone(),
            active_continuation_workflow: None,
            continuation_controller,
            scheduler: Scheduler::new(SchedulerLimits {
                provider_model: self.config.provider_model_concurrency,
                tool: self.config.tool_concurrency,
            }),
            recorded_runtime_context: None,
            allow_initial_runtime_context_recording: false,
            shutdown_token: CancellationToken::new(),
        };

        restore_child_session_mirrors(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            &self.config,
            &mut run_state,
            &restored_agent_bindings,
        )?;

        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            &mut run_state,
            system_actor(),
            Some(format!("run:{run_id}")),
            EventV1::RunStarted(RunStartedEvent {
                run_name,
                workspace_root: workspace_root.display().to_string(),
            }),
        )?;

        for (agent_id, profile, parent_agent_id) in restored_agent_bindings {
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                &mut run_state,
                system_actor(),
                Some(format!("agent:{agent_id}")),
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id,
                    profile,
                    parent_agent_id,
                }),
            )?;
        }

        self.run_state = Some(run_state);
        Ok(run_info)
    }

    fn get_event_store_internal(&self) -> Result<Arc<JsonlFileEventStore>, CoordinatorError> {
        let run_state = self
            .run_state
            .as_ref()
            .ok_or(CoordinatorError::RunNotStarted)?;
        Ok(run_state.event_store.clone())
    }

    async fn stop_run_internal(&mut self, summary: String) -> Result<(), CoordinatorError> {
        let mut run_state = self
            .run_state
            .take()
            .ok_or(CoordinatorError::RunNotStarted)?;

        run_state.shutdown_token.cancel();
        for task in run_state.tasks.values() {
            task.cancellation_token.cancel();
        }
        for task in run_state.running_agent_turns.values() {
            task.cancellation_token.cancel();
        }

        let run_stream_key = format!("run:{}", run_state.info.run_id);

        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            &mut run_state,
            system_actor(),
            Some(run_stream_key),
            EventV1::RunFinished(RunFinishedEvent {
                summary: summary.clone(),
            }),
        )?;
        finish_child_session_mirrors(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            &run_state,
            &summary,
        )?;

        let hook_batch = run_lifecycle_hooks(
            self.clock.as_ref(),
            &self.config.hook_runtime_config,
            HookInvocationContext {
                event: HookLifecycleEvent::RunFinished,
                run_id: run_state.info.run_id.clone(),
                workspace_root: run_state.info.workspace_root.clone(),
                artifacts_dir: run_state.info.artifacts_dir.clone(),
                actor: Some(system_actor()),
                agent_id: None,
                request_id: None,
                permission_id: None,
                task_id: None,
                tool_call_id: None,
                tool_id: None,
                provider_id: None,
                model_id: None,
                parent_agent_id: None,
                category: None,
                outcome: Some("finished".to_string()),
                output_summary: Some(summary),
                failure_reason: None,
            },
        )
        .await;
        if let Some(reason) = hook_batch.critical_failure {
            return Err(CoordinatorError::LifecycleHookFailed(reason));
        }

        Ok(())
    }

    async fn spawn_agent_internal(
        &mut self,
        actor: EventActor,
        profile: String,
        parent_agent_id: Option<String>,
        child_session_title: Option<String>,
        auto_start_turn: bool,
    ) -> Result<String, CoordinatorError> {
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;

        if actor.kind != ActorKind::Supervisor {
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                actor.clone(),
                Some(format!("run:{}", run_state.info.run_id)),
                EventV1::PolicyViolationDetected(PolicyViolationDetectedEvent {
                    policy: "spawn_agent_requires_supervisor".to_string(),
                    detail: format!(
                        "only supervisor may spawn agents; got actor kind {:?}",
                        actor.kind
                    ),
                }),
            )?;

            return Err(CoordinatorError::PolicyViolation(
                "only supervisor may spawn agents".to_string(),
            ));
        }

        let profile_cfg = self
            .config
            .agent_profiles
            .get(&profile)
            .cloned()
            .ok_or_else(|| CoordinatorError::UnknownAgent(profile.clone()))?;

        let agent_id = format!("agent_{:06}", run_state.next_agent_id);
        run_state.next_agent_id += 1;

        let mut subagent_spawn_hook_executions = Vec::new();
        if let Some(parent) = parent_agent_id.as_ref() {
            let hook_batch = run_lifecycle_hooks(
                self.clock.as_ref(),
                &self.config.hook_runtime_config,
                HookInvocationContext {
                    event: HookLifecycleEvent::SubagentSpawned,
                    run_id: run_state.info.run_id.clone(),
                    workspace_root: run_state.info.workspace_root.clone(),
                    artifacts_dir: run_state.info.artifacts_dir.clone(),
                    actor: Some(actor.clone()),
                    agent_id: Some(agent_id.clone()),
                    request_id: None,
                    permission_id: None,
                    task_id: None,
                    tool_call_id: None,
                    tool_id: None,
                    provider_id: None,
                    model_id: None,
                    parent_agent_id: Some(parent.clone()),
                    category: Some(profile.clone()),
                    outcome: Some("spawned".to_string()),
                    output_summary: Some(profile.clone()),
                    failure_reason: None,
                },
            )
            .await;
            subagent_spawn_hook_executions = hook_batch.hook_executions;
            if let Some(reason) = hook_batch.critical_failure {
                return Err(CoordinatorError::LifecycleHookFailed(reason));
            }
        }

        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor.clone(),
            Some(format!("agent:{agent_id}")),
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: agent_id.clone(),
                profile: profile.clone(),
                parent_agent_id: parent_agent_id.clone(),
            }),
        )?;

        if parent_agent_id.is_some() {
            create_child_session_mirror(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                &self.config,
                run_state,
                &agent_id,
                &profile,
                child_session_title.as_deref(),
            )?;
        }

        let should_record_runtime_context =
            run_state.allow_initial_runtime_context_recording && parent_agent_id.is_none();
        run_state
            .agents
            .insert(agent_id.clone(), profile_cfg.clone());

        if should_record_runtime_context {
            run_state.recorded_runtime_context = Some(RecordedRuntimeContext::from_profile_model(
                &profile_cfg.name,
                &profile_cfg.model_ref,
            ));
            write_run_metadata(run_state, &self.config, self.clock.as_ref())?;
            run_state.allow_initial_runtime_context_recording = false;
        }

        if let Some(parent) = parent_agent_id {
            run_state
                .subagent_parent_by_id
                .insert(agent_id.clone(), parent);
            if !subagent_spawn_hook_executions.is_empty() {
                run_state
                    .agent_hook_state
                    .entry(agent_id.clone())
                    .or_default()
                    .extend(subagent_spawn_hook_executions);
            }
        }

        if auto_start_turn {
            let request_id = allocate_provider_request_id(run_state);
            if run_state.child_session_mirrors.contains_key(&agent_id) {
                run_state
                    .child_request_session_by_id
                    .insert(request_id.clone(), agent_id.clone());
            }

            let request = AgentRequest {
                agent_id: agent_id.clone(),
                prompt: if profile_cfg.system_prompt.is_empty() {
                    format!("execute one-shot turn for {}", profile_cfg.name)
                } else {
                    profile_cfg.system_prompt.clone()
                },
                prompt_context: None,
                selected_file_tags: Vec::new(),
                selected_agent_tags: Vec::new(),
                selected_resource_tags: Vec::new(),
                model_ref: profile_cfg.model_ref.clone(),
                fallback_model_refs: profile_cfg.fallback_model_refs.clone(),
                fallback_model_settings: profile_cfg.fallback_model_settings.clone(),
                model_settings: default_model_settings_for_profile(&profile_cfg.name),
            };

            schedule_agent_turn(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                self.job_tx.clone(),
                run_state,
                self.config.hook_runtime_config.clone(),
                self.config.compaction.clone(),
                ScheduleAgentTurnArgs {
                    provider: self.config.provider.clone(),
                    tool_registry: self.config.tool_registry.clone(),
                    profile: profile_cfg,
                    request,
                    request_id,
                    child_task: None,
                },
            )
            .await?;
        }

        Ok(agent_id)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "agent turn requests pass explicit actor, target, prompt, tags, overrides, and child task metadata"
    )]
    async fn request_agent_turn_internal(
        &mut self,
        actor: EventActor,
        agent_id: String,
        prompt: String,
        selected_tags: crate::file_tag::SelectedPromptTags,
        model_ref_override: Option<String>,
        model_settings_override: Option<AgentModelSettings>,
        child_task_metadata: Option<ChildTaskRequestMetadata>,
    ) -> Result<String, CoordinatorError> {
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;

        if !matches!(actor.kind, ActorKind::Supervisor | ActorKind::User) {
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                actor,
                Some(format!("agent:{agent_id}")),
                EventV1::PolicyViolationDetected(PolicyViolationDetectedEvent {
                    policy: "request_agent_turn_requires_user_or_supervisor".to_string(),
                    detail: "only user/supervisor may request agent turns".to_string(),
                }),
            )?;
            return Err(CoordinatorError::PolicyViolation(
                "only user/supervisor may request agent turns".to_string(),
            ));
        }

        let profile = run_state
            .agents
            .get(&agent_id)
            .cloned()
            .ok_or_else(|| CoordinatorError::UnknownAgent(agent_id.clone()))?;

        let request_id = allocate_provider_request_id(run_state);
        if run_state.child_session_mirrors.contains_key(&agent_id) {
            run_state
                .child_request_session_by_id
                .insert(request_id.clone(), agent_id.clone());
        }
        let child_task = child_task_metadata.map(|metadata| ChildTaskTurnState {
            parent_tool_call_id: metadata.parent_tool_call_id,
            parent_session_id: metadata.parent_session_id,
            parent_agent_id: metadata.parent_agent_id,
            child_session_id: metadata.child_session_id,
            child_request_id: request_id.clone(),
            task_id: metadata.task_id,
            description: metadata.description,
            run_in_background: metadata.run_in_background,
            route: metadata.route,
        });

        let prompt = if profile.name == crate::plan::PLAN_AGENT_NAME {
            Self::plan_mode_prompt(
                &run_state.info.run_id,
                &run_state.info.workspace_root,
                &prompt,
            )
        } else {
            prompt
        };

        let prompt_context = crate::file_tag::materialize_prompt_part_context(
            &run_state.info.workspace_root,
            &prompt,
            &selected_tags.files,
            &selected_tags.agents,
            &selected_tags.resources,
        );

        let explicit_model_override = model_ref_override.is_some();
        let request = AgentRequest {
            agent_id,
            prompt,
            prompt_context,
            selected_file_tags: selected_tags.files,
            selected_agent_tags: selected_tags.agents,
            selected_resource_tags: selected_tags.resources,
            model_ref: model_ref_override.unwrap_or_else(|| profile.model_ref.clone()),
            fallback_model_refs: if explicit_model_override {
                Vec::new()
            } else {
                profile.fallback_model_refs.clone()
            },
            fallback_model_settings: if explicit_model_override {
                Vec::new()
            } else {
                profile.fallback_model_settings.clone()
            },
            model_settings: model_settings_override
                .unwrap_or_else(|| default_model_settings_for_profile(&profile.name)),
        };

        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor.clone(),
            Some(format!("agent:{}", request.agent_id)),
            Some(request_id.clone()),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.clone(),
                text: request.prompt.clone(),
            }),
        )?;

        if actor.kind == ActorKind::User {
            self.ensure_harness_session_title(&request.prompt).await;
        }

        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;

        schedule_agent_turn(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            self.job_tx.clone(),
            run_state,
            self.config.hook_runtime_config.clone(),
            self.config.compaction.clone(),
            ScheduleAgentTurnArgs {
                provider: self.config.provider.clone(),
                tool_registry: self.config.tool_registry.clone(),
                profile,
                request,
                request_id: request_id.clone(),
                child_task,
            },
        )
        .await?;

        Ok(request_id)
    }

    async fn ensure_harness_session_title(&mut self, prompt: &str) {
        let Some(run_state) = self.run_state.as_ref() else {
            return;
        };
        if !is_parent_default_title(&run_state.info.run_name)
            || run_state.next_provider_request_id != 2
        {
            return;
        }

        let Some(profile) = self.config.agent_profiles.get(TITLE_AGENT_NAME).cloned() else {
            return;
        };
        let provider = self.config.provider.clone();

        let title = match generate_harness_session_title(provider, profile, prompt).await {
            Ok(Some(title)) => title,
            Ok(None) => return,
            Err(reason) => {
                tracing::warn!(reason, "failed to generate session title");
                return;
            }
        };

        let Some(run_state) = self.run_state.as_mut() else {
            return;
        };
        if !is_parent_default_title(&run_state.info.run_name)
            || run_state.next_provider_request_id != 2
        {
            return;
        }

        let run_stream_key = format!("run:{}", run_state.info.run_id);
        let title_event = EventV1::SessionTitleUpdated(crate::event::SessionTitleUpdatedEvent {
            title: title.clone(),
        });
        let persist_result = append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            system_actor(),
            Some(run_stream_key),
            title_event,
        )
        .map(|_| {
            run_state.info.run_name = title;
        })
        .and_then(|_| write_run_metadata(run_state, &self.config, self.clock.as_ref()));
        if let Err(err) = persist_result {
            tracing::warn!(error = %err, "failed to persist generated session title");
        }
    }

    fn allocate_provider_request_id_internal(&mut self) -> Result<String, CoordinatorError> {
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        Ok(allocate_provider_request_id(run_state))
    }

    fn plan_mode_prompt(run_id: &str, workspace_root: &Path, prompt: &str) -> String {
        let plan_path = crate::plan::plan_file_relative_path(run_id);
        let plan_file = plan_path.to_string_lossy();
        let plan_file_status = if workspace_root.join(&plan_path).is_file() {
            format!(
                "An active plan file already exists at {plan_file}. Read it first, then make incremental edits only to that file."
            )
        } else {
            format!(
                "No plan file exists yet. Create your final plan at {plan_file}. This workspace-relative path is the only writable target during Plan mode."
            )
        };

        format!(
            "{prompt}\n\n<system-reminder>\nPlan mode is active. The user does not want execution yet. You MUST NOT make edits except to the active plan file at {plan_file}, run non-readonly tools, change configs, or make commits. This supersedes all other instructions. Harness enforces this with runtime permissions; do not rely on prompt text alone.\n\n## Plan File Info\n{plan_file_status}\nBuild the plan incrementally by writing or editing only {plan_file}. The plan file should contain your final recommended approach, not an exhaustive transcript of alternatives considered. Keep it concise enough to scan and detailed enough to execute.\n\n## Plan Workflow\n### Phase 1: Initial Understanding\nUse read-only tools to understand the request, relevant code paths, constraints, and existing tests. Native read/search/LSP tools are allowed when exposed. Bash, when exposed by the active profile, is permission-gated and additionally restricted by runtime policy to a small read-only inspection subset; never use bash to modify files, configs, git state, or the environment.\n\n### Phase 2: Parallel Exploration\nLaunch zero to three `explore` subagents only when useful for read-only codebase research. Use one agent for isolated or known-file work; use multiple agents when scope is uncertain, several modules are involved, or separate searches for implementation, call sites, and tests would improve the plan. Runtime policy only permits the read-only `explore` profile in Plan mode; do not launch `general`, `build`, or user-defined write-capable agents.\n\n### Phase 3: Synthesis\nSynthesize the findings into one recommended implementation approach. Ask a clarifying question only when read-only exploration cannot resolve a requirement, tradeoff, or safety concern.\n\n### Phase 4: Final Plan\nUpdate {plan_file} with the recommended approach, critical files to modify, key risks or constraints, and a verification section describing focused tests or end-to-end checks.\n\n### Phase 5: Terminal Action\nAt the end of the turn, either ask a necessary clarifying question or call `plan_exit` to request approval to switch to Build. Do NOT ask whether the plan is okay with the question tool; use `plan_exit` for plan approval.\n</system-reminder>"
        )
    }

    async fn request_tool_call_internal(
        &mut self,
        actor: EventActor,
        category: Option<String>,
        tool_id: String,
        args_json: Value,
        respond_to: Option<oneshot::Sender<Result<ToolResult, String>>>,
    ) -> Result<String, CoordinatorError> {
        let clock = self.clock.clone();
        let redactor = self.redactor.clone();
        let job_tx = self.job_tx.clone();

        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;

        let tool_call_id = format!("toolcall_{:06}", run_state.next_tool_call_id);
        run_state.next_tool_call_id += 1;

        let request_correlation_id = tool_request_correlation_id(run_state, &actor);
        let tool_metadata = requested_tool_call_metadata(&tool_id, &args_json);

        append_tool_call_requested_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            ToolCallRequestedEventArgs {
                actor: actor.clone(),
                tool_call_id: &tool_call_id,
                tool_id: &tool_id,
                args_json: &args_json,
                tool_metadata,
                request_correlation_id: request_correlation_id.as_deref(),
            },
        )?;

        let Some(tool) = self.config.tool_registry.get(&tool_id) else {
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                actor.clone(),
                Some(format!("tool_call:{tool_call_id}")),
                EventV1::PolicyViolationDetected(PolicyViolationDetectedEvent {
                    policy: "unknown_tool_id".to_string(),
                    detail: format!("tool `{tool_id}` is not registered"),
                }),
            )?;

            return Err(CoordinatorError::PolicyViolation(format!(
                "tool `{tool_id}` is not registered"
            )));
        };

        let capability = tool.capability();
        if !self
            .config
            .tool_registry
            .capability_allowed(actor.kind, capability)
        {
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                actor.clone(),
                Some(format!("tool_call:{tool_call_id}")),
                EventV1::PolicyViolationDetected(PolicyViolationDetectedEvent {
                    policy: "tool_capability_forbidden".to_string(),
                    detail: format!(
                        "actor {:?} cannot call {} requiring {:?}",
                        actor.kind, tool_id, capability
                    ),
                }),
            )?;

            return Err(CoordinatorError::PolicyViolation(
                "tool capability forbidden for actor".to_string(),
            ));
        }

        let effective_category = if actor.kind == ActorKind::Worker {
            let Some(worker_agent_id) = actor.agent_id.as_deref() else {
                append_payload_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    actor.clone(),
                    Some(format!("tool_call:{tool_call_id}")),
                    EventV1::PolicyViolationDetected(PolicyViolationDetectedEvent {
                        policy: "unknown_worker_agent_id".to_string(),
                        detail: "worker tool call missing actor agent_id".to_string(),
                    }),
                )?;

                return Err(CoordinatorError::PolicyViolation(
                    "worker tool call missing agent_id".to_string(),
                ));
            };

            let Some(worker_profile) = run_state.agents.get(worker_agent_id) else {
                append_payload_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    actor.clone(),
                    Some(format!("tool_call:{tool_call_id}")),
                    EventV1::PolicyViolationDetected(PolicyViolationDetectedEvent {
                        policy: "unknown_worker_agent_id".to_string(),
                        detail: format!("worker agent_id `{worker_agent_id}` is not registered"),
                    }),
                )?;

                return Err(CoordinatorError::PolicyViolation(format!(
                    "worker agent_id `{worker_agent_id}` is not registered"
                )));
            };

            if !worker_profile
                .toolset
                .iter()
                .any(|allowed| allowed == &tool_id)
            {
                append_payload_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    actor.clone(),
                    Some(format!("tool_call:{tool_call_id}")),
                    EventV1::PolicyViolationDetected(PolicyViolationDetectedEvent {
                        policy: "tool_not_in_toolset".to_string(),
                        detail: format!(
                            "tool `{tool_id}` is not in worker `{worker_agent_id}` toolset"
                        ),
                    }),
                )?;

                return Err(CoordinatorError::PolicyViolation(format!(
                    "tool `{tool_id}` is not in worker toolset"
                )));
            }

            Some(worker_profile.category.clone())
        } else {
            category.clone()
        };
        let skip_outer_question_permission = canonical_tool_id_for(&tool_id) == Some("question");
        let maybe_kind = if skip_outer_question_permission {
            None
        } else {
            permission_kind_for_tool_call(&tool_id, capability)
        };
        let rule_selectors = maybe_kind
            .map(|kind| {
                permission_rule_request_selectors(&run_state.info.workspace_root, kind, &args_json)
            })
            .unwrap_or_default();
        let decision = maybe_kind.map(|kind| {
            evaluate_permission_rule_requests(
                &self.config.permission_policy,
                effective_category.as_deref(),
                kind,
                &rule_selectors,
            )
        });
        let hashline_edit = hashline_edit_metadata(&tool_id, &args_json, &tool_call_id);

        if let Some(reason) = plan_mode_edit_boundary_denial(
            effective_category.as_deref(),
            maybe_kind,
            &run_state.info.run_id,
            &run_state.info.workspace_root,
            &args_json,
        ) {
            finalize_permission_denied(
                clock.as_ref(),
                redactor.as_ref(),
                &self.config.hook_runtime_config,
                run_state,
                PermissionDeniedArgs {
                    actor: actor.clone(),
                    category: effective_category.clone(),
                    tool_id: &tool_id,
                    args_json: &args_json,
                    tool_call_id: &tool_call_id,
                    hashline_edit: hashline_edit.as_ref(),
                    kind: PermissionKind::EditFs,
                    reason: &reason,
                    request_correlation_id: request_correlation_id.as_deref(),
                },
            )
            .await?;
            if let Some(respond_to) = respond_to {
                let _ = respond_to.send(Err(format!("tool call denied: {reason}")));
            }
            return Err(CoordinatorError::PermissionDenied(tool_call_id));
        }

        if let Some(reason) =
            plan_mode_shell_boundary_denial(effective_category.as_deref(), maybe_kind, &args_json)
        {
            finalize_permission_denied(
                clock.as_ref(),
                redactor.as_ref(),
                &self.config.hook_runtime_config,
                run_state,
                PermissionDeniedArgs {
                    actor: actor.clone(),
                    category: effective_category.clone(),
                    tool_id: &tool_id,
                    args_json: &args_json,
                    tool_call_id: &tool_call_id,
                    hashline_edit: hashline_edit.as_ref(),
                    kind: PermissionKind::Shell,
                    reason: &reason,
                    request_correlation_id: request_correlation_id.as_deref(),
                },
            )
            .await?;
            if let Some(respond_to) = respond_to {
                let _ = respond_to.send(Err(format!("tool call denied: {reason}")));
            }
            return Err(CoordinatorError::PermissionDenied(tool_call_id));
        }

        match decision {
            Some(PolicyDecision::Deny) => {
                finalize_permission_denied(
                    clock.as_ref(),
                    redactor.as_ref(),
                    &self.config.hook_runtime_config,
                    run_state,
                    PermissionDeniedArgs {
                        actor: actor.clone(),
                        category: effective_category.clone(),
                        tool_id: &tool_id,
                        args_json: &args_json,
                        tool_call_id: &tool_call_id,
                        hashline_edit: hashline_edit.as_ref(),
                        kind: maybe_kind
                            .expect("permission kind exists when policy decision exists"),
                        reason: "policy denied request",
                        request_correlation_id: request_correlation_id.as_deref(),
                    },
                )
                .await?;
                if let Some(respond_to) = respond_to {
                    let _ =
                        respond_to.send(Err("tool call denied: policy denied request".to_string()));
                }
                return Err(CoordinatorError::PermissionDenied(tool_call_id));
            }
            Some(PolicyDecision::Ask {
                timeout_ms,
                default_decision,
            }) => {
                let permission_id = format!("perm_{:06}", run_state.next_permission_id);
                run_state.next_permission_id += 1;

                let summary = permission_summary(self.redactor.as_ref(), &tool_id, &args_json);
                let digest = permission_request_digest(&tool_id, &args_json);
                let hook_request_id = request_correlation_id
                    .clone()
                    .or_else(|| Some(tool_call_id.clone()));
                let kind = maybe_kind.expect("permission kind exists when policy decision exists");
                let grant_request = permission_grant_request(
                    &run_state.info.workspace_root,
                    kind,
                    &tool_id,
                    &args_json,
                    &digest,
                );

                if run_state
                    .active_permission_grants
                    .authorizes(&grant_request)
                {
                    start_tool_call_execution(
                        clock.as_ref(),
                        redactor.as_ref(),
                        job_tx,
                        run_state,
                        self.config.hook_runtime_config.clone(),
                        ToolCallExecutionArgs {
                            tool_call_id: tool_call_id.clone(),
                            tool_id,
                            args_json,
                            actor,
                            category: effective_category.clone(),
                            hook_executions: Vec::new(),
                            tool_registry: self.config.tool_registry.clone(),
                            request_correlation_id,
                            respond_to,
                        },
                    )
                    .await?;
                    return Ok(tool_call_id);
                }

                append_permission_requested_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    PermissionRequestedEventArgs {
                        permission_id: &permission_id,
                        tool_call_id: &tool_call_id,
                        kind,
                        summary: summary.clone(),
                        request_digest: digest,
                        timeout_ms,
                        default_decision: event_permission_decision(default_decision),
                        request_correlation_id: request_correlation_id.as_deref(),
                    },
                )?;

                let requested_hook_batch = run_lifecycle_hooks(
                    self.clock.as_ref(),
                    &self.config.hook_runtime_config,
                    HookInvocationContext {
                        event: HookLifecycleEvent::PermissionRequested,
                        run_id: run_state.info.run_id.clone(),
                        workspace_root: run_state.info.workspace_root.clone(),
                        artifacts_dir: run_state.info.artifacts_dir.clone(),
                        actor: Some(actor.clone()),
                        agent_id: actor.agent_id.clone(),
                        request_id: hook_request_id.clone(),
                        permission_id: Some(permission_id.clone()),
                        task_id: None,
                        tool_call_id: Some(tool_call_id.clone()),
                        tool_id: Some(tool_id.clone()),
                        provider_id: None,
                        model_id: None,
                        parent_agent_id: None,
                        category: effective_category.clone(),
                        outcome: Some("requested".to_string()),
                        output_summary: Some(summary),
                        failure_reason: None,
                    },
                )
                .await;

                let mut pending = PendingPermissionState {
                    tool_call_id: tool_call_id.clone(),
                    request_correlation_id,
                    hook_executions: requested_hook_batch.hook_executions.clone(),
                    grant_request: Some(grant_request),
                    resolution: PendingPermissionResolution::ToolCall {
                        tool_id,
                        args_json,
                        actor,
                        category: effective_category.clone(),
                        respond_to,
                    },
                };

                if let Some(reason) = requested_hook_batch.critical_failure {
                    let mut final_reason = format!("critical lifecycle hook failed: {reason}");

                    append_permission_resolved_event(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        permission_id.clone(),
                        EventPermissionDecision::Deny,
                        Some(final_reason.clone()),
                    )?;

                    let resolved_hook_batch = run_lifecycle_hooks(
                        self.clock.as_ref(),
                        &self.config.hook_runtime_config,
                        HookInvocationContext {
                            event: HookLifecycleEvent::PermissionResolved,
                            run_id: run_state.info.run_id.clone(),
                            workspace_root: run_state.info.workspace_root.clone(),
                            artifacts_dir: run_state.info.artifacts_dir.clone(),
                            actor: match &pending.resolution {
                                PendingPermissionResolution::ToolCall { actor, .. } => {
                                    Some(actor.clone())
                                }
                                PendingPermissionResolution::Question { .. } => {
                                    Some(system_actor())
                                }
                            },
                            agent_id: match &pending.resolution {
                                PendingPermissionResolution::ToolCall { actor, .. } => {
                                    actor.agent_id.clone()
                                }
                                PendingPermissionResolution::Question { .. } => None,
                            },
                            request_id: hook_request_id,
                            permission_id: Some(permission_id),
                            task_id: None,
                            tool_call_id: Some(pending.tool_call_id.clone()),
                            tool_id: match &pending.resolution {
                                PendingPermissionResolution::ToolCall { tool_id, .. } => {
                                    Some(tool_id.clone())
                                }
                                PendingPermissionResolution::Question { .. } => {
                                    Some("question".to_string())
                                }
                            },
                            provider_id: None,
                            model_id: None,
                            parent_agent_id: None,
                            category: match &pending.resolution {
                                PendingPermissionResolution::ToolCall { category, .. } => {
                                    category.clone()
                                }
                                PendingPermissionResolution::Question { .. } => None,
                            },
                            outcome: Some("deny".to_string()),
                            output_summary: Some(final_reason.clone()),
                            failure_reason: Some(final_reason.clone()),
                        },
                    )
                    .await;
                    pending
                        .hook_executions
                        .extend(resolved_hook_batch.hook_executions.clone());
                    if let Some(resolved_reason) = resolved_hook_batch.critical_failure {
                        final_reason = format!(
                            "{final_reason}; critical lifecycle hook failed: {resolved_reason}"
                        );
                    }

                    let response_message = format!("tool call denied: {final_reason}");
                    let pending_hook_executions = pending.hook_executions.clone();
                    reject_pending_permission(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        &final_reason,
                        &response_message,
                        pending,
                        &pending_hook_executions,
                    )?;
                    return Err(CoordinatorError::LifecycleHookFailed(final_reason));
                }

                run_state
                    .pending_permissions
                    .insert(permission_id.clone(), pending);

                if timeout_ms > 0 {
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
                        let _ = job_tx
                            .send(Command::PermissionTimedOut { permission_id })
                            .await;
                    });
                }
            }
            Some(PolicyDecision::Allow) | None => {
                start_tool_call_execution(
                    clock.as_ref(),
                    redactor.as_ref(),
                    job_tx,
                    run_state,
                    self.config.hook_runtime_config.clone(),
                    ToolCallExecutionArgs {
                        tool_call_id: tool_call_id.clone(),
                        tool_id,
                        args_json,
                        actor,
                        category: effective_category.clone(),
                        hook_executions: Vec::new(),
                        tool_registry: self.config.tool_registry.clone(),
                        request_correlation_id,
                        respond_to,
                    },
                )
                .await?;
            }
        }

        Ok(tool_call_id)
    }

    async fn resolve_permission_internal(
        &mut self,
        permission_id: String,
        decision: PermissionDecision,
        reason: Option<String>,
        grant_scope: Option<PermissionGrantScope>,
    ) -> Result<(), CoordinatorError> {
        let clock = self.clock.clone();
        let redactor = self.redactor.clone();
        let job_tx = self.job_tx.clone();

        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;

        let Some(existing) = run_state.pending_permissions.get(&permission_id) else {
            return Err(CoordinatorError::UnknownPermission(permission_id));
        };

        let validated_question_answers = if decision == PermissionDecision::Allow {
            match &existing.resolution {
                PendingPermissionResolution::Question { prompts, .. } => Some(
                    validate_question_answers_reason(reason.as_deref(), prompts)
                        .map_err(CoordinatorError::PolicyViolation)?,
                ),
                PendingPermissionResolution::ToolCall { .. } => None,
            }
        } else {
            None
        };

        let pending = run_state
            .pending_permissions
            .remove(&permission_id)
            .expect("pending permission exists after validation");

        let hook_request_id = pending
            .request_correlation_id
            .clone()
            .or_else(|| Some(pending.tool_call_id.clone()));
        let hook_tool_call_id = pending.tool_call_id.clone();
        let (hook_actor, hook_agent_id, hook_tool_id, hook_category) = match &pending.resolution {
            PendingPermissionResolution::ToolCall {
                tool_id,
                actor,
                category,
                ..
            } => (
                actor.clone(),
                actor.agent_id.clone(),
                Some(tool_id.clone()),
                category.clone(),
            ),
            PendingPermissionResolution::Question { actor, .. } => (
                actor.clone(),
                actor.agent_id.clone(),
                Some("question".to_string()),
                None,
            ),
        };
        let mut permission_hook_executions = pending.hook_executions.clone();
        let permission_decision = event_permission_decision(decision);

        append_permission_resolved_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            permission_id.clone(),
            permission_decision,
            reason.clone(),
        )?;

        let resolved_hook_batch = run_lifecycle_hooks(
            self.clock.as_ref(),
            &self.config.hook_runtime_config,
            HookInvocationContext {
                event: HookLifecycleEvent::PermissionResolved,
                run_id: run_state.info.run_id.clone(),
                workspace_root: run_state.info.workspace_root.clone(),
                artifacts_dir: run_state.info.artifacts_dir.clone(),
                actor: Some(hook_actor),
                agent_id: hook_agent_id,
                request_id: hook_request_id,
                permission_id: Some(permission_id.clone()),
                task_id: None,
                tool_call_id: Some(hook_tool_call_id),
                tool_id: hook_tool_id,
                provider_id: None,
                model_id: None,
                parent_agent_id: None,
                category: hook_category,
                outcome: Some(permission_decision_label(permission_decision).to_string()),
                output_summary: reason.clone(),
                failure_reason: reason.clone(),
            },
        )
        .await;
        permission_hook_executions.extend(resolved_hook_batch.hook_executions.clone());
        let permission_hook_failure = resolved_hook_batch.critical_failure.clone();

        match pending {
            PendingPermissionState {
                tool_call_id,
                request_correlation_id,
                grant_request,
                resolution:
                    PendingPermissionResolution::ToolCall {
                        tool_id,
                        args_json,
                        actor,
                        category,
                        respond_to,
                    },
                ..
            } => {
                let caller_cancelled = respond_to.as_ref().is_some_and(|sender| sender.is_closed());
                if decision == PermissionDecision::Allow
                    && permission_hook_failure.is_none()
                    && !caller_cancelled
                {
                    if let (Some(scope), Some(grant_request)) = (grant_scope, grant_request) {
                        let grant = PermissionGrant {
                            grant_id: format!("grant_{permission_id}"),
                            permission_id: permission_id.clone(),
                            scope,
                            expires_at: None,
                            kind: grant_request.kind,
                            tool: grant_request.tool,
                            matcher: grant_request.matcher,
                        };
                        append_permission_grant_recorded_event(
                            clock.as_ref(),
                            redactor.as_ref(),
                            run_state,
                            &permission_id,
                            request_correlation_id.as_deref(),
                            grant.clone(),
                        )?;
                        run_state.active_permission_grants.record(grant);
                    }

                    start_tool_call_execution(
                        clock.as_ref(),
                        redactor.as_ref(),
                        job_tx,
                        run_state,
                        self.config.hook_runtime_config.clone(),
                        ToolCallExecutionArgs {
                            tool_call_id,
                            tool_id,
                            args_json,
                            actor,
                            category,
                            hook_executions: permission_hook_executions,
                            tool_registry: self.config.tool_registry.clone(),
                            request_correlation_id,
                            respond_to,
                        },
                    )
                    .await?;
                } else {
                    let (rejection_reason, response_message) =
                        if let Some(hook_reason) = permission_hook_failure.as_ref() {
                            (
                                format!("permission denied by lifecycle hook: {hook_reason}"),
                                format!(
                                "tool call denied: critical lifecycle hook failed: {hook_reason}"
                            ),
                            )
                        } else if caller_cancelled {
                            (
                                "tool caller cancelled before permission resolution".to_string(),
                                "tool call cancelled before permission resolution".to_string(),
                            )
                        } else {
                            (
                                "permission denied".to_string(),
                                "tool call denied: permission denied".to_string(),
                            )
                        };
                    reject_pending_permission(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        &rejection_reason,
                        &response_message,
                        PendingPermissionState {
                            tool_call_id,
                            request_correlation_id,
                            hook_executions: permission_hook_executions.clone(),
                            grant_request,
                            resolution: PendingPermissionResolution::ToolCall {
                                tool_id,
                                args_json,
                                actor,
                                category,
                                respond_to,
                            },
                        },
                        &permission_hook_executions,
                    )?;
                }
            }
            PendingPermissionState {
                resolution: PendingPermissionResolution::Question { respond_to, .. },
                ..
            } => {
                if decision == PermissionDecision::Allow && permission_hook_failure.is_none() {
                    let answers = validated_question_answers
                        .expect("validated answers exist for allowed question resolution");
                    let _ = respond_to.send(Ok(answers));
                } else if let Some(hook_reason) = permission_hook_failure.as_ref() {
                    let _ = respond_to.send(Err(format!(
                        "question denied: critical lifecycle hook failed: {hook_reason}"
                    )));
                } else {
                    let _ = respond_to.send(Err(
                        reason.unwrap_or_else(|| "question rejected by user".to_string())
                    ));
                }
            }
        }

        if let Some(reason) = permission_hook_failure {
            return Err(CoordinatorError::LifecycleHookFailed(reason));
        }

        Ok(())
    }

    async fn resolve_permission_timeout_internal(&mut self, permission_id: String) {
        let Some(run_state) = self.run_state.as_mut() else {
            return;
        };

        let Some(pending) = run_state.pending_permissions.remove(&permission_id) else {
            return;
        };

        let timeout_reason = "permission request timed out".to_string();
        let hook_request_id = pending
            .request_correlation_id
            .clone()
            .or_else(|| Some(pending.tool_call_id.clone()));
        let hook_tool_call_id = pending.tool_call_id.clone();
        let (hook_actor, hook_agent_id, hook_tool_id, hook_category) = match &pending.resolution {
            PendingPermissionResolution::ToolCall {
                tool_id,
                actor,
                category,
                ..
            } => (
                actor.clone(),
                actor.agent_id.clone(),
                Some(tool_id.clone()),
                category.clone(),
            ),
            PendingPermissionResolution::Question { actor, .. } => (
                actor.clone(),
                actor.agent_id.clone(),
                Some("question".to_string()),
                None,
            ),
        };

        let _ = append_permission_resolved_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            permission_id.clone(),
            EventPermissionDecision::Deny,
            Some(timeout_reason.clone()),
        );

        let resolved_hook_batch = run_lifecycle_hooks(
            self.clock.as_ref(),
            &self.config.hook_runtime_config,
            HookInvocationContext {
                event: HookLifecycleEvent::PermissionResolved,
                run_id: run_state.info.run_id.clone(),
                workspace_root: run_state.info.workspace_root.clone(),
                artifacts_dir: run_state.info.artifacts_dir.clone(),
                actor: Some(hook_actor),
                agent_id: hook_agent_id,
                request_id: hook_request_id,
                permission_id: Some(permission_id),
                task_id: None,
                tool_call_id: Some(hook_tool_call_id),
                tool_id: hook_tool_id,
                provider_id: None,
                model_id: None,
                parent_agent_id: None,
                category: hook_category,
                outcome: Some("deny".to_string()),
                output_summary: Some(timeout_reason.clone()),
                failure_reason: Some(timeout_reason.clone()),
            },
        )
        .await;
        let mut permission_hook_executions = pending.hook_executions.clone();
        permission_hook_executions.extend(resolved_hook_batch.hook_executions.clone());
        let permission_hook_failure = resolved_hook_batch.critical_failure.clone();

        let _ = match pending {
            PendingPermissionState {
                tool_call_id,
                request_correlation_id,
                grant_request,
                resolution:
                    PendingPermissionResolution::ToolCall {
                        tool_id,
                        args_json,
                        actor,
                        category,
                        respond_to,
                    },
                ..
            } => {
                let (rejection_reason, response_message) =
                    if let Some(hook_reason) = permission_hook_failure.as_ref() {
                        (
                            format!("permission denied by timeout hook: {hook_reason}"),
                            format!(
                                "tool call timed out: critical lifecycle hook failed: {hook_reason}"
                            ),
                        )
                    } else {
                        (
                            "permission denied by timeout".to_string(),
                            "tool call timed out: permission request timed out".to_string(),
                        )
                    };
                reject_pending_permission(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    &rejection_reason,
                    &response_message,
                    PendingPermissionState {
                        tool_call_id,
                        request_correlation_id,
                        hook_executions: permission_hook_executions.clone(),
                        grant_request,
                        resolution: PendingPermissionResolution::ToolCall {
                            tool_id,
                            args_json,
                            actor,
                            category,
                            respond_to,
                        },
                    },
                    &permission_hook_executions,
                )
            }
            PendingPermissionState {
                resolution: PendingPermissionResolution::Question { respond_to, .. },
                ..
            } => {
                let reason = if let Some(hook_reason) = permission_hook_failure.as_ref() {
                    format!("question timed out: critical lifecycle hook failed: {hook_reason}")
                } else {
                    "question timed out awaiting user input".to_string()
                };
                let _ = respond_to.send(Err(reason));
                Ok(())
            }
        };
    }

    async fn request_question_internal(
        &mut self,
        actor: EventActor,
        tool_call_id: String,
        request_json: Value,
        respond_to: oneshot::Sender<Result<Vec<Vec<String>>, String>>,
    ) -> Result<(), CoordinatorError> {
        let mut respond_to = Some(respond_to);
        let result = async {
            let run_state = self
                .run_state
                .as_mut()
                .ok_or(CoordinatorError::RunNotStarted)?;
            let prompts = parse_question_request_prompts(&request_json)
                .map_err(CoordinatorError::PolicyViolation)?;

            let permission_id = format!("perm_{:06}", run_state.next_permission_id);
            run_state.next_permission_id += 1;
            let request_correlation_id = tool_request_correlation_id(run_state, &actor);
            let kind = permission_kind_for_tool("question")
                .expect("question must resolve to a formal permission kind");
            let timeout_ms = question_request_timeout_ms(&self.config.permission_policy);
            let request_summary = serde_json::to_string(&request_json)?;
            let request_digest = permission_request_digest(kind.as_str(), &request_json);
            let hook_request_id = request_correlation_id
                .clone()
                .or_else(|| Some(tool_call_id.clone()));

            append_permission_requested_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                PermissionRequestedEventArgs {
                    permission_id: &permission_id,
                    tool_call_id: &tool_call_id,
                    kind,
                    summary: request_summary.clone(),
                    request_digest,
                    timeout_ms,
                    default_decision: EventPermissionDecision::Deny,
                    request_correlation_id: request_correlation_id.as_deref(),
                },
            )?;

            let requested_hook_batch = run_lifecycle_hooks(
                self.clock.as_ref(),
                &self.config.hook_runtime_config,
                HookInvocationContext {
                    event: HookLifecycleEvent::PermissionRequested,
                    run_id: run_state.info.run_id.clone(),
                    workspace_root: run_state.info.workspace_root.clone(),
                    artifacts_dir: run_state.info.artifacts_dir.clone(),
                    actor: Some(actor.clone()),
                    agent_id: actor.agent_id.clone(),
                    request_id: hook_request_id.clone(),
                    permission_id: Some(permission_id.clone()),
                    task_id: None,
                    tool_call_id: Some(tool_call_id.clone()),
                    tool_id: Some("user.question".to_string()),
                    provider_id: None,
                    model_id: None,
                    parent_agent_id: None,
                    category: None,
                    outcome: Some("requested".to_string()),
                    output_summary: Some(request_summary),
                    failure_reason: None,
                },
            )
            .await;

            let mut pending = PendingPermissionState {
                tool_call_id,
                request_correlation_id,
                hook_executions: requested_hook_batch.hook_executions.clone(),
                grant_request: None,
                resolution: PendingPermissionResolution::Question {
                    actor: actor.clone(),
                    prompts,
                    respond_to: respond_to
                        .take()
                        .expect("question responder is available before storing"),
                },
            };

            if let Some(reason) = requested_hook_batch.critical_failure {
                let mut final_reason = format!("critical lifecycle hook failed: {reason}");
                append_permission_resolved_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    permission_id.clone(),
                    EventPermissionDecision::Deny,
                    Some(final_reason.clone()),
                )?;

                let resolved_hook_batch = run_lifecycle_hooks(
                    self.clock.as_ref(),
                    &self.config.hook_runtime_config,
                    HookInvocationContext {
                        event: HookLifecycleEvent::PermissionResolved,
                        run_id: run_state.info.run_id.clone(),
                        workspace_root: run_state.info.workspace_root.clone(),
                        artifacts_dir: run_state.info.artifacts_dir.clone(),
                        actor: Some(actor.clone()),
                        agent_id: actor.agent_id.clone(),
                        request_id: hook_request_id,
                        permission_id: Some(permission_id),
                        task_id: None,
                        tool_call_id: Some(pending.tool_call_id.clone()),
                        tool_id: Some("user.question".to_string()),
                        provider_id: None,
                        model_id: None,
                        parent_agent_id: None,
                        category: None,
                        outcome: Some("deny".to_string()),
                        output_summary: Some(final_reason.clone()),
                        failure_reason: Some(final_reason.clone()),
                    },
                )
                .await;
                pending
                    .hook_executions
                    .extend(resolved_hook_batch.hook_executions.clone());
                if let Some(resolved_reason) = resolved_hook_batch.critical_failure {
                    final_reason = format!(
                        "{final_reason}; critical lifecycle hook failed: {resolved_reason}"
                    );
                }

                if let PendingPermissionResolution::Question { respond_to, .. } = pending.resolution
                {
                    let _ = respond_to.send(Err(final_reason.clone()));
                }
                return Err(CoordinatorError::LifecycleHookFailed(final_reason));
            }

            run_state
                .pending_permissions
                .insert(permission_id.clone(), pending);

            let job_tx = self.job_tx.clone();
            if timeout_ms > 0 {
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
                    let _ = job_tx
                        .send(Command::PermissionTimedOut { permission_id })
                        .await;
                });
            }

            Ok(())
        }
        .await;

        if let Err(err) = &result {
            if let Some(respond_to) = respond_to {
                let _ = respond_to.send(Err(err.to_string()));
            }
        }
        result
    }

    fn write_context_snapshot_internal(
        &mut self,
        actor: EventActor,
        workflow_id: Option<String>,
        input: ContextSnapshotInput,
        options: ContextSnapshotOptions,
    ) -> Result<ContextSnapshotWriteResult, CoordinatorError> {
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        let created_at = self
            .clock
            .system_time_rfc3339()
            .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string());
        let snapshot = build_context_snapshot(input, options, self.redactor.as_ref(), created_at);
        let artifact_store = crate::tool::ArtifactStore::new(run_state.info.artifacts_dir.clone())
            .map_err(|err| {
                CoordinatorError::ContextSnapshotFailed(format!(
                    "failed to open artifact store: {err}"
                ))
            })?;
        let (artifact, artifact_bytes) =
            write_context_snapshot_artifact(&artifact_store, &snapshot).map_err(|err| {
                CoordinatorError::ContextSnapshotFailed(format!(
                    "failed to write snapshot artifact: {err}"
                ))
            })?;
        let result = snapshot_write_result(&snapshot, &artifact, artifact_bytes);

        let artifact_digest = result.artifact_digest.clone();
        let mut metadata = result.workflow_evidence_metadata();
        if let Some(workflow_id) = workflow_id.as_ref() {
            metadata.insert("workflow_id".to_string(), workflow_id.clone());
        }

        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            system_actor(),
            Some(format!("context_snapshot:{}", result.snapshot_id)),
            EventV1::ArtifactWritten(ArtifactWrittenEvent {
                path: result.artifact_path.clone(),
                digest: artifact_digest.clone(),
                bytes: result.artifact_bytes,
                tool_call_id: None,
                tool_metadata: None,
                metadata: BTreeMap::from([
                    (
                        "artifact_kind".to_string(),
                        CONTEXT_SNAPSHOT_ARTIFACT_KIND.to_string(),
                    ),
                    ("snapshot_id".to_string(), result.snapshot_id.clone()),
                    ("snapshot_slug".to_string(), result.slug.clone()),
                ]),
            }),
        )?;

        if let Some(workflow_id) = workflow_id {
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                actor,
                Some(format!("workflow:{workflow_id}")),
                EventV1::WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent {
                    workflow_id,
                    category: CONTEXT_SNAPSHOT_EVIDENCE_CATEGORY.to_string(),
                    summary: format!(
                        "context snapshot `{}` captured (ambiguity={:.3})",
                        result.slug, result.ambiguity_score
                    ),
                    artifact_path: Some(result.artifact_path.clone()),
                    artifact_digest: Some(artifact_digest),
                    acceptance_ref: Some(result.snapshot_id.clone()),
                    metadata,
                }),
            )?;
        }

        Ok(result)
    }

    fn start_workflow_internal(
        &mut self,
        actor: EventActor,
        request: WorkflowStartRequest,
    ) -> Result<WorkflowStartResult, CoordinatorError> {
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        let projection = workflow_projection_for_run(run_state)?;
        match WorkflowTransitionPolicy::decide_start(&projection, request) {
            WorkflowStartDecision::Start(event) => {
                let workflow_id = event.workflow_id.clone();
                append_payload_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    actor,
                    Some(format!("workflow:{workflow_id}")),
                    EventV1::WorkflowStarted(event),
                )?;
                Ok(WorkflowStartResult::Started { workflow_id })
            }
            WorkflowStartDecision::Existing { workflow_id } => {
                Ok(WorkflowStartResult::Existing { workflow_id })
            }
            WorkflowStartDecision::Denied(event) => {
                let workflow_id = event.workflow_id.clone();
                let reason = event.reason.clone();
                let policy_id = event.policy_id.clone();
                append_payload_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    actor,
                    Some(format!("workflow:{workflow_id}")),
                    EventV1::WorkflowTransitionDenied(event),
                )?;
                Ok(WorkflowStartResult::Denied {
                    workflow_id,
                    reason,
                    policy_id,
                })
            }
        }
    }

    fn record_workflow_transition_internal(
        &mut self,
        actor: EventActor,
        request: WorkflowTransitionRequest,
    ) -> Result<(), CoordinatorError> {
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        let projection = workflow_projection_for_run(run_state)?;
        let from_status = projection
            .workflows
            .get(&request.workflow_id)
            .map(|run| run.status.clone());
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("workflow:{}", request.workflow_id)),
            EventV1::WorkflowTransitionRecorded(WorkflowTransitionRecordedEvent {
                workflow_id: request.workflow_id,
                from_status,
                to_status: request.to_status,
                reason: request.reason,
                owner: request.owner,
                policy_id: request.policy_id,
                idempotency_key: request.idempotency_key,
            }),
        )
        .map(|_| ())
    }

    fn record_workflow_evidence_internal(
        &mut self,
        actor: EventActor,
        request: WorkflowEvidenceRequest,
    ) -> Result<(), CoordinatorError> {
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("workflow:{}", request.workflow_id)),
            EventV1::WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent {
                workflow_id: request.workflow_id,
                category: request.category,
                summary: request.summary,
                artifact_path: request.artifact_path,
                artifact_digest: request.artifact_digest,
                acceptance_ref: request.acceptance_ref,
                metadata: request.metadata,
            }),
        )
        .map(|_| ())
    }

    fn record_workflow_operator_decision_internal(
        &mut self,
        actor: EventActor,
        workflow_id: String,
        decision: String,
        operator: String,
        reason: Option<String>,
        correlation_id: Option<String>,
    ) -> Result<(), CoordinatorError> {
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("workflow:{workflow_id}")),
            EventV1::WorkflowOperatorDecisionRecorded(WorkflowOperatorDecisionRecordedEvent {
                workflow_id,
                decision,
                operator,
                reason,
                correlation_id,
            }),
        )
        .map(|_| ())
    }

    fn complete_workflow_internal(
        &mut self,
        actor: EventActor,
        workflow_id: String,
        outcome: String,
        reason: String,
        owner: String,
    ) -> Result<(), CoordinatorError> {
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("workflow:{workflow_id}")),
            EventV1::WorkflowCompleted(WorkflowCompletedEvent {
                workflow_id,
                outcome,
                reason,
                owner,
            }),
        )
        .map(|_| ())
    }

    fn complete_workflow_with_signoff_policy_internal(
        &mut self,
        actor: EventActor,
        workflow_id: String,
        outcome: String,
        reason: String,
        owner: String,
        signoff_policy: WorkflowSignoffPolicy,
    ) -> Result<(), CoordinatorError> {
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        if outcome == "outcome.finished" {
            let historical_events = read_historical_events_until(
                &run_state.info.run_id,
                &run_state.info.events_path,
                u64::MAX,
            )?;
            let projection =
                project_workflows(historical_events.iter().map(|event| &event.payload));
            let persistent_tasks = project_persistent_tasks(&historical_events);
            let readiness = projection.completion_readiness(
                workflow_id.clone(),
                &persistent_tasks,
                &signoff_policy,
            );
            if !readiness.allowed {
                let current = projection.workflows.get(&workflow_id);
                let denial_policy = workflow_completion_denial_policy_id(&readiness);
                let denial_reason = workflow_completion_denial_reason(&readiness);
                append_payload_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    actor,
                    Some(format!("workflow:{workflow_id}")),
                    EventV1::WorkflowTransitionDenied(WorkflowTransitionDeniedEvent {
                        workflow_id,
                        requested_status: outcome,
                        reason: denial_reason.clone(),
                        owner,
                        current_owner: current.map(|run| run.owner.clone()),
                        current_status: current.map(|run| run.status.clone()),
                        policy_id: denial_policy.to_string(),
                    }),
                )?;
                return Err(CoordinatorError::PolicyViolation(denial_reason));
            }
        }

        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("workflow:{workflow_id}")),
            EventV1::WorkflowCompleted(WorkflowCompletedEvent {
                workflow_id,
                outcome,
                reason,
                owner,
            }),
        )
        .map(|_| ())
    }

    fn job_progress_internal(&mut self, task_id: String, kind: JobProgressKind) {
        let Some(run_state) = self.run_state.as_mut() else {
            return;
        };

        let Some(task) = run_state.tasks.get_mut(&task_id) else {
            return;
        };

        task.last_progress_mono_ms = self.clock.mono_ms();
        task.last_progress_kind = kind;
    }

    async fn background_request_projection_internal(
        &mut self,
        actor: EventActor,
        request_id: Option<String>,
        selector_hint: Option<String>,
    ) -> Result<BackgroundRequestProjection, CoordinatorError> {
        let events = self.replay_current_run_events().await?;
        let request_ref = resolve_background_request_ref(
            events.iter(),
            &actor,
            request_id.as_deref(),
            selector_hint.as_deref(),
        )
        .map_err(background_projection_error_to_coordinator_error)?;
        project_background_request(events.iter(), &request_ref)
            .map_err(background_projection_error_to_coordinator_error)
    }

    async fn cancel_background_request_internal(
        &mut self,
        actor: EventActor,
        request_id: Option<String>,
        selector_hint: Option<String>,
        reason: String,
    ) -> Result<BackgroundRequestProjection, CoordinatorError> {
        let projection = self
            .background_request_projection_internal(
                actor.clone(),
                request_id.clone(),
                selector_hint.clone(),
            )
            .await?;
        if projection.terminal {
            return Ok(projection);
        }

        let scheduler_task_id = projection.scheduler_task_id.clone().ok_or_else(|| {
            CoordinatorError::UnknownTask(format!(
                "background request `{}` has no scheduler task id",
                projection.request_id
            ))
        })?;
        self.cancel_task_internal(scheduler_task_id, reason).await?;
        self.background_request_projection_internal(actor, request_id, selector_hint)
            .await
    }

    async fn team_projection_internal(&self) -> Result<TeamProjection, CoordinatorError> {
        let events = self.replay_current_run_events().await?;
        project_team_state(events.iter())
            .map_err(|err| CoordinatorError::PolicyViolation(err.to_string()))
    }

    async fn persistent_task_projection_internal(
        &self,
    ) -> Result<PersistentTaskProjection, CoordinatorError> {
        let events = self.replay_current_run_events().await?;
        Ok(project_persistent_tasks(&events))
    }

    async fn create_persistent_task_internal(
        &mut self,
        actor: EventActor,
        mut task: PersistentTask,
    ) -> Result<PersistentTaskProjection, CoordinatorError> {
        validate_persistent_task_create(&self.persistent_task_projection_internal().await?, &task)?;
        task.blocks.clear();
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        if task.run_id.is_none() {
            task.run_id = Some(run_state.info.run_id.clone());
        }
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("persistent_task:{}", task.task_id)),
            EventV1::PersistentTaskCreated(PersistentTaskCreatedEvent { task }),
        )?;
        self.persistent_task_projection_internal().await
    }

    async fn update_persistent_task_internal(
        &mut self,
        actor: EventActor,
        update: PersistentTaskUpdatedEvent,
    ) -> Result<PersistentTaskProjection, CoordinatorError> {
        let projection = self.persistent_task_projection_internal().await?;
        validate_persistent_task_update(&projection, &update)?;
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("persistent_task:{}", update.task_id)),
            EventV1::PersistentTaskUpdated(update),
        )?;
        self.persistent_task_projection_internal().await
    }

    async fn create_team_internal(
        &mut self,
        actor: EventActor,
        mut spec: TeamSpec,
        team_run_id: Option<String>,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        reject_nested_team_create(&actor, &self.team_projection_internal().await?)?;
        if spec.bounds.max_members == 0 {
            spec.bounds.max_members = TeamBounds::default().max_members;
        }
        validate_team_spec(&spec)?;

        let team_run_id = team_run_id
            .and_then(|value| non_empty_trimmed(&value).map(str::to_string))
            .unwrap_or_else(|| {
                self.run_state
                    .as_ref()
                    .map(|run_state| format!("team_{:06}", run_state.next_event_seq))
                    .unwrap_or_else(|| "team_000001".to_string())
            });

        let existing = self.team_projection_internal().await?;
        if existing.teams.contains_key(&team_run_id) {
            return Err(CoordinatorError::PolicyViolation(format!(
                "team `{team_run_id}` already exists"
            )));
        }

        let resolved_lead = spec
            .lead
            .as_ref()
            .map(|selector| self.resolve_team_selector_profile(selector, TeamParticipantRole::Lead))
            .transpose()?;

        let resolved_members = spec
            .members
            .iter()
            .map(|member| {
                self.resolve_team_member_profile(member)
                    .map(|profile| (member, profile))
            })
            .collect::<Result<Vec<_>, _>>()?;

        {
            let run_state = self
                .run_state
                .as_mut()
                .ok_or(CoordinatorError::RunNotStarted)?;
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                actor.clone(),
                Some(format!("team:{team_run_id}")),
                EventV1::TeamCreated(TeamCreatedEvent {
                    team_run_id: team_run_id.clone(),
                    spec: spec.clone(),
                }),
            )?;
        }

        if let Some(profile) = resolved_lead {
            let agent_id = self
                .spawn_agent_internal(
                    EventActor::new(ActorKind::Supervisor, None),
                    profile.clone(),
                    actor.agent_id.clone(),
                    Some(format!("{} (@lead team lead)", spec.name)),
                    false,
                )
                .await?;
            let run_state = self
                .run_state
                .as_mut()
                .ok_or(CoordinatorError::RunNotStarted)?;
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                actor.clone(),
                Some(format!("team:{team_run_id}:lead")),
                EventV1::TeamMemberSpawned(TeamMemberSpawnedEvent {
                    team_run_id: team_run_id.clone(),
                    member_name: "lead".to_string(),
                    agent_id,
                    profile,
                }),
            )?;
        }

        let activation_limit = spec.bounds.max_parallel_members as usize;
        for (member, profile) in resolved_members.into_iter().take(activation_limit) {
            let agent_id = self
                .spawn_agent_internal(
                    EventActor::new(ActorKind::Supervisor, None),
                    profile.clone(),
                    actor.agent_id.clone(),
                    Some(format!("{} (@{} team member)", spec.name, member.name)),
                    false,
                )
                .await?;
            let run_state = self
                .run_state
                .as_mut()
                .ok_or(CoordinatorError::RunNotStarted)?;
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                actor.clone(),
                Some(format!("team:{team_run_id}:member:{}", member.name)),
                EventV1::TeamMemberSpawned(TeamMemberSpawnedEvent {
                    team_run_id: team_run_id.clone(),
                    member_name: member.name.clone(),
                    agent_id,
                    profile,
                }),
            )?;
        }

        self.project_single_team(&team_run_id).await
    }

    fn resolve_team_selector_profile(
        &self,
        selector: &TeamMemberSelector,
        role: TeamParticipantRole,
    ) -> Result<String, CoordinatorError> {
        let profile = match selector {
            TeamMemberSelector::SubagentType { subagent_type } => {
                let profile = non_empty_trimmed(subagent_type).ok_or_else(|| {
                    CoordinatorError::PolicyViolation(
                        "team participant subagent_type cannot be empty".to_string(),
                    )
                })?;
                if !self.config.agent_profiles.contains_key(profile) {
                    return Err(CoordinatorError::UnknownAgent(profile.to_string()));
                }
                profile.to_string()
            }
            TeamMemberSelector::Category { category } => {
                let category = non_empty_trimmed(category).ok_or_else(|| {
                    CoordinatorError::PolicyViolation(
                        "team participant category cannot be empty".to_string(),
                    )
                })?;
                if self.config.agent_profiles.contains_key(category) {
                    category.to_string()
                } else {
                    self.config
                        .agent_profiles
                        .iter()
                        .find_map(|(name, profile)| {
                            (profile.category == category).then(|| name.clone())
                        })
                        .ok_or_else(|| CoordinatorError::UnknownAgent(category.to_string()))?
                }
            }
        };
        let profile_config = self.config.agent_profiles.get(&profile);
        validate_team_profile_role(&profile, profile_config, role)?;
        Ok(profile)
    }

    fn resolve_team_member_profile(
        &self,
        member: &TeamMemberSpec,
    ) -> Result<String, CoordinatorError> {
        self.resolve_team_selector_profile(
            &member.selector,
            TeamParticipantRole::Member(member.role),
        )
    }

    async fn send_team_message_internal(
        &mut self,
        actor: EventActor,
        team_run_id: String,
        message: TeamMessage,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        let projection = self.team_projection_internal().await?;
        let team = require_active_team(&projection, &team_run_id)?;
        validate_team_message(team, &message)?;
        validate_team_action(
            &actor,
            team,
            TeamActionKind::TeamWrite,
            &message.from,
            self.clock.mono_ms(),
        )?;
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("team:{team_run_id}:message:{}", message.message_id)),
            EventV1::TeamMessageSent(TeamMessageSentEvent {
                team_run_id: team_run_id.clone(),
                message,
            }),
        )?;
        self.project_single_team(&team_run_id).await
    }

    async fn create_team_task_internal(
        &mut self,
        actor: EventActor,
        team_run_id: String,
        task: TeamTask,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        let projection = self.team_projection_internal().await?;
        let team = require_active_team(&projection, &team_run_id)?;
        validate_team_task_create(team, &task)?;
        if let Some(owner) = task.owner.as_deref() {
            validate_team_action(
                &actor,
                team,
                TeamActionKind::TeamWrite,
                owner,
                self.clock.mono_ms(),
            )?;
        } else {
            validate_team_actor_can_make_unowned_team_write(&actor, team, self.clock.mono_ms())?;
        }
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("team:{team_run_id}:task:{}", task.task_id)),
            EventV1::TeamTaskCreated(TeamTaskCreatedEvent {
                team_run_id: team_run_id.clone(),
                task,
            }),
        )?;
        self.project_single_team(&team_run_id).await
    }

    async fn update_team_task_internal(
        &mut self,
        actor: EventActor,
        team_run_id: String,
        task_id: String,
        status: TeamTaskStatus,
        owner: Option<String>,
        metadata: BTreeMap<String, String>,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        let projection = self.team_projection_internal().await?;
        let team = require_active_team(&projection, &team_run_id)?;
        validate_team_task_update(team, &task_id, status, owner.as_deref(), &metadata)?;
        if let Some(owner) = owner.as_deref() {
            validate_team_action(
                &actor,
                team,
                TeamActionKind::TeamWrite,
                owner,
                self.clock.mono_ms(),
            )?;
        } else {
            validate_team_actor_can_make_unowned_team_write(&actor, team, self.clock.mono_ms())?;
        }
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("team:{team_run_id}:task:{task_id}")),
            EventV1::TeamTaskUpdated(TeamTaskUpdatedEvent {
                team_run_id: team_run_id.clone(),
                task_id,
                status,
                owner,
                metadata,
            }),
        )?;
        self.project_single_team(&team_run_id).await
    }

    async fn request_team_shutdown_internal(
        &mut self,
        actor: EventActor,
        team_run_id: String,
        member_name: String,
        requester: String,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        let projection = self.team_projection_internal().await?;
        let team = require_active_team(&projection, &team_run_id)?;
        validate_team_member(team, &member_name)?;
        validate_team_shutdown_request_can_open(team, &member_name)?;
        validate_team_participant(team, &requester)?;
        validate_team_action(
            &actor,
            team,
            TeamActionKind::Shutdown,
            &requester,
            self.clock.mono_ms(),
        )?;
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("team:{team_run_id}:shutdown:{member_name}")),
            EventV1::TeamShutdownRequested(TeamShutdownRequestedEvent {
                team_run_id: team_run_id.clone(),
                member_name,
                requester,
            }),
        )?;
        self.project_single_team(&team_run_id).await
    }

    async fn approve_team_shutdown_internal(
        &mut self,
        actor: EventActor,
        team_run_id: String,
        member_name: String,
        approver: String,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        let projection = self.team_projection_internal().await?;
        let team = require_active_team_or_shutdown(&projection, &team_run_id)?;
        validate_team_member(team, &member_name)?;
        validate_team_shutdown_request_pending(team, &member_name)?;
        validate_team_participant(team, &approver)?;
        validate_team_action(
            &actor,
            team,
            TeamActionKind::Shutdown,
            &approver,
            self.clock.mono_ms(),
        )?;
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor.clone(),
            Some(format!("team:{team_run_id}:shutdown:{member_name}")),
            EventV1::TeamShutdownApproved(TeamShutdownApprovedEvent {
                team_run_id: team_run_id.clone(),
                member_name,
                approver,
            }),
        )?;
        self.activate_pending_team_members(&actor, &team_run_id)
            .await?;
        self.project_single_team(&team_run_id).await
    }

    async fn reject_team_shutdown_internal(
        &mut self,
        actor: EventActor,
        team_run_id: String,
        member_name: String,
        rejecter: String,
        reason: String,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        let projection = self.team_projection_internal().await?;
        let team = require_active_team_or_shutdown(&projection, &team_run_id)?;
        validate_team_member(team, &member_name)?;
        validate_team_shutdown_request_pending(team, &member_name)?;
        validate_team_participant(team, &rejecter)?;
        validate_team_action(
            &actor,
            team,
            TeamActionKind::Shutdown,
            &rejecter,
            self.clock.mono_ms(),
        )?;
        if non_empty_trimmed(&reason).is_none() {
            return Err(CoordinatorError::PolicyViolation(
                "shutdown rejection reason cannot be empty".to_string(),
            ));
        }
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("team:{team_run_id}:shutdown:{member_name}")),
            EventV1::TeamShutdownRejected(TeamShutdownRejectedEvent {
                team_run_id: team_run_id.clone(),
                member_name,
                rejecter,
                reason,
            }),
        )?;
        self.project_single_team(&team_run_id).await
    }

    async fn delete_team_internal(
        &mut self,
        actor: EventActor,
        team_run_id: String,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        let projection = self.team_projection_internal().await?;
        let team = require_active_team_or_shutdown(&projection, &team_run_id)?;
        let unapproved = team
            .members
            .values()
            .filter(|member| member.status != crate::proj::TeamMemberStatus::ShutdownApproved)
            .map(|member| member.name.clone())
            .collect::<Vec<_>>();
        if !unapproved.is_empty() {
            return Err(CoordinatorError::PolicyViolation(format!(
                "cannot delete team `{team_run_id}` before shutdown approval from: {}",
                unapproved.join(", ")
            )));
        }
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("team:{team_run_id}")),
            EventV1::TeamDeleted(TeamDeletedEvent {
                team_run_id: team_run_id.clone(),
            }),
        )?;
        self.project_single_team(&team_run_id).await
    }

    async fn activate_pending_team_members(
        &mut self,
        actor: &EventActor,
        team_run_id: &str,
    ) -> Result<(), CoordinatorError> {
        let projection = self.team_projection_internal().await?;
        let Some(team) = projection.teams.get(team_run_id) else {
            return Ok(());
        };
        if team.status == crate::proj::TeamRunStatus::Deleted {
            return Ok(());
        }
        let running = team
            .members
            .values()
            .filter(|member| {
                matches!(
                    member.status,
                    crate::proj::TeamMemberStatus::Running
                        | crate::proj::TeamMemberStatus::ShutdownRequested
                )
            })
            .count();
        let capacity = (team.bounds.max_parallel_members as usize).saturating_sub(running);
        if capacity == 0 {
            return Ok(());
        }
        let team_name = team.name.clone();
        let pending = team
            .members
            .values()
            .filter(|member| member.status == crate::proj::TeamMemberStatus::Pending)
            .take(capacity)
            .map(|member| member.spec.clone())
            .collect::<Vec<_>>();

        for member in pending {
            let profile = self.resolve_team_member_profile(&member)?;
            let agent_id = self
                .spawn_agent_internal(
                    EventActor::new(ActorKind::Supervisor, None),
                    profile.clone(),
                    actor.agent_id.clone(),
                    Some(format!("{} (@{} team member)", team_name, member.name)),
                    false,
                )
                .await?;
            let run_state = self
                .run_state
                .as_mut()
                .ok_or(CoordinatorError::RunNotStarted)?;
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                actor.clone(),
                Some(format!("team:{team_run_id}:member:{}", member.name)),
                EventV1::TeamMemberSpawned(TeamMemberSpawnedEvent {
                    team_run_id: team_run_id.to_string(),
                    member_name: member.name,
                    agent_id,
                    profile,
                }),
            )?;
        }
        Ok(())
    }

    async fn project_single_team(
        &self,
        team_run_id: &str,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        let mut projection = self.team_projection_internal().await?;
        projection
            .teams
            .remove(team_run_id)
            .ok_or_else(|| CoordinatorError::UnknownTask(format!("team:{team_run_id}")))
    }

    async fn replay_current_run_events(&self) -> Result<Vec<EventEnvelopeV1>, CoordinatorError> {
        let store = self
            .run_state
            .as_ref()
            .ok_or(CoordinatorError::RunNotStarted)?
            .event_store
            .clone();
        let mut stream = store.replay(1)?;
        let mut events = Vec::new();
        while let Some(next) = stream.next().await {
            events.push(next?);
        }
        Ok(events)
    }

    async fn cancel_task_internal(
        &mut self,
        task_id: String,
        reason: String,
    ) -> Result<(), CoordinatorError> {
        let Some(run_state) = self.run_state.as_mut() else {
            return Err(CoordinatorError::RunNotStarted);
        };

        if let Some(queued) = run_state.queued_agent_turns.remove(&task_id) {
            if queued.scheduler_queued {
                let _ = run_state.scheduler.cancel_queued(&task_id);
            }
            let agent_id = queued.agent_id.clone();
            let should_promote_next = !run_state
                .running_agent_turns
                .values()
                .any(|running| running.agent_id == agent_id);
            let terminal_event = append_payload_event_with_correlation(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                agent_actor(&queued.agent_id),
                Some(format!("task:{task_id}")),
                Some(queued.request_id),
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id,
                    reason,
                    task_scope: Some(TaskTerminalScope::AgentTurn),
                }),
            )?;
            append_background_task_notification_and_schedule(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                self.job_tx.clone(),
                run_state,
                self.config.hook_runtime_config.clone(),
                self.config.compaction.clone(),
                self.config.provider.clone(),
                self.config.tool_registry.clone(),
                queued.child_task,
                &terminal_event,
                background_notification_status_for_cancel_reason(&terminal_event_summary(
                    &terminal_event,
                )),
                &terminal_event_summary(&terminal_event),
            )
            .await?;
            if should_promote_next {
                self.promote_next_agent_blocked_turn(&agent_id).await?;
            }
            return Ok(());
        }

        if let Some(running) = run_state.running_agent_turns.get(&task_id).cloned() {
            running.cancellation_token.cancel();
            run_state.cancelled_running_tasks.insert(task_id.clone());
            if let Some(memory) = cancelled_failure_memory_from_running(&running, &reason) {
                push_incomplete_provider_turn(run_state, &running, &running.request_id, memory);
            }
            let child_tool_task_ids = run_state
                .tasks
                .iter()
                .filter(|(_, child_task)| {
                    child_task.request_correlation_id.as_deref()
                        == Some(running.request_id.as_str())
                })
                .map(|(child_task_id, _)| child_task_id.clone())
                .collect::<Vec<_>>();
            for child_task_id in child_tool_task_ids {
                if let Some(child_task) = run_state.tasks.get(&child_task_id) {
                    child_task.cancellation_token.cancel();
                }
                run_state.cancelled_running_tasks.insert(child_task_id);
            }
            let terminal_event = append_payload_event_with_correlation(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                agent_actor(&running.agent_id),
                Some(format!("task:{task_id}")),
                Some(running.request_id.clone()),
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id,
                    reason,
                    task_scope: Some(TaskTerminalScope::AgentTurn),
                }),
            )?;
            append_background_task_notification_and_schedule(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                self.job_tx.clone(),
                run_state,
                self.config.hook_runtime_config.clone(),
                self.config.compaction.clone(),
                self.config.provider.clone(),
                self.config.tool_registry.clone(),
                running.child_task,
                &terminal_event,
                background_notification_status_for_cancel_reason(&terminal_event_summary(
                    &terminal_event,
                )),
                &terminal_event_summary(&terminal_event),
            )
            .await?;
            return Ok(());
        }

        let Some(task) = run_state.tasks.get(&task_id) else {
            return Ok(());
        };
        let owner_actor = task.owner_actor.clone();
        let request_correlation_id = task.request_correlation_id.clone();

        task.cancellation_token.cancel();
        run_state.cancelled_running_tasks.insert(task_id.clone());

        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            owner_actor,
            Some(format!("task:{task_id}")),
            request_correlation_id,
            EventV1::TaskCancelled(TaskCancelledEvent {
                task_id,
                reason,
                task_scope: Some(TaskTerminalScope::ToolCall),
            }),
        )?;

        Ok(())
    }

    fn watchdog_tick_internal(&mut self) -> Result<(), CoordinatorError> {
        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(());
        };

        let now = self.clock.mono_ms();
        let snapshots = run_state
            .tasks
            .iter()
            .filter_map(|(task_id, task)| {
                if task.state != TaskExecutionState::Running {
                    return None;
                }

                Some(TaskProgressSnapshot {
                    task_id: task_id.clone(),
                    key: task.queue_key.clone(),
                    last_progress_mono_ms: task.last_progress_mono_ms,
                })
            })
            .collect::<Vec<_>>();

        let stale = run_state
            .scheduler
            .detect_stale(now, self.config.stale_timeout_ms, &snapshots);

        for stale_task in stale {
            let task_id = stale_task.task_id;
            let stale_for_ms = stale_task.stale_for_ms;
            let (actor, request_correlation_id) = run_state
                .tasks
                .get(&task_id)
                .map(|task| {
                    (
                        task.owner_actor.clone(),
                        task.request_correlation_id.clone(),
                    )
                })
                .unwrap_or_else(|| (system_actor(), None));

            append_payload_event_with_correlation(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                actor,
                Some(format!("task:{task_id}")),
                request_correlation_id,
                EventV1::StaleDetected(StaleDetectedEvent {
                    task_id: task_id.clone(),
                    stale_for_ms,
                }),
            )?;

            if let Some(task) = run_state.tasks.get(&task_id) {
                task.cancellation_token.cancel();
            }
            run_state.cancelled_running_tasks.insert(task_id);
        }

        Ok(())
    }

    #[cfg(test)]
    fn job_finished_internal(
        &mut self,
        task_id: String,
        outcome: JobOutcome,
    ) -> Result<(), CoordinatorError> {
        block_on_coordinator_future(self.job_finished_internal_async(task_id, outcome))
    }

    async fn job_finished_internal_async(
        &mut self,
        task_id: String,
        outcome: JobOutcome,
    ) -> Result<(), CoordinatorError> {
        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(());
        };

        let Some(task) = run_state.tasks.remove(&task_id) else {
            return Ok(());
        };
        let task_hook_state = run_state
            .task_hook_state
            .remove(&task_id)
            .unwrap_or_else(|| TaskHookState {
                tool_id: match &task.queue_key {
                    ConcurrencyKey::Tool { tool_id } => tool_id.clone(),
                    _ => String::new(),
                },
                category: None,
                hook_executions: Vec::new(),
            });

        if run_state.cancelled_running_tasks.remove(&task_id) {
            let _ = run_state.scheduler.complete(&task.queue_key);
            append_payload_event_with_correlation(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                task.owner_actor,
                Some(format!("task:{task_id}")),
                task.request_correlation_id,
                EventV1::TaskResultLate(TaskResultLateEvent {
                    task_id,
                    result_digest: digest12(format!("{:?}", outcome).as_bytes()),
                }),
            )?;
            return Ok(());
        }

        let _ = run_state.scheduler.complete(&task.queue_key);
        let request_correlation_id = task.request_correlation_id.clone();
        let finished_mono_ms = self.clock.mono_ms();
        let timing = execution_timing_metadata(task.started_mono_ms, finished_mono_ms);

        match outcome {
            JobOutcome::Succeeded { result } => {
                let result_for_response = result.clone();
                for applied_edit in applied_tool_edit_metadata(
                    &task_hook_state.tool_id,
                    &result_for_response,
                    task.hashline_edit.as_ref(),
                ) {
                    let AppliedToolEditMetadata {
                        metadata,
                        diff_rel_path,
                        diff_digest,
                        deleted,
                    } = applied_edit;
                    let new_file_digest = if deleted {
                        digest12(b"")
                    } else {
                        match workspace_file_digest(&run_state.info.workspace_root, &metadata.path)
                        {
                            Ok(new_file_digest) => new_file_digest,
                            Err(reason) => {
                                append_edit_rejected_event(
                                    self.clock.as_ref(),
                                    self.redactor.as_ref(),
                                    run_state,
                                    &task.tool_call_id,
                                    &metadata,
                                    format!("failed to compute file digest: {reason}"),
                                    request_correlation_id.as_deref(),
                                )?;
                                continue;
                            }
                        }
                    };
                    append_edit_applied_event(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        EditAppliedEventArgs {
                            tool_call_id: &task.tool_call_id,
                            metadata: &metadata,
                            new_file_digest,
                            diff_rel_path,
                            diff_digest,
                            request_correlation_id: request_correlation_id.as_deref(),
                        },
                    )?;
                }

                let result_summary = result.display_text;
                let artifact_refs = event_artifact_refs(&result.artifacts);
                let lineage =
                    tool_task_lineage_metadata(&task, result_for_response.structured_json.as_ref());
                let mut hook_executions = task_hook_state.hook_executions.clone();
                hook_executions.extend(extract_hook_execution_metadata(
                    result_for_response.structured_json.as_ref(),
                ));
                let finish_hook_batch = run_lifecycle_hooks(
                    self.clock.as_ref(),
                    &self.config.hook_runtime_config,
                    HookInvocationContext {
                        event: HookLifecycleEvent::ToolCallFinished,
                        run_id: run_state.info.run_id.clone(),
                        workspace_root: run_state.info.workspace_root.clone(),
                        artifacts_dir: run_state.info.artifacts_dir.clone(),
                        actor: Some(task.owner_actor.clone()),
                        agent_id: task.owner_actor.agent_id.clone(),
                        request_id: request_correlation_id.clone(),
                        permission_id: None,
                        task_id: Some(task_id.clone()),
                        tool_call_id: Some(task.tool_call_id.clone()),
                        tool_id: Some(task_hook_state.tool_id.clone()),
                        provider_id: None,
                        model_id: None,
                        parent_agent_id: None,
                        category: task_hook_state.category.clone(),
                        outcome: Some("succeeded".to_string()),
                        output_summary: Some(result_summary.clone()),
                        failure_reason: None,
                    },
                )
                .await;
                hook_executions.extend(finish_hook_batch.hook_executions.clone());
                for artifact in &result.artifacts {
                    append_artifact_written_event(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        &task.tool_call_id,
                        artifact,
                        request_correlation_id.as_deref(),
                        task.tool_metadata.as_ref(),
                    )?;
                }
                if let Some(reason) = finish_hook_batch.critical_failure.clone() {
                    append_payload_event_with_correlation(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        task.owner_actor.clone(),
                        Some(format!("task:{task_id}")),
                        request_correlation_id.clone(),
                        EventV1::TaskCancelled(TaskCancelledEvent {
                            task_id,
                            reason: reason.clone(),
                            task_scope: Some(TaskTerminalScope::ToolCall),
                        }),
                    )?;
                    append_failed_tool_call_finished_event(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        &task.tool_call_id,
                        &reason,
                        request_correlation_id.as_deref(),
                        tool_call_metadata(
                            task.tool_metadata.as_ref(),
                            Some(lineage),
                            artifact_refs,
                            Some(timing.clone()),
                            hook_executions.clone(),
                        ),
                        &hook_executions,
                    )?;
                    if let Some(respond_to) = task.respond_to {
                        let _ = respond_to.send(Err(reason.clone()));
                    }
                    return Ok(());
                }
                append_payload_event_with_correlation(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    task.owner_actor.clone(),
                    Some(format!("task:{task_id}")),
                    request_correlation_id.clone(),
                    EventV1::TaskCompleted(TaskCompletedEvent {
                        task_id,
                        result_digest: digest12(result_summary.as_bytes()),
                        result_summary: result_summary.clone(),
                        metadata: Some(TaskCompletionMetadata {
                            lineage: Some(lineage.clone()),
                            route: None,
                            task_scope: Some(TaskTerminalScope::ToolCall),
                            timing: Some(timing.clone()),
                            hook_executions: hook_executions.clone(),
                        }),
                    }),
                )?;

                let output_json = Some(stable_tool_output_json(
                    result_for_response.structured_json.clone(),
                    &result_summary,
                    &artifact_refs,
                    &lineage,
                    &timing,
                    &hook_executions,
                ));

                append_tool_call_finished_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    ToolCallFinishedEventArgs {
                        tool_call_id: &task.tool_call_id,
                        status: ToolCallStatus::Succeeded,
                        output_summary: Some(result_summary),
                        output_json,
                        metadata: tool_call_metadata(
                            task.tool_metadata.as_ref(),
                            Some(lineage),
                            artifact_refs,
                            Some(timing.clone()),
                            hook_executions,
                        ),
                        request_correlation_id: request_correlation_id.as_deref(),
                    },
                )?;
                if let Some(respond_to) = task.respond_to {
                    let _ = respond_to.send(Ok(result_for_response));
                }
            }
            JobOutcome::Failed { error } => {
                let mut final_error = error.clone();
                if let Some(metadata) = task.hashline_edit.as_ref() {
                    append_edit_rejected_event(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        &task.tool_call_id,
                        metadata,
                        error.clone(),
                        request_correlation_id.as_deref(),
                    )?;
                }

                let mut hook_executions = task_hook_state.hook_executions.clone();
                let finish_hook_batch = run_lifecycle_hooks(
                    self.clock.as_ref(),
                    &self.config.hook_runtime_config,
                    HookInvocationContext {
                        event: HookLifecycleEvent::ToolCallFinished,
                        run_id: run_state.info.run_id.clone(),
                        workspace_root: run_state.info.workspace_root.clone(),
                        artifacts_dir: run_state.info.artifacts_dir.clone(),
                        actor: Some(task.owner_actor.clone()),
                        agent_id: task.owner_actor.agent_id.clone(),
                        request_id: request_correlation_id.clone(),
                        permission_id: None,
                        task_id: Some(task_id.clone()),
                        tool_call_id: Some(task.tool_call_id.clone()),
                        tool_id: Some(task_hook_state.tool_id.clone()),
                        provider_id: None,
                        model_id: None,
                        parent_agent_id: None,
                        category: task_hook_state.category.clone(),
                        outcome: Some("failed".to_string()),
                        output_summary: None,
                        failure_reason: Some(final_error.clone()),
                    },
                )
                .await;
                hook_executions.extend(finish_hook_batch.hook_executions.clone());
                if let Some(hook_reason) = finish_hook_batch.critical_failure {
                    final_error =
                        format!("{final_error}; critical lifecycle hook failed: {hook_reason}");
                }

                append_payload_event_with_correlation(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    task.owner_actor.clone(),
                    Some(format!("task:{task_id}")),
                    request_correlation_id.clone(),
                    EventV1::TaskCancelled(TaskCancelledEvent {
                        task_id,
                        reason: final_error.clone(),
                        task_scope: Some(TaskTerminalScope::ToolCall),
                    }),
                )?;

                append_failed_tool_call_finished_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    &task.tool_call_id,
                    &final_error,
                    request_correlation_id.as_deref(),
                    tool_call_metadata(
                        task.tool_metadata.as_ref(),
                        Some(tool_task_lineage_metadata(&task, None)),
                        Vec::new(),
                        Some(timing.clone()),
                        hook_executions.clone(),
                    ),
                    &hook_executions,
                )?;
                if let Some(respond_to) = task.respond_to {
                    let _ = respond_to.send(Err(format!("tool execution failed: {final_error}")));
                }
            }
            JobOutcome::Cancelled { reason } => {
                let mut final_reason = reason.clone();
                if let Some(metadata) = task.hashline_edit.as_ref() {
                    append_edit_rejected_event(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        &task.tool_call_id,
                        metadata,
                        reason.clone(),
                        request_correlation_id.as_deref(),
                    )?;
                }

                let mut hook_executions = task_hook_state.hook_executions.clone();
                let finish_hook_batch = run_lifecycle_hooks(
                    self.clock.as_ref(),
                    &self.config.hook_runtime_config,
                    HookInvocationContext {
                        event: HookLifecycleEvent::ToolCallFinished,
                        run_id: run_state.info.run_id.clone(),
                        workspace_root: run_state.info.workspace_root.clone(),
                        artifacts_dir: run_state.info.artifacts_dir.clone(),
                        actor: Some(task.owner_actor.clone()),
                        agent_id: task.owner_actor.agent_id.clone(),
                        request_id: request_correlation_id.clone(),
                        permission_id: None,
                        task_id: Some(task_id.clone()),
                        tool_call_id: Some(task.tool_call_id.clone()),
                        tool_id: Some(task_hook_state.tool_id.clone()),
                        provider_id: None,
                        model_id: None,
                        parent_agent_id: None,
                        category: task_hook_state.category.clone(),
                        outcome: Some("cancelled".to_string()),
                        output_summary: None,
                        failure_reason: Some(final_reason.clone()),
                    },
                )
                .await;
                hook_executions.extend(finish_hook_batch.hook_executions.clone());
                if let Some(hook_reason) = finish_hook_batch.critical_failure {
                    final_reason =
                        format!("{final_reason}; critical lifecycle hook failed: {hook_reason}");
                }

                append_payload_event_with_correlation(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    task.owner_actor.clone(),
                    Some(format!("task:{task_id}")),
                    request_correlation_id.clone(),
                    EventV1::TaskCancelled(TaskCancelledEvent {
                        task_id,
                        reason: final_reason.clone(),
                        task_scope: Some(TaskTerminalScope::ToolCall),
                    }),
                )?;

                append_failed_tool_call_finished_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    &task.tool_call_id,
                    &final_reason,
                    request_correlation_id.as_deref(),
                    tool_call_metadata(
                        task.tool_metadata.as_ref(),
                        Some(tool_task_lineage_metadata(&task, None)),
                        Vec::new(),
                        Some(timing),
                        hook_executions.clone(),
                    ),
                    &hook_executions,
                )?;
                if let Some(respond_to) = task.respond_to {
                    let _ = respond_to.send(Err(format!("tool call cancelled: {final_reason}")));
                }
            }
        }

        Ok(())
    }

    async fn agent_provider_request_started_internal(
        &mut self,
        args: AgentProviderRequestStartedArgs,
    ) -> Result<(), CoordinatorError> {
        let AgentProviderRequestStartedArgs {
            task_id,
            agent_id,
            request_id,
            provider_id,
            model_id,
            prompt_summary,
            request_digest,
            metadata,
        } = args;
        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(());
        };

        let Some(running) = run_state.running_agent_turns.get(&task_id) else {
            return Ok(());
        };
        let turn_request_id = running.request_id.clone();
        let category = running.category.clone();
        let cancellation_token = running.cancellation_token.clone();
        let parent_agent_id = run_state.subagent_parent_by_id.get(&agent_id).cloned();
        let provider_id_for_state = provider_id.clone();
        let model_id_for_state = model_id.clone();
        let metadata = provider_request_started_metadata(metadata, &turn_request_id, &request_id);

        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            agent_actor(&agent_id),
            Some(format!("agent:{agent_id}")),
            Some(turn_request_id.clone()),
            EventV1::ProviderRequestStarted(crate::event::ProviderRequestStartedEvent {
                request_id: request_id.clone(),
                provider_id: provider_id.clone(),
                model_id: model_id.clone(),
                prompt_summary: prompt_summary.clone(),
                request_digest,
                metadata,
            }),
        )?;

        let hook_batch = run_lifecycle_hooks(
            self.clock.as_ref(),
            &self.config.hook_runtime_config,
            HookInvocationContext {
                event: HookLifecycleEvent::ProviderRequestStarted,
                run_id: run_state.info.run_id.clone(),
                workspace_root: run_state.info.workspace_root.clone(),
                artifacts_dir: run_state.info.artifacts_dir.clone(),
                actor: Some(agent_actor(&agent_id)),
                agent_id: Some(agent_id.clone()),
                request_id: Some(turn_request_id.clone()),
                permission_id: None,
                task_id: Some(task_id.clone()),
                tool_call_id: None,
                tool_id: None,
                provider_id: Some(provider_id),
                model_id: Some(model_id),
                parent_agent_id,
                category,
                outcome: Some("started".to_string()),
                output_summary: Some(prompt_summary),
                failure_reason: None,
            },
        )
        .await;
        if let Some(running) = run_state.running_agent_turns.get_mut(&task_id) {
            running.latest_provider_request_id = Some(request_id.clone());
            running.latest_provider_id = Some(provider_id_for_state);
            running.latest_model_id = Some(model_id_for_state);
            running
                .hook_executions
                .extend(hook_batch.hook_executions.clone());
        }
        if let Some(reason) = hook_batch.critical_failure {
            cancellation_token.cancel();
            if run_state.cancelled_running_tasks.insert(task_id.clone()) {
                append_payload_event_with_correlation(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    agent_actor(&agent_id),
                    Some(format!("task:{task_id}")),
                    Some(turn_request_id),
                    EventV1::TaskCancelled(TaskCancelledEvent {
                        task_id,
                        reason,
                        task_scope: Some(TaskTerminalScope::AgentTurn),
                    }),
                )?;
            }
        }

        Ok(())
    }

    fn agent_provider_stream_delta_internal(
        &mut self,
        task_id: String,
        agent_id: String,
        request_id: String,
        delta: String,
    ) -> Result<(), CoordinatorError> {
        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(());
        };

        let Some(turn_request_id) = run_state
            .running_agent_turns
            .get(&task_id)
            .map(|running| running.request_id.clone())
        else {
            return Ok(());
        };

        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            agent_actor(&agent_id),
            Some(format!("agent:{agent_id}")),
            Some(turn_request_id),
            EventV1::ProviderStreamDelta(crate::event::ProviderStreamDeltaEvent {
                request_id,
                delta,
            }),
        )?;

        Ok(())
    }

    fn agent_provider_reasoning_delta_internal(
        &mut self,
        task_id: String,
        agent_id: String,
        request_id: String,
        delta: String,
    ) -> Result<(), CoordinatorError> {
        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(());
        };

        let Some(turn_request_id) = run_state
            .running_agent_turns
            .get(&task_id)
            .map(|running| running.request_id.clone())
        else {
            return Ok(());
        };

        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            agent_actor(&agent_id),
            Some(format!("agent:{agent_id}")),
            Some(turn_request_id),
            EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent { request_id, delta }),
        )?;

        Ok(())
    }

    async fn agent_provider_request_finished_internal(
        &mut self,
        args: AgentProviderRequestFinishedArgs,
    ) -> Result<(), CoordinatorError> {
        let AgentProviderRequestFinishedArgs {
            task_id,
            agent_id,
            request_id,
            finish_reason,
            output_digest,
            usage,
            metadata,
        } = args;

        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(());
        };

        let Some(running) = run_state.running_agent_turns.get(&task_id) else {
            return Ok(());
        };
        let turn_request_id = running.request_id.clone();
        let category = running.category.clone();
        let cancellation_token = running.cancellation_token.clone();
        let parent_agent_id = run_state.subagent_parent_by_id.get(&agent_id).cloned();
        let usage_for_state = usage.clone();
        let metadata = provider_request_finished_metadata(metadata, &turn_request_id, &request_id);

        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            agent_actor(&agent_id),
            Some(format!("agent:{agent_id}")),
            Some(turn_request_id.clone()),
            EventV1::ProviderRequestFinished(crate::event::ProviderRequestFinishedEvent {
                request_id: request_id.clone(),
                finish_reason: finish_reason.clone(),
                output_digest: output_digest.clone(),
                usage: usage.clone(),
                metadata,
            }),
        )?;

        let hook_batch = run_lifecycle_hooks(
            self.clock.as_ref(),
            &self.config.hook_runtime_config,
            HookInvocationContext {
                event: HookLifecycleEvent::ProviderRequestFinished,
                run_id: run_state.info.run_id.clone(),
                workspace_root: run_state.info.workspace_root.clone(),
                artifacts_dir: run_state.info.artifacts_dir.clone(),
                actor: Some(agent_actor(&agent_id)),
                agent_id: Some(agent_id.clone()),
                request_id: Some(turn_request_id.clone()),
                permission_id: None,
                task_id: Some(task_id.clone()),
                tool_call_id: None,
                tool_id: None,
                provider_id: None,
                model_id: None,
                parent_agent_id,
                category,
                outcome: Some(finish_reason.clone()),
                output_summary: output_digest,
                failure_reason: None,
            },
        )
        .await;
        if let Some(running) = run_state.running_agent_turns.get_mut(&task_id) {
            running.latest_provider_usage = usage_for_state;
            running
                .hook_executions
                .extend(hook_batch.hook_executions.clone());
        }
        if let Some(reason) = hook_batch.critical_failure {
            cancellation_token.cancel();
            if run_state.cancelled_running_tasks.insert(task_id.clone()) {
                append_payload_event_with_correlation(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    agent_actor(&agent_id),
                    Some(format!("task:{task_id}")),
                    Some(turn_request_id),
                    EventV1::TaskCancelled(TaskCancelledEvent {
                        task_id,
                        reason,
                        task_scope: Some(TaskTerminalScope::AgentTurn),
                    }),
                )?;
            }
        }

        Ok(())
    }

    fn agent_assistant_message_finished_internal(
        &mut self,
        task_id: String,
        agent_id: String,
        request_id: String,
        assistant_output: String,
        tool_call_count: usize,
        assistant_message: Option<ProviderAssistantMessageMetadata>,
    ) -> Result<(), CoordinatorError> {
        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(());
        };

        let Some(turn_request_id) = run_state
            .running_agent_turns
            .get(&task_id)
            .map(|running| running.request_id.clone())
        else {
            return Ok(());
        };

        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            agent_actor(&agent_id),
            Some(format!("agent:{agent_id}")),
            Some(turn_request_id),
            EventV1::AssistantMessageFinished(crate::event::AssistantMessageFinishedEvent {
                request_id,
                tool_call_count,
                assistant_message,
            }),
        )?;

        if let Some(running) = run_state.running_agent_turns.get_mut(&task_id) {
            running.latest_assistant_output = Some(assistant_output);
        }

        Ok(())
    }

    async fn compact_agent_context_internal(
        &mut self,
        task_id: Option<&str>,
        agent_id: &str,
        through_request_id: Option<String>,
        trigger_reason: &str,
        usage: Option<harness_providers::CompletionUsage>,
    ) -> Result<CompactAgentContextResult, CoordinatorError> {
        let (existing_context, trigger, hook_context) = {
            let Some(run_state) = self.run_state.as_ref() else {
                return Err(CoordinatorError::RunNotStarted);
            };

            let existing_context = run_state
                .provider_context_by_agent
                .get(agent_id)
                .cloned()
                .unwrap_or_default();
            let manual_tokens_before = (trigger_reason == "manual")
                .then(|| approximate_provider_context_tokens(&existing_context));

            let running_turn = task_id
                .and_then(|task_id| run_state.running_agent_turns.get(task_id))
                .or_else(|| {
                    run_state.running_agent_turns.values().find(|running| {
                        running.agent_id == agent_id
                            && through_request_id
                                .as_deref()
                                .is_none_or(|request_id| running.request_id == request_id)
                    })
                });

            let trigger = if let Some(running) = running_turn {
                let prompt_tokens_estimate = (trigger_reason == "pre_prompt")
                    .then(|| approximate_text_tokens(&running.request_prompt));
                ProviderCompactionTrigger {
                    agent_id: agent_id.to_string(),
                    profile_name: running.profile_name.clone(),
                    model_ref: running.model_ref.clone(),
                    provider_id: running.latest_provider_id.clone(),
                    model_id: running.latest_model_id.clone(),
                    through_request_id,
                    trigger_reason: trigger_reason.to_string(),
                    tokens_before: usage
                        .as_ref()
                        .map(|usage| usage.prompt_tokens)
                        .or(manual_tokens_before),
                    prompt_tokens_estimate,
                    estimate_source: None,
                }
            } else {
                let profile = run_state
                    .agents
                    .get(agent_id)
                    .cloned()
                    .ok_or_else(|| CoordinatorError::UnknownAgent(agent_id.to_string()))?;
                ProviderCompactionTrigger {
                    agent_id: agent_id.to_string(),
                    profile_name: profile.name,
                    model_ref: profile.model_ref,
                    provider_id: None,
                    model_id: None,
                    through_request_id,
                    trigger_reason: trigger_reason.to_string(),
                    tokens_before: usage
                        .as_ref()
                        .map(|usage| usage.prompt_tokens)
                        .or(manual_tokens_before),
                    prompt_tokens_estimate: None,
                    estimate_source: None,
                }
            };

            let hook_context = HookInvocationContext {
                event: HookLifecycleEvent::CompactionRequested,
                run_id: run_state.info.run_id.clone(),
                workspace_root: run_state.info.workspace_root.clone(),
                artifacts_dir: run_state.info.artifacts_dir.clone(),
                actor: Some(agent_actor(agent_id)),
                agent_id: Some(agent_id.to_string()),
                request_id: trigger.through_request_id.clone(),
                permission_id: None,
                task_id: task_id.map(str::to_string),
                tool_call_id: None,
                tool_id: None,
                provider_id: trigger.provider_id.clone(),
                model_id: trigger.model_id.clone(),
                parent_agent_id: run_state.subagent_parent_by_id.get(agent_id).cloned(),
                category: Some(trigger.profile_name.clone()),
                outcome: Some(trigger.trigger_reason.clone()),
                output_summary: trigger.tokens_before.map(|tokens| tokens.to_string()),
                failure_reason: None,
            };

            (existing_context, trigger, hook_context)
        };

        let requested_hook_batch = run_lifecycle_hooks(
            self.clock.as_ref(),
            &self.config.hook_runtime_config,
            hook_context,
        )
        .await;

        if let Some(reason) = requested_hook_batch.critical_failure {
            let Some(run_state) = self.run_state.as_mut() else {
                return Err(CoordinatorError::RunNotStarted);
            };
            append_compaction_failed_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                &trigger,
                &reason,
                None,
                None,
            )?;
            return Err(CoordinatorError::LifecycleHookFailed(reason));
        }
        let summary_override = compaction_summary_override_from_hooks(&requested_hook_batch);
        let summary_decision = if let Some(summary) = summary_override {
            CompactionSummaryDecision::hook(summary)
        } else if self.config.compaction.model_backed {
            match self.model_backed_compaction_summary(&trigger).await {
                Ok(summary) => CompactionSummaryDecision::model(
                    compaction_summary_model_ref(&self.config.compaction, &trigger),
                    summary.summary,
                    false,
                    summary.split_prefix_summary,
                ),
                Err(reason) => {
                    tracing::warn!(%reason, agent_id = %trigger.agent_id, "model-backed compaction summary fell back to deterministic summary");
                    CompactionSummaryDecision::model(
                        compaction_summary_model_ref(&self.config.compaction, &trigger),
                        String::new(),
                        true,
                        None,
                    )
                }
            }
        } else {
            CompactionSummaryDecision::deterministic(&trigger)
        };

        let Some(run_state) = self.run_state.as_mut() else {
            return Err(CoordinatorError::RunNotStarted);
        };

        let updated_context = match compact_provider_context(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            &trigger,
            &self.config.compaction,
            &summary_decision,
        ) {
            Ok(Some(compaction)) => compaction,
            Ok(None) if trigger.trigger_reason == "overflow_retry" => {
                let reason = "overflow retry requested compaction, but no checkpoint reduced the active provider context"
                    .to_string();
                append_compaction_failed_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    &trigger,
                    &reason,
                    None,
                    None,
                )?;
                return Err(CoordinatorError::CompactionFailed(reason));
            }
            Ok(None) => {
                return Ok(CompactAgentContextResult::NoOp {
                    context: existing_context,
                })
            }
            Err(err) => {
                let reason = err.to_string();
                let _ = append_compaction_failed_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    &trigger,
                    &reason,
                    None,
                    None,
                );
                return Err(err);
            }
        };

        if trigger.trigger_reason == "overflow_retry" {
            if let (Some(task_id), Some(request_id)) =
                (task_id, trigger.through_request_id.as_ref())
            {
                run_state
                    .overflow_retry_compacted_context_by_attempt
                    .insert(
                        (task_id.to_string(), request_id.clone()),
                        updated_context.updated_context.clone(),
                    );
            }
        }

        Ok(CompactAgentContextResult::CheckpointWritten {
            context: updated_context.updated_context,
            checkpoint_id: updated_context.checkpoint_id,
            tokens_before_estimate: updated_context.tokens_before_estimate,
            tokens_after_estimate: updated_context.tokens_after_estimate,
        })
    }

    async fn compact_failed_terminal_agent_context(
        &mut self,
        request: FailedTerminalCompactionRequest,
    ) {
        let should_attempt = {
            let Some(run_state) = self.run_state.as_mut() else {
                return;
            };
            mark_failed_terminal_compaction_attempt(run_state, &request)
        };
        if !should_attempt {
            return;
        }

        match self
            .compact_agent_context_internal(
                Some(&request.task_id),
                &request.agent_id,
                Some(request.request_id.clone()),
                &request.trigger_reason,
                None,
            )
            .await
        {
            Ok(CompactAgentContextResult::CheckpointWritten { .. })
            | Ok(CompactAgentContextResult::NoOp { .. }) => {}
            Err(err) => {
                tracing::warn!(
                    task_id = %request.task_id,
                    agent_id = %request.agent_id,
                    request_id = %request.request_id,
                    trigger_reason = %request.trigger_reason,
                    error = %err,
                    "failed-terminal provider context compaction did not complete; preserving original task terminal outcome"
                );
            }
        }
    }

    async fn model_backed_compaction_summary(
        &self,
        trigger: &ProviderCompactionTrigger,
    ) -> Result<ModelBackedCompactionSummary, String> {
        let run_state = self
            .run_state
            .as_ref()
            .ok_or_else(|| "run is not started".to_string())?;
        model_backed_compaction_summary_for(
            self.config.provider.clone(),
            &self.config.compaction,
            run_state,
            trigger,
            self.redactor.as_ref(),
        )
        .await
    }

    fn start_continuation_internal(
        &mut self,
        actor: EventActor,
        mode: String,
        command: String,
        bounds: ContinuationBounds,
        workflow: Option<WorkflowEventMetadata>,
    ) -> Result<String, CoordinatorError> {
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        let continuation_id = format!("cont_{:06}", run_state.next_event_seq);
        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("continuation:{continuation_id}")),
            None,
            EventV1::ContinuationStarted(ContinuationStartedEvent {
                continuation_id: continuation_id.clone(),
                mode: mode.clone(),
                command: command.clone(),
                max_iterations: bounds.max_iterations,
                max_wall_clock_ms: bounds.max_wall_clock_ms,
                max_provider_calls: bounds.max_provider_calls,
                max_tool_calls: bounds.max_tool_calls,
                workflow: workflow.clone(),
            }),
        )?;
        run_state.active_continuation_id = Some(continuation_id.clone());
        run_state.active_continuation_workflow = workflow;
        run_state.continuation_controller.start_at(
            continuation_id.clone(),
            mode,
            command,
            bounds,
            self.clock.mono_ms(),
        );
        Ok(continuation_id)
    }

    fn stop_continuation_internal(
        &mut self,
        actor: EventActor,
        reason: String,
    ) -> Result<(), CoordinatorError> {
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        let continuation_id = run_state
            .active_continuation_id
            .clone()
            .unwrap_or_else(|| "none".to_string());
        let workflow = run_state.active_continuation_workflow.clone();
        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("continuation:{continuation_id}")),
            None,
            EventV1::ContinuationStopped(ContinuationStoppedEvent {
                continuation_id,
                reason,
                workflow,
            }),
        )?;
        run_state.active_continuation_id = None;
        run_state.active_continuation_workflow = None;
        run_state.continuation_controller.stop();
        Ok(())
    }

    async fn trigger_continuation_reminder_internal(
        &mut self,
        trigger: ContinuationReminderTrigger,
    ) -> Result<Option<String>, CoordinatorError> {
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;

        if agent_has_active_or_queued_turn(run_state, &trigger.agent_id) {
            return Ok(None);
        }

        let Some(profile) = run_state.agents.get(&trigger.agent_id).cloned() else {
            return Err(CoordinatorError::UnknownAgent(trigger.agent_id));
        };
        if profile.name == crate::plan::PLAN_AGENT_NAME {
            return Err(CoordinatorError::PolicyViolation(
                "continuation is disabled for the plan profile".to_string(),
            ));
        }

        let Some(active) = run_state.continuation_controller.active().cloned() else {
            return Ok(None);
        };
        let continuation_id = active.continuation_id.clone();
        run_state
            .continuation_controller
            .record_activity(trigger.provider_calls, trigger.tool_calls);
        let incomplete_todos = trigger
            .incomplete_todos
            .unwrap_or(!trigger.done_marker_seen);
        let decision = run_state.continuation_controller.queue_idle_reminder(
            incomplete_todos,
            trigger.done_marker_seen,
            self.clock.mono_ms(),
        );

        match decision {
            Some(ContinuationDecision::ReminderQueued {
                iteration,
                reminder,
            }) => {
                let workflow =
                    run_state
                        .active_continuation_workflow
                        .clone()
                        .map(|mut workflow| {
                            workflow.iteration = Some(iteration);
                            workflow
                        });
                append_payload_event_with_correlation(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    trigger.actor,
                    Some(format!("continuation:{continuation_id}")),
                    None,
                    EventV1::ContinuationReminderQueued(ContinuationReminderQueuedEvent {
                        continuation_id,
                        iteration,
                        reminder: reminder.clone(),
                        reason: trigger.reason,
                        workflow,
                    }),
                )?;
                let request_id = allocate_provider_request_id(run_state);
                let prompt = continuation_reminder_prompt(
                    &active.mode,
                    iteration,
                    &reminder,
                    &active.command,
                );
                schedule_agent_turn(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    self.job_tx.clone(),
                    run_state,
                    self.config.hook_runtime_config.clone(),
                    self.config.compaction.clone(),
                    ScheduleAgentTurnArgs {
                        provider: self.config.provider.clone(),
                        tool_registry: self.config.tool_registry.clone(),
                        profile: profile.clone(),
                        request: AgentRequest {
                            agent_id: trigger.agent_id,
                            prompt,
                            prompt_context: None,
                            selected_file_tags: Vec::new(),
                            selected_agent_tags: Vec::new(),
                            selected_resource_tags: Vec::new(),
                            model_ref: profile.model_ref.clone(),
                            fallback_model_refs: profile.fallback_model_refs.clone(),
                            fallback_model_settings: profile.fallback_model_settings.clone(),
                            model_settings: default_model_settings_for_profile(&profile.name),
                        },
                        request_id: request_id.clone(),
                        child_task: None,
                    },
                )
                .await?;
                Ok(Some(request_id))
            }
            Some(ContinuationDecision::LimitReached { limit, iteration }) => {
                let workflow =
                    run_state
                        .active_continuation_workflow
                        .clone()
                        .map(|mut workflow| {
                            workflow.iteration = Some(iteration);
                            workflow.stop_reason = Some(limit.to_string());
                            workflow
                        });
                append_payload_event_with_correlation(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    trigger.actor,
                    Some(format!("continuation:{continuation_id}")),
                    None,
                    EventV1::ContinuationLimitReached(ContinuationLimitReachedEvent {
                        continuation_id,
                        limit: limit.to_string(),
                        iteration,
                        workflow,
                    }),
                )?;
                run_state.active_continuation_id = None;
                run_state.active_continuation_workflow = None;
                Ok(None)
            }
            Some(ContinuationDecision::Stopped) => {
                let reason = if trigger.done_marker_seen {
                    "done_marker"
                } else {
                    "todos_completed"
                };
                let workflow =
                    run_state
                        .active_continuation_workflow
                        .clone()
                        .map(|mut workflow| {
                            workflow.stop_reason = Some(reason.to_string());
                            workflow
                        });
                append_payload_event_with_correlation(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    trigger.actor,
                    Some(format!("continuation:{continuation_id}")),
                    None,
                    EventV1::ContinuationStopped(ContinuationStoppedEvent {
                        continuation_id,
                        reason: reason.to_string(),
                        workflow,
                    }),
                )?;
                run_state.active_continuation_id = None;
                run_state.active_continuation_workflow = None;
                Ok(None)
            }
            None => Ok(None),
        }
    }

    fn queue_continuation_reminder_internal(
        &mut self,
        actor: EventActor,
        continuation_id: String,
        iteration: u32,
        reminder: String,
        reason: String,
    ) -> Result<(), CoordinatorError> {
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        let workflow = (run_state.active_continuation_id.as_deref() == Some(&continuation_id))
            .then(|| run_state.active_continuation_workflow.clone())
            .flatten()
            .map(|mut workflow| {
                workflow.iteration = Some(iteration);
                workflow
            });
        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("continuation:{continuation_id}")),
            None,
            EventV1::ContinuationReminderQueued(ContinuationReminderQueuedEvent {
                continuation_id,
                iteration,
                reminder,
                reason,
                workflow,
            }),
        )?;
        Ok(())
    }

    fn reach_continuation_limit_internal(
        &mut self,
        actor: EventActor,
        continuation_id: String,
        limit: String,
        iteration: u32,
    ) -> Result<(), CoordinatorError> {
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        let workflow = (run_state.active_continuation_id.as_deref() == Some(&continuation_id))
            .then(|| run_state.active_continuation_workflow.clone())
            .flatten()
            .map(|mut workflow| {
                workflow.iteration = Some(iteration);
                workflow.stop_reason = Some(limit.clone());
                workflow
            });
        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("continuation:{continuation_id}")),
            None,
            EventV1::ContinuationLimitReached(ContinuationLimitReachedEvent {
                continuation_id,
                limit,
                iteration,
                workflow,
            }),
        )?;
        run_state.active_continuation_id = None;
        run_state.active_continuation_workflow = None;
        run_state.continuation_controller.stop();
        Ok(())
    }

    async fn switch_agent_turn_provider_model_slot_internal(
        &mut self,
        task_id: String,
        agent_id: String,
        model_ref: String,
        model_settings: AgentModelSettings,
    ) -> Result<bool, CoordinatorError> {
        let model = crate::agent::AgentModelRef::parse(&model_ref);
        let provider_id = model.provider_id;
        let model_id = model.model_id;
        let base_new_key = ConcurrencyKey::ProviderModel {
            provider_id: provider_id.clone(),
            model_id: model_id.clone(),
        };
        let dequeued = {
            let Some(run_state) = self.run_state.as_mut() else {
                return Err(CoordinatorError::RunNotStarted);
            };
            let new_key = nested_provider_model_queue_key(
                run_state,
                &agent_id,
                provider_id,
                model_id,
                base_new_key,
            );
            let Some(running) = run_state.running_agent_turns.get(&task_id) else {
                return Err(CoordinatorError::UnknownTask(task_id));
            };
            if running.agent_id != agent_id {
                return Err(CoordinatorError::PolicyViolation(format!(
                    "agent turn `{task_id}` is not owned by agent `{agent_id}`"
                )));
            }
            let old_key = running.queue_key.clone();
            if old_key == new_key {
                if let Some(running) = run_state.running_agent_turns.get_mut(&task_id) {
                    running.model_ref = model_ref;
                    running.model_settings = model_settings;
                }
                return Ok(true);
            }

            match run_state
                .scheduler
                .schedule(task_id.clone(), new_key.clone())
            {
                ScheduleDecision::Started(_) => {
                    let dequeued = run_state.scheduler.complete(&old_key);
                    if let Some(running) = run_state.running_agent_turns.get_mut(&task_id) {
                        running.queue_key = new_key;
                        running.model_ref = model_ref;
                        running.model_settings = model_settings;
                    }
                    dequeued
                }
                ScheduleDecision::Queued(_) => {
                    let _ = run_state.scheduler.cancel_queued(&task_id);
                    return Ok(false);
                }
            }
        };

        if !dequeued.is_empty() {
            let Some(run_state) = self.run_state.as_mut() else {
                return Err(CoordinatorError::RunNotStarted);
            };
            for task in dequeued {
                if let Some(queued) = run_state.queued_agent_turns.get(&task.task_id).cloned() {
                    append_agent_turn_task_scheduled_event(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        AgentTurnTaskScheduledEventArgs {
                            task_id: &queued.task_id,
                            agent_id: &queued.agent_id,
                            request_id: &queued.request_id,
                            queue_key: &queued.queue_key,
                            state: TaskScheduleState::Started,
                        },
                    )?;

                    let Some(queued) = run_state.queued_agent_turns.remove(&task.task_id) else {
                        continue;
                    };
                    start_agent_turn_execution(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        self.job_tx.clone(),
                        run_state,
                        self.config.hook_runtime_config.clone(),
                        self.config.compaction.clone(),
                        self.config.provider.clone(),
                        self.config.tool_registry.clone(),
                        queued,
                    )
                    .await?;
                }
            }
        }

        Ok(true)
    }

    async fn promote_next_agent_blocked_turn(
        &mut self,
        agent_id: &str,
    ) -> Result<(), CoordinatorError> {
        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(());
        };
        if run_state
            .running_agent_turns
            .values()
            .any(|running| running.agent_id == agent_id)
        {
            return Ok(());
        }

        let Some(blocked_task_id) = next_agent_blocked_turn_id(run_state, agent_id) else {
            return Ok(());
        };
        let Some(queued) = run_state.queued_agent_turns.get(&blocked_task_id).cloned() else {
            return Ok(());
        };

        match run_state
            .scheduler
            .schedule(blocked_task_id.clone(), queued.queue_key.clone())
        {
            ScheduleDecision::Started(_) => {
                append_agent_turn_task_scheduled_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    AgentTurnTaskScheduledEventArgs {
                        task_id: &queued.task_id,
                        agent_id: &queued.agent_id,
                        request_id: &queued.request_id,
                        queue_key: &queued.queue_key,
                        state: TaskScheduleState::Started,
                    },
                )?;

                let Some(queued) = run_state.queued_agent_turns.remove(&blocked_task_id) else {
                    return Ok(());
                };
                start_agent_turn_execution(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    self.job_tx.clone(),
                    run_state,
                    self.config.hook_runtime_config.clone(),
                    self.config.compaction.clone(),
                    self.config.provider.clone(),
                    self.config.tool_registry.clone(),
                    queued,
                )
                .await?;
            }
            ScheduleDecision::Queued(_) => {
                if let Some(queued) = run_state.queued_agent_turns.get_mut(&blocked_task_id) {
                    queued.scheduler_queued = true;
                }
            }
        }

        Ok(())
    }

    async fn agent_turn_finished_internal(
        &mut self,
        task_id: String,
        _agent_id: String,
        request_id: String,
        outcome: AgentTurnTaskOutcome,
    ) -> Result<(), CoordinatorError> {
        let (
            dequeued,
            terminal_compaction,
            finished_agent_id,
            continuation_should_check,
            continuation_done_marker_seen,
            continuation_incomplete_todos,
            continuation_tool_calls,
        ) = {
            let Some(run_state) = self.run_state.as_mut() else {
                return Ok(());
            };

            let Some(running) = run_state.running_agent_turns.remove(&task_id) else {
                return Ok(());
            };

            let finished_agent_id = running.agent_id.clone();
            let continuation_should_check =
                matches!(&outcome, AgentTurnTaskOutcome::Succeeded { .. });
            let continuation_done_marker_seen = match &outcome {
                AgentTurnTaskOutcome::Succeeded { output, .. } => {
                    continuation_done_marker_seen(output)
                }
                AgentTurnTaskOutcome::Failed { .. } => true,
            };
            let continuation_incomplete_todos =
                continuation_incomplete_todos_for_request(run_state, &request_id);
            let continuation_tool_calls =
                continuation_tool_call_count_for_request(run_state, &request_id);
            let was_cancelled = run_state.cancelled_running_tasks.remove(&task_id);
            let dequeued = run_state.scheduler.complete(&running.queue_key);
            let finished_mono_ms = self.clock.mono_ms();
            let subagent_parent_id = run_state
                .subagent_parent_by_id
                .get(&running.agent_id)
                .cloned();
            let (hook_outcome, hook_output_summary, hook_failure_reason) = match &outcome {
                AgentTurnTaskOutcome::Succeeded { output, .. } => {
                    ("succeeded".to_string(), Some(output.clone()), None)
                }
                AgentTurnTaskOutcome::Failed { reason, .. } => {
                    ("failed".to_string(), None, Some(reason.clone()))
                }
            };
            let finished_hook_batch = run_lifecycle_hooks(
                self.clock.as_ref(),
                &self.config.hook_runtime_config,
                HookInvocationContext {
                    event: HookLifecycleEvent::AgentTurnFinished,
                    run_id: run_state.info.run_id.clone(),
                    workspace_root: run_state.info.workspace_root.clone(),
                    artifacts_dir: run_state.info.artifacts_dir.clone(),
                    actor: Some(agent_actor(&running.agent_id)),
                    agent_id: Some(running.agent_id.clone()),
                    request_id: Some(request_id.clone()),
                    permission_id: None,
                    task_id: Some(task_id.clone()),
                    tool_call_id: None,
                    tool_id: None,
                    provider_id: None,
                    model_id: None,
                    parent_agent_id: None,
                    category: running.category.clone(),
                    outcome: Some(hook_outcome.clone()),
                    output_summary: hook_output_summary.clone(),
                    failure_reason: hook_failure_reason.clone(),
                },
            )
            .await;
            let mut hook_executions = running.hook_executions.clone();
            hook_executions.extend(finished_hook_batch.hook_executions.clone());
            let mut critical_hook_failure = finished_hook_batch.critical_failure.clone();

            if let Some(parent_agent_id) = subagent_parent_id {
                let subagent_finished_hook_batch = run_lifecycle_hooks(
                    self.clock.as_ref(),
                    &self.config.hook_runtime_config,
                    HookInvocationContext {
                        event: HookLifecycleEvent::SubagentFinished,
                        run_id: run_state.info.run_id.clone(),
                        workspace_root: run_state.info.workspace_root.clone(),
                        artifacts_dir: run_state.info.artifacts_dir.clone(),
                        actor: Some(agent_actor(&running.agent_id)),
                        agent_id: Some(running.agent_id.clone()),
                        request_id: Some(request_id.clone()),
                        permission_id: None,
                        task_id: Some(task_id.clone()),
                        tool_call_id: None,
                        tool_id: None,
                        provider_id: None,
                        model_id: None,
                        parent_agent_id: Some(parent_agent_id),
                        category: running.category.clone(),
                        outcome: Some(hook_outcome),
                        output_summary: hook_output_summary,
                        failure_reason: hook_failure_reason,
                    },
                )
                .await;
                hook_executions.extend(subagent_finished_hook_batch.hook_executions.clone());
                if let Some(reason) = subagent_finished_hook_batch.critical_failure {
                    critical_hook_failure = Some(match critical_hook_failure {
                        Some(existing) => format!("{existing}; {reason}"),
                        None => reason,
                    });
                }
            }

            let mut terminal_compaction = None;

            if was_cancelled {
                let memory = match &outcome {
                    AgentTurnTaskOutcome::Failed { reason, memory } => memory
                        .clone()
                        .or_else(|| cancelled_failure_memory_from_running(&running, reason)),
                    AgentTurnTaskOutcome::Succeeded { .. } => {
                        cancelled_failure_memory_from_running(&running, "job cancelled")
                    }
                };
                let has_incomplete_memory = memory.is_some();
                if let Some(memory) = memory {
                    push_incomplete_provider_turn(run_state, &running, &request_id, memory);
                }
                if has_incomplete_memory {
                    terminal_compaction = Some(FailedTerminalCompactionRequest::new(
                        task_id.clone(),
                        running.agent_id.clone(),
                        request_id.clone(),
                        "aborted_response",
                    ));
                }
            } else {
                match outcome {
                    AgentTurnTaskOutcome::Succeeded { output, messages } => {
                        if let Some(reason) = critical_hook_failure.clone() {
                            push_incomplete_provider_turn(
                                run_state,
                                &running,
                                &request_id,
                                AgentTurnFailureMemory::failed(
                                    "hook_failure",
                                    reason.clone(),
                                    output.clone(),
                                    running.latest_provider_request_id.clone(),
                                ),
                            );
                            let terminal_event = append_payload_event_with_correlation(
                                self.clock.as_ref(),
                                self.redactor.as_ref(),
                                run_state,
                                agent_actor(&running.agent_id),
                                Some(format!("task:{task_id}")),
                                Some(request_id.clone()),
                                EventV1::TaskCancelled(TaskCancelledEvent {
                                    task_id: task_id.clone(),
                                    reason,
                                    task_scope: Some(TaskTerminalScope::AgentTurn),
                                }),
                            )?;
                            append_background_task_notification_and_schedule(
                                self.clock.as_ref(),
                                self.redactor.as_ref(),
                                self.job_tx.clone(),
                                run_state,
                                self.config.hook_runtime_config.clone(),
                                self.config.compaction.clone(),
                                self.config.provider.clone(),
                                self.config.tool_registry.clone(),
                                running.child_task.clone(),
                                &terminal_event,
                                BackgroundTaskNotificationStatus::Failed,
                                &terminal_event_summary(&terminal_event),
                            )
                            .await?;
                            terminal_compaction = Some(FailedTerminalCompactionRequest::new(
                                task_id.clone(),
                                running.agent_id.clone(),
                                request_id.clone(),
                                "failed_response",
                            ));
                        } else {
                            let lineage =
                                agent_turn_child_lineage(run_state, &running, &request_id);
                            run_state
                                .provider_context_by_agent
                                .entry(running.agent_id.clone())
                                .or_default()
                                .push_turn(ProviderConversationTurn {
                                    user_prompt: running.request_prompt.clone(),
                                    assistant_response: output.clone(),
                                    request_id: Some(request_id.clone()),
                                    first_seq: None,
                                    last_seq: None,
                                    artifacts: Vec::new(),
                                    messages,
                                    ..ProviderConversationTurn::default()
                                });
                            let terminal_event = append_payload_event_with_correlation(
                                self.clock.as_ref(),
                                self.redactor.as_ref(),
                                run_state,
                                agent_actor(&running.agent_id),
                                Some(format!("task:{task_id}")),
                                Some(request_id.clone()),
                                EventV1::TaskCompleted(TaskCompletedEvent {
                                    task_id,
                                    result_digest: digest12(output.as_bytes()),
                                    result_summary: output,
                                    metadata: Some(TaskCompletionMetadata {
                                        lineage,
                                        route: running
                                            .child_task
                                            .as_ref()
                                            .and_then(|child_task| child_task.route.clone()),
                                        task_scope: Some(TaskTerminalScope::AgentTurn),
                                        timing: Some(execution_timing_metadata(
                                            running.started_mono_ms,
                                            finished_mono_ms,
                                        )),
                                        hook_executions,
                                    }),
                                }),
                            )?;
                            append_background_task_notification_and_schedule(
                                self.clock.as_ref(),
                                self.redactor.as_ref(),
                                self.job_tx.clone(),
                                run_state,
                                self.config.hook_runtime_config.clone(),
                                self.config.compaction.clone(),
                                self.config.provider.clone(),
                                self.config.tool_registry.clone(),
                                running.child_task.clone(),
                                &terminal_event,
                                BackgroundTaskNotificationStatus::Completed,
                                &terminal_event_summary(&terminal_event),
                            )
                            .await?;

                            let proactive_trigger = ProviderCompactionTrigger {
                                agent_id: running.agent_id.clone(),
                                profile_name: running.profile_name.clone(),
                                model_ref: running.model_ref.clone(),
                                provider_id: running.latest_provider_id.clone(),
                                model_id: running.latest_model_id.clone(),
                                through_request_id: Some(request_id.clone()),
                                trigger_reason: "proactive".to_string(),
                                tokens_before: running
                                    .latest_provider_usage
                                    .as_ref()
                                    .map(|usage| usage.prompt_tokens),
                                prompt_tokens_estimate: None,
                                estimate_source: None,
                            };
                            let summary_decision = if self.config.compaction.model_backed {
                                match model_backed_compaction_summary_for(
                                    self.config.provider.clone(),
                                    &self.config.compaction,
                                    run_state,
                                    &proactive_trigger,
                                    self.redactor.as_ref(),
                                )
                                .await
                                {
                                    Ok(summary) => CompactionSummaryDecision::model(
                                        compaction_summary_model_ref(
                                            &self.config.compaction,
                                            &proactive_trigger,
                                        ),
                                        summary.summary,
                                        false,
                                        summary.split_prefix_summary,
                                    ),
                                    Err(reason) => {
                                        tracing::warn!(%reason, agent_id = %running.agent_id, "model-backed proactive compaction summary fell back to deterministic summary");
                                        CompactionSummaryDecision::model(
                                            compaction_summary_model_ref(
                                                &self.config.compaction,
                                                &proactive_trigger,
                                            ),
                                            String::new(),
                                            true,
                                            None,
                                        )
                                    }
                                }
                            } else {
                                CompactionSummaryDecision::deterministic(&proactive_trigger)
                            };
                            if let Err(err) = compact_provider_context(
                                self.clock.as_ref(),
                                self.redactor.as_ref(),
                                run_state,
                                &proactive_trigger,
                                &self.config.compaction,
                                &summary_decision,
                            ) {
                                tracing::warn!(
                                    agent_id = %running.agent_id,
                                    error = %err,
                                    "provider context compaction failed after successful agent turn"
                                );
                            }
                        }
                    }
                    AgentTurnTaskOutcome::Failed { reason, memory } => {
                        let reason = match critical_hook_failure.clone() {
                            Some(hook_reason) => {
                                format!("{reason}; critical lifecycle hook failed: {hook_reason}")
                            }
                            None => reason,
                        };
                        let mut memory = memory.or_else(|| {
                            critical_hook_failure.clone().map(|_| {
                                AgentTurnFailureMemory::failed(
                                    "hook_failure",
                                    reason.clone(),
                                    "",
                                    running.latest_provider_request_id.clone(),
                                )
                            })
                        });
                        if let Some(memory) = &mut memory {
                            memory.failure_reason = reason.clone();
                        }
                        let terminal_trigger_reason = memory
                            .as_ref()
                            .filter(|memory| {
                                memory.status == ProviderConversationTurnStatus::Aborted
                            })
                            .map(|_| "aborted_response")
                            .unwrap_or("failed_response");
                        let has_incomplete_memory = memory.is_some();
                        if let Some(memory) = memory {
                            push_incomplete_provider_turn(run_state, &running, &request_id, memory);
                        }
                        let terminal_event = append_payload_event_with_correlation(
                            self.clock.as_ref(),
                            self.redactor.as_ref(),
                            run_state,
                            agent_actor(&running.agent_id),
                            Some(format!("task:{task_id}")),
                            Some(request_id.clone()),
                            EventV1::TaskCancelled(TaskCancelledEvent {
                                task_id: task_id.clone(),
                                reason: reason.clone(),
                                task_scope: Some(TaskTerminalScope::AgentTurn),
                            }),
                        )?;
                        append_background_task_notification_and_schedule(
                            self.clock.as_ref(),
                            self.redactor.as_ref(),
                            self.job_tx.clone(),
                            run_state,
                            self.config.hook_runtime_config.clone(),
                            self.config.compaction.clone(),
                            self.config.provider.clone(),
                            self.config.tool_registry.clone(),
                            running.child_task.clone(),
                            &terminal_event,
                            background_notification_status_for_cancel_reason(&reason),
                            &reason,
                        )
                        .await?;
                        if has_incomplete_memory {
                            terminal_compaction = Some(FailedTerminalCompactionRequest::new(
                                task_id.clone(),
                                running.agent_id.clone(),
                                request_id.clone(),
                                terminal_trigger_reason,
                            ));
                        }
                    }
                }
            }

            (
                dequeued,
                terminal_compaction,
                finished_agent_id,
                continuation_should_check && !was_cancelled,
                continuation_done_marker_seen,
                continuation_incomplete_todos,
                continuation_tool_calls,
            )
        };

        if let Some(request) = terminal_compaction {
            self.compact_failed_terminal_agent_context(request).await;
        }

        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(());
        };

        for task in dequeued {
            if let Some(queued) = run_state.queued_agent_turns.get(&task.task_id).cloned() {
                append_agent_turn_task_scheduled_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    AgentTurnTaskScheduledEventArgs {
                        task_id: &queued.task_id,
                        agent_id: &queued.agent_id,
                        request_id: &queued.request_id,
                        queue_key: &queued.queue_key,
                        state: TaskScheduleState::Started,
                    },
                )?;

                let Some(queued) = run_state.queued_agent_turns.remove(&task.task_id) else {
                    continue;
                };
                start_agent_turn_execution(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    self.job_tx.clone(),
                    run_state,
                    self.config.hook_runtime_config.clone(),
                    self.config.compaction.clone(),
                    self.config.provider.clone(),
                    self.config.tool_registry.clone(),
                    queued,
                )
                .await?;
            }
        }

        schedule_pending_agent_wakeups_for_idle_agent(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            self.job_tx.clone(),
            run_state,
            self.config.hook_runtime_config.clone(),
            self.config.compaction.clone(),
            self.config.provider.clone(),
            self.config.tool_registry.clone(),
            &finished_agent_id,
        )
        .await?;

        self.promote_next_agent_blocked_turn(&finished_agent_id)
            .await?;

        if continuation_should_check {
            self.trigger_continuation_reminder_internal(ContinuationReminderTrigger {
                actor: system_actor(),
                agent_id: finished_agent_id,
                reason: format!("agent_turn_finished:{request_id}"),
                done_marker_seen: continuation_done_marker_seen,
                incomplete_todos: continuation_incomplete_todos,
                provider_calls: 1,
                tool_calls: continuation_tool_calls,
            })
            .await?;
        }

        Ok(())
    }
}

fn continuation_reminder_prompt(
    mode: &str,
    iteration: u32,
    reminder: &str,
    command: &str,
) -> String {
    let command = command.trim();
    format!(
        "[CONTINUATION {mode} iteration {iteration}]\n{reminder}\n\nContinuation command: {command}\n\nContinue the requested work from that continuation command through the normal coordinator/tool flow. If every required task is complete, respond with `DONE` and do not start new work."
    )
}

fn continuation_done_marker_seen(output: &str) -> bool {
    output.lines().any(|line| {
        let normalized = line
            .trim()
            .trim_matches(|ch: char| ch.is_ascii_punctuation() || matches!(ch, '✅' | '✓' | '✔'))
            .trim()
            .to_ascii_lowercase();
        matches!(
            normalized.as_str(),
            "done"
                | "complete"
                | "completed"
                | "all done"
                | "all tasks complete"
                | "no pending work"
        )
    })
}

fn continuation_tool_call_count_for_request(run_state: &RunState, request_id: &str) -> u32 {
    let through_seq = run_state.next_event_seq.saturating_sub(1);
    let Ok(events) = read_historical_events_until(
        &run_state.info.run_id,
        &run_state.info.events_path,
        through_seq,
    ) else {
        return 0;
    };
    events
        .iter()
        .filter(|event| event.correlation_id.as_deref() == Some(request_id))
        .filter(|event| matches!(event.payload, EventV1::ToolCallFinished(_)))
        .count()
        .try_into()
        .unwrap_or(u32::MAX)
}

fn continuation_incomplete_todos_for_request(
    run_state: &RunState,
    request_id: &str,
) -> Option<bool> {
    let through_seq = run_state.next_event_seq.saturating_sub(1);
    let events = read_historical_events_until(
        &run_state.info.run_id,
        &run_state.info.events_path,
        through_seq,
    )
    .ok()?;
    events
        .iter()
        .filter(|event| event.correlation_id.as_deref() == Some(request_id))
        .filter_map(|event| match &event.payload {
            EventV1::ToolCallFinished(payload) => payload.output_json.as_ref(),
            _ => None,
        })
        .filter_map(todo_output_has_incomplete_items)
        .next_back()
}

fn todo_output_has_incomplete_items(value: &Value) -> Option<bool> {
    let todos = value.get("todos").and_then(Value::as_array)?;
    if todos.is_empty() {
        return Some(false);
    }
    Some(todos.iter().any(|todo| {
        let status = todo
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        !matches!(status, "completed" | "done" | "cancelled")
    }))
}

struct RunState {
    info: RunInfo,
    event_store: Arc<JsonlFileEventStore>,
    next_event_seq: u64,
    next_agent_id: u64,
    next_tool_call_id: u64,
    next_task_id: u64,
    next_provider_request_id: u64,
    next_permission_id: u64,
    agents: BTreeMap<String, AgentProfile>,
    provider_context_by_agent: BTreeMap<String, ProviderContext>,
    tasks: BTreeMap<String, TaskState>,
    task_hook_state: BTreeMap<String, TaskHookState>,
    agent_hook_state: BTreeMap<String, Vec<HookExecutionMetadata>>,
    subagent_parent_by_id: BTreeMap<String, String>,
    child_session_mirrors: BTreeMap<String, ChildSessionMirror>,
    child_request_session_by_id: BTreeMap<String, String>,
    background_notification_child_requests: BTreeSet<String>,
    pending_agent_wakeups: BTreeMap<String, Vec<PendingAgentWakeup>>,
    pending_permissions: BTreeMap<String, PendingPermissionState>,
    active_permission_grants: PermissionGrantSet,
    cancelled_running_tasks: BTreeSet<String>,
    queued_agent_turns: BTreeMap<String, QueuedAgentTurn>,
    running_agent_turns: BTreeMap<String, RunningAgentTurn>,
    failed_terminal_compaction_attempts: BTreeSet<(String, String)>,
    overflow_retry_compacted_context_by_attempt: BTreeMap<(String, String), ProviderContext>,
    active_continuation_id: Option<String>,
    active_continuation_workflow: Option<WorkflowEventMetadata>,
    continuation_controller: ContinuationController,
    scheduler: Scheduler,
    recorded_runtime_context: Option<RecordedRuntimeContext>,
    allow_initial_runtime_context_recording: bool,
    shutdown_token: CancellationToken,
}

#[derive(Debug)]
struct ChildSessionMirror {
    event_store: Arc<JsonlFileEventStore>,
    append_parent_finish: bool,
}

#[derive(Debug, Clone)]
struct QueuedAgentTurn {
    task_id: String,
    agent_id: String,
    request_id: String,
    profile: AgentProfile,
    request: AgentRequest,
    queue_key: ConcurrencyKey,
    scheduler_queued: bool,
    child_task: Option<ChildTaskTurnState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChildTaskTurnState {
    parent_tool_call_id: String,
    parent_session_id: String,
    parent_agent_id: Option<String>,
    child_session_id: String,
    child_request_id: String,
    task_id: String,
    description: String,
    run_in_background: bool,
    route: Option<TaskRouteMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingAgentWakeup {
    request_id: String,
    notification_text: String,
}

struct ContinuationReminderTrigger {
    actor: EventActor,
    agent_id: String,
    reason: String,
    done_marker_seen: bool,
    incomplete_todos: Option<bool>,
    provider_calls: u32,
    tool_calls: u32,
}

#[derive(Debug, Clone)]
struct RunningAgentTurn {
    agent_id: String,
    request_id: String,
    request_prompt: String,
    profile_name: String,
    model_ref: String,
    model_settings: AgentModelSettings,
    category: Option<String>,
    queue_key: ConcurrencyKey,
    cancellation_token: CancellationToken,
    started_mono_ms: u64,
    hook_executions: Vec<HookExecutionMetadata>,
    latest_provider_usage: Option<harness_providers::CompletionUsage>,
    latest_provider_request_id: Option<String>,
    latest_assistant_output: Option<String>,
    latest_provider_id: Option<String>,
    latest_model_id: Option<String>,
    child_task: Option<ChildTaskTurnState>,
}

fn cancelled_failure_memory_from_running(
    running: &RunningAgentTurn,
    reason: &str,
) -> Option<AgentTurnFailureMemory> {
    let provider_request_id = running.latest_provider_request_id.clone()?;
    Some(AgentTurnFailureMemory::aborted(
        "cancelled",
        reason.to_string(),
        running.latest_assistant_output.clone().unwrap_or_default(),
        Some(provider_request_id),
    ))
}

fn push_incomplete_provider_turn(
    run_state: &mut RunState,
    running: &RunningAgentTurn,
    fallback_request_id: &str,
    memory: AgentTurnFailureMemory,
) {
    let request_id = memory
        .provider_request_id
        .clone()
        .or_else(|| running.latest_provider_request_id.clone())
        .unwrap_or_else(|| fallback_request_id.to_string());
    let context = run_state
        .provider_context_by_agent
        .entry(running.agent_id.clone())
        .or_default();
    if context.preserved_turns.iter().any(|turn| {
        turn.request_id.as_deref() == Some(request_id.as_str()) && !turn.status.is_completed()
    }) {
        return;
    }

    context.push_turn(ProviderConversationTurn {
        user_prompt: running.request_prompt.clone(),
        assistant_response: memory.partial_assistant_output,
        status: memory.status,
        failure_stage: Some(memory.failure_stage),
        failure_reason: truncated_failure_reason(&memory.failure_reason),
        request_id: Some(request_id),
        first_seq: None,
        last_seq: None,
        artifacts: Vec::new(),
        messages: Vec::new(),
    });
}

fn truncated_failure_reason(reason: &str) -> Option<String> {
    let reason = reason.trim();
    if reason.is_empty() {
        None
    } else {
        Some(truncate_with_ellipsis(
            reason,
            PROVIDER_CONTEXT_COMPACTION_TURN_EXCERPT_MAX_CHARS,
        ))
    }
}

fn agent_turn_child_lineage(
    run_state: &RunState,
    running: &RunningAgentTurn,
    request_id: &str,
) -> Option<TaskLineageMetadata> {
    if let Some(child_task) = running.child_task.as_ref() {
        return Some(TaskLineageMetadata {
            parent_tool_call_id: Some(child_task.parent_tool_call_id.clone()),
            parent_task_id: None,
            parent_request_id: None,
            parent_session_id: Some(child_task.parent_session_id.clone()),
            child_session_id: Some(child_task.child_session_id.clone()),
            child_request_id: Some(child_task.child_request_id.clone()),
            child_provider_id: running.latest_provider_id.clone(),
            child_model_id: running.latest_model_id.clone(),
        });
    }

    run_state
        .child_session_mirrors
        .contains_key(&running.agent_id)
        .then(|| TaskLineageMetadata {
            parent_session_id: Some(run_state.info.run_id.clone()),
            child_session_id: Some(running.agent_id.clone()),
            child_request_id: Some(request_id.to_string()),
            ..TaskLineageMetadata::default()
        })
}

fn reject_nested_team_create(
    actor: &EventActor,
    projection: &TeamProjection,
) -> Result<(), CoordinatorError> {
    let Some(agent_id) = actor.agent_id.as_deref() else {
        return Ok(());
    };
    let is_team_member = projection.teams.values().any(|team| {
        team.members
            .values()
            .any(|member| member.agent_id.as_deref() == Some(agent_id))
    });
    if is_team_member {
        return Err(CoordinatorError::PolicyViolation(
            "team members cannot create nested teams".to_string(),
        ));
    }
    Ok(())
}

fn validate_team_spec(spec: &TeamSpec) -> Result<(), CoordinatorError> {
    if spec.version != 1 {
        return Err(CoordinatorError::PolicyViolation(
            "team spec version must be 1".to_string(),
        ));
    }
    if non_empty_trimmed(&spec.name).is_none() {
        return Err(CoordinatorError::PolicyViolation(
            "team name cannot be empty".to_string(),
        ));
    }
    validate_team_text_field("team name", &spec.name)?;
    if let Some(description) = spec.description.as_deref() {
        validate_team_text_field("team description", description)?;
    }
    if spec.members.is_empty() || spec.members.len() > TEAM_MAX_MEMBERS {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team must have between 1 and {TEAM_MAX_MEMBERS} members"
        )));
    }
    if spec.bounds.max_members == 0 || spec.bounds.max_members as usize > TEAM_MAX_MEMBERS {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team max_members must be between 1 and {TEAM_MAX_MEMBERS}"
        )));
    }
    if spec.bounds.max_parallel_members == 0
        || spec.bounds.max_parallel_members > spec.bounds.max_members
    {
        return Err(CoordinatorError::PolicyViolation(
            "team max_parallel_members must be between 1 and max_members".to_string(),
        ));
    }
    if spec.bounds.max_messages_per_run == 0 {
        return Err(CoordinatorError::PolicyViolation(
            "team max_messages_per_run must be greater than zero".to_string(),
        ));
    }
    if spec.bounds.max_wall_clock_minutes == 0 || spec.bounds.max_member_turns == 0 {
        return Err(CoordinatorError::PolicyViolation(
            "team wall-clock and member-turn bounds must be greater than zero".to_string(),
        ));
    }
    if spec.members.len() > spec.bounds.max_members as usize {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team member count exceeds max_members bound {}",
            spec.bounds.max_members
        )));
    }
    let mut names = BTreeSet::new();
    for member in spec.members.iter() {
        if non_empty_trimmed(&member.name).is_none() {
            return Err(CoordinatorError::PolicyViolation(
                "team member name cannot be empty".to_string(),
            ));
        }
        if matches!(member.name.as_str(), "lead" | "*") {
            return Err(CoordinatorError::PolicyViolation(format!(
                "team member name `{}` is reserved",
                member.name
            )));
        }
        validate_team_text_field("team member name", &member.name)?;
        if let Some(prompt) = member.prompt.as_deref() {
            validate_team_text_field("team member prompt", prompt)?;
        }
        if !names.insert(member.name.clone()) {
            return Err(CoordinatorError::PolicyViolation(format!(
                "duplicate team member `{}`",
                member.name
            )));
        }
    }
    Ok(())
}

fn validate_team_text_field(label: &str, value: &str) -> Result<(), CoordinatorError> {
    if value.chars().count() > TEAM_TEXT_FIELD_MAX_CHARS {
        return Err(CoordinatorError::PolicyViolation(format!(
            "{label} exceeds {TEAM_TEXT_FIELD_MAX_CHARS} characters"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum TeamParticipantRole {
    Lead,
    Member(TeamMemberRole),
}

fn validate_team_profile_role(
    profile: &str,
    profile_config: Option<&AgentProfile>,
    role: TeamParticipantRole,
) -> Result<(), CoordinatorError> {
    let read_only = is_read_only_team_profile(profile, profile_config);
    match role {
        TeamParticipantRole::Lead if read_only => Err(CoordinatorError::PolicyViolation(format!(
            "team lead profile `{profile}` is read-only or planning-only"
        ))),
        TeamParticipantRole::Member(TeamMemberRole::Member) if read_only => {
            Err(CoordinatorError::PolicyViolation(format!(
                "team member profile `{profile}` is read-only or planning-only; mark the member role as research or use task delegation for ad hoc research"
            )))
        }
        TeamParticipantRole::Member(TeamMemberRole::Research) if !read_only => {
            Err(CoordinatorError::PolicyViolation(format!(
                "research team member profile `{profile}` must be read-only or planning-only"
            )))
        }
        _ => Ok(()),
    }
}

fn is_read_only_team_profile(profile: &str, profile_config: Option<&AgentProfile>) -> bool {
    if matches!(
        profile,
        "oracle"
            | "librarian"
            | "explore"
            | "metis"
            | "momus"
            | "multimodal-looker"
            | "prometheus"
            | "plan"
    ) {
        return true;
    }
    profile_config.is_some_and(|profile| {
        matches!(
            profile.category.as_str(),
            "explore" | "oracle" | "librarian" | "plan" | "research" | "read_only"
        )
    })
}

fn require_active_team<'a>(
    projection: &'a TeamProjection,
    team_run_id: &str,
) -> Result<&'a TeamRunProjection, CoordinatorError> {
    let team = projection
        .teams
        .get(team_run_id)
        .ok_or_else(|| CoordinatorError::UnknownTask(format!("team:{team_run_id}")))?;
    if team.status == crate::proj::TeamRunStatus::Deleted {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team `{team_run_id}` is deleted"
        )));
    }
    Ok(team)
}

fn require_active_team_or_shutdown<'a>(
    projection: &'a TeamProjection,
    team_run_id: &str,
) -> Result<&'a TeamRunProjection, CoordinatorError> {
    require_active_team(projection, team_run_id)
}

fn validate_team_member(
    team: &TeamRunProjection,
    member_name: &str,
) -> Result<(), CoordinatorError> {
    if team.members.contains_key(member_name) {
        Ok(())
    } else {
        Err(CoordinatorError::PolicyViolation(format!(
            "unknown team member `{member_name}`"
        )))
    }
}

fn validate_team_participant(
    team: &TeamRunProjection,
    participant: &str,
) -> Result<(), CoordinatorError> {
    if participant == "lead" || team.members.contains_key(participant) {
        Ok(())
    } else {
        Err(CoordinatorError::PolicyViolation(format!(
            "unknown team participant `{participant}`"
        )))
    }
}

fn validate_team_actor_can_act_as(
    actor: &EventActor,
    team: &TeamRunProjection,
    participant: &str,
) -> Result<(), CoordinatorError> {
    if actor.kind != ActorKind::Worker {
        return Ok(());
    }
    let Some(actor_agent_id) = actor.agent_id.as_deref() else {
        return Err(CoordinatorError::PolicyViolation(
            "worker team action missing agent_id".to_string(),
        ));
    };
    if participant == "lead" {
        if team.lead.as_ref().and_then(|lead| lead.agent_id.as_deref()) == Some(actor_agent_id) {
            return Ok(());
        }
        return Err(CoordinatorError::PolicyViolation(
            "worker team members cannot act as lead".to_string(),
        ));
    }
    let Some(member) = team.members.get(participant) else {
        return Err(CoordinatorError::PolicyViolation(format!(
            "unknown team participant `{participant}`"
        )));
    };
    if member.agent_id.as_deref() != Some(actor_agent_id) {
        return Err(CoordinatorError::PolicyViolation(format!(
            "worker `{actor_agent_id}` cannot act as team participant `{participant}`"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TeamActionKind {
    TeamWrite,
    Shutdown,
}

fn validate_team_action(
    actor: &EventActor,
    team: &TeamRunProjection,
    action: TeamActionKind,
    participant: &str,
    now_mono_ms: u64,
) -> Result<(), CoordinatorError> {
    validate_team_participant(team, participant)?;
    validate_team_participant_can_perform(team, action, participant, now_mono_ms)?;
    validate_team_actor_can_act_as(actor, team, participant)
}

fn validate_team_actor_can_make_unowned_team_write(
    actor: &EventActor,
    team: &TeamRunProjection,
    now_mono_ms: u64,
) -> Result<(), CoordinatorError> {
    validate_team_wall_clock(team, now_mono_ms)?;
    if actor.kind != ActorKind::Worker {
        return Ok(());
    }
    let participant = team_participant_for_worker_actor(actor, team)?;
    validate_team_participant_can_perform(
        team,
        TeamActionKind::TeamWrite,
        &participant,
        now_mono_ms,
    )
}

fn validate_team_participant_can_perform(
    team: &TeamRunProjection,
    action: TeamActionKind,
    participant: &str,
    now_mono_ms: u64,
) -> Result<(), CoordinatorError> {
    if action == TeamActionKind::TeamWrite {
        validate_team_wall_clock(team, now_mono_ms)?;
    }
    if participant == "lead" {
        return Ok(());
    }
    let member = team.members.get(participant).ok_or_else(|| {
        CoordinatorError::PolicyViolation(format!("unknown team participant `{participant}`"))
    })?;
    match action {
        TeamActionKind::TeamWrite => {
            if member.role == TeamMemberRole::Research {
                return Err(CoordinatorError::PolicyViolation(format!(
                    "research team member `{participant}` cannot mutate team messages or tasks"
                )));
            }
            match member.status {
                crate::proj::TeamMemberStatus::Pending => {
                    return Err(CoordinatorError::PolicyViolation(format!(
                        "team member `{participant}` is not active"
                    )));
                }
                crate::proj::TeamMemberStatus::ShutdownApproved => {
                    return Err(CoordinatorError::PolicyViolation(format!(
                        "team member `{participant}` is shutdown-approved and cannot mutate team state"
                    )));
                }
                crate::proj::TeamMemberStatus::Running
                | crate::proj::TeamMemberStatus::ShutdownRequested => {}
            }
            if team.bounds_consumption.member_turns >= team.bounds.max_member_turns {
                return Err(CoordinatorError::PolicyViolation(format!(
                    "team `{}` has reached max_member_turns {}",
                    team.team_run_id, team.bounds.max_member_turns
                )));
            }
        }
        TeamActionKind::Shutdown => {
            if member.status == crate::proj::TeamMemberStatus::ShutdownApproved {
                return Err(CoordinatorError::PolicyViolation(format!(
                    "team member `{participant}` is shutdown-approved and cannot make further shutdown decisions"
                )));
            }
        }
    }
    Ok(())
}

fn validate_team_wall_clock(
    team: &TeamRunProjection,
    now_mono_ms: u64,
) -> Result<(), CoordinatorError> {
    let Some(created_mono_ms) = team.created_mono_ms else {
        return Ok(());
    };
    let limit_ms = u64::from(team.bounds.max_wall_clock_minutes).saturating_mul(60_000);
    if now_mono_ms.saturating_sub(created_mono_ms) >= limit_ms {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team `{}` has exceeded max_wall_clock_minutes {}",
            team.team_run_id, team.bounds.max_wall_clock_minutes
        )));
    }
    Ok(())
}

fn team_participant_for_worker_actor(
    actor: &EventActor,
    team: &TeamRunProjection,
) -> Result<String, CoordinatorError> {
    let Some(actor_agent_id) = actor.agent_id.as_deref() else {
        return Err(CoordinatorError::PolicyViolation(
            "worker team action missing agent_id".to_string(),
        ));
    };
    if team.lead.as_ref().and_then(|lead| lead.agent_id.as_deref()) == Some(actor_agent_id) {
        return Ok("lead".to_string());
    }
    team.members
        .values()
        .find(|member| member.agent_id.as_deref() == Some(actor_agent_id))
        .map(|member| member.name.clone())
        .ok_or_else(|| {
            CoordinatorError::PolicyViolation(format!(
                "worker `{actor_agent_id}` is not a participant in team `{}`",
                team.team_run_id
            ))
        })
}

fn validate_team_shutdown_request_can_open(
    team: &TeamRunProjection,
    member_name: &str,
) -> Result<(), CoordinatorError> {
    match team
        .shutdown_requests
        .get(member_name)
        .map(|request| request.status)
    {
        Some(crate::proj::TeamMemberStatus::ShutdownRequested) => {
            Err(CoordinatorError::PolicyViolation(format!(
                "shutdown request for team member `{member_name}` is already pending"
            )))
        }
        Some(crate::proj::TeamMemberStatus::ShutdownApproved) => {
            Err(CoordinatorError::PolicyViolation(format!(
                "shutdown for team member `{member_name}` is already approved"
            )))
        }
        _ => Ok(()),
    }
}

fn validate_team_shutdown_request_pending(
    team: &TeamRunProjection,
    member_name: &str,
) -> Result<(), CoordinatorError> {
    match team
        .shutdown_requests
        .get(member_name)
        .map(|request| request.status)
    {
        Some(crate::proj::TeamMemberStatus::ShutdownRequested) => Ok(()),
        _ => Err(CoordinatorError::PolicyViolation(format!(
            "team member `{member_name}` has no pending shutdown request"
        ))),
    }
}

fn validate_team_message(
    team: &TeamRunProjection,
    message: &TeamMessage,
) -> Result<(), CoordinatorError> {
    if message.version != 1 {
        return Err(CoordinatorError::PolicyViolation(
            "team message version must be 1".to_string(),
        ));
    }
    if non_empty_trimmed(&message.message_id).is_none() {
        return Err(CoordinatorError::PolicyViolation(
            "team message id cannot be empty".to_string(),
        ));
    }
    if team.messages.len() >= team.bounds.max_messages_per_run as usize {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team `{}` has reached max_messages_per_run {}",
            team.team_run_id, team.bounds.max_messages_per_run
        )));
    }
    validate_team_text_field("team message id", &message.message_id)?;
    if team
        .messages
        .iter()
        .any(|existing| existing.message_id == message.message_id)
    {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team message `{}` already exists",
            message.message_id
        )));
    }
    validate_team_text_field("team message sender", &message.from)?;
    validate_team_text_field("team message recipient", &message.to)?;
    if let Some(summary) = message.summary.as_deref() {
        validate_team_text_field("team message summary", summary)?;
    }
    if message.body.len() > TEAM_MESSAGE_BODY_MAX_BYTES {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team message body exceeds {TEAM_MESSAGE_BODY_MAX_BYTES} bytes"
        )));
    }
    if message.references.len() > TEAM_REFERENCE_LIMIT {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team message references exceed {TEAM_REFERENCE_LIMIT} entries"
        )));
    }
    for reference in &message.references {
        validate_team_text_field("team reference path", &reference.path)?;
        if reference.path.starts_with('/') || reference.path.contains("..") {
            return Err(CoordinatorError::PolicyViolation(
                "team reference path must be workspace-relative and must not contain traversal"
                    .to_string(),
            ));
        }
        if let Some(description) = reference.description.as_deref() {
            validate_team_text_field("team reference description", description)?;
        }
    }
    validate_team_participant(team, &message.from)?;
    if message.to == "*" {
        if message.from != "lead" || message.kind != TeamMessageKind::Announcement {
            return Err(CoordinatorError::PolicyViolation(
                "only lead may broadcast announcements".to_string(),
            ));
        }
    } else {
        validate_team_participant(team, &message.to)?;
    }
    Ok(())
}

fn validate_team_task_create(
    team: &TeamRunProjection,
    task: &TeamTask,
) -> Result<(), CoordinatorError> {
    if task.version != 1 {
        return Err(CoordinatorError::PolicyViolation(
            "team task version must be 1".to_string(),
        ));
    }
    if non_empty_trimmed(&task.task_id).is_none() {
        return Err(CoordinatorError::PolicyViolation(
            "team task id cannot be empty".to_string(),
        ));
    }
    validate_team_text_field("team task id", &task.task_id)?;
    validate_team_text_field("team task subject", &task.subject)?;
    validate_team_text_field("team task description", &task.description)?;
    if team.tasks.contains_key(&task.task_id) {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team task `{}` already exists",
            task.task_id
        )));
    }
    validate_team_metadata(&task.metadata)?;
    if let Some(owner) = task.owner.as_deref() {
        validate_team_participant(team, owner)?;
    }
    for blocker in task.blocked_by.iter() {
        if !team.tasks.contains_key(blocker) {
            return Err(CoordinatorError::PolicyViolation(format!(
                "team task `{}` depends on unknown task `{blocker}`",
                task.task_id
            )));
        }
    }
    Ok(())
}

fn validate_persistent_task_create(
    projection: &PersistentTaskProjection,
    task: &PersistentTask,
) -> Result<(), CoordinatorError> {
    if task.version != 1 {
        return Err(CoordinatorError::PolicyViolation(
            "persistent task version must be 1".to_string(),
        ));
    }
    if non_empty_trimmed(&task.task_id).is_none() {
        return Err(CoordinatorError::PolicyViolation(
            "persistent task id cannot be empty".to_string(),
        ));
    }
    validate_team_text_field("persistent task id", &task.task_id)?;
    if let Some(run_id) = task.run_id.as_deref() {
        validate_team_text_field("persistent task run_id", run_id)?;
    }
    if let Some(thread_id) = task.thread_id.as_deref() {
        validate_team_text_field("persistent task thread_id", thread_id)?;
    }
    validate_team_text_field("persistent task subject", &task.subject)?;
    validate_team_text_field("persistent task description", &task.description)?;
    if let Some(active_form) = task.active_form.as_deref() {
        validate_team_text_field("persistent task active_form", active_form)?;
    }
    if let Some(owner) = task.owner.as_deref() {
        validate_team_text_field("persistent task owner", owner)?;
    }
    if projection.tasks.contains_key(&task.task_id) {
        return Err(CoordinatorError::PolicyViolation(format!(
            "persistent task `{}` already exists",
            task.task_id
        )));
    }
    validate_team_metadata(&task.metadata)?;
    for blocker in &task.blocked_by {
        validate_team_text_field("persistent task blocker", blocker)?;
        if blocker == &task.task_id {
            return Err(CoordinatorError::PolicyViolation(format!(
                "persistent task `{}` cannot block itself",
                task.task_id
            )));
        }
        if !projection.tasks.contains_key(blocker) {
            return Err(CoordinatorError::PolicyViolation(format!(
                "persistent task `{}` depends on unknown task `{blocker}`",
                task.task_id
            )));
        }
    }
    Ok(())
}

fn validate_persistent_task_update(
    projection: &PersistentTaskProjection,
    update: &PersistentTaskUpdatedEvent,
) -> Result<(), CoordinatorError> {
    let mut candidate = projection
        .tasks
        .get(&update.task_id)
        .cloned()
        .ok_or_else(|| CoordinatorError::UnknownTask(format!("persistent:{}", update.task_id)))?;
    if let Some(subject) = update.subject.as_deref() {
        validate_team_text_field("persistent task subject", subject)?;
    }
    if let Some(description) = update.description.as_deref() {
        validate_team_text_field("persistent task description", description)?;
    }
    if let Some(active_form) = update.active_form.as_deref() {
        validate_team_text_field("persistent task active_form", active_form)?;
    }
    if let Some(owner) = update.owner.as_deref() {
        validate_team_text_field("persistent task owner", owner)?;
    }
    if let Some(blocked_by) = update.blocked_by.as_ref() {
        for blocker in blocked_by {
            validate_team_text_field("persistent task blocker", blocker)?;
            if blocker == &update.task_id {
                return Err(CoordinatorError::PolicyViolation(format!(
                    "persistent task `{}` cannot block itself",
                    update.task_id
                )));
            }
            if !projection.tasks.contains_key(blocker) {
                return Err(CoordinatorError::PolicyViolation(format!(
                    "persistent task `{}` depends on unknown task `{blocker}`",
                    update.task_id
                )));
            }
        }
    }
    validate_team_metadata(&update.metadata)?;
    apply_persistent_task_update(&mut candidate, update);
    let mut candidate_tasks = projection.tasks.clone();
    candidate_tasks.insert(candidate.task_id.clone(), candidate.clone());
    if let Some(blocker) = candidate.blocked_by.iter().find(|blocker| {
        has_persistent_task_dependency_path(&candidate_tasks, blocker, &candidate.task_id)
    }) {
        return Err(CoordinatorError::PolicyViolation(format!(
            "persistent task `{}` dependency cycle through `{blocker}`",
            candidate.task_id
        )));
    }
    if matches!(
        candidate.status,
        PersistentTaskStatus::Claimed
            | PersistentTaskStatus::InProgress
            | PersistentTaskStatus::Completed
    ) {
        if let Some(blocker) = blocked_by_incomplete(&candidate_tasks, &candidate.task_id) {
            return Err(CoordinatorError::PolicyViolation(format!(
                "persistent task `{}` is blocked by incomplete task `{blocker}`",
                candidate.task_id
            )));
        }
    }
    Ok(())
}

fn validate_team_task_update(
    team: &TeamRunProjection,
    task_id: &str,
    status: TeamTaskStatus,
    owner: Option<&str>,
    metadata: &BTreeMap<String, String>,
) -> Result<(), CoordinatorError> {
    let task = team.tasks.get(task_id).ok_or_else(|| {
        CoordinatorError::UnknownTask(format!("team:{}/task:{task_id}", team.team_run_id))
    })?;
    if let Some(owner) = owner {
        validate_team_participant(team, owner)?;
    }
    validate_team_metadata(metadata)?;
    if matches!(
        status,
        TeamTaskStatus::Claimed | TeamTaskStatus::InProgress | TeamTaskStatus::Completed
    ) {
        let incomplete = task
            .blocked_by
            .iter()
            .filter(|blocked_by| {
                team.tasks
                    .get(*blocked_by)
                    .is_none_or(|candidate| candidate.status != TeamTaskStatus::Completed)
            })
            .cloned()
            .collect::<Vec<_>>();
        if !incomplete.is_empty() {
            return Err(CoordinatorError::PolicyViolation(format!(
                "team task `{task_id}` is blocked by incomplete tasks: {}",
                incomplete.join(", ")
            )));
        }
    }
    Ok(())
}

fn validate_team_metadata(metadata: &BTreeMap<String, String>) -> Result<(), CoordinatorError> {
    if metadata.len() > TEAM_TASK_METADATA_MAX_ENTRIES {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team task metadata exceeds {TEAM_TASK_METADATA_MAX_ENTRIES} entries"
        )));
    }
    for (key, value) in metadata {
        validate_team_metadata_field("team task metadata key", key)?;
        validate_team_metadata_field("team task metadata value", value)?;
    }
    Ok(())
}

fn validate_team_metadata_field(label: &str, value: &str) -> Result<(), CoordinatorError> {
    if value.chars().count() > TEAM_TASK_METADATA_MAX_CHARS {
        return Err(CoordinatorError::PolicyViolation(format!(
            "{label} exceeds {TEAM_TASK_METADATA_MAX_CHARS} characters"
        )));
    }
    Ok(())
}

fn background_notification_status_for_cancel_reason(
    reason: &str,
) -> BackgroundTaskNotificationStatus {
    let lower = reason.to_ascii_lowercase();
    if lower.contains("cancel") || lower.contains("aborted") {
        BackgroundTaskNotificationStatus::Cancelled
    } else {
        BackgroundTaskNotificationStatus::Failed
    }
}

fn terminal_event_summary(event: &EventEnvelopeV1) -> String {
    match &event.payload {
        EventV1::TaskCompleted(payload) => payload.result_summary.clone(),
        EventV1::TaskCancelled(payload) => payload.reason.clone(),
        _ => String::new(),
    }
}

fn background_projection_error_to_coordinator_error(
    err: BackgroundRequestProjectionError,
) -> CoordinatorError {
    match err {
        BackgroundRequestProjectionError::Unauthorized => {
            CoordinatorError::PermissionDenied(err.to_string())
        }
        BackgroundRequestProjectionError::MissingSelector
        | BackgroundRequestProjectionError::UnknownRequest(_)
        | BackgroundRequestProjectionError::UnknownSelector(_)
        | BackgroundRequestProjectionError::MissingProjection(_) => {
            CoordinatorError::UnknownTask(err.to_string())
        }
    }
}

fn background_terminal_event_matches_task(
    event: &EventEnvelopeV1,
    scheduler_task_id: &str,
) -> bool {
    match &event.payload {
        EventV1::TaskCompleted(payload) => {
            payload.task_id == scheduler_task_id
                || payload
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.task_scope)
                    == Some(TaskTerminalScope::AgentTurn)
        }
        EventV1::TaskCancelled(payload) => {
            payload.task_id == scheduler_task_id
                || payload.task_scope == Some(TaskTerminalScope::AgentTurn)
        }
        _ => false,
    }
}

fn background_task_notification_text(notification: &BackgroundTaskNotificationEvent) -> String {
    format!(
        "[BACKGROUND TASK {}]\nID: {}\nRequest ID: {}\nDescription: {}\nStatus: {}\n\n{}\n\nUse background_output(request_id=\"{}\") for full details or task(session_id=\"{}\") to continue analysis from the child session.",
        notification.status.as_str().to_ascii_uppercase(),
        notification.task_id,
        notification.child_request_id,
        notification.description,
        notification.status.as_str(),
        notification.summary,
        notification.child_request_id,
        notification.child_session_id,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "background notification scheduling needs explicit coordinator dependencies"
)]
async fn append_background_task_notification_and_schedule<C, R>(
    clock: &C,
    redactor: &R,
    job_tx: mpsc::Sender<Command>,
    run_state: &mut RunState,
    hook_runtime_config: HookRuntimeConfig,
    compaction_config: CompactionRuntimeConfig,
    provider: Arc<dyn Provider>,
    tool_registry: Arc<ToolRegistry>,
    child_task: Option<ChildTaskTurnState>,
    terminal_event: &EventEnvelopeV1,
    status: BackgroundTaskNotificationStatus,
    summary: &str,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let Some(child_task) = child_task.filter(|metadata| metadata.run_in_background) else {
        return Ok(());
    };

    if !run_state
        .background_notification_child_requests
        .insert(child_task.child_request_id.clone())
    {
        return Ok(());
    }

    let parent_agent_id = child_task.parent_agent_id.clone();
    let parent_profile = parent_agent_id
        .as_deref()
        .and_then(|agent_id| run_state.agents.get(agent_id))
        .cloned();
    let delivered_turn_request_id = parent_profile
        .as_ref()
        .map(|_| allocate_provider_request_id(run_state));
    let capped_description = truncate_with_ellipsis(
        &redactor.redact_text(&child_task.description),
        BACKGROUND_TASK_NOTIFICATION_DESCRIPTION_MAX_CHARS,
    );
    let capped_summary = truncate_with_ellipsis(
        &redactor.redact_text(summary),
        BACKGROUND_TASK_NOTIFICATION_SUMMARY_MAX_CHARS,
    );
    let notification = BackgroundTaskNotificationEvent {
        parent_session_id: child_task.parent_session_id.clone(),
        parent_agent_id: parent_agent_id.clone(),
        child_session_id: child_task.child_session_id.clone(),
        child_request_id: child_task.child_request_id.clone(),
        task_id: child_task.task_id.clone(),
        description: capped_description,
        status,
        summary: capped_summary,
        terminal_event_id: terminal_event.event_id.clone(),
        terminal_task_id: terminal_terminal_task_id(terminal_event),
        delivered_turn_request_id: delivered_turn_request_id.clone(),
    };
    let notification_text = format!(
        "<system-reminder>\n{}\n</system-reminder>",
        background_task_notification_text(&notification)
    );

    append_payload_event_with_correlation(
        clock,
        redactor,
        run_state,
        system_actor(),
        Some(format!(
            "background_task_notification:{}",
            notification.child_request_id
        )),
        Some(notification.child_request_id.clone()),
        EventV1::BackgroundTaskNotification(notification),
    )?;

    let (Some(parent_agent_id), Some(parent_profile), Some(delivered_turn_request_id)) =
        (parent_agent_id, parent_profile, delivered_turn_request_id)
    else {
        return Ok(());
    };

    append_payload_event_with_correlation(
        clock,
        redactor,
        run_state,
        system_actor(),
        Some(format!("agent:{parent_agent_id}")),
        Some(delivered_turn_request_id.clone()),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: delivered_turn_request_id.clone(),
            text: notification_text.clone(),
        }),
    )?;

    if run_state
        .running_agent_turns
        .values()
        .any(|running| running.agent_id == parent_agent_id)
    {
        run_state
            .pending_agent_wakeups
            .entry(parent_agent_id)
            .or_default()
            .push(PendingAgentWakeup {
                request_id: delivered_turn_request_id,
                notification_text,
            });
        return Ok(());
    }

    schedule_agent_turn(
        clock,
        redactor,
        job_tx,
        run_state,
        hook_runtime_config,
        compaction_config,
        ScheduleAgentTurnArgs {
            provider,
            tool_registry,
            profile: parent_profile.clone(),
            request: AgentRequest {
                agent_id: parent_agent_id,
                prompt: notification_text,
                prompt_context: None,
                selected_file_tags: Vec::new(),
                selected_agent_tags: Vec::new(),
                selected_resource_tags: Vec::new(),
                model_ref: parent_profile.model_ref.clone(),
                fallback_model_refs: parent_profile.fallback_model_refs.clone(),
                fallback_model_settings: parent_profile.fallback_model_settings.clone(),
                model_settings: default_model_settings_for_profile(&parent_profile.name),
            },
            request_id: delivered_turn_request_id,
            child_task: None,
        },
    )
    .await
}

#[expect(
    clippy::too_many_arguments,
    reason = "pending wakeup scheduling needs explicit coordinator dependencies"
)]
async fn schedule_pending_agent_wakeups_for_idle_agent<C, R>(
    clock: &C,
    redactor: &R,
    job_tx: mpsc::Sender<Command>,
    run_state: &mut RunState,
    hook_runtime_config: HookRuntimeConfig,
    compaction_config: CompactionRuntimeConfig,
    provider: Arc<dyn Provider>,
    tool_registry: Arc<ToolRegistry>,
    agent_id: &str,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    if run_state
        .running_agent_turns
        .values()
        .any(|running| running.agent_id == agent_id)
    {
        return Ok(());
    }

    let Some(wakeups) = run_state.pending_agent_wakeups.remove(agent_id) else {
        return Ok(());
    };
    let Some(parent_profile) = run_state.agents.get(agent_id).cloned() else {
        return Ok(());
    };

    for wakeup in wakeups {
        schedule_agent_turn(
            clock,
            redactor,
            job_tx.clone(),
            run_state,
            hook_runtime_config.clone(),
            compaction_config.clone(),
            ScheduleAgentTurnArgs {
                provider: provider.clone(),
                tool_registry: tool_registry.clone(),
                profile: parent_profile.clone(),
                request: AgentRequest {
                    agent_id: agent_id.to_string(),
                    prompt: wakeup.notification_text,
                    prompt_context: None,
                    selected_file_tags: Vec::new(),
                    selected_agent_tags: Vec::new(),
                    selected_resource_tags: Vec::new(),
                    model_ref: parent_profile.model_ref.clone(),
                    fallback_model_refs: parent_profile.fallback_model_refs.clone(),
                    fallback_model_settings: parent_profile.fallback_model_settings.clone(),
                    model_settings: default_model_settings_for_profile(&parent_profile.name),
                },
                request_id: wakeup.request_id,
                child_task: None,
            },
        )
        .await?;
    }

    Ok(())
}

fn terminal_terminal_task_id(event: &EventEnvelopeV1) -> String {
    match &event.payload {
        EventV1::TaskCompleted(payload) => payload.task_id.clone(),
        EventV1::TaskCancelled(payload) => payload.task_id.clone(),
        EventV1::TaskResultLate(payload) => payload.task_id.clone(),
        _ => String::new(),
    }
}

#[derive(Debug, Clone)]
struct AppliedCompaction {
    updated_context: ProviderContext,
    checkpoint_id: String,
    tokens_before_estimate: Option<u32>,
    tokens_after_estimate: Option<u32>,
}

#[derive(Debug, Clone)]
enum CompactAgentContextResult {
    CheckpointWritten {
        context: ProviderContext,
        checkpoint_id: String,
        tokens_before_estimate: Option<u32>,
        tokens_after_estimate: Option<u32>,
    },
    NoOp {
        context: ProviderContext,
    },
}

impl CompactAgentContextResult {
    fn into_context(self) -> ProviderContext {
        match self {
            Self::CheckpointWritten { context, .. } | Self::NoOp { context } => context,
        }
    }

    fn into_manual_outcome(self) -> ManualCompactionOutcome {
        match self {
            Self::CheckpointWritten {
                checkpoint_id,
                tokens_before_estimate,
                tokens_after_estimate,
                ..
            } => ManualCompactionOutcome::CheckpointWritten {
                checkpoint_id,
                tokens_before_estimate,
                tokens_after_estimate,
            },
            Self::NoOp { .. } => ManualCompactionOutcome::NoOp,
        }
    }
}

#[derive(Debug, Clone)]
struct FailedTerminalCompactionRequest {
    task_id: String,
    agent_id: String,
    request_id: String,
    trigger_reason: String,
}

impl FailedTerminalCompactionRequest {
    fn new(
        task_id: impl Into<String>,
        agent_id: impl Into<String>,
        request_id: impl Into<String>,
        trigger_reason: impl Into<String>,
    ) -> Self {
        Self {
            task_id: task_id.into(),
            agent_id: agent_id.into(),
            request_id: request_id.into(),
            trigger_reason: trigger_reason.into(),
        }
    }

    fn attempt_key(&self) -> (String, String) {
        (self.task_id.clone(), self.request_id.clone())
    }
}

fn mark_failed_terminal_compaction_attempt(
    run_state: &mut RunState,
    request: &FailedTerminalCompactionRequest,
) -> bool {
    let key = request.attempt_key();
    if !run_state
        .failed_terminal_compaction_attempts
        .insert(key.clone())
    {
        return false;
    }

    if let Some(overflow_context) = run_state
        .overflow_retry_compacted_context_by_attempt
        .get(&key)
    {
        let current_context = run_state
            .provider_context_by_agent
            .get(&request.agent_id)
            .cloned()
            .unwrap_or_default();
        if &current_context == overflow_context {
            return false;
        }
    }

    true
}

fn agent_has_active_or_queued_turn(run_state: &RunState, agent_id: &str) -> bool {
    run_state
        .running_agent_turns
        .values()
        .any(|running| running.agent_id == agent_id)
        || run_state
            .queued_agent_turns
            .values()
            .any(|queued| queued.agent_id == agent_id)
}

fn next_agent_blocked_turn_id(run_state: &RunState, agent_id: &str) -> Option<String> {
    run_state
        .queued_agent_turns
        .values()
        .filter(|queued| queued.agent_id == agent_id && !queued.scheduler_queued)
        .min_by(|left, right| left.task_id.cmp(&right.task_id))
        .map(|queued| queued.task_id.clone())
}

#[derive(Debug, Clone)]
struct ProviderCompactionTrigger {
    agent_id: String,
    profile_name: String,
    model_ref: String,
    provider_id: Option<String>,
    model_id: Option<String>,
    through_request_id: Option<String>,
    trigger_reason: String,
    tokens_before: Option<u32>,
    prompt_tokens_estimate: Option<u32>,
    estimate_source: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct ProviderContextTriggerEstimate {
    tokens_before_estimate: u32,
    input_budget: u32,
    reserve: u32,
    source: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskExecutionState {
    Running,
}

#[derive(Debug, Clone)]
struct HashlineEditMetadata {
    edit_id: String,
    path: String,
    summary: String,
    patch_digest: String,
}

#[derive(Debug, Clone)]
struct AppliedToolEditMetadata {
    metadata: HashlineEditMetadata,
    diff_rel_path: Option<String>,
    diff_digest: Option<String>,
    deleted: bool,
}

struct TaskState {
    tool_call_id: String,
    tool_metadata: Option<ToolIdentityMetadata>,
    owner_actor: EventActor,
    request_correlation_id: Option<String>,
    queue_key: ConcurrencyKey,
    state: TaskExecutionState,
    cancellation_token: CancellationToken,
    started_mono_ms: u64,
    last_progress_mono_ms: u64,
    last_progress_kind: JobProgressKind,
    hashline_edit: Option<HashlineEditMetadata>,
    respond_to: Option<oneshot::Sender<Result<ToolResult, String>>>,
}

#[derive(Debug, Clone, Default)]
struct TaskHookState {
    tool_id: String,
    category: Option<String>,
    hook_executions: Vec<HookExecutionMetadata>,
}

struct PendingPermissionState {
    tool_call_id: String,
    request_correlation_id: Option<String>,
    hook_executions: Vec<HookExecutionMetadata>,
    grant_request: Option<PermissionGrantRequest>,
    resolution: PendingPermissionResolution,
}

enum PendingPermissionResolution {
    ToolCall {
        tool_id: String,
        args_json: Value,
        actor: EventActor,
        category: Option<String>,
        respond_to: Option<oneshot::Sender<Result<ToolResult, String>>>,
    },
    Question {
        actor: EventActor,
        prompts: Vec<QuestionPromptSpec>,
        respond_to: oneshot::Sender<Result<Vec<Vec<String>>, String>>,
    },
}

#[derive(Debug, Clone, Deserialize)]
struct QuestionRequestSpec {
    questions: Vec<QuestionPromptSpec>,
}

#[derive(Debug, Clone, Deserialize)]
struct QuestionPromptSpec {
    #[serde(rename = "question")]
    _question: String,
    header: String,
    options: Vec<QuestionOptionSpec>,
    #[serde(default)]
    multiple: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
struct QuestionOptionSpec {
    label: String,
    #[serde(rename = "description")]
    _description: String,
}

impl QuestionAnswerPrompt for QuestionPromptSpec {
    fn header(&self) -> &str {
        &self.header
    }

    fn multiple(&self) -> bool {
        self.multiple.unwrap_or(false)
    }

    fn canonical_option_label<'a>(&'a self, answer: &str) -> Option<&'a str> {
        self.options
            .iter()
            .find(|option| option.label.eq_ignore_ascii_case(answer))
            .map(|option| option.label.as_str())
    }
}

#[derive(Debug, Clone, Default)]
struct HookExecutionBatch {
    hook_executions: Vec<HookExecutionMetadata>,
    critical_failure: Option<String>,
}

struct LifecycleHookCommandOutput {
    output_digest: String,
    output_summary: String,
    effects: Vec<HookEffectMetadata>,
}

#[derive(Debug, Clone)]
struct HookInvocationContext {
    event: HookLifecycleEvent,
    run_id: String,
    workspace_root: PathBuf,
    artifacts_dir: PathBuf,
    actor: Option<EventActor>,
    agent_id: Option<String>,
    request_id: Option<String>,
    permission_id: Option<String>,
    task_id: Option<String>,
    tool_call_id: Option<String>,
    tool_id: Option<String>,
    provider_id: Option<String>,
    model_id: Option<String>,
    parent_agent_id: Option<String>,
    category: Option<String>,
    outcome: Option<String>,
    output_summary: Option<String>,
    failure_reason: Option<String>,
}

struct AgentTurnTaskScheduledEventArgs<'a> {
    task_id: &'a str,
    agent_id: &'a str,
    request_id: &'a str,
    queue_key: &'a ConcurrencyKey,
    state: TaskScheduleState,
}

struct ScheduleAgentTurnArgs {
    provider: Arc<dyn Provider>,
    tool_registry: Arc<ToolRegistry>,
    profile: AgentProfile,
    request: AgentRequest,
    request_id: String,
    child_task: Option<ChildTaskTurnState>,
}

struct PermissionDeniedArgs<'a> {
    actor: EventActor,
    category: Option<String>,
    tool_id: &'a str,
    args_json: &'a Value,
    tool_call_id: &'a str,
    hashline_edit: Option<&'a HashlineEditMetadata>,
    kind: PermissionKind,
    reason: &'a str,
    request_correlation_id: Option<&'a str>,
}

struct ToolCallRequestedEventArgs<'a> {
    actor: EventActor,
    tool_call_id: &'a str,
    tool_id: &'a str,
    args_json: &'a Value,
    tool_metadata: Option<ToolCallMetadata>,
    request_correlation_id: Option<&'a str>,
}

struct AgentProviderRequestStartedArgs {
    task_id: String,
    agent_id: String,
    request_id: String,
    provider_id: String,
    model_id: String,
    prompt_summary: String,
    request_digest: String,
    metadata: Option<ProviderRequestStartedMetadata>,
}

struct AgentProviderRequestFinishedArgs {
    task_id: String,
    agent_id: String,
    request_id: String,
    finish_reason: String,
    output_digest: Option<String>,
    usage: Option<harness_providers::CompletionUsage>,
    metadata: Option<ProviderRequestFinishedMetadata>,
}

struct ToolCallExecutionArgs {
    tool_call_id: String,
    tool_id: String,
    args_json: Value,
    actor: EventActor,
    category: Option<String>,
    hook_executions: Vec<HookExecutionMetadata>,
    tool_registry: Arc<ToolRegistry>,
    request_correlation_id: Option<String>,
    respond_to: Option<oneshot::Sender<Result<ToolResult, String>>>,
}

struct PermissionRequestedEventArgs<'a> {
    permission_id: &'a str,
    tool_call_id: &'a str,
    kind: PermissionKind,
    summary: String,
    request_digest: String,
    timeout_ms: u64,
    default_decision: EventPermissionDecision,
    request_correlation_id: Option<&'a str>,
}

struct ToolCallFinishedEventArgs<'a> {
    tool_call_id: &'a str,
    status: ToolCallStatus,
    output_summary: Option<String>,
    output_json: Option<Value>,
    metadata: Option<ToolCallMetadata>,
    request_correlation_id: Option<&'a str>,
}

struct EditAppliedEventArgs<'a> {
    tool_call_id: &'a str,
    metadata: &'a HashlineEditMetadata,
    new_file_digest: String,
    diff_rel_path: Option<String>,
    diff_digest: Option<String>,
    request_correlation_id: Option<&'a str>,
}

struct TurnStartPhaseResult {
    cancellation_token: CancellationToken,
    critical_failure: Option<String>,
}

#[cfg(test)]
fn block_on_coordinator_future<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("coordinator helper runtime")
        .block_on(future)
}

async fn run_lifecycle_hooks<C>(
    clock: &C,
    runtime: &HookRuntimeConfig,
    context: HookInvocationContext,
) -> HookExecutionBatch
where
    C: Clock + ?Sized,
{
    let mut batch = HookExecutionBatch::default();

    for (index, hook) in runtime.hooks.lifecycle.iter().enumerate() {
        if hook.event != context.event {
            continue;
        }

        let hook_name = hook_identifier(hook, index);
        let command_digest = digest12(hook.command.join("\u{0}").as_bytes());
        if runtime.hooks.disabled || runtime.hooks.disabled_hooks.contains(&hook_name) {
            batch.hook_executions.push(HookExecutionMetadata {
                hook_name,
                status: HookExecutionStatus::Skipped,
                hook_event: Some(context.event.as_str().to_string()),
                hook_phase: Some(context.event.phase().as_str().to_string()),
                command_digest: Some(command_digest),
                output_digest: None,
                output_summary: Some("disabled by hooks config".to_string()),
                duration_ms: Some(0),
                effects: Vec::new(),
            });
            continue;
        }

        if runtime.suppress_execution {
            batch.hook_executions.push(HookExecutionMetadata {
                hook_name,
                status: HookExecutionStatus::Skipped,
                hook_event: Some(context.event.as_str().to_string()),
                hook_phase: Some(context.event.phase().as_str().to_string()),
                command_digest: Some(command_digest),
                output_digest: None,
                output_summary: Some("suppressed during deterministic execution".to_string()),
                duration_ms: Some(0),
                effects: Vec::new(),
            });
            continue;
        }

        let (metadata, failure) =
            execute_lifecycle_hook(clock, runtime, hook, index, &context).await;
        let deny_failure = hook_deny_failure(&metadata, &context);
        batch.hook_executions.push(metadata);
        if let Some(failure) = deny_failure.or_else(|| hook.critical.then_some(failure).flatten()) {
            batch.critical_failure = Some(failure);
            break;
        }
    }

    batch
}

async fn execute_lifecycle_hook<C>(
    clock: &C,
    runtime: &HookRuntimeConfig,
    hook: &LifecycleHookConfig,
    index: usize,
    context: &HookInvocationContext,
) -> (HookExecutionMetadata, Option<String>)
where
    C: Clock + ?Sized,
{
    let hook_name = hook_identifier(hook, index);
    let command_digest = digest12(hook.command.join("\u{0}").as_bytes());
    let started_mono_ms = clock.mono_ms();

    let execution = execute_lifecycle_hook_command(runtime, hook, &hook_name, context).await;
    let finished_mono_ms = clock.mono_ms();

    match execution {
        Ok(output) => (
            HookExecutionMetadata {
                hook_name,
                status: HookExecutionStatus::Succeeded,
                hook_event: Some(context.event.as_str().to_string()),
                hook_phase: Some(context.event.phase().as_str().to_string()),
                command_digest: Some(command_digest),
                output_digest: Some(output.output_digest),
                output_summary: Some(output.output_summary),
                duration_ms: Some(finished_mono_ms.saturating_sub(started_mono_ms)),
                effects: output.effects,
            },
            None,
        ),
        Err((reason, output_summary)) => {
            let failure = format!(
                "hook `{hook_name}` for `{}` failed: {reason}",
                context.event.as_str()
            );
            (
                HookExecutionMetadata {
                    hook_name,
                    status: HookExecutionStatus::Failed,
                    hook_event: Some(context.event.as_str().to_string()),
                    hook_phase: Some(context.event.phase().as_str().to_string()),
                    command_digest: Some(command_digest),
                    output_digest: Some(digest12(reason.as_bytes())),
                    output_summary: Some(output_summary),
                    duration_ms: Some(finished_mono_ms.saturating_sub(started_mono_ms)),
                    effects: Vec::new(),
                },
                Some(failure),
            )
        }
    }
}

async fn execute_lifecycle_hook_command(
    runtime: &HookRuntimeConfig,
    hook: &LifecycleHookConfig,
    hook_name: &str,
    context: &HookInvocationContext,
) -> Result<LifecycleHookCommandOutput, (String, String)> {
    let executable = hook.command.first().ok_or_else(|| {
        (
            format!("hook `{hook_name}` is missing a command executable"),
            "no output".to_string(),
        )
    })?;

    if !hook_executable_allowed(&runtime.shell_allowlist, executable) {
        return Err((
            format!("executable `{executable}` is not in the shell allowlist"),
            "no output".to_string(),
        ));
    }

    let cwd = resolve_hook_cwd(
        &runtime.shell_allowlist,
        &context.workspace_root,
        hook.cwd.as_deref(),
    )
    .map_err(|err| (err, "no output".to_string()))?;
    let mut command = tokio::process::Command::new(executable);
    command.args(&hook.command[1..]);
    command.current_dir(&cwd);
    command.kill_on_drop(true);
    for (key, value) in hook_environment(hook_name, context, &cwd, hook) {
        command.env(key, value);
    }

    let output = tokio::time::timeout(Duration::from_millis(hook.timeout_ms), command.output())
        .await
        .map_err(|_| {
            (
                format!("timed out after {} ms", hook.timeout_ms),
                "no output".to_string(),
            )
        })?
        .map_err(|err| {
            (
                format!("failed to execute command: {err}"),
                "no output".to_string(),
            )
        })?;

    let redactor = DefaultRedactor::default();
    let stdout = redactor.redact_text(&String::from_utf8_lossy(&output.stdout));
    let stderr = redactor.redact_text(&String::from_utf8_lossy(&output.stderr));
    let output_summary = summarize_hook_output(&stdout, &stderr);
    let output_digest =
        digest12(format!("{}\u{0}{}\u{0}{:?}", stdout, stderr, output.status).as_bytes());
    let effects = parse_hook_effects(&stdout, &stderr);

    if output.status.success() {
        Ok(LifecycleHookCommandOutput {
            output_digest,
            output_summary,
            effects,
        })
    } else {
        Err((
            format!(
                "exit status {:?}: {output_summary}",
                output.status.code().unwrap_or(-1)
            ),
            output_summary,
        ))
    }
}

fn hook_deny_failure(
    metadata: &HookExecutionMetadata,
    context: &HookInvocationContext,
) -> Option<String> {
    metadata.effects.iter().find_map(|effect| {
        if effect.kind != HookEffectKind::Deny {
            return None;
        }
        let summary = effect
            .summary
            .as_deref()
            .and_then(non_empty_trimmed)
            .unwrap_or("hook requested denial");
        Some(format!(
            "hook `{}` denied `{}` during `{}`: {summary}",
            metadata.hook_name,
            context.event.as_str(),
            context.event.phase().as_str()
        ))
    })
}

fn hook_identifier(hook: &LifecycleHookConfig, index: usize) -> String {
    hook.id
        .clone()
        .unwrap_or_else(|| format!("{}_{:02}", hook.event.as_str(), index + 1))
}

fn hook_executable_allowed(allowlist: &ShellAllowlist, executable: &str) -> bool {
    allowlist
        .executables
        .iter()
        .any(|allowed| allowed == executable)
}

fn resolve_hook_cwd(
    allowlist: &ShellAllowlist,
    workspace_root: &Path,
    cwd: Option<&str>,
) -> Result<PathBuf, String> {
    let cwd = match cwd {
        Some(value) => workspace_root.join(value),
        None => workspace_root.to_path_buf(),
    };

    if allowlist.cwd_roots.is_empty() {
        return Ok(cwd);
    }

    let canonical_cwd = cwd
        .canonicalize()
        .map_err(|err| format!("failed to resolve cwd: {err}"))?;
    let allowed = allowlist.cwd_roots.iter().any(|root| {
        workspace_root
            .join(root)
            .canonicalize()
            .map(|allowed_root| canonical_cwd.starts_with(&allowed_root))
            .unwrap_or(false)
    });

    if allowed {
        Ok(canonical_cwd)
    } else {
        Err(format!(
            "cwd {} is not in the shell allowlist",
            canonical_cwd.display()
        ))
    }
}

fn hook_environment(
    hook_name: &str,
    context: &HookInvocationContext,
    cwd: &Path,
    hook: &LifecycleHookConfig,
) -> BTreeMap<String, String> {
    let mut env = hook.env.clone();
    env.insert("HARNESS_HOOK_ID".to_string(), hook_name.to_string());
    env.insert(
        "HARNESS_HOOK_EVENT".to_string(),
        context.event.as_str().to_string(),
    );
    env.insert("HARNESS_HOOK_RUN_ID".to_string(), context.run_id.clone());
    env.insert(
        "HARNESS_HOOK_WORKSPACE_ROOT".to_string(),
        context.workspace_root.display().to_string(),
    );
    env.insert(
        "HARNESS_HOOK_ARTIFACTS_DIR".to_string(),
        context.artifacts_dir.display().to_string(),
    );
    env.insert("HARNESS_HOOK_CWD".to_string(), cwd.display().to_string());
    if let Some(actor) = context.actor.as_ref() {
        env.insert(
            "HARNESS_HOOK_ACTOR_KIND".to_string(),
            format!("{:?}", actor.kind).to_ascii_lowercase(),
        );
        if let Some(actor_agent_id) = actor.agent_id.as_ref() {
            env.insert(
                "HARNESS_HOOK_ACTOR_AGENT_ID".to_string(),
                actor_agent_id.clone(),
            );
        }
    }
    if let Some(agent_id) = context.agent_id.as_ref() {
        env.insert("HARNESS_HOOK_AGENT_ID".to_string(), agent_id.clone());
    }
    if let Some(request_id) = context.request_id.as_ref() {
        env.insert("HARNESS_HOOK_REQUEST_ID".to_string(), request_id.clone());
    }
    if let Some(permission_id) = context.permission_id.as_ref() {
        env.insert(
            "HARNESS_HOOK_PERMISSION_ID".to_string(),
            permission_id.clone(),
        );
    }
    if let Some(task_id) = context.task_id.as_ref() {
        env.insert("HARNESS_HOOK_TASK_ID".to_string(), task_id.clone());
    }
    if let Some(tool_call_id) = context.tool_call_id.as_ref() {
        env.insert(
            "HARNESS_HOOK_TOOL_CALL_ID".to_string(),
            tool_call_id.clone(),
        );
    }
    if let Some(tool_id) = context.tool_id.as_ref() {
        env.insert("HARNESS_HOOK_TOOL_ID".to_string(), tool_id.clone());
    }
    if let Some(provider_id) = context.provider_id.as_ref() {
        env.insert("HARNESS_HOOK_PROVIDER_ID".to_string(), provider_id.clone());
    }
    if let Some(model_id) = context.model_id.as_ref() {
        env.insert("HARNESS_HOOK_MODEL_ID".to_string(), model_id.clone());
    }
    if let Some(parent_agent_id) = context.parent_agent_id.as_ref() {
        env.insert(
            "HARNESS_HOOK_PARENT_AGENT_ID".to_string(),
            parent_agent_id.clone(),
        );
    }
    if let Some(category) = context.category.as_ref() {
        env.insert("HARNESS_HOOK_CATEGORY".to_string(), category.clone());
    }
    if let Some(outcome) = context.outcome.as_ref() {
        env.insert("HARNESS_HOOK_OUTCOME".to_string(), outcome.clone());
    }
    if let Some(output_summary) = context.output_summary.as_ref() {
        env.insert(
            "HARNESS_HOOK_OUTPUT_SUMMARY".to_string(),
            output_summary.clone(),
        );
    }
    if let Some(failure_reason) = context.failure_reason.as_ref() {
        env.insert(
            "HARNESS_HOOK_FAILURE_REASON".to_string(),
            failure_reason.clone(),
        );
    }

    env.insert(
        "HARNESS_HOOK_CONTEXT_JSON".to_string(),
        json!({
            "hook_id": hook_name,
            "event": context.event.as_str(),
            "run_id": context.run_id,
            "workspace_root": context.workspace_root.display().to_string(),
            "artifacts_dir": context.artifacts_dir.display().to_string(),
            "cwd": cwd.display().to_string(),
            "actor": context.actor.as_ref().map(|actor| json!({
                "kind": format!("{:?}", actor.kind).to_ascii_lowercase(),
                "agent_id": actor.agent_id,
            })),
            "agent_id": context.agent_id,
            "request_id": context.request_id,
            "permission_id": context.permission_id,
            "task_id": context.task_id,
            "tool_call_id": context.tool_call_id,
            "tool_id": context.tool_id,
            "provider_id": context.provider_id,
            "model_id": context.model_id,
            "parent_agent_id": context.parent_agent_id,
            "category": context.category,
            "outcome": context.outcome,
            "output_summary": context.output_summary,
            "failure_reason": context.failure_reason,
        })
        .to_string(),
    );

    env
}

fn summarize_hook_output(stdout: &str, stderr: &str) -> String {
    let stdout = non_empty_trimmed(stdout);
    let stderr = non_empty_trimmed(stderr);
    let combined = if stderr.is_none() {
        stdout
    } else if stdout.is_none() {
        stderr
    } else {
        Some("stdout/stderr captured")
    };

    combined
        .map(|output| truncate_with_ellipsis(output, 160))
        .unwrap_or_else(|| "no output".to_string())
}

fn parse_hook_effects(stdout: &str, stderr: &str) -> Vec<HookEffectMetadata> {
    let redactor = DefaultRedactor::default();
    [stdout, stderr]
        .iter()
        .filter_map(|raw| parse_hook_effect_source(&redactor, raw))
        .flatten()
        .collect()
}

fn parse_hook_effect_source<R: Redactor + ?Sized>(
    redactor: &R,
    raw: &str,
) -> Option<Vec<HookEffectMetadata>> {
    let trimmed = non_empty_trimmed(raw)?;
    let json_value: Value = serde_json::from_str(trimmed).ok()?;
    let redacted = redact_value(redactor, &json_value);
    let mut effects = Vec::new();

    if let Some(items) = redacted.get("effects").and_then(Value::as_array) {
        for item in items {
            if let Some(effect) = parse_hook_effect_metadata(item) {
                effects.push(effect);
            }
        }
    }

    if let Some(items) = redacted.get("hook_effects").and_then(Value::as_array) {
        for item in items {
            if let Some(effect) = parse_hook_effect_metadata(item) {
                effects.push(effect);
            }
        }
    }

    if let Some(effect) = parse_hook_effect_metadata(&redacted) {
        effects.push(effect);
    }

    if let Some(items) = redacted.get("artifact_refs").and_then(Value::as_array) {
        for item in items {
            let Some(artifact_ref) = parse_hook_artifact_ref(item) else {
                continue;
            };
            effects.push(HookEffectMetadata {
                kind: HookEffectKind::WriteArtifact,
                summary: Some("hook wrote redacted artifact".to_string()),
                artifact_ref: Some(artifact_ref),
            });
        }
    }

    if effects.is_empty() {
        None
    } else {
        Some(deduplicate_hook_effects(effects))
    }
}

fn deduplicate_hook_effects(effects: Vec<HookEffectMetadata>) -> Vec<HookEffectMetadata> {
    let mut deduped = Vec::new();
    for effect in effects {
        if deduped.iter().any(|existing| existing == &effect) {
            continue;
        }
        deduped.push(effect);
    }
    deduped
}

fn parse_hook_effect_metadata(value: &Value) -> Option<HookEffectMetadata> {
    let object = value.as_object()?;
    let kind = extract_object_string(object, &["kind", "effect", "type", "action"])
        .and_then(|kind| parse_hook_effect_kind(&kind))?;
    let summary = extract_object_string(object, &["summary", "message", "reason", "diagnostic"])
        .map(|summary| truncate_with_ellipsis(&summary, 240));
    let artifact_ref = object
        .get("artifact_ref")
        .or_else(|| object.get("artifact"))
        .and_then(parse_hook_artifact_ref);

    Some(HookEffectMetadata {
        kind,
        summary,
        artifact_ref,
    })
}

fn parse_hook_artifact_ref(value: &Value) -> Option<EventArtifactRef> {
    let object = value.as_object()?;
    let path = extract_object_string(object, &["path", "rel_path", "relative_path"])?;
    let digest = extract_object_string(object, &["digest", "sha256", "blake3"]);
    Some(EventArtifactRef { path, digest })
}

fn parse_hook_effect_kind(kind: &str) -> Option<HookEffectKind> {
    match kind.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "allow" => Some(HookEffectKind::Allow),
        "deny" | "block" | "cancel" => Some(HookEffectKind::Deny),
        "modify_context" | "transform" | "transform_context" | "context_transform" => {
            Some(HookEffectKind::TransformContext)
        }
        "reminder" | "request_reminder" | "usage_reminder" => Some(HookEffectKind::RequestReminder),
        "artifact" | "write_artifact" | "redacted_artifact" => Some(HookEffectKind::WriteArtifact),
        "diagnostic" | "add_diagnostic" | "diagnostics" => Some(HookEffectKind::AddDiagnostic),
        "truncate" | "truncate_output" | "truncation" => Some(HookEffectKind::TruncateOutput),
        "recover" | "recovery" | "retry" => Some(HookEffectKind::Recover),
        "notify" | "notification" => Some(HookEffectKind::Notify),
        _ => None,
    }
}

async fn start_tool_call_execution<C, R>(
    clock: &C,
    redactor: &R,
    job_tx: mpsc::Sender<Command>,
    run_state: &mut RunState,
    hook_runtime_config: HookRuntimeConfig,
    args: ToolCallExecutionArgs,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let ToolCallExecutionArgs {
        tool_call_id,
        tool_id,
        args_json,
        actor,
        category,
        hook_executions,
        tool_registry,
        request_correlation_id,
        respond_to,
    } = args;
    let mut respond_to = respond_to;
    let tool_metadata = tool_identity_metadata(&tool_id, &args_json);

    let Some(tool) = tool_registry.get(&tool_id) else {
        append_payload_event(
            clock,
            redactor,
            run_state,
            actor,
            Some(format!("tool_call:{tool_call_id}")),
            EventV1::PolicyViolationDetected(PolicyViolationDetectedEvent {
                policy: "unknown_tool_id".to_string(),
                detail: format!("tool `{tool_id}` is not registered"),
            }),
        )?;

        append_failed_tool_call_finished_event(
            clock,
            redactor,
            run_state,
            &tool_call_id,
            "unknown tool",
            request_correlation_id.as_deref(),
            requested_tool_call_metadata(&tool_id, &args_json),
            &[],
        )?;
        return Err(CoordinatorError::PolicyViolation(format!(
            "tool `{tool_id}` is not registered"
        )));
    };

    let actor_kind = actor.kind;
    if !tool_registry.capability_allowed(actor_kind, tool.capability()) {
        append_payload_event(
            clock,
            redactor,
            run_state,
            actor,
            Some(format!("tool_call:{tool_call_id}")),
            EventV1::PolicyViolationDetected(PolicyViolationDetectedEvent {
                policy: "tool_capability_forbidden".to_string(),
                detail: format!(
                    "actor {:?} cannot call {} requiring {:?}",
                    actor_kind,
                    tool_id,
                    tool.capability()
                ),
            }),
        )?;

        append_failed_tool_call_finished_event(
            clock,
            redactor,
            run_state,
            &tool_call_id,
            "capability forbidden",
            request_correlation_id.as_deref(),
            requested_tool_call_metadata(&tool_id, &args_json),
            &[],
        )?;
        return Err(CoordinatorError::PolicyViolation(
            "tool capability forbidden for actor".to_string(),
        ));
    }

    let hashline_edit = hashline_edit_metadata(&tool_id, &args_json, &tool_call_id);

    append_tool_call_started_event(
        clock,
        redactor,
        run_state,
        &tool_call_id,
        request_correlation_id.as_deref(),
    )?;

    if let Some(metadata) = hashline_edit.as_ref() {
        append_edit_proposed_event(
            clock,
            redactor,
            run_state,
            &tool_call_id,
            metadata,
            request_correlation_id.as_deref(),
        )?;
    }

    let started_hook_batch = run_lifecycle_hooks(
        clock,
        &hook_runtime_config,
        HookInvocationContext {
            event: HookLifecycleEvent::ToolCallStarted,
            run_id: run_state.info.run_id.clone(),
            workspace_root: run_state.info.workspace_root.clone(),
            artifacts_dir: run_state.info.artifacts_dir.clone(),
            actor: Some(actor.clone()),
            agent_id: actor.agent_id.clone(),
            request_id: request_correlation_id.clone(),
            permission_id: None,
            task_id: None,
            tool_call_id: Some(tool_call_id.clone()),
            tool_id: Some(tool_id.clone()),
            provider_id: None,
            model_id: None,
            parent_agent_id: None,
            category: category.clone(),
            outcome: Some("started".to_string()),
            output_summary: None,
            failure_reason: None,
        },
    )
    .await;
    let mut initial_hook_executions = hook_executions;
    initial_hook_executions.extend(started_hook_batch.hook_executions.clone());
    if let Some(reason) = started_hook_batch.critical_failure.clone() {
        append_failed_tool_call_finished_event(
            clock,
            redactor,
            run_state,
            &tool_call_id,
            &reason,
            request_correlation_id.as_deref(),
            tool_call_metadata(
                tool_metadata.as_ref(),
                None,
                Vec::new(),
                None,
                initial_hook_executions.clone(),
            ),
            &initial_hook_executions,
        )?;
        if let Some(respond_to) = respond_to.take() {
            let _ = respond_to.send(Err(reason.clone()));
        }
        return Err(CoordinatorError::LifecycleHookFailed(reason.to_string()));
    }

    let task_id = format!("task_{:06}", run_state.next_task_id);
    run_state.next_task_id += 1;

    let queue_key = ConcurrencyKey::Tool {
        tool_id: tool_id.clone(),
    };

    append_payload_event_with_correlation(
        clock,
        redactor,
        run_state,
        actor.clone(),
        Some(format!("task:{task_id}")),
        request_correlation_id.clone(),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: task_id.clone(),
            state: TaskScheduleState::Started,
            queue_key: Some(queue_key.queue_key()),
        }),
    )?;

    let cancellation_token = run_state.shutdown_token.child_token();
    let run_id = run_state.info.run_id.clone();
    let workspace_root = run_state.info.workspace_root.clone();
    let artifacts_dir = run_state.info.artifacts_dir.clone();
    let coordinator = CoordinatorHandle { tx: job_tx.clone() };
    let current_model = actor.agent_id.as_deref().and_then(|agent_id| {
        run_state
            .running_agent_turns
            .values()
            .find(|turn| turn.agent_id == agent_id)
            .map(|turn| (turn.model_ref.clone(), turn.model_settings.clone()))
    });
    run_state.tasks.insert(
        task_id.clone(),
        TaskState {
            tool_call_id: tool_call_id.clone(),
            tool_metadata,
            owner_actor: actor.clone(),
            request_correlation_id,
            queue_key,
            state: TaskExecutionState::Running,
            cancellation_token: cancellation_token.clone(),
            started_mono_ms: clock.mono_ms(),
            last_progress_mono_ms: clock.mono_ms(),
            last_progress_kind: JobProgressKind::Heartbeat,
            hashline_edit,
            respond_to,
        },
    );
    run_state.task_hook_state.insert(
        task_id.clone(),
        TaskHookState {
            tool_id: tool_id.clone(),
            category: category.clone(),
            hook_executions: initial_hook_executions,
        },
    );

    tokio::spawn(async move {
        let _ = job_tx
            .send(Command::JobProgress {
                task_id: task_id.clone(),
                kind: JobProgressKind::Heartbeat,
            })
            .await;

        let context = ToolContext {
            run_id,
            workspace_root,
            artifacts_dir,
            actor,
            category,
            tool_call_id: tool_call_id.clone(),
            current_model_ref: current_model
                .as_ref()
                .map(|(model_ref, _)| model_ref.clone()),
            current_model_settings: current_model.as_ref().map(|(_, settings)| settings.clone()),
            coordinator,
        };

        tokio::select! {
            _ = cancellation_token.cancelled() => {
                let _ = job_tx
                    .send(Command::JobFinished {
                        task_id,
                        outcome: JobOutcome::Cancelled {
                            reason: "job cancelled".to_string(),
                        },
                    })
                    .await;
            }
            result = tool.call(context, args_json) => {
                let outcome = match result {
                    Ok(result) => JobOutcome::Succeeded { result },
                    Err(err) => JobOutcome::Failed {
                        error: err.to_string(),
                    },
                };

                let _ = job_tx
                    .send(Command::JobFinished {
                        task_id,
                        outcome,
                    })
                    .await;
            }
        }
    });

    Ok(())
}

fn nested_provider_model_queue_key(
    run_state: &RunState,
    agent_id: &str,
    provider_id: String,
    model_id: String,
    base_queue_key: ConcurrencyKey,
) -> ConcurrencyKey {
    let Some(parent_agent_id) = run_state.subagent_parent_by_id.get(agent_id) else {
        return base_queue_key;
    };
    let base_queue_key_display = base_queue_key.queue_key();
    let parent_holds_same_model = run_state.running_agent_turns.values().any(|turn| {
        turn.agent_id == *parent_agent_id && turn.queue_key.queue_key() == base_queue_key_display
    });
    if !parent_holds_same_model {
        return base_queue_key;
    }

    ConcurrencyKey::NestedProviderModel {
        provider_id,
        model_id,
        parent_agent_id: parent_agent_id.clone(),
        agent_id: agent_id.to_string(),
    }
}

async fn schedule_agent_turn<C, R>(
    clock: &C,
    redactor: &R,
    job_tx: mpsc::Sender<Command>,
    run_state: &mut RunState,
    hook_runtime_config: HookRuntimeConfig,
    compaction_config: CompactionRuntimeConfig,
    args: ScheduleAgentTurnArgs,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let ScheduleAgentTurnArgs {
        provider,
        tool_registry,
        profile,
        request,
        request_id,
        child_task,
    } = args;
    let model = crate::agent::AgentModelRef::parse(&request.model_ref);
    let agent_id = request.agent_id.clone();
    let task_id = format!("task_{:06}", run_state.next_task_id);
    run_state.next_task_id += 1;

    let provider_id = model.provider_id.clone();
    let model_id = model.model_id.clone();
    let base_queue_key = ConcurrencyKey::ProviderModel {
        provider_id: model.provider_id,
        model_id: model.model_id,
    };
    let queue_key = nested_provider_model_queue_key(
        run_state,
        &agent_id,
        provider_id,
        model_id,
        base_queue_key,
    );

    if agent_has_active_or_queued_turn(run_state, &agent_id) {
        append_agent_turn_task_scheduled_event(
            clock,
            redactor,
            run_state,
            AgentTurnTaskScheduledEventArgs {
                task_id: &task_id,
                agent_id: &agent_id,
                request_id: &request_id,
                queue_key: &queue_key,
                state: TaskScheduleState::Queued,
            },
        )?;

        run_state.queued_agent_turns.insert(
            task_id.clone(),
            QueuedAgentTurn {
                task_id,
                agent_id,
                request_id,
                profile,
                request,
                queue_key,
                scheduler_queued: false,
                child_task,
            },
        );

        return Ok(());
    }

    match run_state
        .scheduler
        .schedule(task_id.clone(), queue_key.clone())
    {
        ScheduleDecision::Started(_) => {
            append_agent_turn_task_scheduled_event(
                clock,
                redactor,
                run_state,
                AgentTurnTaskScheduledEventArgs {
                    task_id: &task_id,
                    agent_id: &agent_id,
                    request_id: &request_id,
                    queue_key: &queue_key,
                    state: TaskScheduleState::Started,
                },
            )?;

            start_agent_turn_execution(
                clock,
                redactor,
                job_tx,
                run_state,
                hook_runtime_config,
                compaction_config,
                provider,
                tool_registry,
                QueuedAgentTurn {
                    task_id,
                    agent_id,
                    request_id,
                    profile,
                    request,
                    queue_key,
                    scheduler_queued: false,
                    child_task,
                },
            )
            .await?;
        }
        ScheduleDecision::Queued(_) => {
            append_agent_turn_task_scheduled_event(
                clock,
                redactor,
                run_state,
                AgentTurnTaskScheduledEventArgs {
                    task_id: &task_id,
                    agent_id: &agent_id,
                    request_id: &request_id,
                    queue_key: &queue_key,
                    state: TaskScheduleState::Queued,
                },
            )?;

            run_state.queued_agent_turns.insert(
                task_id.clone(),
                QueuedAgentTurn {
                    task_id,
                    agent_id,
                    request_id,
                    profile,
                    request,
                    queue_key,
                    scheduler_queued: true,
                    child_task,
                },
            );
        }
    }

    Ok(())
}

fn append_agent_turn_task_scheduled_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    args: AgentTurnTaskScheduledEventArgs<'_>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let AgentTurnTaskScheduledEventArgs {
        task_id,
        agent_id,
        request_id,
        queue_key,
        state,
    } = args;

    append_payload_event_with_correlation(
        clock,
        redactor,
        run_state,
        agent_actor(agent_id),
        Some(format!("task:{task_id}")),
        Some(request_id.to_string()),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: task_id.to_string(),
            state,
            queue_key: Some(queue_key.queue_key()),
        }),
    )
}

fn recompute_provider_context_for_agent(run_state: &RunState, agent_id: &str) -> ProviderContext {
    run_state
        .provider_context_by_agent
        .get(agent_id)
        .cloned()
        .unwrap_or_default()
}

fn provider_request_started_metadata(
    metadata: Option<ProviderRequestStartedMetadata>,
    turn_request_id: &str,
    provider_request_id: &str,
) -> Option<ProviderRequestStartedMetadata> {
    let mut metadata = metadata.unwrap_or_default();
    metadata
        .turn_id
        .get_or_insert_with(|| turn_request_id.to_string());
    metadata
        .provider_call_id
        .get_or_insert_with(|| provider_request_id.to_string());
    Some(metadata)
}

fn provider_request_finished_metadata(
    metadata: Option<ProviderRequestFinishedMetadata>,
    turn_request_id: &str,
    provider_request_id: &str,
) -> Option<ProviderRequestFinishedMetadata> {
    let mut metadata = metadata.unwrap_or_default();
    metadata
        .turn_id
        .get_or_insert_with(|| turn_request_id.to_string());
    metadata
        .provider_call_id
        .get_or_insert_with(|| provider_request_id.to_string());
    Some(metadata)
}

async fn run_turn_start_phase<C>(
    clock: &C,
    run_state: &mut RunState,
    hook_runtime_config: &HookRuntimeConfig,
    task: &QueuedAgentTurn,
) -> TurnStartPhaseResult
where
    C: Clock + ?Sized,
{
    let cancellation_token = run_state.shutdown_token.child_token();
    let category = Some(task.profile.category.clone());
    let mut hook_executions = run_state
        .agent_hook_state
        .remove(&task.agent_id)
        .unwrap_or_default();

    let started_hook_batch = run_lifecycle_hooks(
        clock,
        hook_runtime_config,
        HookInvocationContext {
            event: HookLifecycleEvent::AgentTurnStarted,
            run_id: run_state.info.run_id.clone(),
            workspace_root: run_state.info.workspace_root.clone(),
            artifacts_dir: run_state.info.artifacts_dir.clone(),
            actor: Some(agent_actor(&task.agent_id)),
            agent_id: Some(task.agent_id.clone()),
            request_id: Some(task.request_id.clone()),
            permission_id: None,
            task_id: Some(task.task_id.clone()),
            tool_call_id: None,
            tool_id: None,
            provider_id: None,
            model_id: None,
            parent_agent_id: None,
            category: category.clone(),
            outcome: Some("started".to_string()),
            output_summary: Some(task.request.prompt.clone()),
            failure_reason: None,
        },
    )
    .await;
    hook_executions.extend(started_hook_batch.hook_executions.clone());

    run_state.running_agent_turns.insert(
        task.task_id.clone(),
        RunningAgentTurn {
            agent_id: task.agent_id.clone(),
            request_id: task.request_id.clone(),
            request_prompt: task.request.prompt.clone(),
            profile_name: task.profile.name.clone(),
            model_ref: task.request.model_ref.clone(),
            model_settings: task.request.model_settings.clone(),
            category,
            queue_key: task.queue_key.clone(),
            cancellation_token: cancellation_token.clone(),
            started_mono_ms: clock.mono_ms(),
            hook_executions,
            latest_provider_usage: None,
            latest_provider_request_id: None,
            latest_assistant_output: None,
            latest_provider_id: None,
            latest_model_id: None,
            child_task: task.child_task.clone(),
        },
    );

    TurnStartPhaseResult {
        cancellation_token,
        critical_failure: started_hook_batch.critical_failure,
    }
}

async fn request_agent_context_compaction(
    job_tx: &mpsc::Sender<Command>,
    task: &QueuedAgentTurn,
    trigger_reason: &str,
    usage: Option<harness_providers::CompletionUsage>,
) -> Result<ProviderContext, CoordinatorError> {
    let (respond_to, response_rx) = oneshot::channel();
    job_tx
        .send(Command::CompactAgentContext {
            task_id: task.task_id.clone(),
            agent_id: task.agent_id.clone(),
            request_id: task.request_id.clone(),
            trigger_reason: trigger_reason.to_string(),
            usage,
            respond_to,
        })
        .await
        .map_err(|_| CoordinatorError::CommandChannelClosed)?;

    response_rx
        .await
        .map_err(|_| CoordinatorError::ResponseChannelClosed)?
}

#[expect(
    clippy::too_many_arguments,
    reason = "coordinator launch wiring intentionally passes explicit runtime dependencies"
)]
async fn start_agent_turn_execution<C, R>(
    clock: &C,
    _redactor: &R,
    job_tx: mpsc::Sender<Command>,
    run_state: &mut RunState,
    hook_runtime_config: HookRuntimeConfig,
    compaction_config: CompactionRuntimeConfig,
    provider: Arc<dyn Provider>,
    tool_registry: Arc<ToolRegistry>,
    task: QueuedAgentTurn,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let turn_start = run_turn_start_phase(clock, run_state, &hook_runtime_config, &task).await;
    let cancellation_token = turn_start.cancellation_token;

    if let Some(reason) = turn_start.critical_failure {
        warn_command_send_failure(
            job_tx
                .send(Command::AgentTurnFinished {
                    task_id: task.task_id,
                    agent_id: task.agent_id,
                    request_id: task.request_id,
                    outcome: AgentTurnTaskOutcome::Failed {
                        reason,
                        memory: None,
                    },
                })
                .await,
            "agent_turn_finished_from_hook_failure",
        );
        return Ok(());
    }

    let provider_context = recompute_provider_context_for_agent(run_state, &task.agent_id);

    tokio::spawn(async move {
        let task_id = task.task_id.clone();
        let agent_id = task.agent_id.clone();
        let request_id = task.request_id.clone();

        tokio::select! {
            _ = cancellation_token.cancelled() => {
                warn_command_send_failure(job_tx.send(Command::AgentTurnFinished {
                    task_id,
                    agent_id,
                    request_id,
                    outcome: AgentTurnTaskOutcome::Failed {
                        reason: "job cancelled".to_string(),
                        memory: Some(AgentTurnFailureMemory::aborted(
                            "cancelled",
                            "job cancelled",
                            "",
                            None,
                        )),
                    },
                }).await, "agent_turn_finished_from_cancellation");
            }
            outcome = async {
                let mut prior_context = provider_context;
                let mut overflow_retry_attempted = false;

                let pre_prompt_critical_failure = match request_agent_context_compaction(
                    &job_tx,
                    &task,
                    "pre_prompt",
                    None,
                )
                .await
                {
                    Ok(compacted_context) => {
                        prior_context = compacted_context;
                        None
                    }
                    Err(CoordinatorError::LifecycleHookFailed(reason)) => Some(format!(
                        "pre-prompt compaction critical lifecycle hook failed: {reason}"
                    )),
                    Err(err) => {
                        tracing::warn!(
                            agent_id = %task.agent_id,
                            request_id = %task.request_id,
                            error = %err,
                            "pre-prompt provider context compaction failed; continuing without checkpoint"
                        );
                        None
                    }
                };

                if let Some(reason) = pre_prompt_critical_failure {
                    AgentTurnOutcome::failed(reason)
                } else {
                    loop {
                    let outcome = run_agent_turn_phase_loop(AgentTurnPhaseLoopRequest {
                        provider: provider.clone(),
                        tool_registry: tool_registry.clone(),
                        task: &task,
                        prior_context: &prior_context,
                        job_tx: job_tx.clone(),
                        cancellation_token: cancellation_token.clone(),
                        allow_context_window_fallback: overflow_retry_attempted,
                    })
                    .await;

                    match &outcome {
                        AgentTurnOutcome::Failed { reason, memory }
                            if compaction_config.auto_retry_overflow
                                && !overflow_retry_attempted
                                && is_provider_context_overflow_reason(reason) =>
                        {
                            match request_agent_context_compaction(
                                &job_tx,
                                &task,
                                "overflow_retry",
                                None,
                            )
                            .await
                            {
                                Ok(compacted_context) => {
                                    overflow_retry_attempted = true;
                                    prior_context = compacted_context;
                                    continue;
                                }
                                Err(err) => {
                                    let reason = format!(
                                        "{reason}; overflow compaction failed: {err}"
                                    );
                                    let mut memory = memory.clone();
                                    if let Some(memory) = &mut memory {
                                        memory.reason = reason.clone();
                                    }
                                    break AgentTurnOutcome::Failed { reason, memory };
                                }
                            }
                        }
                        AgentTurnOutcome::Failed { reason, memory }
                            if overflow_retry_attempted
                                && is_provider_context_overflow_reason(reason) =>
                        {
                            let reason = format!(
                                "{reason}; overflow persisted after checkpoint compaction; likely the active prompt or latest preserved turn still exceeds the provider window"
                            );
                            let mut memory = memory.clone().unwrap_or_else(|| {
                                AgentTurnFailure::new(
                                    ProviderConversationTurnStatus::Failed,
                                    "overflow_retry_failed",
                                    reason.clone(),
                                    "",
                                    None,
                                )
                            });
                            memory.status = ProviderConversationTurnStatus::Failed;
                            memory.failure_stage = "overflow_retry_failed".to_string();
                            memory.reason = reason.clone();
                            break AgentTurnOutcome::Failed {
                                reason,
                                memory: Some(memory),
                            };
                        }
                        _ => break outcome,
                    }
                    }
                }
            } => {
                let outcome = match outcome {
                    AgentTurnOutcome::Succeeded {
                        output,
                        messages,
                    } => AgentTurnTaskOutcome::Succeeded {
                        output,
                        messages,
                    },
                    AgentTurnOutcome::Failed { reason, memory } => AgentTurnTaskOutcome::Failed {
                        reason,
                        memory: memory.map(AgentTurnFailureMemory::from),
                    },
                };
                warn_command_send_failure(job_tx.send(Command::AgentTurnFinished {
                    task_id: task.task_id,
                    agent_id: task.agent_id,
                    request_id: task.request_id,
                    outcome,
                }).await, "agent_turn_finished");
            }
        }
    });

    Ok(())
}

struct AgentTurnPhaseLoopRequest<'a> {
    provider: Arc<dyn Provider>,
    tool_registry: Arc<ToolRegistry>,
    task: &'a QueuedAgentTurn,
    prior_context: &'a ProviderContext,
    job_tx: mpsc::Sender<Command>,
    cancellation_token: CancellationToken,
    allow_context_window_fallback: bool,
}

struct AgentProviderTurnState {
    model: AgentModelRef,
    model_settings: AgentModelSettings,
    tool_defs: Vec<ToolDef>,
    messages: Vec<CompletionMessage>,
    total_tool_calls: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderFallbackAttempt {
    attempt: u32,
    from_model_ref: String,
    reason_class: String,
    retryable: bool,
}

enum AgentToolPhaseDecision {
    RunTools(Vec<AssistantToolIntent>),
    TurnEnd { output: String },
}

struct ProviderStreamPhaseRequest<'a> {
    provider: Arc<dyn Provider>,
    profile: &'a AgentProfile,
    request: &'a AgentRequest,
    turn_request_id: &'a str,
    provider_request_id: String,
    model: AgentModelRef,
    messages: &'a [CompletionMessage],
    tool_defs: &'a [ToolDef],
    job_tx: mpsc::Sender<Command>,
    task_id: &'a str,
    agent_id: &'a str,
    fallback_attempt: Option<ProviderFallbackAttempt>,
    model_settings: AgentModelSettings,
}

async fn run_agent_turn_phase_loop(request: AgentTurnPhaseLoopRequest<'_>) -> AgentTurnOutcome {
    let AgentTurnPhaseLoopRequest {
        provider,
        tool_registry,
        task,
        prior_context,
        job_tx,
        cancellation_token,
        allow_context_window_fallback,
    } = request;

    let mut turn_state = match prepare_provider_transform_phase(
        &task.profile,
        &task.request,
        prior_context,
        tool_registry.as_ref(),
    ) {
        Ok(turn_state) => turn_state,
        Err(reason) => return AgentTurnOutcome::failed(reason),
    };
    let current_turn_start_index = turn_state.messages.len().saturating_sub(1);
    let mut fallback_models = fallback_models_with_settings(&task.request);
    let mut fallback_cooldowns = BTreeSet::from([task.request.model_ref.clone()]);
    fallback_models.retain(|(model_ref, _)| !fallback_cooldowns.contains(model_ref));
    let mut fallback_attempt: Option<ProviderFallbackAttempt> = None;
    let mut fallback_attempt_count = 0_u32;

    loop {
        if cancellation_token.is_cancelled() {
            return AgentTurnOutcome::failed_with_memory(
                "job cancelled",
                AgentTurnFailure::new(
                    ProviderConversationTurnStatus::Aborted,
                    "cancelled",
                    "job cancelled",
                    "",
                    None,
                ),
            );
        }

        let provider_request_id = match allocate_provider_request_id_phase(&job_tx).await {
            Ok(request_id) => request_id,
            Err(reason) => return AgentTurnOutcome::failed(reason),
        };

        let assistant_response = match run_provider_stream_phase(ProviderStreamPhaseRequest {
            provider: provider.clone(),
            profile: &task.profile,
            request: &task.request,
            turn_request_id: &task.request_id,
            provider_request_id,
            model: turn_state.model.clone(),
            messages: &turn_state.messages,
            tool_defs: &turn_state.tool_defs,
            job_tx: job_tx.clone(),
            task_id: &task.task_id,
            agent_id: &task.agent_id,
            fallback_attempt: fallback_attempt.take(),
            model_settings: turn_state.model_settings.clone(),
        })
        .await
        {
            Ok(response) => response,
            Err(mut failure) => {
                let reason = normalize_provider_phase_error(failure.to_string());
                failure.reason = reason.clone();
                let class = classify_provider_error(&reason);
                let can_try_fallback = class.is_retryable()
                    && (!matches!(
                        class,
                        crate::provider_recovery::ProviderErrorClass::ContextWindow
                    ) || allow_context_window_fallback);
                if can_try_fallback {
                    let from_model_ref = agent_model_ref_to_model_ref(&turn_state.model);
                    fallback_cooldowns.insert(from_model_ref.clone());
                    while let Some((next_model_ref, next_model_settings)) =
                        fallback_models.first().cloned()
                    {
                        fallback_models.remove(0);
                        if fallback_cooldowns.contains(&next_model_ref) {
                            continue;
                        }
                        let slot_acquired = match switch_agent_turn_provider_model_slot_phase(
                            &job_tx,
                            &task.task_id,
                            &task.agent_id,
                            &next_model_ref,
                            next_model_settings.clone(),
                        )
                        .await
                        {
                            Ok(acquired) => acquired,
                            Err(reason) => return AgentTurnOutcome::failed(reason),
                        };
                        if !slot_acquired {
                            fallback_cooldowns.insert(next_model_ref);
                            continue;
                        }
                        turn_state.model = AgentModelRef::parse(&next_model_ref);
                        turn_state.model_settings = next_model_settings;
                        fallback_attempt_count = fallback_attempt_count.saturating_add(1);
                        fallback_attempt = Some(ProviderFallbackAttempt {
                            attempt: fallback_attempt_count,
                            from_model_ref,
                            reason_class: class.as_str().to_string(),
                            retryable: true,
                        });
                        break;
                    }
                    if fallback_attempt.is_some() {
                        continue;
                    }
                }
                return AgentTurnOutcome::Failed {
                    reason,
                    memory: (failure.failure_stage == "provider_error").then_some(failure),
                };
            }
        };
        if let Err(reason) = append_assistant_message_end_phase(
            &job_tx,
            &task.task_id,
            &task.agent_id,
            &mut turn_state.messages,
            &assistant_response,
        )
        .await
        {
            return AgentTurnOutcome::failed(reason);
        }
        if cancellation_token.is_cancelled() {
            return AgentTurnOutcome::failed_with_memory(
                "job cancelled",
                AgentTurnFailure::new(
                    ProviderConversationTurnStatus::Aborted,
                    "cancelled",
                    "job cancelled",
                    assistant_response.text.clone(),
                    Some(assistant_response.request_id.clone()),
                ),
            );
        }

        match decide_tool_phase(&assistant_response, &mut turn_state.total_tool_calls) {
            Ok(AgentToolPhaseDecision::TurnEnd { output }) => {
                return AgentTurnOutcome::Succeeded {
                    output,
                    messages: completion_messages_to_conversation_messages(
                        &task.profile,
                        &task.request_id,
                        &task.agent_id,
                        &turn_state.messages[current_turn_start_index..],
                    ),
                };
            }
            Ok(AgentToolPhaseDecision::RunTools(tool_intents)) => {
                if let Err(reason) = run_tool_phase(
                    &job_tx,
                    &task.agent_id,
                    Some(task.profile.category.clone()),
                    &task.profile,
                    &mut turn_state.messages,
                    tool_intents,
                )
                .await
                {
                    return AgentTurnOutcome::failed_with_memory(
                        reason.clone(),
                        AgentTurnFailure::new(
                            ProviderConversationTurnStatus::Failed,
                            "tool_failure",
                            reason,
                            assistant_response.text.clone(),
                            Some(assistant_response.request_id.clone()),
                        ),
                    );
                }
            }
            Err(reason) => return AgentTurnOutcome::failed(reason),
        }

        if cancellation_token.is_cancelled() {
            return AgentTurnOutcome::failed_with_memory(
                "job cancelled",
                AgentTurnFailure::new(
                    ProviderConversationTurnStatus::Aborted,
                    "cancelled",
                    "job cancelled",
                    assistant_response.text.clone(),
                    Some(assistant_response.request_id.clone()),
                ),
            );
        }
    }
}

fn agent_model_ref_to_model_ref(model: &AgentModelRef) -> String {
    format!("{}:{}", model.provider_id, model.model_id)
}

fn normalize_provider_phase_error(reason: String) -> String {
    if reason.contains("empty tool_call_id") && !reason.contains("invalid") {
        format!("invalid provider tool_call_id: {reason}")
    } else {
        reason
    }
}

async fn generate_harness_session_title(
    provider: Arc<dyn Provider>,
    profile: AgentProfile,
    prompt: &str,
) -> Result<Option<String>, String> {
    let model = AgentModelRef::parse(&profile.model_ref);
    let mut stream = provider
        .stream_completion(CompletionRequest {
            provider_id: Some(model.provider_id),
            model_id: model.model_id,
            messages: vec![
                CompletionMessage {
                    role: MessageRole::System,
                    content: profile.system_prompt,
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
                CompletionMessage {
                    role: MessageRole::User,
                    content: TITLE_GENERATION_USER_PROMPT.to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
                CompletionMessage {
                    role: MessageRole::User,
                    content: prompt.to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
            ],
            temperature: profile.temperature,
            max_tokens: None,
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            tools: None,
            tool_choice: None,
            stream: true,
        })
        .await;

    let mut text = String::new();
    while let Some(event) = stream.next().await {
        match event {
            ProviderStreamEvent::TextDelta(delta) => text.push_str(&delta),
            ProviderStreamEvent::Error { message } => return Err(message),
            ProviderStreamEvent::Start
            | ProviderStreamEvent::Started { .. }
            | ProviderStreamEvent::ReasoningDelta(_)
            | ProviderStreamEvent::ToolCallDelta { .. }
            | ProviderStreamEvent::ToolCallComplete { .. }
            | ProviderStreamEvent::Done { .. }
            | ProviderStreamEvent::DoneWithMetadata { .. } => {}
        }
    }

    Ok(clean_generated_title(&text))
}

fn prepare_provider_transform_phase(
    profile: &AgentProfile,
    request: &AgentRequest,
    prior_context: &ProviderContext,
    tool_registry: &ToolRegistry,
) -> Result<AgentProviderTurnState, String> {
    let model = AgentModelRef::parse(&request.model_ref);
    let tool_defs = build_provider_tool_defs(profile, tool_registry)?;
    let provider_prompt = request.provider_prompt();
    let messages = build_provider_context_messages(profile, prior_context, &provider_prompt);

    Ok(AgentProviderTurnState {
        model,
        model_settings: request.model_settings.clone(),
        tool_defs,
        messages,
        total_tool_calls: 0,
    })
}

fn fallback_models_with_settings(request: &AgentRequest) -> Vec<(String, AgentModelSettings)> {
    request
        .fallback_model_refs
        .iter()
        .enumerate()
        .map(|(index, model_ref)| {
            (
                model_ref.clone(),
                request
                    .fallback_model_settings
                    .get(index)
                    .cloned()
                    .unwrap_or_default(),
            )
        })
        .collect()
}

async fn switch_agent_turn_provider_model_slot_phase(
    job_tx: &mpsc::Sender<Command>,
    task_id: &str,
    agent_id: &str,
    model_ref: &str,
    model_settings: AgentModelSettings,
) -> Result<bool, String> {
    let (respond_to, response) = oneshot::channel();
    job_tx
        .send(Command::SwitchAgentTurnProviderModelSlot {
            task_id: task_id.to_string(),
            agent_id: agent_id.to_string(),
            model_ref: model_ref.to_string(),
            model_settings,
            respond_to,
        })
        .await
        .map_err(|err| format!("failed to switch provider model scheduler slot: {err}"))?;
    response
        .await
        .map_err(|_| "coordinator dropped provider model scheduler slot switch".to_string())?
        .map_err(|err| err.to_string())
}

async fn allocate_provider_request_id_phase(
    job_tx: &mpsc::Sender<Command>,
) -> Result<String, String> {
    let (respond_to, response_rx) = oneshot::channel();
    job_tx
        .send(Command::AllocateProviderRequestId { respond_to })
        .await
        .map_err(|_| "provider request id channel closed".to_string())?;
    response_rx
        .await
        .map_err(|_| "provider request id response channel closed".to_string())?
        .map_err(|err| err.to_string())
}

async fn run_provider_stream_phase(
    request: ProviderStreamPhaseRequest<'_>,
) -> Result<AssistantResponse, AgentTurnFailure> {
    let ProviderStreamPhaseRequest {
        provider,
        profile,
        request,
        turn_request_id,
        provider_request_id,
        model,
        messages,
        tool_defs,
        job_tx,
        task_id,
        agent_id,
        fallback_attempt,
        model_settings,
    } = request;
    let task_id = task_id.to_string();
    let agent_id = agent_id.to_string();

    stream_assistant_response_once(
        StreamAssistantResponseOnceRequest {
            provider,
            profile,
            model,
            model_settings,
            turn_request_id: turn_request_id.to_string(),
            provider_request_id,
            prompt_summary: &request.prompt,
            context: ProviderBoundaryContext::ProviderMessages { messages },
            tool_defs,
        },
        |event| {
            let event = apply_fallback_metadata(event, fallback_attempt.as_ref());
            let job_tx = job_tx.clone();
            let task_id = task_id.clone();
            let agent_id = agent_id.clone();
            async move {
                if let Err(reason) =
                    emit_agent_runtime_event_phase(job_tx, task_id, agent_id, event).await
                {
                    tracing::warn!(reason, "failed to emit agent runtime phase event");
                }
            }
        },
    )
    .await
}

fn apply_fallback_metadata(
    event: AgentRuntimeEvent,
    fallback: Option<&ProviderFallbackAttempt>,
) -> AgentRuntimeEvent {
    let Some(fallback) = fallback else {
        return event;
    };

    match event {
        AgentRuntimeEvent::ProviderRequestStarted(mut started) => {
            let metadata = started.metadata.get_or_insert_with(Default::default);
            metadata.fallback_attempt = Some(fallback.attempt);
            metadata.fallback_from_model_ref = Some(fallback.from_model_ref.clone());
            metadata.fallback_reason_class = Some(fallback.reason_class.clone());
            metadata.fallback_retryable = Some(fallback.retryable);
            AgentRuntimeEvent::ProviderRequestStarted(started)
        }
        AgentRuntimeEvent::ProviderRequestFinished(mut finished) => {
            let metadata = finished.metadata.get_or_insert_with(Default::default);
            metadata.fallback_attempt = Some(fallback.attempt);
            metadata.fallback_from_model_ref = Some(fallback.from_model_ref.clone());
            metadata.fallback_reason_class = Some(fallback.reason_class.clone());
            metadata.fallback_retryable = Some(fallback.retryable);
            AgentRuntimeEvent::ProviderRequestFinished(finished)
        }
        event => event,
    }
}

async fn emit_agent_runtime_event_phase(
    job_tx: mpsc::Sender<Command>,
    task_id: String,
    agent_id: String,
    event: AgentRuntimeEvent,
) -> Result<(), String> {
    match event {
        AgentRuntimeEvent::ProviderRequestStarted(started) => job_tx
            .send(Command::AgentProviderRequestStarted {
                task_id,
                agent_id,
                request_id: started.request_id,
                provider_id: started.provider_id,
                model_id: started.model_id,
                prompt_summary: started.prompt_summary,
                request_digest: started.request_digest,
                metadata: started.metadata,
            })
            .await
            .map_err(|_| "provider request start channel closed".to_string()),
        AgentRuntimeEvent::ProviderStreamDelta { request_id, delta } => job_tx
            .send(Command::AgentProviderStreamDelta {
                task_id,
                agent_id,
                request_id,
                delta,
            })
            .await
            .map_err(|_| "provider stream delta channel closed".to_string()),
        AgentRuntimeEvent::ProviderReasoningDelta { request_id, delta } => job_tx
            .send(Command::AgentProviderReasoningDelta {
                task_id,
                agent_id,
                request_id,
                delta,
            })
            .await
            .map_err(|_| "provider reasoning delta channel closed".to_string()),
        AgentRuntimeEvent::ProviderRequestFinished(finished) => {
            let (respond_to, response_rx) = oneshot::channel();
            job_tx
                .send(Command::AgentProviderRequestFinished {
                    task_id,
                    agent_id,
                    request_id: finished.request_id,
                    finish_reason: finished.finish_reason,
                    output_digest: finished.output_digest,
                    usage: finished.usage,
                    metadata: finished.metadata,
                    respond_to: Some(respond_to),
                })
                .await
                .map_err(|_| "provider request finish channel closed".to_string())?;
            response_rx
                .await
                .map_err(|_| "provider request finish response channel closed".to_string())?
                .map_err(|err| err.to_string())
        }
    }
}

async fn append_assistant_message_end_phase(
    job_tx: &mpsc::Sender<Command>,
    task_id: &str,
    agent_id: &str,
    messages: &mut Vec<CompletionMessage>,
    response: &AssistantResponse,
) -> Result<(), String> {
    let assistant_tool_calls = (!response.tool_intents.is_empty()).then(|| {
        response
            .tool_intents
            .iter()
            .map(|tool_call| AssistantToolCall {
                tool_call_id: tool_call.tool_call_id.clone(),
                function_name: tool_call.function_name.clone(),
                arguments_json: tool_call.arguments_json.clone(),
            })
            .collect::<Vec<_>>()
    });

    messages.push(CompletionMessage {
        role: MessageRole::Assistant,
        content: response.text.clone(),
        name: None,
        tool_call_id: None,
        assistant_tool_calls,
    });

    let (respond_to, response_rx) = oneshot::channel();
    job_tx
        .send(Command::AgentAssistantMessageFinished {
            task_id: task_id.to_string(),
            agent_id: agent_id.to_string(),
            request_id: response.request_id.clone(),
            assistant_output: response.text.clone(),
            tool_call_count: response.tool_intents.len(),
            assistant_message: response.finished_metadata.assistant_message.clone(),
            respond_to,
        })
        .await
        .map_err(|_| "assistant message finish channel closed".to_string())?;
    response_rx
        .await
        .map_err(|_| "assistant message finish response channel closed".to_string())?
        .map_err(|err| err.to_string())
}

fn decide_tool_phase(
    response: &AssistantResponse,
    total_tool_calls: &mut usize,
) -> Result<AgentToolPhaseDecision, String> {
    if response.tool_intents.is_empty() {
        return Ok(AgentToolPhaseDecision::TurnEnd {
            output: response.text.clone(),
        });
    }

    *total_tool_calls += response.tool_intents.len();
    if *total_tool_calls > MAX_TOOL_CALLS_TOTAL {
        return Err(format!(
            "agent turn exceeded MAX_TOOL_CALLS_TOTAL={MAX_TOOL_CALLS_TOTAL}"
        ));
    }

    Ok(AgentToolPhaseDecision::RunTools(
        response.tool_intents.clone(),
    ))
}

async fn run_tool_phase(
    job_tx: &mpsc::Sender<Command>,
    agent_id: &str,
    category: Option<String>,
    profile: &AgentProfile,
    messages: &mut Vec<CompletionMessage>,
    tool_intents: Vec<AssistantToolIntent>,
) -> Result<(), String> {
    let mut tool_phase_tasks = tokio::task::JoinSet::new();
    let tool_count = tool_intents.len();

    for (source_index, tool_call) in tool_intents.into_iter().enumerate() {
        let job_tx = job_tx.clone();
        let agent_id = agent_id.to_string();
        let category = category.clone();
        let tool_id = tool_call.tool_id.clone();
        let args_json = tool_call.arguments.clone();

        tool_phase_tasks.spawn(async move {
            let result =
                execute_agent_tool_phase(&job_tx, &agent_id, category, tool_id, args_json).await;
            AgentToolPhaseResult {
                source_index,
                tool_call,
                result,
            }
        });
    }

    let mut source_ordered_results = (0..tool_count).map(|_| None).collect::<Vec<_>>();
    while let Some(joined) = tool_phase_tasks.join_next().await {
        let phase_result = joined.map_err(|err| format!("tool phase task failed: {err}"))?;
        let source_index = phase_result.source_index;
        source_ordered_results[source_index] = Some(phase_result);
    }

    for phase_result in source_ordered_results {
        let AgentToolPhaseResult {
            tool_call, result, ..
        } = phase_result.expect("tool phase result exists for every source index");
        let tool_result = match result {
            Ok(result) => result,
            Err(reason)
                if matches!(
                    profile.tool_failure_mode,
                    ToolFailureMode::ContinueAsToolMessage
                ) =>
            {
                ToolResult::structured(
                    format!("tool call `{}` failed: {reason}", tool_call.function_name),
                    json!({
                        "error": reason,
                        "status": "failed"
                    }),
                )
            }
            Err(reason) => {
                return Err(format!(
                    "tool call `{}` failed closed: {reason}",
                    tool_call.function_name
                ));
            }
        };

        append_tool_result_message_phase(messages, &tool_call, &tool_result);
    }

    Ok(())
}

struct AgentToolPhaseResult {
    source_index: usize,
    tool_call: AssistantToolIntent,
    result: Result<ToolResult, String>,
}

async fn execute_agent_tool_phase(
    job_tx: &mpsc::Sender<Command>,
    agent_id: &str,
    category: Option<String>,
    tool_id: String,
    args_json: Value,
) -> Result<ToolResult, String> {
    let (respond_to, response_rx) = oneshot::channel();
    job_tx
        .send(Command::ExecuteAgentToolCall {
            actor: EventActor::new(ActorKind::Worker, Some(agent_id.to_string())),
            category,
            tool_id,
            args_json,
            respond_to,
        })
        .await
        .map_err(|_| "tool call channel closed".to_string())?;
    response_rx
        .await
        .map_err(|_| "tool call response channel closed".to_string())?
}

fn append_tool_result_message_phase(
    messages: &mut Vec<CompletionMessage>,
    tool_call: &AssistantToolIntent,
    tool_result: &ToolResult,
) {
    messages.push(CompletionMessage {
        role: MessageRole::Tool,
        content: tool_result_to_message_content(tool_result),
        name: Some(tool_call.function_name.clone()),
        tool_call_id: Some(tool_call.tool_call_id.clone()),
        assistant_tool_calls: None,
    });
}

fn completion_messages_to_conversation_messages(
    profile: &AgentProfile,
    request_id: &str,
    agent_id: &str,
    messages: &[CompletionMessage],
) -> Vec<ConversationMessage> {
    let mapping =
        crate::tool::build_tool_function_name_mapping(profile.toolset.iter().map(String::as_str));
    let mut tool_ids_by_call_id = BTreeMap::new();
    let mut conversation_messages = Vec::new();

    for message in messages {
        match message.role {
            MessageRole::System => {}
            MessageRole::User => {
                conversation_messages.push(ConversationMessage::User(ConversationUserMessage {
                    request_id: request_id.to_string(),
                    text: message.content.clone(),
                    seq: None,
                    agent_id: Some(agent_id.to_string()),
                }))
            }
            MessageRole::Assistant => {
                let tool_calls = message
                    .assistant_tool_calls
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .map(|tool_call| {
                        let tool_id = mapping
                            .tool_id_for_function_name(&tool_call.function_name)
                            .unwrap_or(&tool_call.function_name)
                            .to_string();
                        tool_ids_by_call_id.insert(tool_call.tool_call_id.clone(), tool_id.clone());
                        ConversationToolCall {
                            tool_call_id: tool_call.tool_call_id.clone(),
                            tool_id,
                            args_summary: provider_tool_arguments_json(&tool_call.arguments_json),
                            args_digest: digest12(tool_call.arguments_json.as_bytes()),
                            seq: None,
                            metadata: None,
                        }
                    })
                    .collect();
                conversation_messages.push(ConversationMessage::Assistant(
                    ConversationAssistantMessage {
                        request_id: request_id.to_string(),
                        agent_id: Some(agent_id.to_string()),
                        text: message.content.clone(),
                        tool_calls,
                        stop_reason: None,
                        first_seq: None,
                        last_seq: None,
                        provider_id: None,
                        model_id: None,
                        output_digest: None,
                    },
                ));
            }
            MessageRole::Tool => {
                let tool_call_id = message.tool_call_id.clone().unwrap_or_default();
                let tool_id = message
                    .name
                    .as_deref()
                    .and_then(|name| mapping.tool_id_for_function_name(name))
                    .map(str::to_string)
                    .or_else(|| tool_ids_by_call_id.get(&tool_call_id).cloned())
                    .or_else(|| message.name.clone());
                conversation_messages.push(ConversationMessage::ToolResult(Box::new(
                    ConversationToolResultMessage {
                        request_id: request_id.to_string(),
                        tool_call_id,
                        tool_id,
                        status: provider_tool_message_status(&message.content),
                        output_summary: non_empty_trimmed(&message.content)
                            .map(|_| message.content.clone()),
                        output_digest: (!message.content.is_empty())
                            .then(|| digest12(message.content.as_bytes())),
                        output_json: None,
                        seq: None,
                        metadata: None,
                    },
                )));
            }
        }
    }

    conversation_messages
}

fn provider_tool_message_status(content: &str) -> ToolCallStatus {
    let trimmed = content.trim_start();
    if trimmed.starts_with("tool call `") && trimmed.contains("` failed: ") {
        ToolCallStatus::Failed
    } else {
        ToolCallStatus::Succeeded
    }
}

async fn finalize_permission_denied<C, R>(
    clock: &C,
    redactor: &R,
    hook_runtime_config: &HookRuntimeConfig,
    run_state: &mut RunState,
    args: PermissionDeniedArgs<'_>,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let PermissionDeniedArgs {
        actor,
        category,
        tool_id,
        args_json,
        tool_call_id,
        hashline_edit,
        kind,
        reason,
        request_correlation_id,
    } = args;

    let permission_id = format!("perm_{:06}", run_state.next_permission_id);
    run_state.next_permission_id += 1;
    let denial_reason = format!("{} ({})", reason, kind.as_str());

    append_permission_resolved_event(
        clock,
        redactor,
        run_state,
        permission_id.clone(),
        EventPermissionDecision::Deny,
        Some(denial_reason.clone()),
    )?;

    let resolved_hook_batch = run_lifecycle_hooks(
        clock,
        hook_runtime_config,
        HookInvocationContext {
            event: HookLifecycleEvent::PermissionResolved,
            run_id: run_state.info.run_id.clone(),
            workspace_root: run_state.info.workspace_root.clone(),
            artifacts_dir: run_state.info.artifacts_dir.clone(),
            actor: Some(actor.clone()),
            agent_id: actor.agent_id.clone(),
            request_id: request_correlation_id
                .map(ToOwned::to_owned)
                .or_else(|| Some(tool_call_id.to_string())),
            permission_id: Some(permission_id),
            task_id: None,
            tool_call_id: Some(tool_call_id.to_string()),
            tool_id: Some(tool_id.to_string()),
            provider_id: None,
            model_id: None,
            parent_agent_id: None,
            category,
            outcome: Some("deny".to_string()),
            output_summary: Some(denial_reason.clone()),
            failure_reason: Some(denial_reason.clone()),
        },
    )
    .await;

    let hook_executions = resolved_hook_batch.hook_executions.clone();
    let mut final_rejection_reason = reason.to_string();
    if let Some(hook_reason) = resolved_hook_batch.critical_failure.clone() {
        final_rejection_reason =
            format!("{final_rejection_reason}; critical lifecycle hook failed: {hook_reason}");
    }

    let tool_metadata = tool_identity_metadata(tool_id, args_json);

    append_tool_call_rejection(
        clock,
        redactor,
        run_state,
        ToolCallRejectionArgs {
            tool_call_id,
            hashline_edit,
            tool_metadata: tool_metadata.as_ref(),
            reason: &final_rejection_reason,
            request_correlation_id,
            hook_executions,
        },
    )?;

    if let Some(reason) = resolved_hook_batch.critical_failure {
        return Err(CoordinatorError::LifecycleHookFailed(reason));
    }

    Ok(())
}

struct ToolCallRejectionArgs<'a> {
    tool_call_id: &'a str,
    hashline_edit: Option<&'a HashlineEditMetadata>,
    tool_metadata: Option<&'a ToolIdentityMetadata>,
    reason: &'a str,
    request_correlation_id: Option<&'a str>,
    hook_executions: Vec<HookExecutionMetadata>,
}

fn append_permission_resolved_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    permission_id: String,
    decision: EventPermissionDecision,
    reason: Option<String>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    append_payload_event(
        clock,
        redactor,
        run_state,
        system_actor(),
        Some(format!("permission:{permission_id}")),
        EventV1::PermissionResolved(PermissionResolvedEvent {
            permission_id,
            decision,
            reason,
        }),
    )
}

fn append_tool_call_rejection<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    args: ToolCallRejectionArgs<'_>,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let ToolCallRejectionArgs {
        tool_call_id,
        hashline_edit,
        tool_metadata,
        reason,
        request_correlation_id,
        hook_executions,
    } = args;

    if let Some(metadata) = hashline_edit {
        append_edit_rejected_event(
            clock,
            redactor,
            run_state,
            tool_call_id,
            metadata,
            reason.to_string(),
            request_correlation_id,
        )?;
    }

    append_failed_tool_call_finished_event(
        clock,
        redactor,
        run_state,
        tool_call_id,
        reason,
        request_correlation_id,
        tool_call_metadata(
            tool_metadata,
            None,
            Vec::new(),
            None,
            hook_executions.clone(),
        ),
        &hook_executions,
    )?;

    Ok(())
}

fn reject_pending_permission<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    reason: &str,
    response_message: &str,
    pending: PendingPermissionState,
    hook_executions: &[HookExecutionMetadata],
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let PendingPermissionResolution::ToolCall {
        tool_id,
        args_json,
        respond_to,
        ..
    } = pending.resolution
    else {
        return Ok(());
    };

    let hashline_edit = hashline_edit_metadata(&tool_id, &args_json, &pending.tool_call_id);
    let tool_metadata = tool_identity_metadata(&tool_id, &args_json);
    append_tool_call_rejection(
        clock,
        redactor,
        run_state,
        ToolCallRejectionArgs {
            tool_call_id: &pending.tool_call_id,
            hashline_edit: hashline_edit.as_ref(),
            tool_metadata: tool_metadata.as_ref(),
            reason,
            request_correlation_id: pending.request_correlation_id.as_deref(),
            hook_executions: hook_executions.to_vec(),
        },
    )?;

    if let Some(respond_to) = respond_to {
        let _ = respond_to.send(Err(response_message.to_string()));
    }

    Ok(())
}

fn parse_question_answers_reason(reason: Option<&str>) -> Result<Vec<Vec<String>>, String> {
    let Some(reason) = reason.and_then(non_empty_trimmed) else {
        return Err("question answers were not provided".to_string());
    };

    serde_json::from_str::<Vec<Vec<String>>>(reason)
        .map_err(|err| format!("invalid question answer payload: {err}"))
}

fn validate_question_answers_reason(
    reason: Option<&str>,
    prompts: &[QuestionPromptSpec],
) -> Result<Vec<Vec<String>>, String> {
    let answers = parse_question_answers_reason(reason)?;
    validate_question_answers(prompts, answers)
}

fn parse_question_request_prompts(request_json: &Value) -> Result<Vec<QuestionPromptSpec>, String> {
    let request = serde_json::from_value::<QuestionRequestSpec>(request_json.clone())
        .map_err(|err| format!("invalid question request payload: {err}"))?;
    validate_question_prompts(request.questions)
}

fn validate_question_prompts(
    prompts: Vec<QuestionPromptSpec>,
) -> Result<Vec<QuestionPromptSpec>, String> {
    if prompts.is_empty() {
        return Err("at least one question is required".to_string());
    }

    Ok(prompts)
}

fn question_request_timeout_ms(permission_policy: &PermissionPolicy) -> u64 {
    match permission_policy.evaluate(None, PermissionKind::Question) {
        PolicyDecision::Ask { timeout_ms, .. } => timeout_ms,
        PolicyDecision::Allow | PolicyDecision::Deny => DEFAULT_QUESTION_TIMEOUT_MS,
    }
}

fn append_payload_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    actor: EventActor,
    stream_key: Option<String>,
    payload: EventV1,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    append_payload_event_with_correlation(
        clock, redactor, run_state, actor, stream_key, None, payload,
    )
}

fn append_payload_event_with_correlation<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    actor: EventActor,
    stream_key: Option<String>,
    correlation_id: Option<String>,
    payload: EventV1,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.clone());
    let mut context = EventContext::new(run_state.next_event_seq, actor);
    context.correlation_id = correlation_id;
    context.stream_key = stream_key;
    let envelope = builder.build(context, payload)?;
    append_built_event(run_state, envelope)
}

fn append_tool_call_requested_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    args: ToolCallRequestedEventArgs<'_>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let ToolCallRequestedEventArgs {
        actor,
        tool_call_id,
        tool_id,
        args_json,
        tool_metadata,
        request_correlation_id,
    } = args;

    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.clone());
    let mut context = EventContext::new(run_state.next_event_seq, actor);
    context.correlation_id = request_correlation_id
        .map(ToOwned::to_owned)
        .or_else(|| Some(tool_call_id.to_string()));
    context.stream_key = Some(format!("tool_call:{tool_call_id}"));
    let envelope =
        builder.tool_call_requested(context, tool_call_id, tool_id, args_json, tool_metadata)?;
    append_built_event(run_state, envelope)
}

fn append_permission_requested_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    args: PermissionRequestedEventArgs<'_>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let PermissionRequestedEventArgs {
        permission_id,
        tool_call_id,
        kind,
        summary,
        request_digest,
        timeout_ms,
        default_decision,
        request_correlation_id,
    } = args;
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.clone());
    let mut context = EventContext::new(run_state.next_event_seq, system_actor());
    context.correlation_id = request_correlation_id
        .map(ToOwned::to_owned)
        .or_else(|| Some(tool_call_id.to_string()));
    context.stream_key = Some(format!("permission:{permission_id}"));

    let envelope = builder.permission_requested(
        context,
        PermissionRequestedArgs {
            permission_id: permission_id.to_string(),
            kind: kind.as_str().to_string(),
            tool_call_id: Some(tool_call_id.to_string()),
            summary,
            request_digest,
            timeout_ms,
            default_decision,
        },
    )?;

    append_built_event(run_state, envelope)
}

fn append_permission_grant_recorded_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    permission_id: &str,
    request_correlation_id: Option<&str>,
    grant: PermissionGrant,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.clone());
    let mut context = EventContext::new(run_state.next_event_seq, system_actor());
    context.correlation_id = request_correlation_id
        .map(ToOwned::to_owned)
        .or_else(|| Some(permission_id.to_string()));
    context.stream_key = Some(format!("permission:{permission_id}"));
    let envelope = builder.build(
        context,
        EventV1::PermissionGrantRecorded(PermissionGrantRecordedEvent { grant }),
    )?;
    append_built_event(run_state, envelope)
}

fn append_tool_call_started_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    tool_call_id: &str,
    request_correlation_id: Option<&str>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.clone());
    let mut context = EventContext::new(run_state.next_event_seq, system_actor());
    context.correlation_id = request_correlation_id
        .map(ToOwned::to_owned)
        .or_else(|| Some(tool_call_id.to_string()));
    context.stream_key = Some(format!("tool_call:{tool_call_id}"));
    let envelope = builder.build(
        context,
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: tool_call_id.to_string(),
        }),
    )?;
    append_built_event(run_state, envelope)
}

fn append_tool_call_finished_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    args: ToolCallFinishedEventArgs<'_>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let ToolCallFinishedEventArgs {
        tool_call_id,
        status,
        output_summary,
        output_json,
        metadata,
        request_correlation_id,
    } = args;
    let output_digest = output_summary.as_ref().map(|s| digest12(s.as_bytes()));
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.clone());
    let mut context = EventContext::new(run_state.next_event_seq, system_actor());
    context.correlation_id = request_correlation_id
        .map(ToOwned::to_owned)
        .or_else(|| Some(tool_call_id.to_string()));
    context.stream_key = Some(format!("tool_call:{tool_call_id}"));
    let envelope = builder.build(
        context,
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: tool_call_id.to_string(),
            status,
            output_summary,
            output_digest,
            output_json,
            metadata,
        }),
    )?;
    append_built_event(run_state, envelope)
}

fn append_edit_proposed_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    tool_call_id: &str,
    metadata: &HashlineEditMetadata,
    request_correlation_id: Option<&str>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.clone());
    let mut context = EventContext::new(run_state.next_event_seq, system_actor());
    context.correlation_id = request_correlation_id
        .map(ToOwned::to_owned)
        .or_else(|| Some(tool_call_id.to_string()));
    context.stream_key = Some(format!("edit:{}", metadata.edit_id));

    let envelope = builder.build(
        context,
        EventV1::EditProposed(EditProposedEvent {
            edit_id: metadata.edit_id.clone(),
            path: metadata.path.clone(),
            summary: metadata.summary.clone(),
            patch_digest: metadata.patch_digest.clone(),
        }),
    )?;

    append_built_event(run_state, envelope)
}

fn append_edit_applied_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    args: EditAppliedEventArgs<'_>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let EditAppliedEventArgs {
        tool_call_id,
        metadata,
        new_file_digest,
        diff_rel_path,
        diff_digest,
        request_correlation_id,
    } = args;
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.clone());
    let mut context = EventContext::new(run_state.next_event_seq, system_actor());
    context.correlation_id = request_correlation_id
        .map(ToOwned::to_owned)
        .or_else(|| Some(tool_call_id.to_string()));
    context.stream_key = Some(format!("edit:{}", metadata.edit_id));

    let envelope = builder.build(
        context,
        EventV1::EditApplied(EditAppliedEvent {
            edit_id: metadata.edit_id.clone(),
            path: metadata.path.clone(),
            new_file_digest,
            diff_rel_path,
            diff_digest,
        }),
    )?;

    append_built_event(run_state, envelope)
}

fn append_edit_rejected_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    tool_call_id: &str,
    metadata: &HashlineEditMetadata,
    reason: String,
    request_correlation_id: Option<&str>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.clone());
    let mut context = EventContext::new(run_state.next_event_seq, system_actor());
    context.correlation_id = request_correlation_id
        .map(ToOwned::to_owned)
        .or_else(|| Some(tool_call_id.to_string()));
    context.stream_key = Some(format!("edit:{}", metadata.edit_id));

    let envelope = builder.build(
        context,
        EventV1::EditRejected(EditRejectedEvent {
            edit_id: metadata.edit_id.clone(),
            path: metadata.path.clone(),
            reason,
        }),
    )?;

    append_built_event(run_state, envelope)
}

#[expect(
    clippy::too_many_arguments,
    reason = "failed tool-call terminal events carry explicit metadata and hook context"
)]
fn append_failed_tool_call_finished_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    tool_call_id: &str,
    reason: &str,
    request_correlation_id: Option<&str>,
    metadata: Option<ToolCallMetadata>,
    hook_executions: &[HookExecutionMetadata],
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    append_tool_call_finished_event(
        clock,
        redactor,
        run_state,
        ToolCallFinishedEventArgs {
            tool_call_id,
            status: ToolCallStatus::Failed,
            output_summary: Some(reason.to_string()),
            output_json: Some(failed_tool_output_json(reason, hook_executions)),
            metadata,
            request_correlation_id,
        },
    )
}

fn append_artifact_written_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    tool_call_id: &str,
    artifact: &crate::tool::ArtifactRef,
    request_correlation_id: Option<&str>,
    tool_metadata: Option<&ToolIdentityMetadata>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let artifact_path = run_state.info.run_dir.join(&artifact.path);
    let bytes = fs::metadata(&artifact_path)
        .map(|meta| meta.len())
        .unwrap_or(0);
    let digest = artifact
        .digest
        .clone()
        .unwrap_or_else(|| digest12(artifact.path.as_bytes()));
    let mut metadata = BTreeMap::new();
    metadata.insert("tool_call_id".to_string(), tool_call_id.to_string());
    if let Some(tool_metadata) = tool_metadata {
        if let Some(canonical_tool_id) = tool_metadata.canonical_tool_id.as_ref() {
            metadata.insert("canonical_tool_id".to_string(), canonical_tool_id.clone());
        }
        if let Some(alias_source_tool_id) = tool_metadata.alias_source_tool_id.as_ref() {
            metadata.insert(
                "alias_source_tool_id".to_string(),
                alias_source_tool_id.clone(),
            );
        }
    }

    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.clone());
    let mut context = EventContext::new(run_state.next_event_seq, system_actor());
    context.correlation_id = request_correlation_id
        .map(ToOwned::to_owned)
        .or_else(|| Some(tool_call_id.to_string()));
    context.stream_key = Some(format!("tool_call:{tool_call_id}"));
    let envelope = builder.build(
        context,
        EventV1::ArtifactWritten(ArtifactWrittenEvent {
            path: artifact.path.clone(),
            digest,
            bytes,
            tool_call_id: Some(tool_call_id.to_string()),
            tool_metadata: tool_metadata.cloned(),
            metadata,
        }),
    )?;

    append_built_event(run_state, envelope)
}

fn create_child_session_mirror<C, R>(
    clock: &C,
    redactor: &R,
    config: &CoordinatorConfig,
    run_state: &mut RunState,
    child_session_id: &str,
    profile: &str,
    child_session_title: Option<&str>,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    if run_state
        .child_session_mirrors
        .contains_key(child_session_id)
    {
        return Ok(());
    }

    let event_store = Arc::new(JsonlFileEventStore::open(
        &config.session_dir,
        child_session_id,
        config.deterministic_store,
    )?);
    let run_dir = config.session_dir.join(child_session_id);
    let title = child_session_title
        .and_then(non_empty_trimmed)
        .map(str::to_string)
        .unwrap_or_else(|| create_default_title(clock, true));

    write_child_session_metadata(
        clock,
        config,
        run_state,
        child_session_id,
        &run_dir,
        &title,
        profile,
    )?;

    let child_appender = ChildPayloadAppender {
        clock,
        redactor,
        event_store: event_store.as_ref(),
        child_run_id: child_session_id,
    };
    child_appender.append(
        system_actor(),
        Some(format!("run:{child_session_id}")),
        None,
        EventV1::RunStarted(RunStartedEvent {
            run_name: title,
            workspace_root: run_state.info.workspace_root.display().to_string(),
        }),
    )?;
    child_appender.append(
        system_actor(),
        Some(format!("agent:{child_session_id}")),
        None,
        EventV1::AgentSpawned(AgentSpawnedEvent {
            agent_id: child_session_id.to_string(),
            profile: profile.to_string(),
            parent_agent_id: None,
        }),
    )?;

    run_state.child_session_mirrors.insert(
        child_session_id.to_string(),
        ChildSessionMirror {
            event_store,
            append_parent_finish: true,
        },
    );
    Ok(())
}

fn restore_child_session_mirrors<C, R>(
    clock: &C,
    redactor: &R,
    config: &CoordinatorConfig,
    run_state: &mut RunState,
    restored_agent_bindings: &[(String, String, Option<String>)],
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    for (agent_id, profile, parent_agent_id) in restored_agent_bindings {
        if parent_agent_id.is_none() {
            continue;
        }

        let run_dir = config.session_dir.join(agent_id);
        if run_dir.join(EVENTS_FILE_NAME).exists() {
            let event_store = Arc::new(JsonlFileEventStore::open_existing(
                &config.session_dir,
                agent_id,
                config.deterministic_store,
            )?);
            run_state.child_session_mirrors.insert(
                agent_id.clone(),
                ChildSessionMirror {
                    event_store,
                    append_parent_finish: false,
                },
            );
        } else {
            create_child_session_mirror(
                clock, redactor, config, run_state, agent_id, profile, None,
            )?;
        }
    }

    Ok(())
}

fn write_child_session_metadata<C>(
    clock: &C,
    config: &CoordinatorConfig,
    run_state: &RunState,
    child_session_id: &str,
    child_run_dir: &Path,
    title: &str,
    profile: &str,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
{
    let created_at = if config.deterministic_store {
        None
    } else {
        clock.system_time_rfc3339()
    };
    let metadata = json!({
        "run_id": child_session_id,
        "run_name": title,
        "workspace_root": run_state.info.workspace_root.display().to_string(),
        "created_at": created_at,
        "config_digest": config.config_digest.clone(),
        "harness_version": config.harness_version.clone(),
        "recorded_runtime_context": null,
        "harness_lineage": {
            "relationship": "task_child_session",
            "parent_run_id": run_state.info.run_id.clone(),
            "parent_session_id": run_state.info.run_id.clone(),
            "child_session_id": child_session_id,
            "profile": profile,
        }
    });
    let meta_path = child_run_dir.join(META_FILE_NAME);
    let body = serde_json::to_string_pretty(&metadata)?;
    fs::write(&meta_path, body).map_err(|source| CoordinatorError::WriteRunMetadata {
        path: meta_path.display().to_string(),
        source,
    })
}

struct ChildPayloadAppender<'a, C, R>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    clock: &'a C,
    redactor: &'a R,
    event_store: &'a JsonlFileEventStore,
    child_run_id: &'a str,
}

impl<C, R> ChildPayloadAppender<'_, C, R>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    fn append(
        &self,
        actor: EventActor,
        stream_key: Option<String>,
        correlation_id: Option<String>,
        payload: EventV1,
    ) -> Result<EventEnvelopeV1, CoordinatorError> {
        let builder = EventBuilder::new(self.clock, self.redactor, self.child_run_id.to_string());
        let mut context = EventContext::new(self.event_store.next_seq()?, actor);
        context.stream_key = stream_key;
        context.correlation_id = correlation_id;
        let envelope = builder.build(context, payload)?;
        Ok(self
            .event_store
            .append(EventEnvelopeWithoutSeqV1::from(envelope))?)
    }
}

fn mirror_event_to_child_session(
    run_state: &mut RunState,
    event: &EventEnvelopeV1,
) -> Result<(), CoordinatorError> {
    let Some(child_session_id) = child_session_id_for_event(run_state, event) else {
        return Ok(());
    };
    let Some(mirror) = run_state.child_session_mirrors.get(&child_session_id) else {
        return Ok(());
    };

    let mut child_event = event.clone();
    child_event.run_id = child_session_id.clone();
    child_event.seq = mirror.event_store.next_seq()?;
    child_event.event_id = format!("evt_{child_session_id}_mirror_{:012}", event.seq);
    if child_event.stream_key.as_deref() == Some(format!("run:{}", run_state.info.run_id).as_str())
    {
        child_event.stream_key = Some(format!("run:{child_session_id}"));
    }

    mirror
        .event_store
        .append(EventEnvelopeWithoutSeqV1::from(child_event))?;
    Ok(())
}

fn child_session_id_for_event(run_state: &RunState, event: &EventEnvelopeV1) -> Option<String> {
    if matches!(
        event.payload,
        EventV1::RunStarted(_) | EventV1::RunFinished(_)
    ) {
        return None;
    }

    if let Some(agent_id) = event.actor.agent_id.as_deref() {
        if run_state.child_session_mirrors.contains_key(agent_id) {
            return Some(agent_id.to_string());
        }
    }

    if let Some(request_id) = event.correlation_id.as_deref() {
        if let Some(child_session_id) = run_state.child_request_session_by_id.get(request_id) {
            return Some(child_session_id.clone());
        }
    }

    match &event.payload {
        EventV1::ProviderRequestStarted(payload) => run_state
            .child_request_session_by_id
            .get(&payload.request_id)
            .cloned(),
        EventV1::ProviderStreamDelta(payload) => run_state
            .child_request_session_by_id
            .get(&payload.request_id)
            .cloned(),
        EventV1::ProviderReasoningDelta(payload) => run_state
            .child_request_session_by_id
            .get(&payload.request_id)
            .cloned(),
        EventV1::ProviderRequestFinished(payload) => run_state
            .child_request_session_by_id
            .get(&payload.request_id)
            .cloned(),
        EventV1::AssistantMessageFinished(payload) => run_state
            .child_request_session_by_id
            .get(&payload.request_id)
            .cloned(),
        _ => None,
    }
}

fn finish_child_session_mirrors<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &RunState,
    summary: &str,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    for (child_session_id, mirror) in &run_state.child_session_mirrors {
        if !mirror.append_parent_finish {
            continue;
        }
        let child_appender = ChildPayloadAppender {
            clock,
            redactor,
            event_store: mirror.event_store.as_ref(),
            child_run_id: child_session_id,
        };
        child_appender.append(
            system_actor(),
            Some(format!("run:{child_session_id}")),
            None,
            EventV1::RunFinished(RunFinishedEvent {
                summary: format!("parent session finished: {summary}"),
            }),
        )?;
    }

    Ok(())
}

fn write_run_metadata(
    run_state: &RunState,
    config: &CoordinatorConfig,
    clock: &dyn Clock,
) -> Result<(), CoordinatorError> {
    let metadata = RunMetadata {
        run_id: run_state.info.run_id.clone(),
        run_name: run_state.info.run_name.clone(),
        workspace_root: run_state.info.workspace_root.display().to_string(),
        created_at: if config.deterministic_store {
            None
        } else {
            clock.system_time_rfc3339()
        },
        config_digest: config.config_digest.clone(),
        harness_version: config.harness_version.clone(),
        recorded_runtime_context: run_state.recorded_runtime_context.clone(),
        mode_source: config.session_mode_source,
    };

    let meta_path = run_state.info.run_dir.join(META_FILE_NAME);
    let body = serde_json::to_string_pretty(&metadata)?;
    fs::write(&meta_path, body).map_err(|source| CoordinatorError::WriteRunMetadata {
        path: meta_path.display().to_string(),
        source,
    })?;

    Ok(())
}

#[derive(Debug, Clone)]
struct CompactionSummaryDecision {
    summary: Option<String>,
    source: SummarySourceRequest,
    split_prefix_summary: Option<SplitPrefixSummaryDecision>,
}

impl CompactionSummaryDecision {
    fn deterministic(trigger: &ProviderCompactionTrigger) -> Self {
        Self {
            summary: None,
            source: SummarySourceRequest::DeterministicForModelRef {
                model_ref: trigger.model_ref.clone(),
            },
            split_prefix_summary: None,
        }
    }

    fn hook(summary: String) -> Self {
        Self {
            summary: Some(summary),
            source: SummarySourceRequest::Hook,
            split_prefix_summary: None,
        }
    }

    fn model(
        model_ref: String,
        summary: String,
        deterministic_fallback: bool,
        split_prefix_summary: Option<SplitPrefixSummaryDecision>,
    ) -> Self {
        Self {
            summary: if non_empty_trimmed(&summary).is_some() {
                Some(summary)
            } else {
                None
            },
            source: SummarySourceRequest::Model {
                model_ref,
                deterministic_fallback,
            },
            split_prefix_summary,
        }
    }
}

#[derive(Debug, Clone)]
enum SummarySourceRequest {
    Hook,
    Model {
        model_ref: String,
        deterministic_fallback: bool,
    },
    Deterministic,
    DeterministicForModelRef {
        model_ref: String,
    },
}

#[derive(Debug, Clone)]
struct ProviderContextCompactionPlan {
    older_turns: Vec<ProviderConversationTurn>,
    recent_turns: Vec<ProviderConversationTurn>,
    pruned_tool_artifacts: Vec<EventArtifactRef>,
    facts: ProviderCompactionFacts,
    tail_boundary: ProviderCompactionTailBoundary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SplitPrefixSummaryDecision {
    summary: String,
    source: SplitPrefixSummarySource,
    fallback_reason: Option<String>,
}

impl SplitPrefixSummaryDecision {
    fn deterministic(summary: String) -> Self {
        Self {
            summary,
            source: SplitPrefixSummarySource::Deterministic,
            fallback_reason: None,
        }
    }

    fn model(summary: String) -> Self {
        Self {
            summary,
            source: SplitPrefixSummarySource::ModelBacked,
            fallback_reason: None,
        }
    }

    fn model_fallback(summary: String, reason: String) -> Self {
        Self {
            summary,
            source: SplitPrefixSummarySource::ModelBackedDeterministicFallback,
            fallback_reason: Some(reason),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SplitPrefixSummarySource {
    Deterministic,
    ModelBacked,
    ModelBackedDeterministicFallback,
}

impl SplitPrefixSummarySource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Deterministic => "deterministic",
            Self::ModelBacked => "model_backed",
            Self::ModelBackedDeterministicFallback => "model_backed_deterministic_fallback",
        }
    }
}

#[derive(Debug, Clone)]
struct ModelBackedCompactionSummary {
    summary: String,
    split_prefix_summary: Option<SplitPrefixSummaryDecision>,
}

fn compact_provider_context<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    trigger: &ProviderCompactionTrigger,
    compaction_config: &CompactionRuntimeConfig,
    summary_decision: &CompactionSummaryDecision,
) -> Result<Option<AppliedCompaction>, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let current_context = run_state
        .provider_context_by_agent
        .get(&trigger.agent_id)
        .cloned()
        .unwrap_or_default();
    let current_context_tokens = approximate_provider_context_tokens(&current_context);

    let metadata = recorded_runtime_context_for_compaction(run_state, trigger);
    if !should_compact_provider_context(&current_context, &metadata, trigger, compaction_config) {
        return Ok(None);
    }

    let trigger_estimate =
        provider_context_trigger_estimate(&current_context, &metadata, trigger, compaction_config);
    let tokens_before_estimate = trigger_estimate
        .as_ref()
        .map(|estimate| estimate.tokens_before_estimate)
        .unwrap_or(current_context_tokens);
    let mut trigger = trigger.clone();
    if trigger.estimate_source.is_none() {
        trigger.estimate_source = trigger_estimate.map(|estimate| estimate.source.to_string());
    }

    let keep_recent_budget = provider_context_keep_recent_tokens(&metadata);
    let Some(checkpoint) = build_provider_context_checkpoint(
        run_state,
        &trigger,
        &current_context,
        redactor,
        keep_recent_budget,
        tokens_before_estimate,
        compaction_config,
        summary_decision,
    ) else {
        return Ok(None);
    };
    let checkpoint_id = checkpoint.metadata.checkpoint_id.clone();
    let updated_context = ProviderContext::from_checkpoint(checkpoint.clone());
    let updated_tokens = approximate_provider_context_tokens(&updated_context);
    if trigger.trigger_reason != "manual" && updated_tokens >= tokens_before_estimate {
        if matches!(
            trigger.trigger_reason.as_str(),
            "pre_prompt" | "failed_response"
        ) {
            let reason = if trigger.trigger_reason == "pre_prompt" {
                format!(
                    "pre-prompt compaction did not reduce estimated provider context: before={tokens_before_estimate}, after={updated_tokens}"
                )
            } else {
                format!(
                    "failed-response compaction did not reduce estimated provider context: before={tokens_before_estimate}, after={updated_tokens}"
                )
            };
            append_compaction_failed_event(
                clock,
                redactor,
                run_state,
                &trigger,
                &reason,
                Some(checkpoint.metadata.checkpoint_id.clone()),
                Some(checkpoint.metadata.through_seq),
            )?;
        }
        return Ok(None);
    }

    append_payload_event(
        clock,
        redactor,
        run_state,
        system_actor(),
        Some(format!("compaction:{}", trigger.agent_id)),
        EventV1::CompactionRequested(CompactionRequestedEvent {
            checkpoint_id: checkpoint.metadata.checkpoint_id.clone(),
            agent_id: checkpoint.metadata.agent_id.clone(),
            trigger_reason: trigger.trigger_reason.clone(),
            through_seq: checkpoint.metadata.through_seq,
            through_request_id: checkpoint.metadata.through_request_id.clone(),
            provider_id: checkpoint.metadata.provider_id.clone(),
            model_id: checkpoint.metadata.model_id.clone(),
            tokens_before: checkpoint.metadata.tokens_before,
            tokens_before_estimate: checkpoint.metadata.tokens_before_estimate,
            estimate_source: trigger.estimate_source.clone(),
        }),
    )?;

    let body =
        serialize_provider_context_checkpoint(&checkpoint, trigger.estimate_source.as_deref())?;
    let artifact_store = crate::tool::ArtifactStore::new(run_state.info.artifacts_dir.clone())
        .map_err(|err| CoordinatorError::ResumeRestoreFailed {
            run_id: run_state.info.run_id.clone(),
            reason: format!("failed to open compaction artifact store: {err}"),
        })?;
    let artifact_name = format!(
        "compactions/{}/{}.json",
        trigger.agent_id, checkpoint.metadata.checkpoint_id
    );
    let artifact = artifact_store
        .write_text(&artifact_name, &body)
        .map_err(|err| CoordinatorError::ResumeRestoreFailed {
            run_id: run_state.info.run_id.clone(),
            reason: format!("failed to write compaction checkpoint artifact: {err}"),
        })?;
    append_compaction_artifact_written_event(clock, redactor, run_state, &checkpoint, &artifact)?;
    append_payload_event(
        clock,
        redactor,
        run_state,
        system_actor(),
        Some(format!("compaction:{}", trigger.agent_id)),
        EventV1::CompactionWritten(CompactionWrittenEvent {
            checkpoint_id: checkpoint.metadata.checkpoint_id.clone(),
            agent_id: checkpoint.metadata.agent_id.clone(),
            artifact_path: artifact.path.clone(),
            artifact_digest: artifact.digest.clone(),
            artifact_bytes: body.len() as u64,
            trigger_reason: trigger.trigger_reason.clone(),
            through_seq: checkpoint.metadata.through_seq,
            through_request_id: checkpoint.metadata.through_request_id.clone(),
            provider_id: checkpoint.metadata.provider_id.clone(),
            model_id: checkpoint.metadata.model_id.clone(),
            tokens_before: checkpoint.metadata.tokens_before,
            tokens_before_estimate: checkpoint.metadata.tokens_before_estimate,
            tokens_after_estimate: checkpoint.metadata.tokens_after_estimate,
            summary_tokens_estimate: checkpoint.metadata.summary_tokens_estimate,
            compacted_turns: checkpoint.metadata.compacted_turns,
            reduction_tokens_estimate: checkpoint.metadata.reduction_tokens_estimate,
            reduction_percent_estimate: checkpoint.metadata.reduction_percent_estimate,
            estimate_source: trigger.estimate_source.clone(),
            preserved_turns: checkpoint.recent_turns.len() as u32,
        }),
    )?;

    append_payload_event(
        clock,
        redactor,
        run_state,
        system_actor(),
        Some(format!("compaction:{}", trigger.agent_id)),
        EventV1::CompactionApplied(CompactionAppliedEvent {
            checkpoint_id: checkpoint.metadata.checkpoint_id.clone(),
            agent_id: trigger.agent_id.clone(),
            through_seq: checkpoint.metadata.through_seq,
            through_request_id: checkpoint.metadata.through_request_id.clone(),
            tokens_before_estimate: checkpoint.metadata.tokens_before_estimate,
            tokens_after_estimate: checkpoint.metadata.tokens_after_estimate,
            summary_tokens_estimate: checkpoint.metadata.summary_tokens_estimate,
            compacted_turns: checkpoint.metadata.compacted_turns,
            preserved_turns: checkpoint.metadata.preserved_turns,
            reduction_tokens_estimate: checkpoint.metadata.reduction_tokens_estimate,
            reduction_percent_estimate: checkpoint.metadata.reduction_percent_estimate,
            estimate_source: trigger.estimate_source.clone(),
        }),
    )?;

    run_state
        .provider_context_by_agent
        .insert(trigger.agent_id.clone(), updated_context.clone());

    Ok(Some(AppliedCompaction {
        updated_context,
        checkpoint_id,
        tokens_before_estimate: checkpoint.metadata.tokens_before_estimate,
        tokens_after_estimate: checkpoint.metadata.tokens_after_estimate,
    }))
}

fn recorded_runtime_context_for_compaction(
    run_state: &RunState,
    trigger: &ProviderCompactionTrigger,
) -> RecordedRuntimeContext {
    let requested_model = AgentModelRef::parse(&trigger.model_ref);
    let requested_provider_id = trigger
        .provider_id
        .as_deref()
        .unwrap_or(requested_model.provider_id.as_str());
    let requested_model_id = trigger
        .model_id
        .as_deref()
        .unwrap_or(requested_model.model_id.as_str());

    if let Some(recorded) = run_state
        .recorded_runtime_context
        .as_ref()
        .filter(|context| {
            context.profile == trigger.profile_name
                && context.provider == requested_provider_id
                && context.model == requested_model_id
        })
    {
        return recorded.clone();
    }

    RecordedRuntimeContext::from_profile_model(&trigger.profile_name, &trigger.model_ref)
}

fn should_compact_provider_context(
    context: &ProviderContext,
    metadata: &RecordedRuntimeContext,
    trigger: &ProviderCompactionTrigger,
    compaction_config: &CompactionRuntimeConfig,
) -> bool {
    if trigger.trigger_reason == "manual" {
        return context.preserved_turns.len() >= 2;
    }

    if trigger.trigger_reason == "overflow_retry" {
        return !context.is_empty();
    }

    if context.preserved_turns.len() < 2 {
        return false;
    }

    provider_context_trigger_estimate(context, metadata, trigger, compaction_config).is_some_and(
        |estimate| {
            estimate.tokens_before_estimate
                >= estimate.input_budget.saturating_sub(estimate.reserve)
        },
    )
}

fn provider_context_trigger_estimate(
    context: &ProviderContext,
    metadata: &RecordedRuntimeContext,
    trigger: &ProviderCompactionTrigger,
    compaction_config: &CompactionRuntimeConfig,
) -> Option<ProviderContextTriggerEstimate> {
    let (input_budget, reserve, uses_fallback_budget) =
        if let Some(input_budget) = metadata.max_input_tokens.or(metadata.context_window_tokens) {
            (
                input_budget,
                provider_context_reserve_tokens(metadata, input_budget),
                false,
            )
        } else if compaction_config.estimated_token_triggers {
            let input_budget = compaction_config.fallback_input_tokens;
            if input_budget == 0 {
                return None;
            }
            (
                input_budget,
                PROVIDER_CONTEXT_COMPACTION_RESERVE_TOKENS.max(input_budget / 8),
                true,
            )
        } else {
            return None;
        };

    let context_tokens = approximate_provider_context_tokens(context);
    let tokens_before_estimate = trigger.tokens_before.unwrap_or_else(|| {
        context_tokens.saturating_add(trigger.prompt_tokens_estimate.unwrap_or(0))
    });
    let source = if uses_fallback_budget {
        "fallback_budget"
    } else if trigger.tokens_before.is_some() {
        "provider_usage"
    } else if trigger.prompt_tokens_estimate.is_some() {
        "estimated_context_and_prompt"
    } else {
        "estimated_context"
    };

    Some(ProviderContextTriggerEstimate {
        tokens_before_estimate,
        input_budget,
        reserve,
        source,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "checkpoint assembly keeps run state, trigger, token estimates, redaction, and summary decision explicit"
)]
fn build_provider_context_checkpoint(
    run_state: &RunState,
    trigger: &ProviderCompactionTrigger,
    context: &ProviderContext,
    redactor: &(impl Redactor + ?Sized),
    keep_recent_budget: u32,
    tokens_before_estimate: u32,
    compaction_config: &CompactionRuntimeConfig,
    summary_decision: &CompactionSummaryDecision,
) -> Option<ProviderContextCheckpoint> {
    let plan = build_provider_context_compaction_plan(
        run_state,
        trigger,
        context,
        redactor,
        keep_recent_budget,
        compaction_config,
        summary_decision.split_prefix_summary.as_ref(),
    )?;
    let metadata = recorded_runtime_context_for_compaction(run_state, trigger);
    let summary_source = build_provider_compaction_summary_source(
        &metadata,
        trigger,
        context.compacted_summary.as_deref(),
        summary_decision.source.clone(),
        compaction_config,
    );
    let summary = summary_decision
        .summary
        .as_deref()
        .and_then(non_empty_trimmed)
        .map(|summary| {
            truncate_with_ellipsis(summary, PROVIDER_CONTEXT_COMPACTION_SUMMARY_MAX_CHARS)
        })
        .unwrap_or_else(|| {
            build_provider_context_summary(
                context.compacted_summary.as_deref(),
                &plan.older_turns,
                &plan.pruned_tool_artifacts,
                &plan.facts,
                &plan.tail_boundary,
                &summary_source,
                compaction_config,
            )
        });
    non_empty_trimmed(&summary)?;
    let summary_tokens_estimate = approximate_text_tokens(&summary);
    let preserved_tokens_estimate = preserved_tokens_estimate(&plan.recent_turns);
    let tokens_after_estimate = summary_tokens_estimate.saturating_add(preserved_tokens_estimate);
    let reduction_tokens_estimate = tokens_before_estimate.saturating_sub(tokens_after_estimate);
    let reduction_percent_estimate = (tokens_before_estimate > 0).then(|| {
        ((u64::from(reduction_tokens_estimate) * 100) / u64::from(tokens_before_estimate)) as u32
    });

    let first_kept_request_id = plan
        .recent_turns
        .first()
        .and_then(|turn| turn.request_id.clone());
    let timeline_entry = ProviderCompactionTimelineEntry {
        entry_type: if trigger.trigger_reason == "manual" {
            "manual_compaction".to_string()
        } else if trigger.trigger_reason == "overflow_retry" {
            "overflow_compaction".to_string()
        } else {
            "proactive_compaction".to_string()
        },
        summary: summarize_compaction_text(&summary),
        first_kept_request_id,
        compacted_turns: plan.older_turns.len() as u32,
        preserved_turns: plan.recent_turns.len() as u32,
        tokens_before_estimate: Some(tokens_before_estimate),
        tokens_after_estimate: Some(tokens_after_estimate),
    };

    Some(ProviderContextCheckpoint {
        metadata: ProviderContextCheckpointMetadata {
            checkpoint_id: format!("checkpoint_{:06}", run_state.next_event_seq),
            agent_id: trigger.agent_id.clone(),
            run_id: run_state.info.run_id.clone(),
            through_seq: run_state.next_event_seq.saturating_sub(1),
            through_request_id: trigger.through_request_id.clone(),
            provider_id: trigger.provider_id.clone(),
            model_id: trigger.model_id.clone(),
            tokens_before: trigger.tokens_before,
            tokens_before_estimate: Some(tokens_before_estimate),
            tokens_after_estimate: Some(tokens_after_estimate),
            summary_tokens_estimate: Some(summary_tokens_estimate),
            compacted_turns: Some(plan.older_turns.len() as u32),
            preserved_turns: Some(plan.recent_turns.len() as u32),
            reduction_tokens_estimate: Some(reduction_tokens_estimate),
            reduction_percent_estimate,
            trigger_reason: Some(trigger.trigger_reason.clone()),
        },
        summary,
        recent_turns: plan.recent_turns,
        pruned_tool_artifacts: plan.pruned_tool_artifacts,
        facts: plan.facts,
        tail_boundary: Some(plan.tail_boundary),
        summary_source: Some(summary_source),
        timeline_entry: Some(timeline_entry),
    })
}

fn serialize_provider_context_checkpoint(
    checkpoint: &ProviderContextCheckpoint,
    estimate_source: Option<&str>,
) -> Result<String, serde_json::Error> {
    let mut value = serde_json::to_value(checkpoint)?;
    if let (Some(source), Some(object)) = (estimate_source, value.as_object_mut()) {
        object.insert(
            "estimate_source".to_string(),
            serde_json::Value::String(source.to_string()),
        );
    }
    serde_json::to_string_pretty(&value)
}

fn build_provider_context_compaction_plan(
    run_state: &RunState,
    trigger: &ProviderCompactionTrigger,
    context: &ProviderContext,
    redactor: &(impl Redactor + ?Sized),
    keep_recent_budget: u32,
    compaction_config: &CompactionRuntimeConfig,
    split_prefix_summary_override: Option<&SplitPrefixSummaryDecision>,
) -> Option<ProviderContextCompactionPlan> {
    let (mut older_turns, mut recent_turns, split_tail, split_prefix_summary) = if compaction_config
        .split_oversized_turns
    {
        if let Some((older_latest_turn, recent_latest_turn, split_prefix_summary)) =
            split_latest_oversized_turn(
                &context.preserved_turns,
                keep_recent_budget,
                trigger.trigger_reason.as_str(),
            )
        {
            let split_prefix_summary = split_prefix_summary_override
                .cloned()
                .unwrap_or(split_prefix_summary);
            let mut older_turns =
                context.preserved_turns[..context.preserved_turns.len() - 1].to_vec();
            older_turns.push(older_latest_turn);
            (
                older_turns,
                vec![recent_latest_turn],
                true,
                Some(split_prefix_summary),
            )
        } else if latest_oversized_turn_needs_summary_only(
            &context.preserved_turns,
            keep_recent_budget,
            trigger.trigger_reason.as_str(),
        ) {
            (context.preserved_turns.clone(), Vec::new(), false, None)
        } else if let Some(split_index) =
            provider_context_split_index(&context.preserved_turns, keep_recent_budget)
        {
            (
                context.preserved_turns[..split_index].to_vec(),
                context.preserved_turns[split_index..].to_vec(),
                false,
                None,
            )
        } else if trigger.trigger_reason == "manual" && context.preserved_turns.len() >= 2 {
            let split_index = context.preserved_turns.len() - 1;
            (
                context.preserved_turns[..split_index].to_vec(),
                context.preserved_turns[split_index..].to_vec(),
                false,
                None,
            )
        } else if trigger.trigger_reason == "overflow_retry" && !context.preserved_turns.is_empty()
        {
            (context.preserved_turns.clone(), Vec::new(), false, None)
        } else {
            return None;
        }
    } else if let Some(split_index) =
        provider_context_split_index(&context.preserved_turns, keep_recent_budget)
    {
        (
            context.preserved_turns[..split_index].to_vec(),
            context.preserved_turns[split_index..].to_vec(),
            false,
            None,
        )
    } else if trigger.trigger_reason == "manual" && context.preserved_turns.len() >= 2 {
        let split_index = context.preserved_turns.len() - 1;
        (
            context.preserved_turns[..split_index].to_vec(),
            context.preserved_turns[split_index..].to_vec(),
            false,
            None,
        )
    } else if trigger.trigger_reason == "overflow_retry" && !context.preserved_turns.is_empty() {
        (context.preserved_turns.clone(), Vec::new(), false, None)
    } else {
        return None;
    };

    for turn in older_turns.iter_mut().chain(recent_turns.iter_mut()) {
        sanitize_provider_turn_failure_metadata(turn, redactor);
    }

    let pruned_tool_artifacts =
        collect_pruned_tool_artifacts(run_state, trigger, context, &older_turns);
    let operational_memory =
        collect_compacted_file_operation_facts(run_state, trigger, context, &older_turns, redactor);
    let facts = build_provider_compaction_facts(
        context,
        &older_turns,
        &pruned_tool_artifacts,
        operational_memory,
    );
    let tail_boundary = build_provider_compaction_tail_boundary(
        &recent_turns,
        preserved_tokens_estimate(&recent_turns),
        keep_recent_budget,
        trigger,
        split_tail,
        split_prefix_summary,
    );

    Some(ProviderContextCompactionPlan {
        older_turns,
        recent_turns,
        pruned_tool_artifacts,
        facts,
        tail_boundary,
    })
}

fn provider_context_keep_recent_tokens(metadata: &RecordedRuntimeContext) -> u32 {
    metadata
        .max_input_tokens
        .or(metadata.context_window_tokens)
        .map(|budget| {
            (budget / 4).clamp(
                PROVIDER_CONTEXT_COMPACTION_KEEP_RECENT_MIN_TOKENS,
                PROVIDER_CONTEXT_COMPACTION_KEEP_RECENT_MAX_TOKENS,
            )
        })
        .unwrap_or(2_048)
}

fn provider_context_reserve_tokens(metadata: &RecordedRuntimeContext, input_budget: u32) -> u32 {
    metadata
        .max_output_tokens
        .unwrap_or(PROVIDER_CONTEXT_COMPACTION_RESERVE_TOKENS)
        .max(PROVIDER_CONTEXT_COMPACTION_RESERVE_TOKENS)
        .min(input_budget.saturating_sub(1))
}

fn provider_context_split_index(
    turns: &[ProviderConversationTurn],
    keep_recent_budget: u32,
) -> Option<usize> {
    if turns.len() < 2 {
        return None;
    }

    let mut keep_from = turns.len() - 1;
    let mut kept_tokens = approximate_turn_tokens(&turns[keep_from]);
    for index in (0..keep_from).rev() {
        let candidate_tokens = approximate_turn_tokens(&turns[index]);
        if kept_tokens.saturating_add(candidate_tokens) > keep_recent_budget {
            break;
        }
        kept_tokens = kept_tokens.saturating_add(candidate_tokens);
        keep_from = index;
    }

    (keep_from > 0).then_some(keep_from)
}

fn split_latest_oversized_turn(
    turns: &[ProviderConversationTurn],
    keep_recent_budget: u32,
    trigger_reason: &str,
) -> Option<(
    ProviderConversationTurn,
    ProviderConversationTurn,
    SplitPrefixSummaryDecision,
)> {
    if turns.is_empty()
        || !matches!(
            trigger_reason,
            "manual" | "overflow_retry" | "pre_prompt" | "failed_response"
        )
    {
        return None;
    }

    let latest = turns.last()?;
    if !can_split_latest_turn_safely(latest) {
        return None;
    }

    let latest_tokens = approximate_turn_tokens(latest);
    if latest_tokens <= keep_recent_budget || latest.assistant_response.chars().count() < 2 {
        return None;
    }

    let suffix_chars = (keep_recent_budget.saturating_mul(4) as usize)
        .max(PROVIDER_CONTEXT_COMPACTION_TURN_EXCERPT_MAX_CHARS)
        .min(latest.assistant_response.chars().count().saturating_sub(1));
    let split_at = latest
        .assistant_response
        .chars()
        .count()
        .saturating_sub(suffix_chars);
    let assistant_prefix = latest
        .assistant_response
        .chars()
        .take(split_at)
        .collect::<String>();
    let assistant_suffix = latest
        .assistant_response
        .chars()
        .skip(split_at)
        .collect::<String>();
    if assistant_prefix.trim().is_empty() || assistant_suffix.trim().is_empty() {
        return None;
    }
    let split_prefix_summary =
        SplitPrefixSummaryDecision::deterministic(summarize_compaction_text(&assistant_prefix));

    let mut older_turn = latest.clone();
    older_turn.user_prompt = format!(
        "{}\n\n[Harness compaction note: earlier prefix of an oversized latest turn; this prefix is summarized in the checkpoint and the suffix remains provider-visible.]",
        latest.user_prompt
    );
    older_turn.assistant_response = assistant_prefix;
    older_turn.messages.clear();
    let mut recent_turn = latest.clone();
    recent_turn.user_prompt = format!(
        "{}\n\n[Harness compaction note: preserved suffix of an oversized latest turn; earlier prefix is summarized in the checkpoint.]",
        latest.user_prompt
    );
    recent_turn.assistant_response = assistant_suffix;
    recent_turn.messages.clear();
    Some((older_turn, recent_turn, split_prefix_summary))
}

fn can_split_latest_turn_safely(turn: &ProviderConversationTurn) -> bool {
    if !turn.artifacts.is_empty() {
        return false;
    }
    if turn.messages.iter().any(|message| match message {
        ConversationMessage::Assistant(assistant) => !assistant.tool_calls.is_empty(),
        ConversationMessage::ToolResult(_) => true,
        ConversationMessage::Checkpoint(_) | ConversationMessage::User(_) => false,
    }) {
        return false;
    }

    match turn.status {
        ProviderConversationTurnStatus::Completed => true,
        ProviderConversationTurnStatus::Failed => {
            turn.failure_stage.as_deref() == Some("provider_error")
        }
        ProviderConversationTurnStatus::Aborted => false,
    }
}

fn latest_oversized_turn_needs_summary_only(
    turns: &[ProviderConversationTurn],
    keep_recent_budget: u32,
    trigger_reason: &str,
) -> bool {
    if !matches!(trigger_reason, "overflow_retry" | "failed_response") {
        return false;
    }

    let Some(latest) = turns.last() else {
        return false;
    };
    approximate_turn_tokens(latest) > keep_recent_budget && !can_split_latest_turn_safely(latest)
}

fn preserved_tokens_estimate(turns: &[ProviderConversationTurn]) -> u32 {
    turns.iter().map(approximate_turn_tokens).sum::<u32>()
}

#[derive(Debug, Clone, Default)]
struct ProviderOperationalMemoryFacts {
    read_files: Vec<ProviderFileOperationFact>,
    modified_files: Vec<ProviderFileOperationFact>,
    operation_facts: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderFileOperationKind {
    Read,
    Modified,
}

impl ProviderFileOperationKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Modified => "modified",
        }
    }
}

fn collect_compacted_file_operation_facts(
    run_state: &RunState,
    trigger: &ProviderCompactionTrigger,
    context: &ProviderContext,
    older_turns: &[ProviderConversationTurn],
    redactor: &(impl Redactor + ?Sized),
) -> ProviderOperationalMemoryFacts {
    if older_turns.is_empty() {
        return ProviderOperationalMemoryFacts::default();
    }

    let lower_bound_seq = context
        .checkpoint
        .as_ref()
        .map(|checkpoint| checkpoint.through_seq)
        .unwrap_or(0);
    let through_seq = run_state.next_event_seq.saturating_sub(1);
    let compacted_request_ids = compacted_request_ids_for_operational_memory(
        run_state,
        trigger,
        context,
        older_turns,
        lower_bound_seq,
        through_seq,
    );
    if compacted_request_ids.is_empty() {
        return ProviderOperationalMemoryFacts::default();
    }

    let events = match read_historical_events_until(
        &run_state.info.run_id,
        &run_state.info.events_path,
        through_seq,
    ) {
        Ok(events) => events,
        Err(_) => return ProviderOperationalMemoryFacts::default(),
    };

    let mut tool_operations: BTreeMap<String, ProviderFileOperationKind> = BTreeMap::new();
    let mut tool_output_paths: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for event in events
        .iter()
        .filter(|event| event.seq > lower_bound_seq && event.seq <= through_seq)
    {
        if !event_belongs_to_compacted_request(event, &compacted_request_ids) {
            continue;
        }
        match &event.payload {
            EventV1::ToolCallRequested(payload) => {
                if let Some(operation) = tool_call_operation(
                    Some(payload.tool_id.as_str()),
                    payload.metadata.as_ref(),
                    None,
                ) {
                    tool_operations.insert(payload.tool_call_id.clone(), operation);
                }
            }
            EventV1::ToolCallFinished(payload) => {
                if let Some(operation) = tool_call_operation(None, payload.metadata.as_ref(), None)
                {
                    tool_operations
                        .entry(payload.tool_call_id.clone())
                        .or_insert(operation);
                }
                let paths = extract_output_json_path_fields(payload.output_json.as_ref());
                if !paths.is_empty() {
                    tool_output_paths.insert(payload.tool_call_id.clone(), paths);
                }
            }
            _ => {}
        }
    }

    let mut read = BTreeMap::new();
    let mut modified = BTreeMap::new();
    for event in events
        .iter()
        .filter(|event| event.seq > lower_bound_seq && event.seq <= through_seq)
    {
        if !event_belongs_to_compacted_request(event, &compacted_request_ids) {
            continue;
        }
        match &event.payload {
            EventV1::EditApplied(payload) => {
                add_file_operation_fact(
                    &mut modified,
                    &run_state.info.workspace_root,
                    &payload.path,
                    ProviderFileOperationKind::Modified,
                    event.seq,
                    format!("edit:{}", payload.edit_id),
                    None,
                    redactor,
                );
            }
            EventV1::ArtifactWritten(payload) => {
                let Some(tool_call_id) = payload.tool_call_id.as_deref() else {
                    continue;
                };
                let operation = tool_call_operation(None, None, payload.tool_metadata.as_ref())
                    .or_else(|| tool_operations.get(tool_call_id).copied())
                    .unwrap_or(ProviderFileOperationKind::Read);
                let paths = extract_artifact_workspace_paths(
                    payload,
                    tool_output_paths.get(tool_call_id).map(Vec::as_slice),
                );
                let summary = payload
                    .metadata
                    .get("summary")
                    .or_else(|| payload.metadata.get("operation_summary"))
                    .map(|value| summarize_compaction_text(value));
                for path in paths {
                    let target = match operation {
                        ProviderFileOperationKind::Read => &mut read,
                        ProviderFileOperationKind::Modified => &mut modified,
                    };
                    add_file_operation_fact(
                        target,
                        &run_state.info.workspace_root,
                        &path,
                        operation,
                        event.seq,
                        format!("artifact:{tool_call_id}"),
                        summary.clone(),
                        redactor,
                    );
                }
            }
            EventV1::ToolCallFinished(payload) => {
                let operation = tool_operations
                    .get(&payload.tool_call_id)
                    .copied()
                    .or_else(|| tool_call_operation(None, payload.metadata.as_ref(), None));
                if operation != Some(ProviderFileOperationKind::Read) {
                    continue;
                }
                for path in extract_output_json_path_fields(payload.output_json.as_ref()) {
                    add_file_operation_fact(
                        &mut read,
                        &run_state.info.workspace_root,
                        &path,
                        ProviderFileOperationKind::Read,
                        event.seq,
                        format!("tool:{}", payload.tool_call_id),
                        payload
                            .output_summary
                            .as_deref()
                            .map(summarize_compaction_text),
                        redactor,
                    );
                }
            }
            _ => {}
        }
    }

    finalize_provider_operational_memory(read, modified)
}

fn compacted_request_ids_for_operational_memory(
    run_state: &RunState,
    trigger: &ProviderCompactionTrigger,
    context: &ProviderContext,
    older_turns: &[ProviderConversationTurn],
    lower_bound_seq: u64,
    through_seq: u64,
) -> BTreeSet<String> {
    let mut request_ids = older_turns
        .iter()
        .filter_map(|turn| turn.request_id.as_deref())
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    if !request_ids.is_empty() {
        return request_ids;
    }

    let Ok(historical_turns) = collect_historical_agent_turns_until(
        &run_state.info.run_id,
        &run_state.info.events_path,
        &trigger.agent_id,
        lower_bound_seq,
        through_seq,
    ) else {
        return BTreeSet::new();
    };
    if historical_turns.len() < context.preserved_turns.len() {
        return BTreeSet::new();
    }
    let aligned_turns = &historical_turns[historical_turns.len() - context.preserved_turns.len()..];
    if !aligned_turns
        .iter()
        .zip(&context.preserved_turns)
        .all(|(historical, current)| {
            historical.user_prompt == current.user_prompt
                && historical.assistant_response == current.assistant_response
        })
    {
        return BTreeSet::new();
    }
    request_ids.extend(
        aligned_turns
            .iter()
            .take(older_turns.len())
            .map(|turn| turn.request_id.clone()),
    );
    request_ids
}

fn read_historical_events_until(
    run_id: &str,
    events_path: &Path,
    through_seq: u64,
) -> Result<Vec<EventEnvelopeV1>, CoordinatorError> {
    let file =
        fs::File::open(events_path).map_err(|source| CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!(
                "failed to open historical events {}: {source}",
                events_path.display()
            ),
        })?;
    let mut expected_seq = 1_u64;
    let mut events = Vec::new();
    for (line_number, line) in BufReader::new(file).lines().enumerate() {
        let Some(event) = parse_historical_event_line(run_id, events_path, line_number, line)?
        else {
            continue;
        };
        validate_historical_event_seq(run_id, events_path, &event, expected_seq)?;
        expected_seq = expected_seq.saturating_add(1);
        if event.seq > through_seq {
            break;
        }
        events.push(event);
    }
    Ok(events)
}

fn parse_historical_event_line(
    run_id: &str,
    events_path: &Path,
    line_number: usize,
    line: io::Result<String>,
) -> Result<Option<EventEnvelopeV1>, CoordinatorError> {
    let line = line.map_err(|source| CoordinatorError::ResumeRestoreFailed {
        run_id: run_id.to_string(),
        reason: format!(
            "failed to read historical event line {} in {}: {source}",
            line_number + 1,
            events_path.display()
        ),
    })?;
    if line.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&line)
        .map(Some)
        .map_err(|source| CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!(
                "invalid historical event line {} in {}: {source}",
                line_number + 1,
                events_path.display()
            ),
        })
}

fn validate_historical_event_seq(
    run_id: &str,
    events_path: &Path,
    event: &EventEnvelopeV1,
    expected_seq: u64,
) -> Result<(), CoordinatorError> {
    if event.seq == expected_seq {
        return Ok(());
    }
    Err(CoordinatorError::ResumeRestoreFailed {
        run_id: run_id.to_string(),
        reason: format!(
            "historical sequence mismatch at {}: expected {expected_seq}, got {}",
            events_path.display(),
            event.seq
        ),
    })
}

fn event_belongs_to_compacted_request(
    event: &EventEnvelopeV1,
    compacted_request_ids: &BTreeSet<String>,
) -> bool {
    event
        .correlation_id
        .as_deref()
        .is_some_and(|request_id| compacted_request_ids.contains(request_id))
}

fn tool_call_operation(
    invoked_tool_id: Option<&str>,
    call_metadata: Option<&ToolCallMetadata>,
    artifact_metadata: Option<&ToolIdentityMetadata>,
) -> Option<ProviderFileOperationKind> {
    let identity = if artifact_metadata.is_some() {
        ResolvedToolIdentity::from_tool_artifact(invoked_tool_id, artifact_metadata)
    } else {
        ResolvedToolIdentity::from_tool_call(invoked_tool_id, call_metadata)
    };
    let operation = [
        identity.canonical_tool_id.as_deref(),
        identity.effective_tool_id.as_deref(),
        identity.invoked_tool_id.as_deref(),
        identity.alias_source_tool_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find_map(operation_for_tool_id);
    operation
}

fn operation_for_tool_id(tool_id: &str) -> Option<ProviderFileOperationKind> {
    let normalized = tool_id.trim().to_ascii_lowercase();
    if matches!(
        normalized.as_str(),
        "edit" | "apply" | "edit.hashline_apply"
    ) {
        return Some(ProviderFileOperationKind::Modified);
    }
    if matches!(
        normalized.as_str(),
        "read" | "grep" | "glob" | "list" | "lsp"
    ) || normalized.starts_with("lsp.")
    {
        return Some(ProviderFileOperationKind::Read);
    }
    None
}

fn extract_output_json_path_fields(output_json: Option<&Value>) -> Vec<String> {
    let Some(value) = output_json else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    collect_direct_path_fields(value, &mut paths);
    for key in ["files", "matches"] {
        if let Some(items) = value.get(key).and_then(Value::as_array) {
            for item in items {
                collect_direct_path_fields(item, &mut paths);
            }
        }
    }
    paths
}

fn collect_direct_path_fields(value: &Value, paths: &mut Vec<String>) {
    for key in ["path", "filePath", "file_path"] {
        if let Some(path) = value
            .get(key)
            .and_then(Value::as_str)
            .and_then(non_empty_trimmed)
        {
            paths.push(path.to_string());
        }
    }
}

fn extract_artifact_workspace_paths(
    payload: &ArtifactWrittenEvent,
    output_paths: Option<&[String]>,
) -> Vec<String> {
    let mut paths = Vec::new();
    for key in ["path", "filePath", "file_path"] {
        if let Some(path) = payload
            .metadata
            .get(key)
            .map(String::as_str)
            .and_then(non_empty_trimmed)
        {
            paths.push(path.to_string());
        }
    }
    if let Some(output_paths) = output_paths {
        paths.extend(output_paths.iter().cloned());
    }
    paths.sort();
    paths.dedup();
    paths
}

#[expect(
    clippy::too_many_arguments,
    reason = "operational-memory fact construction keeps path normalization, provenance, and redaction inputs explicit"
)]
fn add_file_operation_fact(
    facts: &mut BTreeMap<(String, String), ProviderFileOperationFact>,
    workspace_root: &Path,
    raw_path: &str,
    operation: ProviderFileOperationKind,
    seq: u64,
    source: String,
    summary: Option<String>,
    redactor: &(impl Redactor + ?Sized),
) {
    let Some(path) =
        workspace_relative_path_from_maybe_absolute(workspace_root, Path::new(raw_path))
    else {
        return;
    };
    let path = redactor.redact_text(&path);
    let operation = operation.as_str().to_string();
    let summary = summary
        .map(|summary| redactor.redact_text(&summary))
        .map(|summary| summarize_compaction_text(&summary));
    let fact = facts
        .entry((path.clone(), operation.clone()))
        .or_insert_with(|| ProviderFileOperationFact {
            path,
            operation,
            first_seq: Some(seq),
            last_seq: Some(seq),
            sources: Vec::new(),
            summary: None,
        });
    fact.first_seq = Some(fact.first_seq.map_or(seq, |first_seq| first_seq.min(seq)));
    fact.last_seq = Some(fact.last_seq.map_or(seq, |last_seq| last_seq.max(seq)));
    if !fact.sources.iter().any(|existing| existing == &source) {
        fact.sources.push(source);
        fact.sources.sort();
    }
    if fact.summary.is_none() {
        fact.summary = summary;
    }
}

fn finalize_provider_operational_memory(
    read: BTreeMap<(String, String), ProviderFileOperationFact>,
    modified: BTreeMap<(String, String), ProviderFileOperationFact>,
) -> ProviderOperationalMemoryFacts {
    let (read_files, read_omitted) = cap_file_operation_facts(read);
    let (modified_files, modified_omitted) = cap_file_operation_facts(modified);
    let mut operation_facts = Vec::new();
    if read_omitted > 0 {
        operation_facts.push(format!("{read_omitted} additional read file(s) omitted"));
    }
    if modified_omitted > 0 {
        operation_facts.push(format!(
            "{modified_omitted} additional modified file(s) omitted"
        ));
    }
    for fact in read_files.iter().chain(modified_files.iter()) {
        if operation_facts.len() >= PROVIDER_CONTEXT_OPERATION_FACT_LIMIT {
            break;
        }
        let sources = if fact.sources.is_empty() {
            "unknown source".to_string()
        } else {
            fact.sources.join(", ")
        };
        let mut line = format!("{} {} via {}", fact.operation, fact.path, sources);
        if let Some(summary) = fact
            .summary
            .as_deref()
            .filter(|summary| !summary.is_empty())
        {
            line.push_str(": ");
            line.push_str(summary);
        }
        operation_facts.push(summarize_compaction_text(&line));
    }
    operation_facts.truncate(PROVIDER_CONTEXT_OPERATION_FACT_LIMIT);
    ProviderOperationalMemoryFacts {
        read_files,
        modified_files,
        operation_facts,
    }
}

fn cap_file_operation_facts(
    facts: BTreeMap<(String, String), ProviderFileOperationFact>,
) -> (Vec<ProviderFileOperationFact>, usize) {
    let total = facts.len();
    let retained = facts
        .into_values()
        .take(PROVIDER_CONTEXT_FILE_OPERATION_FACT_LIMIT)
        .collect::<Vec<_>>();
    (
        retained,
        total.saturating_sub(PROVIDER_CONTEXT_FILE_OPERATION_FACT_LIMIT),
    )
}

fn build_provider_compaction_facts(
    context: &ProviderContext,
    older_turns: &[ProviderConversationTurn],
    pruned_tool_artifacts: &[EventArtifactRef],
    operational_memory: ProviderOperationalMemoryFacts,
) -> ProviderCompactionFacts {
    let compacted_turns = older_turns
        .iter()
        .map(|turn| ProviderCompactionTurnFact {
            request_id: turn.request_id.clone(),
            first_seq: turn.first_seq,
            last_seq: turn.last_seq,
            user_excerpt: summarize_compaction_text(&turn.user_prompt),
            assistant_excerpt: summarize_compaction_text(&turn.assistant_response),
            status: turn.status,
            failure_stage: turn.failure_stage.clone(),
            failure_reason: turn.failure_reason.clone(),
            artifacts: turn.artifacts.clone(),
        })
        .collect::<Vec<_>>();

    let mut relevant_artifacts = Vec::new();
    let mut artifact_seen = BTreeSet::new();
    for artifact in pruned_tool_artifacts
        .iter()
        .chain(older_turns.iter().flat_map(|turn| turn.artifacts.iter()))
    {
        let key = (artifact.path.clone(), artifact.digest.clone());
        if artifact_seen.insert(key) {
            relevant_artifacts.push(artifact.clone());
        }
    }

    let mut touched_files = operational_memory
        .read_files
        .iter()
        .chain(operational_memory.modified_files.iter())
        .map(|fact| fact.path.clone())
        .collect::<Vec<_>>();
    touched_files.sort();
    touched_files.dedup();

    ProviderCompactionFacts {
        previous_checkpoint_id: context
            .checkpoint
            .as_ref()
            .map(|checkpoint| checkpoint.checkpoint_id.clone()),
        compacted_turns,
        relevant_artifacts,
        read_files: operational_memory.read_files,
        modified_files: operational_memory.modified_files,
        operation_facts: operational_memory.operation_facts,
        touched_files,
        pending_work: Vec::new(),
        blockers: Vec::new(),
    }
}

fn sanitize_provider_turn_failure_metadata(
    turn: &mut ProviderConversationTurn,
    redactor: &(impl Redactor + ?Sized),
) {
    if let Some(reason) = turn.failure_reason.take() {
        let redacted = redactor.redact_text(&reason);
        let summarized = summarize_compaction_text(&redacted);
        turn.failure_reason = if non_empty_trimmed(&summarized).is_some() {
            Some(summarized)
        } else {
            None
        };
    }
}

fn build_provider_compaction_tail_boundary(
    recent_turns: &[ProviderConversationTurn],
    preserved_tokens_estimate: u32,
    keep_recent_budget: u32,
    trigger: &ProviderCompactionTrigger,
    split_tail: bool,
    split_prefix_summary: Option<SplitPrefixSummaryDecision>,
) -> ProviderCompactionTailBoundary {
    let first_preserved = recent_turns.first();
    let mode = if split_tail {
        "split_oversized_turn_tail".to_string()
    } else if recent_turns.is_empty() {
        "summary_only".to_string()
    } else if preserved_tokens_estimate > keep_recent_budget {
        "oversized_whole_turn_tail".to_string()
    } else {
        "whole_turn_tail".to_string()
    };
    let note = if mode == "split_oversized_turn_tail" {
        let mut note = "The latest oversized turn was split inside the checkpoint artifact: the earlier prefix is summarized in the checkpoint and a suffix remains provider-visible as recent context.".to_string();
        if let Some(split_prefix_summary) = split_prefix_summary.as_ref() {
            note.push_str(" Split prefix summary source: ");
            note.push_str(split_prefix_summary.source.as_str());
            note.push('.');
            if let Some(reason) = split_prefix_summary.fallback_reason.as_deref() {
                note.push_str(" Fallback reason: ");
                note.push_str(&summarize_compaction_text(reason));
                note.push('.');
            }
        }
        Some(note)
    } else if mode == "oversized_whole_turn_tail" {
        Some("Latest preserved turn exceeds the keep-recent budget; the harness records this tail boundary but does not split provider/tool turns yet.".to_string())
    } else if matches!(
        trigger.trigger_reason.as_str(),
        "overflow_retry" | "failed_response"
    ) && recent_turns.is_empty()
    {
        Some(format!(
            "{} compaction used summary-only context because preserving or splitting the latest oversized turn would risk invalid provider ordering or still exceed the provider window.",
            trigger.trigger_reason
        ))
    } else {
        None
    };

    ProviderCompactionTailBoundary {
        mode,
        preserved_turns: recent_turns.len() as u32,
        preserved_tokens_estimate,
        preserved_from_request_id: first_preserved.and_then(|turn| turn.request_id.clone()),
        preserved_from_seq: first_preserved.and_then(|turn| turn.first_seq),
        split_prefix_summary: split_prefix_summary.map(|decision| decision.summary),
        note,
    }
}

fn build_provider_compaction_summary_source(
    metadata: &RecordedRuntimeContext,
    trigger: &ProviderCompactionTrigger,
    existing_summary: Option<&str>,
    request: SummarySourceRequest,
    config: &CompactionRuntimeConfig,
) -> ProviderCompactionSummarySource {
    let (strategy, model_ref, model_backed, deterministic_fallback) = match request {
        SummarySourceRequest::Hook => (
            "hook_supplied_summary".to_string(),
            trigger.model_ref.clone(),
            false,
            false,
        ),
        SummarySourceRequest::Model {
            model_ref,
            deterministic_fallback,
        } => (
            if deterministic_fallback {
                "model_backed_deterministic_fallback".to_string()
            } else {
                "model_backed_summary".to_string()
            },
            model_ref,
            true,
            deterministic_fallback,
        ),
        SummarySourceRequest::Deterministic => (
            "deterministic_rolling_summary".to_string(),
            trigger.model_ref.clone(),
            false,
            true,
        ),
        SummarySourceRequest::DeterministicForModelRef { model_ref } => (
            "deterministic_rolling_summary".to_string(),
            model_ref,
            false,
            true,
        ),
    };
    ProviderCompactionSummarySource {
        strategy,
        model_ref,
        provider_id: trigger
            .provider_id
            .clone()
            .or_else(|| Some(metadata.provider.clone())),
        model_id: trigger
            .model_id
            .clone()
            .or_else(|| Some(metadata.model.clone())),
        reasoning_effort: metadata.reasoning_effort.clone(),
        text_verbosity: metadata.text_verbosity.clone(),
        previous_summary_used: existing_summary.and_then(non_empty_trimmed).is_some(),
        model_backed,
        deterministic_fallback,
        summary_contract_version: Some(PROVIDER_CONTEXT_SUMMARY_CONTRACT_VERSION),
        summary_contract_enforced: Some(config.structured_summary_contract),
    }
}

fn compaction_summary_override_from_hooks(batch: &HookExecutionBatch) -> Option<String> {
    batch.hook_executions.iter().rev().find_map(|execution| {
        if execution.status != HookExecutionStatus::Succeeded {
            return None;
        }
        if let Some(summary) = execution.effects.iter().rev().find_map(|effect| {
            if effect.kind != HookEffectKind::TransformContext {
                return None;
            }
            effect
                .summary
                .as_deref()
                .and_then(|summary| {
                    summary
                        .strip_prefix("compaction_summary:")
                        .or(Some(summary))
                })
                .and_then(non_empty_trimmed)
                .map(ToOwned::to_owned)
        }) {
            return Some(summary);
        }
        let summary = execution.output_summary.as_deref()?.trim();
        summary
            .strip_prefix("compaction_summary:")
            .and_then(non_empty_trimmed)
            .map(ToOwned::to_owned)
    })
}

fn compaction_summary_model_ref(
    config: &CompactionRuntimeConfig,
    trigger: &ProviderCompactionTrigger,
) -> String {
    config
        .model_ref
        .as_deref()
        .and_then(non_empty_trimmed)
        .unwrap_or(trigger.model_ref.as_str())
        .to_string()
}

async fn model_backed_compaction_summary_for(
    provider: Arc<dyn Provider>,
    compaction_config: &CompactionRuntimeConfig,
    run_state: &RunState,
    trigger: &ProviderCompactionTrigger,
    redactor: &(impl Redactor + ?Sized),
) -> Result<ModelBackedCompactionSummary, String> {
    let context = run_state
        .provider_context_by_agent
        .get(&trigger.agent_id)
        .cloned()
        .unwrap_or_default();
    let metadata = recorded_runtime_context_for_compaction(run_state, trigger);
    if !should_compact_provider_context(&context, &metadata, trigger, compaction_config) {
        return Err("compaction would be a no-op".to_string());
    }

    let keep_recent_budget = provider_context_keep_recent_tokens(&metadata);
    let tokens_before = approximate_provider_context_tokens(&context);
    let Some(initial_plan) = build_provider_context_compaction_plan(
        run_state,
        trigger,
        &context,
        redactor,
        keep_recent_budget,
        compaction_config,
        None,
    ) else {
        return Err("no compactable provider turns were available".to_string());
    };
    let model_ref = compaction_summary_model_ref(compaction_config, trigger);
    let split_prefix_summary = model_backed_split_prefix_summary_decision(
        provider.clone(),
        &model_ref,
        &initial_plan,
        trigger,
    )
    .await;
    let plan = if let Some(split_prefix_summary) = split_prefix_summary.as_ref() {
        build_provider_context_compaction_plan(
            run_state,
            trigger,
            &context,
            redactor,
            keep_recent_budget,
            compaction_config,
            Some(split_prefix_summary),
        )
        .ok_or_else(|| "no compactable provider turns were available".to_string())?
    } else {
        initial_plan
    };
    let draft_source = build_provider_compaction_summary_source(
        &metadata,
        trigger,
        context.compacted_summary.as_deref(),
        SummarySourceRequest::Deterministic,
        compaction_config,
    );
    let deterministic_draft = build_provider_context_summary(
        context.compacted_summary.as_deref(),
        &plan.older_turns,
        &plan.pruned_tool_artifacts,
        &plan.facts,
        &plan.tail_boundary,
        &draft_source,
        compaction_config,
    );
    let model = AgentModelRef::parse(&model_ref);
    let request = CompletionRequest {
        provider_id: Some(model.provider_id),
        model_id: model.model_id,
        messages: vec![
            CompletionMessage {
                role: MessageRole::System,
                content: "You create Harness provider-context checkpoint summaries. Return only the updated structured checkpoint summary, preserving the requested markdown headings and rolling forward prior summary content instead of appending a raw previous-summary blob.".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
            CompletionMessage {
                role: MessageRole::User,
                content: build_model_compaction_prompt(
                    context.compacted_summary.as_deref(),
                    &plan,
                    &deterministic_draft,
                    compaction_config,
                ),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
        ],
        temperature: None,
        max_tokens: Some(PROVIDER_CONTEXT_COMPACTION_SUMMARY_MAX_CHARS as u32 / 3),
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        tools: None,
        tool_choice: None,
        stream: true,
    };

    let mut stream = provider.stream_completion(request).await;
    let mut output = String::new();
    while let Some(event) = stream.next().await {
        match event {
            ProviderStreamEvent::TextDelta(delta) => output.push_str(&delta),
            ProviderStreamEvent::Error { message } => return Err(message),
            ProviderStreamEvent::Done { .. } | ProviderStreamEvent::DoneWithMetadata { .. } => {
                break
            }
            ProviderStreamEvent::Start
            | ProviderStreamEvent::Started { .. }
            | ProviderStreamEvent::ReasoningDelta(_)
            | ProviderStreamEvent::ToolCallDelta { .. }
            | ProviderStreamEvent::ToolCallComplete { .. } => {}
        }
    }

    validate_model_compaction_summary(&output, tokens_before, &plan, compaction_config).map(
        |summary| ModelBackedCompactionSummary {
            summary,
            split_prefix_summary,
        },
    )
}

async fn model_backed_split_prefix_summary_decision(
    provider: Arc<dyn Provider>,
    model_ref: &str,
    plan: &ProviderContextCompactionPlan,
    trigger: &ProviderCompactionTrigger,
) -> Option<SplitPrefixSummaryDecision> {
    if plan.tail_boundary.mode != "split_oversized_turn_tail" {
        return None;
    }
    let deterministic_summary = plan.tail_boundary.split_prefix_summary.clone()?;
    let Some(prefix_turn) = plan.older_turns.last() else {
        return Some(SplitPrefixSummaryDecision::model_fallback(
            deterministic_summary,
            "split prefix turn was unavailable".to_string(),
        ));
    };

    match model_backed_split_prefix_summary_for(provider, model_ref, prefix_turn).await {
        Ok(summary) => Some(SplitPrefixSummaryDecision::model(summary)),
        Err(reason) => {
            tracing::warn!(
                %reason,
                agent_id = %trigger.agent_id,
                "model-backed split prefix summary fell back to deterministic summary"
            );
            Some(SplitPrefixSummaryDecision::model_fallback(
                deterministic_summary,
                reason,
            ))
        }
    }
}

async fn model_backed_split_prefix_summary_for(
    provider: Arc<dyn Provider>,
    model_ref: &str,
    prefix_turn: &ProviderConversationTurn,
) -> Result<String, String> {
    let model = AgentModelRef::parse(model_ref);
    let request = CompletionRequest {
        provider_id: Some(model.provider_id),
        model_id: model.model_id,
        messages: vec![
            CompletionMessage {
                role: MessageRole::System,
                content: "You summarize oversized Harness turn prefixes for context compaction. Return only the requested markdown summary.".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
            CompletionMessage {
                role: MessageRole::User,
                content: build_split_prefix_summary_prompt(prefix_turn),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
        ],
        temperature: None,
        max_tokens: Some(PROVIDER_CONTEXT_SPLIT_PREFIX_SUMMARY_MAX_CHARS as u32 / 3),
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        tools: None,
        tool_choice: None,
        stream: true,
    };

    let mut stream = provider.stream_completion(request).await;
    let mut output = String::new();
    while let Some(event) = stream.next().await {
        match event {
            ProviderStreamEvent::TextDelta(delta) => output.push_str(&delta),
            ProviderStreamEvent::Error { message } => return Err(message),
            ProviderStreamEvent::Done { .. } | ProviderStreamEvent::DoneWithMetadata { .. } => {
                break
            }
            ProviderStreamEvent::Start
            | ProviderStreamEvent::Started { .. }
            | ProviderStreamEvent::ReasoningDelta(_)
            | ProviderStreamEvent::ToolCallDelta { .. }
            | ProviderStreamEvent::ToolCallComplete { .. } => {}
        }
    }

    validate_model_split_prefix_summary(&output)
}

fn build_split_prefix_summary_prompt(prefix_turn: &ProviderConversationTurn) -> String {
    format!(
        "<conversation>\nUser: {user}\nAssistant prefix: {assistant}\n</conversation>\n\nThis is the PREFIX of a turn that was too large to keep. The SUFFIX (recent work) is retained.\n\nSummarize the prefix to provide context for the retained suffix:\n\n## Original Request\n[What did the user ask for in this turn?]\n\n## Early Progress\n- [Key decisions and work done in the prefix]\n\n## Context for Suffix\n- [Information needed to understand the retained recent work]\n\nBe concise. Focus on what's needed to understand the kept suffix.",
        user = prefix_turn.user_prompt,
        assistant = prefix_turn.assistant_response,
    )
}

fn validate_model_split_prefix_summary(summary: &str) -> Result<String, String> {
    let trimmed = summary.trim();
    if trimmed.is_empty() {
        return Err("model split prefix summary was empty".to_string());
    }
    if trimmed.chars().count() > PROVIDER_CONTEXT_SPLIT_PREFIX_SUMMARY_MAX_CHARS {
        return Err("model split prefix summary exceeded the character budget".to_string());
    }
    for heading in PROVIDER_CONTEXT_SPLIT_PREFIX_SUMMARY_HEADINGS {
        if !summary_contains_heading(trimmed, heading) {
            return Err(format!(
                "model split prefix summary missed required heading `{heading}`"
            ));
        }
    }
    Ok(trimmed.to_string())
}

fn build_model_compaction_prompt(
    existing_summary: Option<&str>,
    plan: &ProviderContextCompactionPlan,
    deterministic_draft: &str,
    config: &CompactionRuntimeConfig,
) -> String {
    let compacted_facts = plan
        .facts
        .compacted_turns
        .iter()
        .enumerate()
        .map(|(index, fact)| {
            format!(
                "{}. user={} assistant={}",
                index + 1,
                fact.user_excerpt,
                fact.assistant_excerpt
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let prior = existing_summary
        .and_then(non_empty_trimmed)
        .unwrap_or("(none)");
    let required_headings = provider_context_summary_required_headings(config).join(", ");
    let split_prefix_summary = plan
        .tail_boundary
        .split_prefix_summary
        .as_deref()
        .unwrap_or("none");
    let operational_memory = operational_memory_summary_block(&plan.facts);

    format!(
        "Update the Harness checkpoint summary for compacted provider context.\n\nRequired output rules:\n- Return markdown only.\n- Keep these headings exactly: {required_headings}.\n- Include `## Operational Memory` with `Read files:` and `Modified files:` subsections when operational memory is present.\n- Roll forward any still-relevant previous summary content into the structured sections. Do not append or label a raw previous-summary blob.\n- If split prefix summary is not `none`, preserve it under Critical Context and Source Facts wording.\n- Keep under {max_chars} characters.\n\nPrevious checkpoint summary:\n{prior}\n\nNew compacted turn facts:\n{compacted_facts}\n\nOperational memory facts:\n{operational_memory}\n\nTail boundary: {mode}; preserved turns: {preserved_turns}; note: {note}; split prefix summary: {split_prefix_summary}\n\nDeterministic Harness draft to improve:\n{deterministic_draft}",
        max_chars = PROVIDER_CONTEXT_COMPACTION_SUMMARY_MAX_CHARS,
        mode = plan.tail_boundary.mode,
        preserved_turns = plan.tail_boundary.preserved_turns,
        note = plan.tail_boundary.note.as_deref().unwrap_or("none"),
    )
}

fn validate_model_compaction_summary(
    summary: &str,
    tokens_before: u32,
    plan: &ProviderContextCompactionPlan,
    config: &CompactionRuntimeConfig,
) -> Result<String, String> {
    let trimmed = summary.trim();
    if trimmed.is_empty() {
        return Err("model summary was empty".to_string());
    }
    if trimmed.chars().count() > PROVIDER_CONTEXT_COMPACTION_SUMMARY_MAX_CHARS {
        return Err("model summary exceeded the checkpoint summary character budget".to_string());
    }
    for heading in provider_context_summary_required_headings(config) {
        if !summary_contains_heading(trimmed, heading) {
            return Err(format!("model summary missed required heading `{heading}`"));
        }
    }
    if let Some(split_prefix_summary) = plan.tail_boundary.split_prefix_summary.as_deref() {
        if !trimmed.contains("Split prefix summary") {
            return Err(
                "model summary missed split prefix summary in Critical Context".to_string(),
            );
        }
        if !trimmed.contains("Source facts: split prefix summary") {
            return Err("model summary missed split prefix summary source facts".to_string());
        }
        let split_prefix_summary = split_prefix_summary.trim();
        let deterministic_excerpt = summarize_compaction_text(split_prefix_summary);
        if !trimmed.contains(split_prefix_summary) && !trimmed.contains(&deterministic_excerpt) {
            return Err("model summary missed split prefix summary content".to_string());
        }
    }
    let tokens_after = approximate_text_tokens(trimmed)
        .saturating_add(preserved_tokens_estimate(&plan.recent_turns));
    if tokens_after >= tokens_before {
        return Err("model summary would not reduce active provider context".to_string());
    }

    Ok(trimmed.to_string())
}

fn summary_contains_heading(summary: &str, heading: &str) -> bool {
    summary.lines().any(|line| line.trim() == heading)
}

fn build_provider_context_summary(
    existing_summary: Option<&str>,
    older_turns: &[ProviderConversationTurn],
    pruned_tool_artifacts: &[EventArtifactRef],
    facts: &ProviderCompactionFacts,
    tail_boundary: &ProviderCompactionTailBoundary,
    summary_source: &ProviderCompactionSummarySource,
    config: &CompactionRuntimeConfig,
) -> String {
    if !config.structured_summary_contract {
        return build_legacy_provider_context_summary(
            existing_summary,
            older_turns,
            pruned_tool_artifacts,
            facts,
            tail_boundary,
            summary_source,
            config,
        );
    }

    build_harness_provider_context_summary(
        existing_summary,
        older_turns,
        pruned_tool_artifacts,
        facts,
        tail_boundary,
        summary_source,
        config,
    )
}

fn build_legacy_provider_context_summary(
    existing_summary: Option<&str>,
    older_turns: &[ProviderConversationTurn],
    pruned_tool_artifacts: &[EventArtifactRef],
    facts: &ProviderCompactionFacts,
    tail_boundary: &ProviderCompactionTailBoundary,
    summary_source: &ProviderCompactionSummarySource,
    config: &CompactionRuntimeConfig,
) -> String {
    let headings = provider_context_summary_required_headings(config);
    let mut lines = Vec::new();
    lines.push(headings[0].to_string());
    lines.push(format!(
        "- Continue the current agent session after compacting {} older turn(s).",
        older_turns.len()
    ));
    lines.push(String::new());

    lines.push(headings[1].to_string());
    if let Some(existing_summary) = existing_summary.and_then(non_empty_trimmed) {
        lines.push("- Preserve still-relevant constraints, decisions, files, and next steps from the previous checkpoint summary.".to_string());
        lines.push(format!(
            "- Prior checkpoint constraints/context carried forward: {}",
            summarize_compaction_text(existing_summary)
        ));
    } else {
        lines.push("- (none recorded explicitly)".to_string());
    }
    lines.push(String::new());

    lines.push(headings[2].to_string());
    lines.push(headings[3].to_string());
    for (index, turn) in older_turns.iter().enumerate() {
        lines.push(format!(
            "- Turn {} user: {}",
            index + 1,
            summarize_compaction_text(&turn.user_prompt)
        ));
        lines.push(format!(
            "  Assistant: {}",
            summarize_compaction_text(&turn.assistant_response)
        ));
    }
    lines.push(headings[4].to_string());
    lines.push(
        "- Continue from the preserved recent turn(s) that follow this checkpoint summary."
            .to_string(),
    );
    lines.push(headings[5].to_string());
    lines.push("- (none recorded explicitly)".to_string());
    lines.push(String::new());

    lines.push(headings[6].to_string());
    lines.push("- Older provider-visible turns were compacted into this checkpoint; preserved recent turns and the current user message take precedence over this lossy summary.".to_string());
    if let Some(split_prefix_summary) = tail_boundary.split_prefix_summary.as_deref() {
        lines.push(format!(
            "- Split prefix summary: {split_prefix_summary}; the provider-visible suffix follows this checkpoint as recent context."
        ));
    }
    if let Some(existing_summary) = existing_summary.and_then(non_empty_trimmed) {
        lines.push(format!(
            "- Prior checkpoint decisions/context were rolled into this structured summary: {}",
            summarize_compaction_text(existing_summary)
        ));
    }
    lines.push(String::new());

    lines.push(headings[7].to_string());
    lines.push("1. Use the preserved recent turn(s) plus this checkpoint summary to continue the user's current task.".to_string());
    lines.push(String::new());

    lines.push(headings[8].to_string());
    lines.push(format!("- Compacted turns: {}", older_turns.len()));
    if let Some(previous_checkpoint_id) = facts.previous_checkpoint_id.as_deref() {
        lines.push(format!(
            "- Previous checkpoint: {previous_checkpoint_id}; this summary rolls forward from it."
        ));
    }
    lines.push(format!(
        "- Tail boundary: {} ({} preserved turn(s), ~{} token(s)).",
        tail_boundary.mode, tail_boundary.preserved_turns, tail_boundary.preserved_tokens_estimate
    ));
    if let Some(note) = tail_boundary.note.as_deref() {
        lines.push(format!("- Tail note: {note}"));
    }
    lines.push(format!(
        "- Summary source: {} using {} (model-backed: {}, deterministic fallback: {}).",
        summary_source.strategy,
        summary_source.model_ref,
        summary_source.model_backed,
        summary_source.deterministic_fallback
    ));
    lines.push("- This summary is deterministic and lossy; verify details against artifacts or the event log when precision matters.".to_string());
    lines.push(String::new());

    lines.push(headings[9].to_string());
    if let Some(split_prefix_summary) = tail_boundary.split_prefix_summary.as_deref() {
        lines.push(format!(
            "- Source facts: split prefix summary: {split_prefix_summary}"
        ));
    }
    if facts.compacted_turns.is_empty() {
        lines.push("- (no compacted turn facts recorded)".to_string());
    } else {
        for fact in facts.compacted_turns.iter().take(8) {
            let request = fact
                .request_id
                .as_deref()
                .map(|request_id| format!(" `{request_id}`"))
                .unwrap_or_default();
            lines.push(format!("- Request{request}: {}", fact.user_excerpt));
            lines.push(format!("  Assistant: {}", fact.assistant_excerpt));
        }
    }
    if !facts.touched_files.is_empty() {
        lines.push("<read-files>".to_string());
        lines.extend(facts.touched_files.iter().take(12).cloned());
        lines.push("</read-files>".to_string());
    }
    lines.push(String::new());

    lines.push(headings[10].to_string());
    let mut artifact_lines = Vec::new();
    let mut seen = BTreeSet::new();
    for artifact in pruned_tool_artifacts {
        if seen.insert((artifact.path.clone(), artifact.digest.clone())) {
            let digest = artifact
                .digest
                .as_deref()
                .map(|value| format!(" ({value})"))
                .unwrap_or_default();
            artifact_lines.push(format!(
                "- {}{}: referenced by compacted turn/tool output",
                artifact.path, digest
            ));
        }
    }
    for turn in older_turns {
        for artifact in &turn.artifacts {
            if seen.insert((artifact.path.clone(), artifact.digest.clone())) {
                let digest = artifact
                    .digest
                    .as_deref()
                    .map(|value| format!(" ({value})"))
                    .unwrap_or_default();
                artifact_lines.push(format!(
                    "- {}{}: referenced by compacted provider turn",
                    artifact.path, digest
                ));
            }
        }
    }
    if artifact_lines.is_empty() {
        lines.push("- (none recorded)".to_string());
    } else {
        lines.extend(artifact_lines.into_iter().take(12));
    }

    truncate_with_ellipsis(
        &lines.join("\n"),
        PROVIDER_CONTEXT_COMPACTION_SUMMARY_MAX_CHARS,
    )
}

fn build_harness_provider_context_summary(
    existing_summary: Option<&str>,
    older_turns: &[ProviderConversationTurn],
    pruned_tool_artifacts: &[EventArtifactRef],
    facts: &ProviderCompactionFacts,
    tail_boundary: &ProviderCompactionTailBoundary,
    summary_source: &ProviderCompactionSummarySource,
    config: &CompactionRuntimeConfig,
) -> String {
    let headings = provider_context_summary_required_headings(config);
    let mut lines = Vec::new();
    lines.push(headings[0].to_string());
    lines.push(format!(
        "- Continue the current agent session after compacting {} older turn(s).",
        older_turns.len()
    ));
    lines.push(String::new());

    lines.push(headings[1].to_string());
    if let Some(existing_summary) = existing_summary.and_then(non_empty_trimmed) {
        lines.push("- Preserve still-relevant constraints, decisions, files, and next steps from the previous checkpoint summary.".to_string());
        lines.push(format!(
            "- Prior checkpoint constraints/context carried forward: {}",
            summarize_compaction_text(existing_summary)
        ));
    } else {
        lines.push("- (none recorded explicitly)".to_string());
    }
    lines.push(String::new());

    lines.push(headings[2].to_string());
    for (index, turn) in older_turns.iter().enumerate() {
        lines.push(format!(
            "- Done turn {} user: {}",
            index + 1,
            summarize_compaction_text(&turn.user_prompt)
        ));
        lines.push(format!(
            "  Assistant: {}",
            summarize_compaction_text(&turn.assistant_response)
        ));
    }
    lines.push("- In progress: continue from the preserved recent turn(s) that follow this checkpoint summary.".to_string());
    lines.push("- Blocked: (none recorded explicitly)".to_string());
    lines.push(String::new());

    lines.push(headings[3].to_string());
    lines.push("- Older provider-visible turns were compacted into this checkpoint; preserved recent turns and the current user message take precedence over this lossy summary.".to_string());
    if let Some(split_prefix_summary) = tail_boundary.split_prefix_summary.as_deref() {
        lines.push(format!(
            "- Split prefix summary: {split_prefix_summary}; the provider-visible suffix follows this checkpoint as recent context."
        ));
    }
    if let Some(existing_summary) = existing_summary.and_then(non_empty_trimmed) {
        lines.push(format!(
            "- Prior checkpoint decisions/context were rolled into this structured summary: {}",
            summarize_compaction_text(existing_summary)
        ));
    }
    lines.push(String::new());

    lines.push(headings[4].to_string());
    lines.push("1. Use the preserved recent turn(s) plus this checkpoint summary to continue the user's current task.".to_string());
    lines.push(String::new());

    lines.push(headings[5].to_string());
    lines.push(format!("- Compacted turns: {}", older_turns.len()));
    if let Some(previous_checkpoint_id) = facts.previous_checkpoint_id.as_deref() {
        lines.push(format!(
            "- Previous checkpoint: {previous_checkpoint_id}; this summary rolls forward from it."
        ));
    }
    lines.push(format!(
        "- Tail boundary: {} ({} preserved turn(s), ~{} token(s)).",
        tail_boundary.mode, tail_boundary.preserved_turns, tail_boundary.preserved_tokens_estimate
    ));
    if let Some(note) = tail_boundary.note.as_deref() {
        lines.push(format!("- Tail note: {note}"));
    }
    lines.push(format!(
        "- Summary source: {} using {} (model-backed: {}, deterministic fallback: {}).",
        summary_source.strategy,
        summary_source.model_ref,
        summary_source.model_backed,
        summary_source.deterministic_fallback
    ));
    if facts.compacted_turns.is_empty() {
        lines.push("- Source facts: (no compacted turn facts recorded)".to_string());
    } else {
        for fact in facts.compacted_turns.iter().take(8) {
            let request = fact
                .request_id
                .as_deref()
                .map(|request_id| format!(" `{request_id}`"))
                .unwrap_or_default();
            lines.push(format!(
                "- Source fact request{request}: {}",
                fact.user_excerpt
            ));
            lines.push(format!("  Assistant: {}", fact.assistant_excerpt));
        }
    }
    if let Some(split_prefix_summary) = tail_boundary.split_prefix_summary.as_deref() {
        lines.push(format!(
            "- Source facts: split prefix summary: {split_prefix_summary}"
        ));
    }
    let mut artifact_lines = Vec::new();
    let mut seen = BTreeSet::new();
    for artifact in pruned_tool_artifacts {
        if seen.insert((artifact.path.clone(), artifact.digest.clone())) {
            let digest = artifact
                .digest
                .as_deref()
                .map(|value| format!(" ({value})"))
                .unwrap_or_default();
            artifact_lines.push(format!(
                "- Artifact {}{}: referenced by compacted turn/tool output",
                artifact.path, digest
            ));
        }
    }
    for turn in older_turns {
        for artifact in &turn.artifacts {
            if seen.insert((artifact.path.clone(), artifact.digest.clone())) {
                let digest = artifact
                    .digest
                    .as_deref()
                    .map(|value| format!(" ({value})"))
                    .unwrap_or_default();
                artifact_lines.push(format!(
                    "- Artifact {}{}: referenced by compacted provider turn",
                    artifact.path, digest
                ));
            }
        }
    }
    if artifact_lines.is_empty() {
        lines.push("- Relevant files/artifacts: (none recorded)".to_string());
    } else {
        lines.extend(artifact_lines.into_iter().take(12));
    }
    lines.push("- This summary is deterministic and lossy; verify details against artifacts or the event log when precision matters.".to_string());
    append_operational_memory_section(&mut lines, facts);

    truncate_with_ellipsis(
        &lines.join("\n"),
        PROVIDER_CONTEXT_COMPACTION_SUMMARY_MAX_CHARS,
    )
}

fn operational_memory_summary_block(facts: &ProviderCompactionFacts) -> String {
    if facts.read_files.is_empty()
        && facts.modified_files.is_empty()
        && facts.operation_facts.is_empty()
    {
        return "(none recorded)".to_string();
    }

    let mut lines = Vec::new();
    lines.push("Read files:".to_string());
    if facts.read_files.is_empty() {
        lines.push("- (none recorded)".to_string());
    } else {
        lines.extend(
            facts
                .read_files
                .iter()
                .take(12)
                .map(file_operation_fact_line),
        );
    }
    lines.push("Modified files:".to_string());
    if facts.modified_files.is_empty() {
        lines.push("- (none recorded)".to_string());
    } else {
        lines.extend(
            facts
                .modified_files
                .iter()
                .take(12)
                .map(file_operation_fact_line),
        );
    }
    lines.extend(
        facts
            .operation_facts
            .iter()
            .take(20)
            .map(|fact| format!("- {fact}")),
    );
    lines.join("\n")
}

fn append_operational_memory_section(lines: &mut Vec<String>, facts: &ProviderCompactionFacts) {
    if facts.read_files.is_empty()
        && facts.modified_files.is_empty()
        && facts.operation_facts.is_empty()
    {
        return;
    }
    lines.push(String::new());
    lines.push("## Operational Memory".to_string());
    lines.push("Read files:".to_string());
    if facts.read_files.is_empty() {
        lines.push("- (none recorded)".to_string());
    } else {
        lines.extend(
            facts
                .read_files
                .iter()
                .take(12)
                .map(file_operation_fact_line),
        );
    }
    lines.push("Modified files:".to_string());
    if facts.modified_files.is_empty() {
        lines.push("- (none recorded)".to_string());
    } else {
        lines.extend(
            facts
                .modified_files
                .iter()
                .take(12)
                .map(file_operation_fact_line),
        );
    }
    lines.extend(
        facts
            .operation_facts
            .iter()
            .take(20)
            .map(|fact| format!("- {fact}")),
    );
}

fn file_operation_fact_line(fact: &ProviderFileOperationFact) -> String {
    let seq = match (fact.first_seq, fact.last_seq) {
        (Some(first), Some(last)) if first == last => format!(" seq {first}"),
        (Some(first), Some(last)) => format!(" seq {first}-{last}"),
        (Some(first), None) => format!(" seq {first}"),
        (None, Some(last)) => format!(" seq {last}"),
        (None, None) => String::new(),
    };
    let sources = if fact.sources.is_empty() {
        String::new()
    } else {
        format!(" via {}", fact.sources.join(", "))
    };
    let summary = fact
        .summary
        .as_deref()
        .filter(|summary| !summary.is_empty())
        .map(|summary| format!(": {summary}"))
        .unwrap_or_default();
    format!("- {}{}{}{}", fact.path, seq, sources, summary)
}

fn summarize_compaction_text(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_with_ellipsis(
        &normalized,
        PROVIDER_CONTEXT_COMPACTION_TURN_EXCERPT_MAX_CHARS,
    )
}

fn approximate_turn_tokens(turn: &ProviderConversationTurn) -> u32 {
    if !turn.messages.is_empty() {
        return turn
            .messages
            .iter()
            .map(approximate_conversation_message_tokens)
            .sum();
    }

    approximate_text_tokens(&turn.user_prompt)
        .saturating_add(approximate_text_tokens(&turn.assistant_response))
}

fn approximate_conversation_message_tokens(message: &ConversationMessage) -> u32 {
    match message {
        ConversationMessage::Checkpoint(checkpoint) => approximate_text_tokens(&checkpoint.summary),
        ConversationMessage::User(user) => approximate_text_tokens(&user.text),
        ConversationMessage::Assistant(assistant) => assistant.tool_calls.iter().fold(
            approximate_text_tokens(&assistant.text),
            |tokens, tool_call| {
                tokens
                    .saturating_add(approximate_text_tokens(&tool_call.tool_call_id))
                    .saturating_add(approximate_text_tokens(&tool_call.tool_id))
                    .saturating_add(approximate_text_tokens(&tool_call.args_summary))
            },
        ),
        ConversationMessage::ToolResult(tool_result) => {
            approximate_text_tokens(&tool_result.tool_call_id)
                .saturating_add(
                    tool_result
                        .tool_id
                        .as_deref()
                        .map(approximate_text_tokens)
                        .unwrap_or(0),
                )
                .saturating_add(
                    tool_result
                        .output_summary
                        .as_deref()
                        .map(approximate_text_tokens)
                        .unwrap_or(0),
                )
                .saturating_add(
                    tool_result
                        .output_json
                        .as_ref()
                        .map(Value::to_string)
                        .as_deref()
                        .map(approximate_text_tokens)
                        .unwrap_or(0),
                )
        }
    }
}

fn approximate_text_tokens(text: &str) -> u32 {
    (text.chars().count() as u32 / 4).max(1)
}

fn approximate_provider_context_tokens(context: &ProviderContext) -> u32 {
    let summary_tokens = context
        .compacted_summary
        .as_deref()
        .map(approximate_text_tokens)
        .unwrap_or(0);
    summary_tokens.saturating_add(
        context
            .preserved_turns
            .iter()
            .map(approximate_turn_tokens)
            .sum::<u32>(),
    )
}

fn collect_pruned_tool_artifacts(
    run_state: &RunState,
    trigger: &ProviderCompactionTrigger,
    context: &ProviderContext,
    older_turns: &[ProviderConversationTurn],
) -> Vec<EventArtifactRef> {
    if older_turns.is_empty() {
        return Vec::new();
    }

    let lower_bound_seq = context
        .checkpoint
        .as_ref()
        .map(|checkpoint| checkpoint.through_seq)
        .unwrap_or(0);
    let historical_turns = match collect_historical_agent_turns_until(
        &run_state.info.run_id,
        &run_state.info.events_path,
        &trigger.agent_id,
        lower_bound_seq,
        run_state.next_event_seq.saturating_sub(1),
    ) {
        Ok(turns) => turns,
        Err(_) => return Vec::new(),
    };

    if historical_turns.len() < context.preserved_turns.len() {
        return Vec::new();
    }

    let aligned_turns = &historical_turns[historical_turns.len() - context.preserved_turns.len()..];
    if !aligned_turns
        .iter()
        .zip(&context.preserved_turns)
        .all(|(historical, current)| {
            historical.user_prompt == current.user_prompt
                && historical.assistant_response == current.assistant_response
        })
    {
        return Vec::new();
    }

    let mut refs = Vec::new();
    let mut seen = BTreeSet::new();
    for historical in aligned_turns.iter().take(older_turns.len()) {
        for artifact in &historical.artifact_refs {
            let key = (artifact.path.clone(), artifact.digest.clone());
            if seen.insert(key) {
                refs.push(artifact.clone());
            }
        }
    }
    refs
}

fn collect_historical_agent_turns_until(
    run_id: &str,
    events_path: &Path,
    agent_id: &str,
    lower_bound_seq: u64,
    through_seq: u64,
) -> Result<Vec<HistoricalCompletedAgentTurn>, CoordinatorError> {
    let file =
        fs::File::open(events_path).map_err(|source| CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!(
                "failed to open historical events {}: {source}",
                events_path.display()
            ),
        })?;

    let mut expected_seq = 1_u64;
    let mut requests: BTreeMap<String, HistoricalRequestState> = BTreeMap::new();
    let mut request_turn_task_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut historical_task_scopes: BTreeMap<String, TaskTerminalScope> = BTreeMap::new();
    let mut request_artifacts: BTreeMap<String, Vec<EventArtifactRef>> = BTreeMap::new();
    let mut turns = Vec::new();

    for (line_number, line) in BufReader::new(file).lines().enumerate() {
        let Some(event) = parse_historical_event_line(run_id, events_path, line_number, line)?
        else {
            continue;
        };
        validate_historical_event_seq(run_id, events_path, &event, expected_seq)?;
        expected_seq = expected_seq.saturating_add(1);

        if event.seq > through_seq {
            break;
        }
        if event.seq <= lower_bound_seq {
            continue;
        }

        match &event.payload {
            EventV1::UserMessageSubmitted(payload) => {
                let request = requests.entry(payload.request_id.clone()).or_default();
                request.first_seq.get_or_insert(event.seq);
                request.user_text = Some(payload.text.clone());
            }
            EventV1::ProviderRequestStarted(payload)
                if event.actor.agent_id.as_deref() == Some(agent_id) =>
            {
                let request = requests.entry(payload.request_id.clone()).or_default();
                request.first_seq.get_or_insert(event.seq);
                request.prompt_summary = Some(payload.prompt_summary.clone());
                request.agent_id = Some(agent_id.to_string());
            }
            EventV1::ProviderStreamDelta(payload)
                if event.actor.agent_id.as_deref() == Some(agent_id) =>
            {
                requests
                    .entry(payload.request_id.clone())
                    .or_default()
                    .assistant_output
                    .push_str(&payload.delta);
            }
            EventV1::TaskScheduled(payload)
                if event.actor.agent_id.as_deref() == Some(agent_id) =>
            {
                let Some(queue_key) = payload.queue_key.as_deref() else {
                    continue;
                };

                let scope = if queue_key.starts_with("provider_model:") {
                    Some(TaskTerminalScope::AgentTurn)
                } else if queue_key.starts_with("tool:") {
                    Some(TaskTerminalScope::ToolCall)
                } else {
                    None
                };

                if let Some(scope) = scope {
                    historical_task_scopes.insert(payload.task_id.clone(), scope);
                    if matches!(scope, TaskTerminalScope::AgentTurn) {
                        if let Some(request_id) = event.correlation_id.as_deref() {
                            request_turn_task_ids
                                .insert(request_id.to_string(), payload.task_id.clone());
                        }
                    }
                }
            }
            EventV1::ArtifactWritten(payload) => {
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };
                request_artifacts
                    .entry(request_id.to_string())
                    .or_default()
                    .push(EventArtifactRef {
                        path: payload.path.clone(),
                        digest: Some(payload.digest.clone()),
                    });
            }
            EventV1::TaskCompleted(payload)
                if event.actor.agent_id.as_deref() == Some(agent_id) =>
            {
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };

                if !historical_task_completion_marks_agent_turn(
                    request_id,
                    payload,
                    &historical_task_scopes,
                    &request_turn_task_ids,
                ) {
                    continue;
                }

                let request_state = requests.remove(request_id).ok_or_else(|| {
                    CoordinatorError::ResumeRestoreFailed {
                        run_id: run_id.to_string(),
                        reason: format!(
                            "missing provider request history for completed request `{request_id}`"
                        ),
                    }
                })?;

                let user_prompt = restore_historical_user_prompt(
                    run_id,
                    request_id,
                    request_state.user_text.clone(),
                    request_state.prompt_summary.clone(),
                )?;
                let assistant_response = if payload.result_summary.is_empty() {
                    request_state.assistant_output.clone()
                } else {
                    payload.result_summary.clone()
                };
                let mut artifact_refs = request_artifacts.remove(request_id).unwrap_or_default();
                artifact_refs.sort_by(|left, right| {
                    left.path
                        .cmp(&right.path)
                        .then_with(|| left.digest.cmp(&right.digest))
                });
                artifact_refs
                    .dedup_by(|left, right| left.path == right.path && left.digest == right.digest);
                turns.push(HistoricalCompletedAgentTurn {
                    request_id: request_id.to_string(),
                    user_prompt,
                    assistant_response,
                    artifact_refs,
                });
            }
            _ => {}
        }
    }

    Ok(turns)
}

fn append_compaction_artifact_written_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    checkpoint: &ProviderContextCheckpoint,
    artifact: &crate::tool::ArtifactRef,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let artifact_path = run_state.info.run_dir.join(&artifact.path);
    let bytes = fs::metadata(&artifact_path)
        .map(|meta| meta.len())
        .unwrap_or(0);
    let digest = artifact
        .digest
        .clone()
        .unwrap_or_else(|| digest12(artifact.path.as_bytes()));
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "artifact_kind".to_string(),
        "provider_context_checkpoint".to_string(),
    );
    metadata.insert(
        "checkpoint_id".to_string(),
        checkpoint.metadata.checkpoint_id.clone(),
    );
    metadata.insert("agent_id".to_string(), checkpoint.metadata.agent_id.clone());

    append_payload_event(
        clock,
        redactor,
        run_state,
        system_actor(),
        Some(format!("compaction:{}", checkpoint.metadata.agent_id)),
        EventV1::ArtifactWritten(ArtifactWrittenEvent {
            path: artifact.path.clone(),
            digest,
            bytes,
            tool_call_id: None,
            tool_metadata: None,
            metadata,
        }),
    )
}

fn append_compaction_failed_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    trigger: &ProviderCompactionTrigger,
    reason: &str,
    checkpoint_id: Option<String>,
    through_seq: Option<u64>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    append_payload_event(
        clock,
        redactor,
        run_state,
        system_actor(),
        Some(format!("compaction:{}", trigger.agent_id)),
        EventV1::CompactionFailed(CompactionFailedEvent {
            agent_id: trigger.agent_id.clone(),
            trigger_reason: trigger.trigger_reason.clone(),
            reason: reason.to_string(),
            checkpoint_id,
            through_seq,
            through_request_id: trigger.through_request_id.clone(),
        }),
    )
}

fn event_permission_decision(decision: PermissionDecision) -> EventPermissionDecision {
    match decision {
        PermissionDecision::Allow => EventPermissionDecision::Allow,
        PermissionDecision::Deny => EventPermissionDecision::Deny,
    }
}

fn permission_decision_label(decision: EventPermissionDecision) -> &'static str {
    match decision {
        EventPermissionDecision::Allow => "allow",
        EventPermissionDecision::Deny => "deny",
    }
}

fn permission_summary(redactor: &dyn Redactor, tool_id: &str, args_json: &Value) -> String {
    let redacted_args = crate::redact::redact_value(redactor, args_json);
    let args = serde_json::to_string(&redacted_args).unwrap_or_else(|_| "null".to_string());
    format!("tool={tool_id} args={args}")
}

fn permission_request_digest(tool_id: &str, args_json: &Value) -> String {
    let canonical = serde_json::to_vec(args_json).unwrap_or_else(|_| b"null".to_vec());
    let mut bytes = Vec::with_capacity(tool_id.len() + 1 + canonical.len());
    bytes.extend_from_slice(tool_id.as_bytes());
    bytes.push(0x1f);
    bytes.extend_from_slice(&canonical);
    digest12(&bytes)
}

fn permission_grant_request(
    workspace_root: &Path,
    kind: PermissionKind,
    tool_id: &str,
    args_json: &Value,
    request_digest: &str,
) -> PermissionGrantRequest {
    PermissionGrantRequest {
        kind,
        tool: permission_tool_selector(tool_id, args_json),
        matcher: permission_grant_matcher(workspace_root, kind, args_json, request_digest),
    }
}

fn permission_tool_selector(tool_id: &str, args_json: &Value) -> PermissionToolSelector {
    let effective_tool_id = effective_mcp_tool_id(tool_id, args_json).unwrap_or_else(|| {
        canonical_tool_id_for(tool_id)
            .unwrap_or(tool_id)
            .to_string()
    });
    let canonical_tool_id = canonical_tool_id_for(tool_id).map(str::to_string);

    PermissionToolSelector {
        effective_tool_id,
        canonical_tool_id,
    }
}

fn permission_grant_matcher(
    workspace_root: &Path,
    kind: PermissionKind,
    args_json: &Value,
    request_digest: &str,
) -> PermissionGrantMatcher {
    match kind {
        PermissionKind::Shell => shell_command_selector(args_json, request_digest)
            .unwrap_or_else(|| request_digest_selector(request_digest)),
        PermissionKind::EditFs => {
            let paths = workspace_path_selector_paths(workspace_root, args_json);
            if paths.len() == 1 {
                PermissionGrantMatcher::WorkspacePath {
                    path: paths.into_iter().next().expect("single path exists"),
                    request_digest: request_digest.to_string(),
                }
            } else {
                request_digest_selector(request_digest)
            }
        }
        _ => request_digest_selector(request_digest),
    }
}

fn evaluate_permission_rule_requests(
    policy: &PermissionPolicy,
    category: Option<&str>,
    kind: PermissionKind,
    selectors: &[PermissionRuleRequest],
) -> PolicyDecision {
    if selectors.is_empty() {
        return policy.evaluate_request(category, kind, None);
    }

    let mut ask_decision = None;
    for selector in selectors {
        match policy.evaluate_request(category, kind, Some(selector)) {
            PolicyDecision::Deny => return PolicyDecision::Deny,
            PolicyDecision::Ask {
                timeout_ms,
                default_decision,
            } => {
                ask_decision = Some(PolicyDecision::Ask {
                    timeout_ms,
                    default_decision,
                });
            }
            PolicyDecision::Allow => {}
        }
    }

    ask_decision.unwrap_or(PolicyDecision::Allow)
}

fn permission_rule_request_selectors(
    workspace_root: &Path,
    kind: PermissionKind,
    args_json: &Value,
) -> Vec<PermissionRuleRequest> {
    match kind {
        PermissionKind::Shell => shell_command_rule_selector(args_json).into_iter().collect(),
        PermissionKind::EditFs => workspace_path_rule_selectors(workspace_root, args_json),
        PermissionKind::Task => task_agent_rule_selectors(args_json),
        PermissionKind::Network
        | PermissionKind::Question
        | PermissionKind::WebFetch
        | PermissionKind::WebSearch
        | PermissionKind::CodeSearch
        | PermissionKind::Lsp => Vec::new(),
    }
}

fn plan_mode_edit_boundary_denial(
    category: Option<&str>,
    kind: Option<PermissionKind>,
    run_id: &str,
    workspace_root: &Path,
    args_json: &Value,
) -> Option<String> {
    if category != Some(crate::plan::PLAN_AGENT_NAME) || kind != Some(PermissionKind::EditFs) {
        return None;
    }

    let active_plan = crate::plan::plan_file_relative_path(run_id)
        .to_string_lossy()
        .to_string();
    let paths = workspace_path_selector_paths(workspace_root, args_json);
    if !paths.is_empty() && paths.iter().all(|path| path == &active_plan) {
        return active_plan_symlink_denial(workspace_root, &active_plan);
    }

    let requested = if paths.is_empty() {
        "<unresolved path>".to_string()
    } else {
        paths.join(", ")
    };
    Some(format!(
        "plan mode may edit only the active plan file `{active_plan}`; requested `{requested}`"
    ))
}

fn plan_mode_shell_boundary_denial(
    category: Option<&str>,
    kind: Option<PermissionKind>,
    args_json: &Value,
) -> Option<String> {
    if category != Some(crate::plan::PLAN_AGENT_NAME) || kind != Some(PermissionKind::Shell) {
        return None;
    }

    let command = args_json
        .get("command")
        .or_else(|| args_json.get("cmd"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|command| !command.is_empty());
    let Some(command) = command else {
        return Some("plan mode bash requires a read-only inspection command".to_string());
    };

    if is_plan_mode_read_only_shell_command(command) {
        None
    } else {
        Some(format!(
            "plan mode bash may only run read-only inspection commands; requested `{command}`"
        ))
    }
}

fn is_plan_mode_read_only_shell_command(command: &str) -> bool {
    let trimmed = command.trim();
    if trimmed.is_empty()
        || contains_shell_control_operator(trimmed)
        || contains_shell_quote_or_escape(trimmed)
    {
        return false;
    }

    let tokens = trimmed.split_whitespace().collect::<Vec<_>>();
    match tokens.as_slice() {
        ["pwd"] => true,
        ["ls", ..] => true,
        ["git", subcommand, args @ ..] => is_plan_mode_read_only_git_command(subcommand, args),
        _ => false,
    }
}

fn is_plan_mode_read_only_git_command(subcommand: &str, args: &[&str]) -> bool {
    match subcommand {
        "status" | "diff" | "log" | "show" | "rev-parse" | "merge-base" => {
            !contains_git_write_output_arg(args) && !contains_git_exec_capable_arg(args)
        }
        "branch" => is_plan_mode_read_only_git_branch(args),
        _ => false,
    }
}

fn contains_git_write_output_arg(args: &[&str]) -> bool {
    args.iter()
        .any(|arg| *arg == "-o" || *arg == "--output" || arg.starts_with("--output="))
}

fn contains_git_exec_capable_arg(args: &[&str]) -> bool {
    args.iter().any(|arg| {
        matches!(*arg, "--ext-diff" | "--textconv")
            || arg.starts_with("--ext-diff=")
            || arg.starts_with("--textconv=")
    })
}

fn is_plan_mode_read_only_git_branch(args: &[&str]) -> bool {
    const MUTATING_FLAGS: &[&str] = &[
        "-d",
        "-D",
        "-m",
        "-M",
        "-c",
        "-C",
        "--copy",
        "--create-reflog",
        "--delete",
        "--edit-description",
        "--move",
        "--no-track",
        "--set-upstream-to",
        "--track",
        "--unset-upstream",
    ];

    !args.iter().any(|arg| {
        MUTATING_FLAGS.contains(arg)
            || arg.starts_with("--set-upstream-to=")
            || !arg.starts_with('-')
    })
}

fn contains_shell_control_operator(command: &str) -> bool {
    command
        .chars()
        .any(|ch| matches!(ch, '>' | '<' | '|' | '&' | ';' | '`'))
        || command.contains("$(")
}

fn contains_shell_quote_or_escape(command: &str) -> bool {
    command.chars().any(|ch| matches!(ch, '\'' | '"' | '\\'))
}

fn active_plan_symlink_denial(workspace_root: &Path, active_plan: &str) -> Option<String> {
    let mut current = workspace_root.to_path_buf();
    for component in Path::new(active_plan).components() {
        match component {
            std::path::Component::Normal(segment) => current.push(segment),
            std::path::Component::CurDir => continue,
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return Some(format!(
                    "plan mode active plan path `{active_plan}` contains an invalid component"
                ));
            }
        }

        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Some(format!(
                    "plan mode active plan path `{active_plan}` must not contain symlink component `{}`",
                    current.display()
                ));
            }
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
            Err(err) => {
                return Some(format!(
                    "plan mode could not verify active plan path `{active_plan}`: {err}"
                ));
            }
        }
    }
    None
}

fn task_agent_rule_selectors(args_json: &Value) -> Vec<PermissionRuleRequest> {
    let mut team_selectors = Vec::new();
    if let Some(members) = args_json.get("members").and_then(Value::as_array) {
        for member in members {
            team_selectors.extend(task_agent_rule_selectors(member));
        }
    }
    if let Some(lead) = args_json.get("lead") {
        team_selectors.extend(task_agent_rule_selectors(lead));
    }
    if !team_selectors.is_empty() {
        team_selectors.sort_by(|left, right| {
            permission_rule_request_key(left).cmp(permission_rule_request_key(right))
        });
        team_selectors.dedup();
        return team_selectors;
    }

    let category = trimmed_arg(args_json, "category");
    let subagent_type = ["subagent_type", "agent", "profile", "profileName"]
        .into_iter()
        .find_map(|key| trimmed_arg(args_json, key));

    match (category, subagent_type) {
        (Some(category), Some(subagent_type)) if category == subagent_type => {
            vec![PermissionRuleRequest::TaskAgent(category)]
        }
        (Some(_), Some(subagent_type)) | (None, Some(subagent_type)) => {
            vec![PermissionRuleRequest::TaskAgent(subagent_type)]
        }
        (Some(category), None) => vec![PermissionRuleRequest::TaskAgent(category.clone())],
        (None, None) => Vec::new(),
    }
}

fn permission_rule_request_key(selector: &PermissionRuleRequest) -> &str {
    match selector {
        PermissionRuleRequest::ShellCommand(value)
        | PermissionRuleRequest::WorkspacePath(value)
        | PermissionRuleRequest::TaskAgent(value) => value,
    }
}

fn trimmed_arg(args_json: &Value, key: &str) -> Option<String> {
    args_json
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn shell_command_rule_selector(args_json: &Value) -> Option<PermissionRuleRequest> {
    args_json
        .get("command")
        .or_else(|| args_json.get("cmd"))
        .and_then(Value::as_str)
        .map(|command| PermissionRuleRequest::ShellCommand(command.to_string()))
}

fn workspace_path_rule_selectors(
    workspace_root: &Path,
    args_json: &Value,
) -> Vec<PermissionRuleRequest> {
    workspace_path_selector_paths(workspace_root, args_json)
        .into_iter()
        .map(PermissionRuleRequest::WorkspacePath)
        .collect()
}

fn request_digest_selector(request_digest: &str) -> PermissionGrantMatcher {
    PermissionGrantMatcher::RequestDigest {
        request_digest: request_digest.to_string(),
    }
}

fn shell_command_selector(
    args_json: &Value,
    request_digest: &str,
) -> Option<PermissionGrantMatcher> {
    let command = args_json
        .get("command")
        .or_else(|| args_json.get("cmd"))
        .and_then(Value::as_str)?;
    let mut command_identity = Vec::new();
    command_identity.extend_from_slice(command.as_bytes());
    if let Some(args) = args_json.get("args").and_then(Value::as_array) {
        command_identity.push(0x1f);
        command_identity.extend_from_slice(&serde_json::to_vec(args).ok()?);
    }
    Some(PermissionGrantMatcher::ShellCommand {
        command_digest: digest12(&command_identity),
        request_digest: request_digest.to_string(),
    })
}

fn workspace_path_selector_paths(workspace_root: &Path, args_json: &Value) -> Vec<String> {
    let mut paths = BTreeSet::new();
    for key in [
        "path",
        "filePath",
        "from_path",
        "fromPath",
        "rename",
        "to_path",
        "toPath",
    ] {
        if let Some(raw_path) = args_json.get(key).and_then(Value::as_str) {
            if let Some(path) =
                workspace_relative_path_from_maybe_absolute(workspace_root, Path::new(raw_path))
            {
                paths.insert(path);
            }
        }
    }
    paths.into_iter().collect()
}

fn tool_request_correlation_id(run_state: &RunState, actor: &EventActor) -> Option<String> {
    if actor.kind != ActorKind::Worker {
        return None;
    }

    let agent_id = actor.agent_id.as_deref()?;
    run_state
        .running_agent_turns
        .values()
        .find(|turn| turn.agent_id == agent_id)
        .map(|turn| turn.request_id.clone())
}

fn allocate_provider_request_id(run_state: &mut RunState) -> String {
    let request_id = format!("req_{:06}", run_state.next_provider_request_id);
    run_state.next_provider_request_id += 1;
    request_id
}

fn hashline_edit_metadata(
    tool_id: &str,
    args_json: &Value,
    tool_call_id: &str,
) -> Option<HashlineEditMetadata> {
    if tool_id != HASHLINE_APPLY_TOOL_ID {
        let canonical_tool_id = canonical_tool_id_for(tool_id)?;
        if canonical_tool_id != "edit" {
            return None;
        }

        let path = args_json
            .get("path")
            .or_else(|| args_json.get("filePath"))
            .and_then(Value::as_str)?;
        let (edit_id, summary) = (
            edit_id_from_native_edit_args(args_json, tool_call_id),
            "rewrite file through native edit tool".to_string(),
        );

        return Some(HashlineEditMetadata {
            edit_id,
            path: path.to_string(),
            summary,
            patch_digest: digest12_json(args_json),
        });
    }

    let patch: HashlinePatch = serde_json::from_value(args_json.clone()).ok()?;
    let patch_digest = digest12_json(&patch);

    Some(HashlineEditMetadata {
        edit_id: patch.edit_id,
        path: patch.path,
        summary: format!("apply hashline patch with {} op(s)", patch.ops.len()),
        patch_digest,
    })
}

fn edit_id_from_native_edit_args(args_json: &Value, tool_call_id: &str) -> String {
    args_json
        .get("editId")
        .or_else(|| args_json.get("edit_id"))
        .and_then(Value::as_str)
        .and_then(non_empty_trimmed)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("edit-{tool_call_id}"))
}

fn hashline_diff_refs(result: &ToolResult) -> (Option<String>, Option<String>) {
    let structured = result.structured_json.as_ref().and_then(Value::as_object);
    let structured_path = structured
        .and_then(|value| value.get("diff_rel_path"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let structured_digest = structured
        .and_then(|value| value.get("diff_digest"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    if structured_path.is_some() && structured_digest.is_some() {
        return (structured_path, structured_digest);
    }

    let artifact = result
        .artifacts
        .iter()
        .find(|artifact| artifact.path.ends_with(".diff"));
    let artifact_path = artifact.map(|artifact| artifact.path.clone());
    let artifact_digest = artifact.and_then(|artifact| artifact.digest.clone());

    (
        structured_path.or(artifact_path),
        structured_digest.or(artifact_digest),
    )
}

fn applied_tool_edit_metadata(
    _tool_id: &str,
    result: &ToolResult,
    fallback: Option<&HashlineEditMetadata>,
) -> Vec<AppliedToolEditMetadata> {
    let Some(metadata) = fallback else {
        return Vec::new();
    };
    let structured = result.structured_json.as_ref().and_then(Value::as_object);
    let mut metadata = metadata.clone();
    if let Some(edit_id) = structured
        .and_then(|value| value.get("edit_id"))
        .and_then(Value::as_str)
        .and_then(non_empty_trimmed)
    {
        metadata.edit_id = edit_id.to_string();
    }
    if let Some(path) = structured
        .and_then(|value| value.get("path"))
        .and_then(Value::as_str)
        .and_then(non_empty_trimmed)
    {
        metadata.path = path.to_string();
    }
    let deleted = structured
        .and_then(|value| value.get("resolved_to_path"))
        .is_none()
        && structured
            .and_then(|value| value.get("resolved_path"))
            .and_then(Value::as_str)
            .is_some_and(|path| !Path::new(path).exists());
    let (diff_rel_path, diff_digest) = hashline_diff_refs(result);
    vec![AppliedToolEditMetadata {
        metadata,
        diff_rel_path,
        diff_digest,
        deleted,
    }]
}

fn requested_tool_call_metadata(tool_id: &str, args_json: &Value) -> Option<ToolCallMetadata> {
    let tool_identity = tool_identity_metadata(tool_id, args_json);
    tool_call_metadata(tool_identity.as_ref(), None, Vec::new(), None, Vec::new())
}

fn tool_identity_metadata(tool_id: &str, args_json: &Value) -> Option<ToolIdentityMetadata> {
    if let Some(canonical_tool_id) = effective_mcp_tool_id(tool_id, args_json) {
        return Some(ToolIdentityMetadata {
            canonical_tool_id: Some(canonical_tool_id),
            alias_source_tool_id: None,
        });
    }

    Some(ToolIdentityMetadata {
        canonical_tool_id: Some(tool_id.to_string()),
        alias_source_tool_id: None,
    })
}

fn effective_mcp_tool_id(tool_id: &str, args_json: &Value) -> Option<String> {
    let mut segments = tool_id.split('.');
    let Some("mcp") = segments.next() else {
        return None;
    };
    let server_id = segments.next()?.trim();
    if server_id.is_empty() {
        return None;
    }

    let suffix = segments.collect::<Vec<_>>().join(".");
    if suffix == "tool.call" {
        let remote_tool_name = args_json
            .get("tool")
            .and_then(Value::as_str)
            .and_then(non_empty_trimmed)?;
        if let Some(tool_id) =
            registered_mcp_server_first_class_tool_id(server_id, remote_tool_name)
        {
            return Some(tool_id);
        }
        return Some(format!(
            "mcp.{server_id}.{}",
            sanitize_mcp_tool_segment(remote_tool_name)
        ));
    }

    Some(tool_id.to_string())
}

fn tool_call_metadata(
    tool_identity: Option<&ToolIdentityMetadata>,
    lineage: Option<TaskLineageMetadata>,
    artifact_refs: Vec<EventArtifactRef>,
    timing: Option<ExecutionTimingMetadata>,
    hook_executions: Vec<HookExecutionMetadata>,
) -> Option<ToolCallMetadata> {
    let canonical_tool_id = tool_identity.and_then(|value| value.canonical_tool_id.clone());
    let alias_source_tool_id = tool_identity.and_then(|value| value.alias_source_tool_id.clone());

    if canonical_tool_id.is_none()
        && alias_source_tool_id.is_none()
        && lineage.is_none()
        && artifact_refs.is_empty()
        && timing.is_none()
        && hook_executions.is_empty()
    {
        return None;
    }

    Some(ToolCallMetadata {
        canonical_tool_id,
        alias_source_tool_id,
        lineage,
        artifact_refs,
        timing,
        hook_executions,
    })
}

fn tool_task_lineage_metadata(
    task: &TaskState,
    output_json: Option<&Value>,
) -> TaskLineageMetadata {
    TaskLineageMetadata {
        parent_tool_call_id: Some(task.tool_call_id.clone()),
        parent_task_id: None,
        parent_request_id: task.request_correlation_id.clone(),
        parent_session_id: extract_lineage_value(output_json, &["parent_session_id"]),
        child_session_id: extract_lineage_value(
            output_json,
            &["child_session_id", "session_id", "task_id"],
        ),
        child_request_id: extract_lineage_value(output_json, &["child_request_id", "request_id"]),
        child_provider_id: extract_lineage_value(
            output_json,
            &["child_provider_id", "provider_id", "provider"],
        ),
        child_model_id: extract_lineage_value(
            output_json,
            &["child_model_id", "model_id", "model"],
        ),
    }
}

fn extract_lineage_value(output_json: Option<&Value>, candidate_keys: &[&str]) -> Option<String> {
    let root = output_json?.as_object()?;
    for key in candidate_keys {
        if let Some(value) = root
            .get(*key)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
        {
            return Some(value);
        }
    }

    let nested = root.get("lineage").and_then(Value::as_object)?;
    for key in candidate_keys {
        if let Some(value) = nested
            .get(*key)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
        {
            return Some(value);
        }
    }

    None
}

fn event_artifact_refs(artifacts: &[crate::tool::ArtifactRef]) -> Vec<EventArtifactRef> {
    let mut refs = artifacts
        .iter()
        .map(|artifact| EventArtifactRef {
            path: artifact.path.clone(),
            digest: artifact.digest.clone(),
        })
        .collect::<Vec<_>>();
    refs.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.digest.cmp(&right.digest))
    });
    refs
}

fn execution_timing_metadata(
    started_mono_ms: u64,
    finished_mono_ms: u64,
) -> ExecutionTimingMetadata {
    ExecutionTimingMetadata {
        started_mono_ms: Some(started_mono_ms),
        finished_mono_ms: Some(finished_mono_ms),
        elapsed_ms: Some(finished_mono_ms.saturating_sub(started_mono_ms)),
    }
}

fn stable_tool_output_json(
    structured_output: Option<Value>,
    output_summary: &str,
    artifact_refs: &[EventArtifactRef],
    lineage: &TaskLineageMetadata,
    timing: &ExecutionTimingMetadata,
    hook_executions: &[HookExecutionMetadata],
) -> Value {
    let harness_metadata = json!({
        "output_summary": output_summary,
        "artifact_refs": artifact_refs,
        "lineage": lineage,
        "timing": timing,
        "hook_executions": hook_executions,
    });

    match structured_output {
        Some(Value::Object(mut value)) => {
            value.insert("_harness".to_string(), harness_metadata);
            Value::Object(value)
        }
        Some(value) => json!({
            "_harness": harness_metadata,
            "structured_output": value,
        }),
        None => json!({
            "_harness": harness_metadata,
        }),
    }
}

fn extract_hook_execution_metadata(output_json: Option<&Value>) -> Vec<HookExecutionMetadata> {
    let Some(output_json) = output_json else {
        return Vec::new();
    };

    let mut hook_executions = Vec::new();
    for source in [
        output_json.get("hook_executions"),
        output_json.get("hooks"),
        output_json
            .get("_harness")
            .and_then(|harness| harness.get("hook_executions")),
    ] {
        let Some(items) = source.and_then(Value::as_array) else {
            continue;
        };

        for item in items {
            let Some(parsed) = parse_hook_execution_metadata(item) else {
                continue;
            };
            if hook_executions.iter().any(|existing| existing == &parsed) {
                continue;
            }
            hook_executions.push(parsed);
        }
    }

    hook_executions
}

fn parse_hook_execution_metadata(value: &Value) -> Option<HookExecutionMetadata> {
    let object = value.as_object()?;
    let hook_name = extract_object_string(object, &["hook_name", "name", "hook", "id", "hook_id"])
        .or_else(|| {
            object
                .get("hook")
                .and_then(Value::as_object)
                .and_then(|hook| extract_object_string(hook, &["name", "id"]))
        })
        .unwrap_or_else(|| "unknown_hook".to_string());

    let status = extract_object_string(object, &["status", "result", "outcome"])
        .map(|status| parse_hook_execution_status(&status))
        .unwrap_or_default();

    Some(HookExecutionMetadata {
        hook_name,
        status,
        hook_event: extract_object_string(object, &["hook_event", "event", "phase", "trigger"]),
        hook_phase: extract_object_string(
            object,
            &["hook_phase", "phase_name", "middleware_phase"],
        ),
        command_digest: extract_object_string(
            object,
            &["command_digest", "command_hash", "command_blake3"],
        ),
        output_digest: extract_object_string(object, &["output_digest", "result_digest", "digest"]),
        output_summary: extract_object_string(
            object,
            &["output_summary", "summary", "message", "output_message"],
        ),
        duration_ms: extract_object_u64(object, &["duration_ms", "elapsed_ms"]),
        effects: object
            .get("effects")
            .or_else(|| object.get("hook_effects"))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(parse_hook_effect_metadata)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
    })
}

fn extract_object_string(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = object
            .get(*key)
            .and_then(Value::as_str)
            .and_then(non_empty_trimmed)
            .map(ToOwned::to_owned)
        {
            return Some(value);
        }
    }

    None
}

fn extract_object_u64(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<u64> {
    for key in keys {
        if let Some(value) = object.get(*key).and_then(Value::as_u64) {
            return Some(value);
        }
    }

    None
}

fn parse_hook_execution_status(status: &str) -> HookExecutionStatus {
    match status.trim().to_ascii_lowercase().as_str() {
        "succeeded" | "success" | "ok" | "passed" => HookExecutionStatus::Succeeded,
        "failed" | "error" => HookExecutionStatus::Failed,
        "skipped" | "ignored" => HookExecutionStatus::Skipped,
        _ => HookExecutionStatus::Unknown,
    }
}

fn failed_tool_output_json(reason: &str, hook_executions: &[HookExecutionMetadata]) -> Value {
    json!({
        "_harness": {
            "status": "failed",
            "error": reason,
            "hook_executions": hook_executions,
        }
    })
}

fn workspace_file_digest(workspace_root: &Path, relative_path: &str) -> Result<String, String> {
    let canonical_workspace = workspace_root
        .canonicalize()
        .map_err(|err| format!("failed to resolve workspace root: {err}"))?;
    let input = Path::new(relative_path);
    let relative = if input.is_absolute() {
        input
            .strip_prefix(&canonical_workspace)
            .map_err(|_| "path must be relative to workspace root".to_string())?
    } else {
        input
    };
    let candidate = canonical_workspace.join(relative);
    let canonical_candidate = candidate
        .canonicalize()
        .map_err(|err| format!("failed to resolve target file: {err}"))?;

    if !canonical_candidate.starts_with(&canonical_workspace) {
        return Err(format!(
            "path {} escapes workspace root {}",
            canonical_candidate.display(),
            canonical_workspace.display()
        ));
    }

    let bytes = fs::read(&canonical_candidate)
        .map_err(|err| format!("failed to read target file for digest: {err}"))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

#[derive(Default)]
struct HistoricalRequestState {
    user_text: Option<String>,
    prompt_summary: Option<String>,
    assistant_output: String,
    messages: Vec<ConversationMessage>,
    active_assistant_message_index: Option<usize>,
    tool_ids_by_call_id: BTreeMap<String, String>,
    agent_id: Option<String>,
    provider_request_id: Option<String>,
    provider_finish_reason: Option<String>,
    first_seq: Option<u64>,
}

#[derive(Debug, Clone)]
struct AppliedCheckpointRecord {
    checkpoint_id: String,
    artifact_path: String,
    through_seq: u64,
    through_request_id: Option<String>,
}

#[derive(Debug, Clone)]
struct HistoricalCompletedAgentTurn {
    request_id: String,
    user_prompt: String,
    assistant_response: String,
    artifact_refs: Vec<EventArtifactRef>,
}

fn historical_conversation_messages_for_completed_turn(
    user_prompt: &str,
    assistant_response: &str,
    request_state: &HistoricalRequestState,
) -> Vec<ConversationMessage> {
    if request_state.messages.is_empty() {
        return Vec::new();
    }

    let request_id = request_state
        .messages
        .iter()
        .find_map(|message| match message {
            ConversationMessage::Assistant(assistant) => Some(assistant.request_id.clone()),
            ConversationMessage::ToolResult(tool_result) => Some(tool_result.request_id.clone()),
            ConversationMessage::User(user) => Some(user.request_id.clone()),
            ConversationMessage::Checkpoint(_) => None,
        })
        .unwrap_or_default();
    let agent_id = request_state.agent_id.clone();

    let mut messages = Vec::with_capacity(request_state.messages.len() + 1);
    messages.push(ConversationMessage::User(ConversationUserMessage {
        request_id,
        text: user_prompt.to_string(),
        seq: request_state.first_seq,
        agent_id,
    }));
    messages.extend(request_state.messages.clone());
    if let Some(ConversationMessage::Assistant(assistant)) = messages.last_mut() {
        if assistant.tool_calls.is_empty() && assistant.text != assistant_response {
            assistant.text = assistant_response.to_string();
        }
    }
    messages
}

fn restore_historical_user_prompt(
    run_id: &str,
    request_id: &str,
    user_text: Option<String>,
    prompt_summary: Option<String>,
) -> Result<String, CoordinatorError> {
    if let Some(user_text) = user_text {
        return Ok(user_text);
    }

    let Some(prompt_summary) = prompt_summary.as_deref().and_then(non_empty_trimmed) else {
        return Err(CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!("missing user message for completed request `{request_id}`"),
        });
    };

    if prompt_summary.ends_with('…') {
        return Err(CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!(
                "missing user message for completed request `{request_id}` and prompt_summary is truncated"
            ),
        });
    }

    Ok(prompt_summary.to_string())
}

fn restore_provider_context_from_history(
    session_dir: &Path,
    run_id: &str,
) -> Result<BTreeMap<String, ProviderContext>, CoordinatorError> {
    let run_dir = session_dir.join(run_id);
    let events_path = run_dir.join(EVENTS_FILE_NAME);
    let historical_events = read_historical_events_until(run_id, &events_path, u64::MAX)?;

    let applied_checkpoints = discover_applied_checkpoints(run_id, &run_dir, &historical_events)?;
    let checkpoint_boundaries = applied_checkpoints
        .iter()
        .map(|(agent_id, checkpoint)| (agent_id.clone(), checkpoint.through_seq))
        .collect::<BTreeMap<_, _>>();

    let mut histories = BTreeMap::new();
    for (agent_id, checkpoint) in &applied_checkpoints {
        let checkpoint_artifact = load_provider_context_checkpoint(run_id, &run_dir, checkpoint)?;
        if checkpoint_artifact.metadata.run_id != run_id {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: format!(
                    "checkpoint `{}` run mismatch: expected `{run_id}`, got `{}`",
                    checkpoint.checkpoint_id, checkpoint_artifact.metadata.run_id
                ),
            });
        }
        if checkpoint_artifact.metadata.checkpoint_id != checkpoint.checkpoint_id {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: format!(
                    "checkpoint artifact id mismatch for agent `{agent_id}`: expected `{}`, got `{}`",
                    checkpoint.checkpoint_id, checkpoint_artifact.metadata.checkpoint_id
                ),
            });
        }
        if checkpoint_artifact.metadata.agent_id != *agent_id {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: format!(
                    "checkpoint `{}` agent mismatch: expected `{agent_id}`, got `{}`",
                    checkpoint.checkpoint_id, checkpoint_artifact.metadata.agent_id
                ),
            });
        }
        if checkpoint_artifact.metadata.through_seq != checkpoint.through_seq {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: format!(
                    "checkpoint `{}` through_seq mismatch: expected `{}`, got `{}`",
                    checkpoint.checkpoint_id,
                    checkpoint.through_seq,
                    checkpoint_artifact.metadata.through_seq
                ),
            });
        }
        if checkpoint_artifact.metadata.through_request_id != checkpoint.through_request_id {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: format!(
                    "checkpoint `{}` through_request_id mismatch: expected `{:?}`, got `{:?}`",
                    checkpoint.checkpoint_id,
                    checkpoint.through_request_id,
                    checkpoint_artifact.metadata.through_request_id
                ),
            });
        }
        histories.insert(
            agent_id.clone(),
            ProviderContext::from_checkpoint(checkpoint_artifact),
        );
    }

    let mut requests: BTreeMap<String, HistoricalRequestState> = BTreeMap::new();
    let mut request_turn_task_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut historical_task_scopes: BTreeMap<String, TaskTerminalScope> = BTreeMap::new();
    let mut request_artifacts: BTreeMap<String, Vec<EventArtifactRef>> = BTreeMap::new();
    let mut agent_turn_agent_by_task: BTreeMap<String, String> = BTreeMap::new();

    for event in &historical_events {
        let replay_agent_event = should_replay_agent_scoped_event(
            event.seq,
            event.actor.agent_id.as_deref(),
            &checkpoint_boundaries,
        );

        match &event.payload {
            EventV1::UserMessageSubmitted(payload) => {
                let request = requests.entry(payload.request_id.clone()).or_default();
                request.first_seq.get_or_insert(event.seq);
                request.user_text = Some(payload.text.clone());
            }
            EventV1::ProviderRequestStarted(payload) => {
                if !replay_agent_event {
                    continue;
                }
                let request_id = event
                    .correlation_id
                    .as_deref()
                    .and_then(non_empty_trimmed)
                    .unwrap_or(&payload.request_id);
                let request = requests.entry(request_id.to_string()).or_default();
                request.first_seq.get_or_insert(event.seq);
                request.prompt_summary = Some(payload.prompt_summary.clone());
                request.provider_request_id = Some(payload.request_id.clone());
                request.messages.push(ConversationMessage::Assistant(
                    ConversationAssistantMessage {
                        request_id: request_id.to_string(),
                        agent_id: event.actor.agent_id.clone(),
                        text: String::new(),
                        tool_calls: Vec::new(),
                        stop_reason: None,
                        first_seq: Some(event.seq),
                        last_seq: Some(event.seq),
                        provider_id: Some(payload.provider_id.clone()),
                        model_id: Some(payload.model_id.clone()),
                        output_digest: None,
                    },
                ));
                request.active_assistant_message_index =
                    Some(request.messages.len().saturating_sub(1));
                if let Some(agent_id) = event.actor.agent_id.as_deref().and_then(non_empty_trimmed)
                {
                    request.agent_id = Some(agent_id.to_string());
                }
            }
            EventV1::ProviderStreamDelta(payload) => {
                if !replay_agent_event {
                    continue;
                }
                let request_id = event
                    .correlation_id
                    .as_deref()
                    .and_then(non_empty_trimmed)
                    .unwrap_or(&payload.request_id);
                let request = requests.entry(request_id.to_string()).or_default();
                request.assistant_output.push_str(&payload.delta);
                if let Some(index) = request.active_assistant_message_index {
                    if let Some(ConversationMessage::Assistant(assistant)) =
                        request.messages.get_mut(index)
                    {
                        assistant.text.push_str(&payload.delta);
                        assistant.last_seq = Some(event.seq);
                    }
                }
            }
            EventV1::ProviderRequestFinished(payload) => {
                if !replay_agent_event {
                    continue;
                }
                let request_id = event
                    .correlation_id
                    .as_deref()
                    .and_then(non_empty_trimmed)
                    .unwrap_or(&payload.request_id);
                let request = requests.entry(request_id.to_string()).or_default();
                request.first_seq.get_or_insert(event.seq);
                request.provider_request_id = Some(payload.request_id.clone());
                request.provider_finish_reason = Some(payload.finish_reason.clone());
                if let Some(index) = request.active_assistant_message_index {
                    if let Some(ConversationMessage::Assistant(assistant)) =
                        request.messages.get_mut(index)
                    {
                        assistant.stop_reason = Some(payload.finish_reason.clone());
                        assistant.output_digest = payload.output_digest.clone();
                        assistant.last_seq = Some(event.seq);
                    }
                }
            }
            EventV1::TaskScheduled(payload) => {
                if !replay_agent_event {
                    continue;
                }
                let Some(queue_key) = payload.queue_key.as_deref() else {
                    continue;
                };

                let scope = if queue_key.starts_with("provider_model:") {
                    Some(TaskTerminalScope::AgentTurn)
                } else if queue_key.starts_with("tool:") {
                    Some(TaskTerminalScope::ToolCall)
                } else {
                    None
                };

                if let Some(scope) = scope {
                    historical_task_scopes.insert(payload.task_id.clone(), scope);
                    if matches!(scope, TaskTerminalScope::AgentTurn) {
                        if let Some(request_id) = event.correlation_id.as_deref() {
                            requests
                                .entry(request_id.to_string())
                                .or_default()
                                .first_seq
                                .get_or_insert(event.seq);
                            request_turn_task_ids
                                .insert(request_id.to_string(), payload.task_id.clone());
                            if let Some(agent_id) = event.actor.agent_id.as_deref() {
                                agent_turn_agent_by_task
                                    .insert(payload.task_id.clone(), agent_id.to_string());
                            }
                        }
                    }
                }
            }
            EventV1::ArtifactWritten(payload) => {
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };
                request_artifacts
                    .entry(request_id.to_string())
                    .or_default()
                    .push(EventArtifactRef {
                        path: payload.path.clone(),
                        digest: Some(payload.digest.clone()),
                    });
            }
            EventV1::ToolCallRequested(payload) => {
                if !replay_agent_event {
                    continue;
                }
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };
                let request = requests.entry(request_id.to_string()).or_default();
                request
                    .tool_ids_by_call_id
                    .insert(payload.tool_call_id.clone(), payload.tool_id.clone());
                if let Some(index) = request.active_assistant_message_index {
                    if let Some(ConversationMessage::Assistant(assistant)) =
                        request.messages.get_mut(index)
                    {
                        assistant.tool_calls.push(ConversationToolCall {
                            tool_call_id: payload.tool_call_id.clone(),
                            tool_id: payload.tool_id.clone(),
                            args_summary: provider_tool_arguments_json(&payload.args_summary),
                            args_digest: payload.args_digest.clone(),
                            seq: Some(event.seq),
                            metadata: payload.metadata.clone(),
                        });
                        assistant.last_seq = Some(event.seq);
                    }
                }
            }
            EventV1::ToolCallFinished(payload) => {
                if !replay_agent_event {
                    continue;
                }
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };
                let Some(request) = requests.get_mut(request_id) else {
                    continue;
                };
                let Some(tool_id) = request
                    .tool_ids_by_call_id
                    .get(&payload.tool_call_id)
                    .cloned()
                else {
                    continue;
                };
                request
                    .messages
                    .push(ConversationMessage::ToolResult(Box::new(
                        ConversationToolResultMessage {
                            request_id: request_id.to_string(),
                            tool_call_id: payload.tool_call_id.clone(),
                            tool_id: Some(tool_id),
                            status: payload.status,
                            output_summary: payload.output_summary.clone(),
                            output_digest: payload.output_digest.clone(),
                            output_json: payload.output_json.clone(),
                            seq: Some(event.seq),
                            metadata: payload.metadata.clone(),
                        },
                    )));
            }
            EventV1::TaskCompleted(payload) => {
                if matches!(
                    historical_task_scopes.get(&payload.task_id),
                    Some(TaskTerminalScope::AgentTurn)
                ) {
                    agent_turn_agent_by_task.remove(&payload.task_id);
                }
                if !replay_agent_event {
                    continue;
                }
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };

                if !historical_task_completion_marks_agent_turn(
                    request_id,
                    payload,
                    &historical_task_scopes,
                    &request_turn_task_ids,
                ) {
                    continue;
                }

                let Some(agent_id) = event
                    .actor
                    .agent_id
                    .as_deref()
                    .and_then(non_empty_trimmed)
                    .map(str::to_string)
                else {
                    return Err(CoordinatorError::ResumeRestoreFailed {
                        run_id: run_id.to_string(),
                        reason: format!(
                            "task completion for request `{request_id}` missing agent actor"
                        ),
                    });
                };
                let request_state = requests.remove(request_id).ok_or_else(|| {
                    CoordinatorError::ResumeRestoreFailed {
                        run_id: run_id.to_string(),
                        reason: format!(
                            "missing provider request history for completed request `{request_id}`"
                        ),
                    }
                })?;

                let user_prompt = restore_historical_user_prompt(
                    run_id,
                    request_id,
                    request_state.user_text.clone(),
                    request_state.prompt_summary.clone(),
                )?;

                let assistant_response = if payload.result_summary.is_empty() {
                    request_state.assistant_output.clone()
                } else {
                    payload.result_summary.clone()
                };
                let messages = historical_conversation_messages_for_completed_turn(
                    &user_prompt,
                    &assistant_response,
                    &request_state,
                );
                let mut artifacts = request_artifacts.remove(request_id).unwrap_or_default();
                artifacts.sort_by(|left, right| {
                    left.path
                        .cmp(&right.path)
                        .then_with(|| left.digest.cmp(&right.digest))
                });
                artifacts
                    .dedup_by(|left, right| left.path == right.path && left.digest == right.digest);

                histories
                    .entry(request_state.agent_id.unwrap_or(agent_id))
                    .or_default()
                    .push_turn(ProviderConversationTurn {
                        user_prompt,
                        assistant_response,
                        request_id: Some(request_id.to_string()),
                        first_seq: request_state.first_seq,
                        last_seq: Some(event.seq),
                        artifacts,
                        messages,
                        ..ProviderConversationTurn::default()
                    });
            }
            EventV1::TaskCancelled(payload) => {
                let agent_id_from_task = if matches!(
                    historical_task_scopes.get(&payload.task_id),
                    Some(TaskTerminalScope::AgentTurn)
                ) {
                    agent_turn_agent_by_task.remove(&payload.task_id)
                } else {
                    None
                };
                if !replay_agent_event {
                    continue;
                }
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };

                if !historical_task_cancellation_marks_agent_turn(
                    request_id,
                    payload,
                    &historical_task_scopes,
                    &request_turn_task_ids,
                ) {
                    continue;
                }

                let Some(request_state) = requests.remove(request_id) else {
                    continue;
                };

                let agent_id = event
                    .actor
                    .agent_id
                    .as_deref()
                    .and_then(non_empty_trimmed)
                    .map(str::to_string)
                    .or(agent_id_from_task)
                    .or_else(|| request_state.agent_id.clone())
                    .ok_or_else(|| CoordinatorError::ResumeRestoreFailed {
                        run_id: run_id.to_string(),
                        reason: format!(
                            "task cancellation for request `{request_id}` missing agent actor"
                        ),
                    })?;
                let user_prompt = restore_historical_user_prompt(
                    run_id,
                    request_id,
                    request_state.user_text.clone(),
                    request_state.prompt_summary.clone(),
                )?;

                let (status, failure_stage) = historical_cancelled_turn_status_stage(
                    request_state.provider_finish_reason.as_deref(),
                    &payload.reason,
                );
                let messages = if failure_stage == "max_iters" {
                    historical_conversation_messages_for_completed_turn(
                        &user_prompt,
                        &request_state.assistant_output,
                        &request_state,
                    )
                } else {
                    Vec::new()
                };
                let provider_request_id = request_state
                    .provider_request_id
                    .unwrap_or_else(|| request_id.to_string());
                histories
                    .entry(agent_id)
                    .or_default()
                    .push_turn(ProviderConversationTurn {
                        user_prompt,
                        assistant_response: request_state.assistant_output,
                        status,
                        failure_stage: Some(failure_stage),
                        failure_reason: truncated_failure_reason(&payload.reason),
                        request_id: Some(provider_request_id),
                        first_seq: request_state.first_seq,
                        last_seq: Some(event.seq),
                        artifacts: Vec::new(),
                        messages,
                    });
            }
            _ => {}
        }
    }

    Ok(histories)
}

fn restore_continuation_controller_from_history(
    session_dir: &Path,
    run_id: &str,
) -> Result<ContinuationController, CoordinatorError> {
    let run_dir = session_dir.join(run_id);
    let events_path = run_dir.join(EVENTS_FILE_NAME);
    let historical_events = read_historical_events_until(run_id, &events_path, u64::MAX)?;
    let mut controller = ContinuationController::default();

    for event in historical_events {
        let event_mono_ms = event.mono_ms;
        match event.payload {
            EventV1::ContinuationStarted(payload) => {
                controller.start_at(
                    payload.continuation_id,
                    payload.mode,
                    payload.command,
                    ContinuationBounds {
                        max_iterations: payload.max_iterations,
                        max_wall_clock_ms: payload.max_wall_clock_ms,
                        max_provider_calls: payload.max_provider_calls,
                        max_tool_calls: payload.max_tool_calls,
                    },
                    event_mono_ms,
                );
            }
            EventV1::ContinuationReminderQueued(payload) => {
                controller.record_reminder(&payload.continuation_id, payload.iteration);
            }
            EventV1::ContinuationStopped(payload) => {
                if controller
                    .active()
                    .is_some_and(|active| active.continuation_id == payload.continuation_id)
                {
                    controller.stop();
                }
            }
            EventV1::ContinuationLimitReached(payload) => {
                if controller
                    .active()
                    .is_some_and(|active| active.continuation_id == payload.continuation_id)
                {
                    controller.stop();
                }
            }
            _ => {}
        }
    }

    Ok(controller)
}

fn should_replay_agent_scoped_event(
    seq: u64,
    agent_id: Option<&str>,
    checkpoint_boundaries: &BTreeMap<String, u64>,
) -> bool {
    let Some(agent_id) = agent_id else {
        return true;
    };

    seq > checkpoint_boundaries.get(agent_id).copied().unwrap_or(0)
}

fn discover_applied_checkpoints(
    run_id: &str,
    run_dir: &Path,
    events: &[EventEnvelopeV1],
) -> Result<BTreeMap<String, AppliedCheckpointRecord>, CoordinatorError> {
    let mut written_by_id = BTreeMap::new();
    let mut latest_applied_by_agent: BTreeMap<String, (u64, String)> = BTreeMap::new();

    for event in events {
        match &event.payload {
            EventV1::CompactionWritten(payload) => {
                written_by_id.insert(payload.checkpoint_id.clone(), payload.clone());
            }
            EventV1::CompactionApplied(payload) => {
                latest_applied_by_agent.insert(
                    payload.agent_id.clone(),
                    (event.seq, payload.checkpoint_id.clone()),
                );
            }
            _ => {}
        }
    }

    let mut applied = BTreeMap::new();
    for (agent_id, (_, checkpoint_id)) in latest_applied_by_agent {
        let Some(written) = written_by_id.get(&checkpoint_id) else {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: format!(
                    "compaction checkpoint `{checkpoint_id}` was applied without a matching written event"
                ),
            });
        };

        if written.agent_id != agent_id {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: format!(
                    "compaction checkpoint `{checkpoint_id}` agent mismatch between applied `{agent_id}` and written `{}`",
                    written.agent_id
                ),
            });
        }

        applied.insert(
            agent_id,
            AppliedCheckpointRecord {
                checkpoint_id: checkpoint_id.clone(),
                artifact_path: written.artifact_path.clone(),
                through_seq: written.through_seq,
                through_request_id: written.through_request_id.clone(),
            },
        );
    }

    let _ = run_dir;
    Ok(applied)
}

fn load_provider_context_checkpoint(
    run_id: &str,
    run_dir: &Path,
    checkpoint: &AppliedCheckpointRecord,
) -> Result<ProviderContextCheckpoint, CoordinatorError> {
    let checkpoint_path = run_dir.join(&checkpoint.artifact_path);
    let body = fs::read_to_string(&checkpoint_path).map_err(|source| {
        CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!(
                "failed to read checkpoint artifact {}: {source}",
                checkpoint_path.display()
            ),
        }
    })?;

    serde_json::from_str(&body).map_err(|source| CoordinatorError::ResumeRestoreFailed {
        run_id: run_id.to_string(),
        reason: format!(
            "invalid checkpoint artifact {}: {source}",
            checkpoint_path.display()
        ),
    })
}

fn historical_task_completion_marks_agent_turn(
    request_id: &str,
    payload: &TaskCompletedEvent,
    historical_task_scopes: &BTreeMap<String, TaskTerminalScope>,
    request_turn_task_ids: &BTreeMap<String, String>,
) -> bool {
    if let Some(scope) = payload
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.task_scope)
    {
        return matches!(scope, TaskTerminalScope::AgentTurn);
    }

    if let Some(scope) = historical_task_scopes.get(&payload.task_id) {
        return matches!(scope, TaskTerminalScope::AgentTurn);
    }

    if let Some(turn_task_id) = request_turn_task_ids.get(request_id) {
        return turn_task_id == &payload.task_id;
    }

    true
}

fn historical_task_cancellation_marks_agent_turn(
    request_id: &str,
    payload: &TaskCancelledEvent,
    historical_task_scopes: &BTreeMap<String, TaskTerminalScope>,
    request_turn_task_ids: &BTreeMap<String, String>,
) -> bool {
    if let Some(scope) = payload.task_scope {
        return matches!(scope, TaskTerminalScope::AgentTurn);
    }

    if let Some(scope) = historical_task_scopes.get(&payload.task_id) {
        return matches!(scope, TaskTerminalScope::AgentTurn);
    }

    if let Some(turn_task_id) = request_turn_task_ids.get(request_id) {
        return turn_task_id == &payload.task_id;
    }

    false
}

fn historical_cancelled_turn_status_stage(
    provider_finish_reason: Option<&str>,
    cancellation_reason: &str,
) -> (ProviderConversationTurnStatus, String) {
    if provider_finish_reason == Some("error") {
        return (
            ProviderConversationTurnStatus::Failed,
            "provider_error".to_string(),
        );
    }

    if cancellation_reason.contains("overflow persisted after checkpoint compaction") {
        return (
            ProviderConversationTurnStatus::Failed,
            "overflow_retry_failed".to_string(),
        );
    }

    if cancellation_reason.contains("failed closed") {
        return (
            ProviderConversationTurnStatus::Failed,
            "tool_failure".to_string(),
        );
    }

    if cancellation_reason.contains("critical lifecycle hook failed")
        || cancellation_reason.contains("lifecycle hook failed")
    {
        return (
            ProviderConversationTurnStatus::Failed,
            "hook_failure".to_string(),
        );
    }

    if cancellation_reason.contains("agent turn exceeded profile max_iters=") {
        return (
            ProviderConversationTurnStatus::Aborted,
            "max_iters".to_string(),
        );
    }

    (
        ProviderConversationTurnStatus::Aborted,
        "cancelled".to_string(),
    )
}

fn checked_next_counter(
    value: u64,
    run_id: &str,
    counter_kind: &'static str,
) -> Result<u64, CoordinatorError> {
    value
        .checked_add(1)
        .ok_or_else(|| CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!("{counter_kind} counter overflow"),
        })
}

fn next_agent_counter_for_run(
    session_dir: &Path,
    run_id: &str,
    minimum_previous_agent_id: u64,
) -> Result<u64, CoordinatorError> {
    let mut max_agent_id = minimum_previous_agent_id;
    let entries =
        fs::read_dir(session_dir).map_err(|source| CoordinatorError::CreateSessionDirectory {
            path: session_dir.display().to_string(),
            source,
        })?;

    for entry in entries {
        let entry = entry.map_err(|source| CoordinatorError::CreateSessionDirectory {
            path: session_dir.display().to_string(),
            source,
        })?;
        if !entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Some(suffix) = name.strip_prefix("agent_") else {
            continue;
        };
        if suffix.len() != 6 || !suffix.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        if let Ok(parsed) = suffix.parse::<u64>() {
            max_agent_id = max_agent_id.max(parsed);
        }
    }

    checked_next_counter(max_agent_id, run_id, "agent id")
}

fn append_built_event(
    run_state: &mut RunState,
    envelope: EventEnvelopeV1,
) -> Result<EventEnvelopeV1, CoordinatorError> {
    let expected_seq = run_state.next_event_seq;
    let appended = run_state
        .event_store
        .append(EventEnvelopeWithoutSeqV1::from(envelope))?;

    if appended.seq != expected_seq {
        return Err(CoordinatorError::EventSequenceMismatch {
            expected: expected_seq,
            actual: appended.seq,
        });
    }

    run_state.next_event_seq += 1;
    mirror_event_to_child_session(run_state, &appended)?;
    Ok(appended)
}

fn system_actor() -> EventActor {
    EventActor::new(ActorKind::System, Some(COORDINATOR_AGENT_ID.to_string()))
}

fn workflow_projection_for_run(
    run_state: &RunState,
) -> Result<crate::workflow::WorkflowProjection, CoordinatorError> {
    let historical_events = read_historical_events_until(
        &run_state.info.run_id,
        &run_state.info.events_path,
        u64::MAX,
    )?;
    Ok(project_workflows(
        historical_events.iter().map(|event| &event.payload),
    ))
}

fn workflow_completion_denial_policy_id(readiness: &WorkflowCompletionReadiness) -> &'static str {
    if !readiness.signoff.allowed {
        "transition.evidence_gated_completion"
    } else if !readiness.tasks.allowed {
        "transition.workflow_tasks_incomplete"
    } else {
        "transition.active_continuation_incomplete"
    }
}

fn workflow_completion_denial_reason(readiness: &WorkflowCompletionReadiness) -> String {
    if !readiness.signoff.allowed {
        let missing = readiness.signoff.missing_evidence_categories.join(", ");
        if missing.is_empty() {
            "workflow completion requires mapped signoff evidence or waiver".to_string()
        } else {
            format!("workflow completion missing signoff evidence: {missing}")
        }
    } else if !readiness.tasks.allowed {
        format!(
            "workflow completion blocked by incomplete workflow-owned tasks: {}",
            readiness.tasks.incomplete_task_ids().join(", ")
        )
    } else {
        format!(
            "workflow completion blocked by active continuations: {}",
            readiness.active_continuation_ids.join(", ")
        )
    }
}

fn agent_actor(agent_id: &str) -> EventActor {
    EventActor::new(ActorKind::Worker, Some(agent_id.to_string()))
}
