use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

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
    ProviderBoundaryContext, ProviderContext, ProviderContextCheckpoint, ProviderConversationTurn,
    ProviderConversationTurnStatus, StreamAssistantResponseOnceRequest, MAX_TOOL_CALLS_TOTAL,
};
use crate::clock::Clock;
use crate::config::{
    registered_hook_runtime_config, CompactionRuntimeConfig, HookLifecycleEvent, HookRuntimeConfig,
    LifecycleHookConfig, ShellAllowlist, ToolFailureMode,
};
use crate::conversation::{
    ConversationAssistantMessage, ConversationMessage, ConversationToolCall,
    ConversationToolResultMessage, ConversationUserMessage,
};
use crate::counter_id::parse_prefixed_counter;
use crate::digest::digest12;
use crate::event::{
    ActorKind, AgentSpawnedEvent, ArtifactWrittenEvent, BackgroundTaskNotificationEvent,
    BackgroundTaskNotificationStatus, CompactionAppliedEvent, CompactionFailedEvent,
    CompactionRequestedEvent, CompactionWrittenEvent, EditAppliedEvent, EditProposedEvent,
    EditRejectedEvent, EventActor, EventBuildError, EventBuilder, EventContext, EventEnvelopeV1,
    EventV1, HookExecutionMetadata, PermissionDecision as EventPermissionDecision,
    PermissionGrantRecordedEvent, PermissionRequestedArgs, PermissionResolvedEvent,
    PolicyViolationDetectedEvent, ProviderAssistantMessageMetadata, ProviderReasoningDeltaEvent,
    ProviderRequestFinishedMetadata, ProviderRequestStartedMetadata, RunFailedEvent,
    RunFinishedEvent, RunStartedEvent, StaleDetectedEvent, TaskCancelledEvent, TaskCompletedEvent,
    TaskCompletionMetadata, TaskLineageMetadata, TaskResultLateEvent, TaskScheduleState,
    TaskScheduledEvent, TaskTerminalScope, TeamBounds, TeamCreatedEvent, TeamDeletedEvent,
    TeamMemberRole, TeamMemberSelector, TeamMemberSpawnedEvent, TeamMemberSpec, TeamMessage,
    TeamMessageKind, TeamMessageSentEvent, TeamShutdownApprovedEvent, TeamShutdownRejectedEvent,
    TeamShutdownRequestedEvent, TeamSpec, TeamTask, TeamTaskCreatedEvent, TeamTaskStatus,
    TeamTaskUpdatedEvent, ToolCallFinishedEvent, ToolCallMetadata, ToolCallStartedEvent,
    ToolCallStatus, ToolIdentityMetadata, UserMessageSubmittedEvent,
};
use crate::perm::{
    permission_kind_for_tool, permission_kind_for_tool_call, PermissionDecision, PermissionGrant,
    PermissionGrantRequest, PermissionGrantScope, PermissionGrantSet, PermissionKind,
    PermissionPolicy, PolicyDecision,
};
use crate::proj::{
    inspect_resume_plan, project_background_request, project_team_state,
    resolve_background_request_ref, BackgroundRequestProjection, BackgroundRequestProjectionError,
    RecordedRuntimeContext, RunMetadata, SessionModeSource, TeamProjection, TeamRunProjection,
};
use crate::provider_args::provider_tool_arguments_json;
use crate::redact::Redactor;
use crate::sched::{
    ConcurrencyKey, ScheduleDecision, Scheduler, SchedulerLimits, TaskProgressSnapshot,
};
use crate::session_paths::{ARTIFACTS_DIR_NAME, META_FILE_NAME};
use crate::session_title::{
    clean_generated_title, is_parent_default_title, TITLE_AGENT_NAME, TITLE_GENERATION_USER_PROMPT,
};
use crate::store::{EventEnvelopeWithoutSeqV1, EventStore, EventStoreError, JsonlFileEventStore};
use crate::text::{non_empty_trimmed, truncate_with_ellipsis};
use crate::tool::{ToolContext, ToolRegistry, ToolResult, ToolRunState};
use harness_providers::{
    AssistantToolCall, CompletionMessage, CompletionRequest, MessageRole, Provider,
    ProviderStreamEvent, ToolDef,
};

mod child_session;
mod hooks;
mod permission;
mod provider_context;
mod question;
mod task_category;
mod team;
mod tool_execution;
mod tool_metadata;

use self::child_session::{
    create_child_session_mirror, finish_child_session_mirrors, mirror_event_to_child_session,
    restore_child_session_mirrors, ChildSessionMirror,
};
use self::team::{
    reject_nested_team_create, require_active_team, require_active_team_or_shutdown,
    validate_team_action, validate_team_actor_can_make_unowned_team_write, validate_team_member,
    validate_team_message, validate_team_participant, validate_team_profile_role,
    validate_team_shutdown_request_can_open, validate_team_shutdown_request_pending,
    validate_team_task_create, validate_team_task_update, TeamActionKind, TeamParticipantRole,
};

use self::permission::{
    evaluate_permission_rule_requests, event_permission_decision, permission_decision_label,
    permission_grant_request, permission_request_digest, permission_rule_request_selectors,
    permission_summary, plan_mode_edit_boundary_denial, plan_mode_shell_boundary_denial,
};

#[cfg(test)]
use self::hooks::summarize_hook_output;

pub use self::hooks::{
    LifecycleHookCommandExecutor, LifecycleHookCommandInvocation, LifecycleHookCommandOutput,
    TokioLifecycleHookCommandExecutor,
};

pub use self::task_category::{
    task_category_fallback_chain, task_category_fallback_disabled_for_parent,
    task_category_fallback_profile, TASK_CATEGORY_FALLBACK_DISABLED_PARENT_PROFILES,
    TASK_CATEGORY_FALLBACK_PROFILE,
};

use self::provider_context::{
    approximate_provider_context_tokens, approximate_text_tokens, compaction_summary_model_ref,
    compaction_summary_override_from_hooks, is_provider_context_overflow_reason,
    model_backed_compaction_summary_for, restore_provider_context_from_history,
    serialize_provider_context_checkpoint, truncated_failure_reason, CompactionSummaryDecision,
    ModelBackedCompactionSummary, ProviderCompactionTrigger, ProviderContextCompactionRequest,
};
use self::question::{
    parse_question_request_prompts, question_request_timeout_ms, validate_question_answers_reason,
    QuestionPromptSpec,
};
use self::tool_metadata::{
    applied_tool_edit_metadata, event_artifact_refs, execution_timing_metadata,
    extract_hook_execution_metadata, failed_tool_output_json, hashline_edit_metadata,
    requested_tool_call_metadata, stable_tool_output_json, tool_call_metadata,
    tool_identity_metadata, tool_task_lineage_metadata, AppliedToolEditMetadata,
    HashlineEditMetadata,
};

#[cfg(test)]
use self::provider_context::{
    build_model_compaction_prompt, build_provider_context_summary,
    provider_context_summary_required_headings, validate_model_compaction_summary,
    ProviderContextCompactionPlan, PROVIDER_CONTEXT_COMPACTION_TURN_EXCERPT_MAX_CHARS,
    PROVIDER_CONTEXT_SUMMARY_CONTRACT_VERSION,
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
const BACKGROUND_TASK_NOTIFICATION_SUMMARY_MAX_CHARS: usize = 511;
const BACKGROUND_TASK_NOTIFICATION_DESCRIPTION_MAX_CHARS: usize = 160;
const TEAM_MESSAGE_BODY_MAX_BYTES: usize = 32 * 1024;
const TEAM_TEXT_FIELD_MAX_CHARS: usize = 512;
const TEAM_TASK_METADATA_MAX_ENTRIES: usize = 32;
const TEAM_TASK_METADATA_MAX_CHARS: usize = 256;
const TEAM_REFERENCE_LIMIT: usize = 32;
const TEAM_MAX_MEMBERS: usize = 8;

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
    pub hook_command_executor: Arc<dyn LifecycleHookCommandExecutor + Send + Sync>,
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
            hook_command_executor: Arc::new(TokioLifecycleHookCommandExecutor),
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
    FailRun {
        error: String,
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

    pub async fn fail_run(&self, error: impl Into<String>) -> Result<(), CoordinatorError> {
        let error = error.into();
        self.request(|respond_to| Command::FailRun { error, respond_to })
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
            Command::FailRun { error, respond_to } => {
                let result = self.fail_run_internal(error).await;
                warn_oneshot_send_failure(respond_to.send(result), "fail_run");
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
            scheduler: Scheduler::new(SchedulerLimits {
                provider_model: self.config.provider_model_concurrency,
                tool: self.config.tool_concurrency,
            }),
            recorded_runtime_context: None,
            allow_initial_runtime_context_recording: true,
            shutdown_token: CancellationToken::new(),
            tool_state: ToolRunState::default(),
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

        let hook_batch = hooks::run_lifecycle_hooks(
            self.clock.as_ref(),
            self.config.hook_command_executor.as_ref(),
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
            scheduler: Scheduler::new(SchedulerLimits {
                provider_model: self.config.provider_model_concurrency,
                tool: self.config.tool_concurrency,
            }),
            recorded_runtime_context: None,
            allow_initial_runtime_context_recording: false,
            shutdown_token: CancellationToken::new(),
            tool_state: ToolRunState::default(),
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

        let hook_batch = hooks::run_lifecycle_hooks(
            self.clock.as_ref(),
            self.config.hook_command_executor.as_ref(),
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

    async fn fail_run_internal(&mut self, error: String) -> Result<(), CoordinatorError> {
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
            EventV1::RunFailed(RunFailedEvent {
                error: error.clone(),
            }),
        )?;

        let hook_batch = hooks::run_lifecycle_hooks(
            self.clock.as_ref(),
            self.config.hook_command_executor.as_ref(),
            &self.config.hook_runtime_config,
            HookInvocationContext {
                event: HookLifecycleEvent::RunFailed,
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
                outcome: Some("failed".to_string()),
                output_summary: None,
                failure_reason: Some(error),
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
            let hook_batch = hooks::run_lifecycle_hooks(
                self.clock.as_ref(),
                self.config.hook_command_executor.as_ref(),
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
                model_settings: default_model_settings_for_profile(&profile_cfg.name),
            };

            schedule_agent_turn(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                self.config.hook_command_executor.clone(),
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

        let request = AgentRequest {
            agent_id,
            prompt,
            prompt_context,
            selected_file_tags: selected_tags.files,
            selected_agent_tags: selected_tags.agents,
            selected_resource_tags: selected_tags.resources,
            model_ref: model_ref_override.unwrap_or_else(|| profile.model_ref.clone()),
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
            self.config.hook_command_executor.clone(),
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
        let raw_permission_kind = permission_kind_for_tool_call(&tool_id, capability);
        let skip_outer_question_permission = raw_permission_kind == Some(PermissionKind::Question);
        let maybe_kind = if skip_outer_question_permission {
            None
        } else {
            raw_permission_kind
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
                self.config.hook_command_executor.as_ref(),
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
                self.config.hook_command_executor.as_ref(),
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
                    self.config.hook_command_executor.as_ref(),
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

                if run_state.permission_grant_authorizes(&grant_request) {
                    tool_execution::start_tool_call_execution(
                        clock.as_ref(),
                        redactor.as_ref(),
                        self.config.hook_command_executor.clone(),
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

                let requested_hook_batch = hooks::run_lifecycle_hooks(
                    self.clock.as_ref(),
                    self.config.hook_command_executor.as_ref(),
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

                    let resolved_hook_batch = hooks::run_lifecycle_hooks(
                        self.clock.as_ref(),
                        self.config.hook_command_executor.as_ref(),
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

                run_state.insert_pending_permission(permission_id.clone(), pending);

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
                tool_execution::start_tool_call_execution(
                    clock.as_ref(),
                    redactor.as_ref(),
                    self.config.hook_command_executor.clone(),
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

        let Some(existing) = run_state.pending_permission(&permission_id) else {
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
            .take_pending_permission(&permission_id)
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

        let resolved_hook_batch = hooks::run_lifecycle_hooks(
            self.clock.as_ref(),
            self.config.hook_command_executor.as_ref(),
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
                        run_state.record_permission_grant(grant);
                    }

                    tool_execution::start_tool_call_execution(
                        clock.as_ref(),
                        redactor.as_ref(),
                        self.config.hook_command_executor.clone(),
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

        let Some(pending) = run_state.take_pending_permission(&permission_id) else {
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

        let resolved_hook_batch = hooks::run_lifecycle_hooks(
            self.clock.as_ref(),
            self.config.hook_command_executor.as_ref(),
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

            let requested_hook_batch = hooks::run_lifecycle_hooks(
                self.clock.as_ref(),
                self.config.hook_command_executor.as_ref(),
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

                let resolved_hook_batch = hooks::run_lifecycle_hooks(
                    self.clock.as_ref(),
                    self.config.hook_command_executor.as_ref(),
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

            run_state.insert_pending_permission(permission_id.clone(), pending);

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
        team::validate_team_spec(&spec)?;

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
                self.config.hook_command_executor.clone(),
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
                self.config.hook_command_executor.clone(),
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
                let lineage = tool_task_lineage_metadata(
                    &task.tool_call_id,
                    task.request_correlation_id.as_deref(),
                    result_for_response.structured_json.as_ref(),
                );
                let mut hook_executions = task_hook_state.hook_executions.clone();
                hook_executions.extend(extract_hook_execution_metadata(
                    result_for_response.structured_json.as_ref(),
                ));
                let finish_hook_batch = hooks::run_lifecycle_hooks(
                    self.clock.as_ref(),
                    self.config.hook_command_executor.as_ref(),
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
                let finish_hook_batch = hooks::run_lifecycle_hooks(
                    self.clock.as_ref(),
                    self.config.hook_command_executor.as_ref(),
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
                        Some(tool_task_lineage_metadata(
                            &task.tool_call_id,
                            task.request_correlation_id.as_deref(),
                            None,
                        )),
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
                let finish_hook_batch = hooks::run_lifecycle_hooks(
                    self.clock.as_ref(),
                    self.config.hook_command_executor.as_ref(),
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
                        Some(tool_task_lineage_metadata(
                            &task.tool_call_id,
                            task.request_correlation_id.as_deref(),
                            None,
                        )),
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

        let hook_batch = hooks::run_lifecycle_hooks(
            self.clock.as_ref(),
            self.config.hook_command_executor.as_ref(),
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

        let hook_batch = hooks::run_lifecycle_hooks(
            self.clock.as_ref(),
            self.config.hook_command_executor.as_ref(),
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

        let requested_hook_batch = hooks::run_lifecycle_hooks(
            self.clock.as_ref(),
            self.config.hook_command_executor.as_ref(),
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
        let summary_override =
            compaction_summary_override_from_hooks(&requested_hook_batch.hook_executions);
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

        if let ("overflow_retry", Some(task_id), Some(request_id)) = (
            trigger.trigger_reason.as_str(),
            task_id,
            trigger.through_request_id.as_deref(),
        ) {
            run_state.record_overflow_retry_compacted_context(
                task_id,
                request_id,
                updated_context.updated_context.clone(),
            );
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
            run_state.failed_terminal_compaction_attempt_should_run(&request)
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

    async fn promote_next_agent_blocked_turn(
        &mut self,
        agent_id: &str,
    ) -> Result<(), CoordinatorError> {
        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(());
        };
        if run_state.agent_has_running_turn(agent_id) {
            return Ok(());
        }

        let Some(blocked_task_id) = run_state.next_agent_blocked_turn_id(agent_id) else {
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
                    self.config.hook_command_executor.clone(),
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
                run_state.mark_queued_agent_turn_scheduler_queued(&blocked_task_id);
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
        let (dequeued, terminal_compaction, finished_agent_id) = {
            let Some(run_state) = self.run_state.as_mut() else {
                return Ok(());
            };

            let Some(running) = run_state.running_agent_turns.remove(&task_id) else {
                return Ok(());
            };

            let finished_agent_id = running.agent_id.clone();
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
            let finished_hook_batch = hooks::run_lifecycle_hooks(
                self.clock.as_ref(),
                self.config.hook_command_executor.as_ref(),
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
                let subagent_finished_hook_batch = hooks::run_lifecycle_hooks(
                    self.clock.as_ref(),
                    self.config.hook_command_executor.as_ref(),
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
                                self.config.hook_command_executor.clone(),
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
                                self.config.hook_command_executor.clone(),
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
                            self.config.hook_command_executor.clone(),
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

            (dequeued, terminal_compaction, finished_agent_id)
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
                    self.config.hook_command_executor.clone(),
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
            self.config.hook_command_executor.clone(),
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

        Ok(())
    }
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
    scheduler: Scheduler,
    recorded_runtime_context: Option<RecordedRuntimeContext>,
    allow_initial_runtime_context_recording: bool,
    shutdown_token: CancellationToken,
    tool_state: ToolRunState,
}

impl RunState {
    fn agent_has_active_or_queued_turn(&self, agent_id: &str) -> bool {
        self.running_agent_turns
            .values()
            .any(|running| running.agent_id == agent_id)
            || self
                .queued_agent_turns
                .values()
                .any(|queued| queued.agent_id == agent_id)
    }

    fn agent_has_running_turn(&self, agent_id: &str) -> bool {
        self.running_agent_turns
            .values()
            .any(|running| running.agent_id == agent_id)
    }

    fn next_agent_blocked_turn_id(&self, agent_id: &str) -> Option<String> {
        self.queued_agent_turns
            .values()
            .filter(|queued| queued.agent_id == agent_id && !queued.scheduler_queued)
            .min_by(|left, right| left.task_id.cmp(&right.task_id))
            .map(|queued| queued.task_id.clone())
    }

    fn queue_agent_turn(&mut self, queued: QueuedAgentTurn) {
        self.queued_agent_turns
            .insert(queued.task_id.clone(), queued);
    }

    fn mark_queued_agent_turn_scheduler_queued(&mut self, task_id: &str) {
        if let Some(queued) = self.queued_agent_turns.get_mut(task_id) {
            queued.scheduler_queued = true;
        }
    }

    fn begin_running_agent_turn<C>(
        &mut self,
        clock: &C,
        task: &QueuedAgentTurn,
        hook_executions: Vec<HookExecutionMetadata>,
        cancellation_token: CancellationToken,
    ) where
        C: Clock + ?Sized,
    {
        self.running_agent_turns.insert(
            task.task_id.clone(),
            RunningAgentTurn {
                agent_id: task.agent_id.clone(),
                request_id: task.request_id.clone(),
                request_prompt: task.request.prompt.clone(),
                profile_name: task.profile.name.clone(),
                model_ref: task.request.model_ref.clone(),
                model_settings: task.request.model_settings.clone(),
                category: Some(task.profile.category.clone()),
                queue_key: task.queue_key.clone(),
                cancellation_token,
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
    }

    fn pending_permission(&self, permission_id: &str) -> Option<&PendingPermissionState> {
        self.pending_permissions.get(permission_id)
    }

    fn insert_pending_permission(
        &mut self,
        permission_id: String,
        pending: PendingPermissionState,
    ) {
        self.pending_permissions.insert(permission_id, pending);
    }

    fn take_pending_permission(&mut self, permission_id: &str) -> Option<PendingPermissionState> {
        self.pending_permissions.remove(permission_id)
    }

    fn record_permission_grant(&mut self, grant: PermissionGrant) {
        self.active_permission_grants.record(grant);
    }

    fn permission_grant_authorizes(&self, grant_request: &PermissionGrantRequest) -> bool {
        self.active_permission_grants.authorizes(grant_request)
    }

    fn record_overflow_retry_compacted_context(
        &mut self,
        task_id: &str,
        request_id: &str,
        context: ProviderContext,
    ) {
        self.overflow_retry_compacted_context_by_attempt
            .insert((task_id.to_string(), request_id.to_string()), context);
    }

    fn failed_terminal_compaction_attempt_should_run(
        &mut self,
        request: &FailedTerminalCompactionRequest,
    ) -> bool {
        let key = request.attempt_key();
        if !self.failed_terminal_compaction_attempts.insert(key.clone()) {
            return false;
        }

        if let Some(overflow_context) = self.overflow_retry_compacted_context_by_attempt.get(&key) {
            let current_context = self
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
}

#[derive(Debug, Clone)]
struct QueuedAgentTurn {
    task_id: String,
    agent_id: String,
    session_id: String,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingAgentWakeup {
    request_id: String,
    notification_text: String,
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

fn background_task_notification_reminder_text(
    notification: &BackgroundTaskNotificationEvent,
) -> String {
    format!(
        "<system-reminder>\n{}\n</system-reminder>",
        background_task_notification_text(notification)
    )
}

fn build_background_task_notification<R>(
    redactor: &R,
    child_task: &ChildTaskTurnState,
    parent_agent_id: Option<String>,
    delivered_turn_request_id: Option<String>,
    terminal_event: &EventEnvelopeV1,
    status: BackgroundTaskNotificationStatus,
    summary: &str,
) -> BackgroundTaskNotificationEvent
where
    R: Redactor + ?Sized,
{
    let capped_description = truncate_with_ellipsis(
        &redactor.redact_text(&child_task.description),
        BACKGROUND_TASK_NOTIFICATION_DESCRIPTION_MAX_CHARS,
    );
    let capped_summary = truncate_with_ellipsis(
        &redactor.redact_text(summary),
        BACKGROUND_TASK_NOTIFICATION_SUMMARY_MAX_CHARS,
    );

    BackgroundTaskNotificationEvent {
        parent_session_id: child_task.parent_session_id.clone(),
        parent_agent_id,
        child_session_id: child_task.child_session_id.clone(),
        child_request_id: child_task.child_request_id.clone(),
        task_id: child_task.task_id.clone(),
        description: capped_description,
        status,
        summary: capped_summary,
        terminal_event_id: terminal_event.event_id.clone(),
        terminal_task_id: terminal_terminal_task_id(terminal_event),
        delivered_turn_request_id,
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "background notification scheduling needs explicit coordinator dependencies"
)]
async fn append_background_task_notification_and_schedule<C, R>(
    clock: &C,
    redactor: &R,
    hook_command_executor: Arc<dyn LifecycleHookCommandExecutor + Send + Sync>,
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
    let notification = build_background_task_notification(
        redactor,
        &child_task,
        parent_agent_id.clone(),
        delivered_turn_request_id.clone(),
        terminal_event,
        status,
        summary,
    );
    let notification_text = background_task_notification_reminder_text(&notification);

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

    if run_state.agent_has_running_turn(&parent_agent_id) {
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
        hook_command_executor,
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
    hook_command_executor: Arc<dyn LifecycleHookCommandExecutor + Send + Sync>,
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
    if run_state.agent_has_running_turn(agent_id) {
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
            hook_command_executor.clone(),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskExecutionState {
    Running,
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

#[derive(Debug, Clone, Default)]
struct HookExecutionBatch {
    hook_executions: Vec<HookExecutionMetadata>,
    critical_failure: Option<String>,
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

#[expect(
    clippy::too_many_arguments,
    reason = "agent turn scheduling needs explicit coordinator dependencies"
)]
async fn schedule_agent_turn<C, R>(
    clock: &C,
    redactor: &R,
    hook_command_executor: Arc<dyn LifecycleHookCommandExecutor + Send + Sync>,
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
    let session_id = run_state.info.run_id.clone();

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

    if run_state.agent_has_active_or_queued_turn(&agent_id) {
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

        run_state.queue_agent_turn(QueuedAgentTurn {
            task_id,
            agent_id,
            session_id,
            request_id,
            profile,
            request,
            queue_key,
            scheduler_queued: false,
            child_task,
        });

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
                hook_command_executor,
                job_tx,
                run_state,
                hook_runtime_config,
                compaction_config,
                provider,
                tool_registry,
                QueuedAgentTurn {
                    task_id,
                    agent_id,
                    session_id,
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

            run_state.queue_agent_turn(QueuedAgentTurn {
                task_id,
                agent_id,
                session_id,
                request_id,
                profile,
                request,
                queue_key,
                scheduler_queued: true,
                child_task,
            });
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
    hook_command_executor: &(dyn LifecycleHookCommandExecutor + Send + Sync),
    run_state: &mut RunState,
    hook_runtime_config: &HookRuntimeConfig,
    task: &QueuedAgentTurn,
) -> TurnStartPhaseResult
where
    C: Clock + ?Sized,
{
    let cancellation_token = run_state.shutdown_token.child_token();
    let mut hook_executions = run_state
        .agent_hook_state
        .remove(&task.agent_id)
        .unwrap_or_default();

    let started_hook_batch = hooks::run_lifecycle_hooks(
        clock,
        hook_command_executor,
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
            category: Some(task.profile.category.clone()),
            outcome: Some("started".to_string()),
            output_summary: Some(task.request.prompt.clone()),
            failure_reason: None,
        },
    )
    .await;
    hook_executions.extend(started_hook_batch.hook_executions.clone());

    run_state.begin_running_agent_turn(clock, task, hook_executions, cancellation_token.clone());

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
    hook_command_executor: Arc<dyn LifecycleHookCommandExecutor + Send + Sync>,
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
    let turn_start = run_turn_start_phase(
        clock,
        hook_command_executor.as_ref(),
        run_state,
        &hook_runtime_config,
        &task,
    )
    .await;
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
}

struct AgentProviderTurnState {
    model: AgentModelRef,
    tool_defs: Vec<ToolDef>,
    messages: Vec<CompletionMessage>,
    total_tool_calls: usize,
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
    session_id: &'a str,
}

async fn run_agent_turn_phase_loop(request: AgentTurnPhaseLoopRequest<'_>) -> AgentTurnOutcome {
    let AgentTurnPhaseLoopRequest {
        provider,
        tool_registry,
        task,
        prior_context,
        job_tx,
        cancellation_token,
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
            session_id: &task.session_id,
        })
        .await
        {
            Ok(response) => response,
            Err(mut failure) => {
                let reason = normalize_provider_phase_error(failure.to_string());
                failure.reason = reason.clone();
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
            context: Default::default(),
            stream: true,
        })
        .await;

    let mut text = String::new();
    while let Some(event) = stream.next().await {
        match event {
            ProviderStreamEvent::TextDelta(delta) => text.push_str(&delta),
            ProviderStreamEvent::Error { message, .. } => return Err(message),
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
        tool_defs,
        messages,
        total_tool_calls: 0,
    })
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
        session_id,
    } = request;
    let task_id = task_id.to_string();
    let agent_id = agent_id.to_string();

    stream_assistant_response_once(
        StreamAssistantResponseOnceRequest {
            provider,
            profile,
            model,
            model_settings: request.model_settings.clone(),
            turn_request_id: turn_request_id.to_string(),
            provider_request_id,
            session_id: Some(session_id.to_string()),
            prompt_summary: &request.prompt,
            context: ProviderBoundaryContext::ProviderMessages { messages },
            tool_defs,
        },
        |event| {
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
    hook_command_executor: &(dyn LifecycleHookCommandExecutor + Send + Sync),
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

    let resolved_hook_batch = hooks::run_lifecycle_hooks(
        clock,
        hook_command_executor,
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
    let context = event_context_with_keys(run_state, actor, stream_key, correlation_id);
    let envelope = builder.build(context, payload)?;
    append_built_event(run_state, envelope)
}

fn event_context_with_keys(
    run_state: &RunState,
    actor: EventActor,
    stream_key: Option<String>,
    correlation_id: Option<String>,
) -> EventContext {
    let mut context = EventContext::new(run_state.next_event_seq, actor);
    context.correlation_id = correlation_id;
    context.stream_key = stream_key;
    context
}

fn event_context_with_correlation_fallback(
    run_state: &RunState,
    actor: EventActor,
    stream_key: String,
    request_correlation_id: Option<&str>,
    fallback_correlation_id: &str,
) -> EventContext {
    event_context_with_keys(
        run_state,
        actor,
        Some(stream_key),
        Some(
            request_correlation_id
                .unwrap_or(fallback_correlation_id)
                .to_string(),
        ),
    )
}

fn tool_call_event_context(
    run_state: &RunState,
    actor: EventActor,
    tool_call_id: &str,
    request_correlation_id: Option<&str>,
) -> EventContext {
    event_context_with_correlation_fallback(
        run_state,
        actor,
        format!("tool_call:{tool_call_id}"),
        request_correlation_id,
        tool_call_id,
    )
}

fn permission_event_context(
    run_state: &RunState,
    permission_id: &str,
    request_correlation_id: Option<&str>,
    fallback_correlation_id: &str,
) -> EventContext {
    event_context_with_correlation_fallback(
        run_state,
        system_actor(),
        format!("permission:{permission_id}"),
        request_correlation_id,
        fallback_correlation_id,
    )
}

fn edit_event_context(
    run_state: &RunState,
    edit_id: &str,
    tool_call_id: &str,
    request_correlation_id: Option<&str>,
) -> EventContext {
    event_context_with_correlation_fallback(
        run_state,
        system_actor(),
        format!("edit:{edit_id}"),
        request_correlation_id,
        tool_call_id,
    )
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
    let context = tool_call_event_context(run_state, actor, tool_call_id, request_correlation_id);
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
    let context = permission_event_context(
        run_state,
        permission_id,
        request_correlation_id,
        tool_call_id,
    );

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
    let context = permission_event_context(
        run_state,
        permission_id,
        request_correlation_id,
        permission_id,
    );
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
    let context = tool_call_event_context(
        run_state,
        system_actor(),
        tool_call_id,
        request_correlation_id,
    );
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
    let context = tool_call_event_context(
        run_state,
        system_actor(),
        tool_call_id,
        request_correlation_id,
    );
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
    let context = edit_event_context(
        run_state,
        &metadata.edit_id,
        tool_call_id,
        request_correlation_id,
    );

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
    let context = edit_event_context(
        run_state,
        &metadata.edit_id,
        tool_call_id,
        request_correlation_id,
    );

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
    let context = edit_event_context(
        run_state,
        &metadata.edit_id,
        tool_call_id,
        request_correlation_id,
    );

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
    let context = tool_call_event_context(
        run_state,
        system_actor(),
        tool_call_id,
        request_correlation_id,
    );
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
    let Some(decision) = ProviderContextCompactionRequest::new(
        run_state,
        trigger.clone(),
        compaction_config,
        summary_decision,
    )
    .plan(redactor) else {
        return Ok(None);
    };
    let trigger = decision.trigger;
    let checkpoint = decision.checkpoint;
    let checkpoint_id = checkpoint.metadata.checkpoint_id.clone();
    let updated_context = decision.updated_context;
    let tokens_before_estimate = decision.tokens_before_estimate;
    let updated_tokens = decision.tokens_after_estimate;

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
            summary_source: checkpoint.summary_source.clone(),
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

fn agent_actor(agent_id: &str) -> EventActor {
    EventActor::new(ActorKind::Worker, Some(agent_id.to_string()))
}
