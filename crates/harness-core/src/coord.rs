// allow: SIZE_OK — coordinator module (scheduling + event append + lifecycle)
use crate::UnwrapOrAbort;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::agent::{
    build_provider_context_messages, build_provider_tool_defs_for_model,
    default_model_settings_for_profile, default_provider, stream_assistant_response_once,
    tool_result_to_message_content, AgentModelRef, AgentModelSettings, AgentProfile, AgentRequest,
    AgentRuntimeEvent, AgentTurnFailure, AgentTurnOutcome, AssistantResponse, AssistantToolIntent,
    ProviderBoundaryContext, ProviderContext, ProviderConversationTurn,
    ProviderConversationTurnStatus, StreamAssistantResponseOnceRequest, MAX_TOOL_CALLS_TOTAL,
};
use crate::clock::Clock;
use crate::config::{
    registered_hook_runtime_config, CompactionRuntimeConfig, FormatterConfig, HookLifecycleEvent,
    HookRuntimeConfig, LifecycleHookConfig, ProviderRetryRuntimeConfig, ShellAllowlist,
    ToolFailureMode,
};
use crate::conversation::{
    ConversationAssistantMessage, ConversationMessage, ConversationToolCall,
    ConversationToolResultMessage, ConversationUserMessage,
};
use crate::counter_id::parse_prefixed_counter;
use crate::digest::digest12;
use crate::event::{
    ActorKind, AgentSpawnedEvent, BackgroundTaskNotificationStatus, CompactionAppliedEvent,
    CompactionRequestedEvent, CompactionWrittenEvent, EventActor, EventBuildError, EventEnvelopeV1,
    EventV1, HookExecutionMetadata, PermissionDecision as EventPermissionDecision,
    PolicyViolationDetectedEvent, ProviderAssistantMessageMetadata, ProviderReasoningDeltaEvent,
    ProviderRequestFinishedMetadata, ProviderRequestRetryMetadata, ProviderRequestStartedMetadata,
    RunFailedEvent, RunFinishedEvent, RunStartedEvent, SessionTitleUpdatedEvent,
    StaleDetectedEvent, TaskCancelledEvent, TaskCompletedEvent, TaskCompletionMetadata,
    TaskLineageMetadata, TaskResultLateEvent, TaskScheduleState, TaskScheduledEvent,
    TaskTerminalScope, ToolCallMetadata, ToolCallStatus, ToolIdentityMetadata,
    UserMessageSubmittedEvent,
};
use crate::perm::{
    permission_kind_for_tool_call, PermissionDecision, PermissionGrant, PermissionGrantRequest,
    PermissionGrantScope, PermissionGrantSet, PermissionKind, PermissionPolicy, PolicyDecision,
};
use crate::proj::{
    inspect_resume_plan, project_background_request, resolve_background_request_ref,
    BackgroundRequestProjection, RecordedRuntimeContext, RunMetadata, SessionModeSource,
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
use crate::store::{EventStore, EventStoreError, JsonlFileEventStore};
use crate::text::{non_empty_trimmed, truncate_with_ellipsis};
use crate::tool::{ToolContext, ToolRegistry, ToolResult, ToolRunState};
use harness_providers::{
    AssistantToolCall, CompletionMessage, CompletionRequest, MessageRole, Provider,
    ProviderErrorCategory, ProviderStreamEvent, ToolDef,
};

mod agent_turn_completion;
mod agent_turn_phases;
mod agent_turn_runtime;
mod background_notifications;
mod child_session;
mod command_loop;
mod event_helpers;
mod formatter;
mod handle;
mod hooks;

pub use formatter::{
    formatter_status, run_formatter_for_path, FormatterStatus, RealFormatterDiscovery,
};
mod permission;
mod provider_context;
mod provider_lifecycle;
mod question;
mod revert;
mod run_lifecycle;
mod snapshot;
mod state;
mod task_category;
mod task_lifecycle;
mod tool_execution;
mod tool_metadata;

#[cfg(test)]
pub(in crate::coord) use self::agent_turn_completion::compact_provider_context;
pub(in crate::coord) use self::agent_turn_completion::{
    CompactAgentContextResult, FailedTerminalCompactionRequest,
};
#[cfg(test)]
use self::agent_turn_phases::{
    completion_messages_to_conversation_messages, provider_tool_message_status,
};
use self::agent_turn_phases::{
    generate_harness_session_title, run_agent_turn_phase_loop, AgentTurnPhaseLoopRequest,
};
use self::agent_turn_runtime::{
    append_agent_turn_task_scheduled_event, schedule_agent_turn, start_agent_turn_execution,
    AgentTurnTaskScheduledEventArgs, ScheduleAgentTurnArgs,
};
use self::background_notifications::{
    append_background_task_notification_and_schedule,
    background_notification_status_for_cancel_reason,
    background_projection_error_to_coordinator_error, background_terminal_event_matches_task,
    schedule_pending_agent_wakeups_for_idle_agent, terminal_event_summary,
};
use self::child_session::{
    create_child_session_mirror, finish_child_session_mirrors, mirror_event_to_child_session,
    restore_child_session_mirrors, ChildSessionMirror,
};
pub(in crate::coord) use self::event_helpers::{
    agent_actor, append_artifact_written_event, append_compaction_artifact_written_event,
    append_compaction_failed_event, append_edit_applied_event, append_edit_proposed_event,
    append_edit_rejected_event, append_failed_tool_call_finished_event, append_payload_event,
    append_payload_event_with_correlation, append_permission_grant_recorded_event,
    append_permission_requested_event, append_permission_resolved_event,
    append_tool_call_finished_event, append_tool_call_requested_event,
    append_tool_call_started_event, system_actor,
};
pub use self::handle::CoordinatorHandle;

use self::permission::{
    evaluate_permission_rule_requests, event_permission_decision, permission_grant_request,
    permission_request_digest, permission_rule_request_selectors, permission_summary,
    plan_mode_edit_boundary_denial, plan_mode_shell_boundary_denial,
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
use self::question::QuestionPromptSpec;
use self::run_lifecycle::write_run_metadata;

#[cfg(test)]
use self::state::RunningAgentTurn;
use self::state::{
    agent_turn_child_lineage, cancelled_failure_memory_from_running, push_incomplete_provider_turn,
    AgentProviderRequestFinishedArgs, AgentProviderRequestStartedArgs, ChildTaskTurnState,
    EditAppliedEventArgs, HookExecutionBatch, HookInvocationContext, PendingAgentWakeup,
    PendingPermissionResolution, PendingPermissionState, PermissionDeniedArgs,
    PermissionRequestedEventArgs, QueuedAgentTurn, RunState, TaskExecutionState, TaskHookState,
    TaskState, ToolCallExecutionArgs, ToolCallFinishedEventArgs, ToolCallRequestedEventArgs,
};
pub use self::state::{AgentTurnFailureMemory, AgentTurnTaskOutcome};
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
    pub provider_retry: ProviderRetryRuntimeConfig,
    pub formatter: FormatterConfig,
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
            provider_retry: ProviderRetryRuntimeConfig::default(),
            formatter: FormatterConfig::default(),
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
    pub run_id: crate::ids::RunId,
    pub run_name: crate::ids::RunName,
    pub workspace_root: PathBuf,
    pub run_dir: PathBuf,
    pub artifacts_dir: PathBuf,
    pub events_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSnapshotSummary {
    pub request_id: crate::ids::RequestId,
    pub artifact_path: String,
    pub file_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRevertSummary {
    pub request_id: crate::ids::RequestId,
    pub restored_paths: Vec<String>,
    pub removed_paths: Vec<String>,
    pub failed_paths: Vec<(String, String)>,
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
    GetRunInfo {
        respond_to: oneshot::Sender<Result<RunInfo, CoordinatorError>>,
    },
    UpdateSessionTitle {
        title: String,
        respond_to: oneshot::Sender<Result<RunInfo, CoordinatorError>>,
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
    BackgroundForegroundChildTasks {
        respond_to: oneshot::Sender<Result<usize, CoordinatorError>>,
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
    SnapshotWorkspace {
        request_id: String,
        respond_to: oneshot::Sender<Result<WorkspaceSnapshotSummary, CoordinatorError>>,
    },
    RevertWorkspace {
        snapshot_request_id: String,
        respond_to: oneshot::Sender<Result<WorkspaceRevertSummary, CoordinatorError>>,
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
    pub parent_session_id: crate::ids::SessionId,
    pub parent_agent_id: Option<String>,
    pub child_session_id: crate::ids::SessionId,
    pub task_id: crate::ids::TaskId,
    pub description: String,
    pub run_in_background: bool,
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
    #[error("session title must not be empty")]
    InvalidSessionTitle,
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
    #[error("workspace snapshot failed: {0}")]
    SnapshotFailed(String),
    #[error("workspace revert failed: {0}")]
    RevertFailed(String),
    #[error("snapshot `{0}` not found")]
    SnapshotNotFound(String),
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
}

#[cfg(test)]
fn block_on_coordinator_future<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_abort()
        .block_on(future)
}

pub(in crate::coord) async fn finalize_permission_denied<C, R>(
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
            run_id: run_state.info.run_id.to_string(),
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

pub(in crate::coord) fn reject_pending_permission<C, R>(
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

pub(in crate::coord) fn tool_request_correlation_id(
    run_state: &RunState,
    actor: &EventActor,
) -> Option<String> {
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
