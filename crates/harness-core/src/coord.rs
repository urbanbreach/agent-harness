use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

use crate::agent::{
    default_model_settings_for_profile, default_provider, run_multi_turn_streaming,
    AgentModelSettings, AgentProfile, AgentRequest, AgentRuntimeEvent, AgentTurnOutcome,
    MultiTurnStreamingRequest, ProviderConversationTurn,
};
use crate::clock::Clock;
use crate::config::{
    registered_hook_runtime_config, registered_mcp_server_first_class_tool_id, HookLifecycleEvent,
    HookRuntimeConfig, LifecycleHookConfig, ShellAllowlist,
};
use crate::edit::hashline::HashlinePatch;
use crate::event::{
    ActorKind, AgentSpawnedEvent, ArtifactWrittenEvent, EditAppliedEvent, EditProposedEvent,
    EditRejectedEvent, EventActor, EventArtifactRef, EventBuildError, EventBuilder, EventContext,
    EventEnvelopeV1, EventV1, ExecutionTimingMetadata, HookExecutionMetadata, HookExecutionStatus,
    PermissionDecision as EventPermissionDecision, PermissionRequestedArgs,
    PermissionResolvedEvent, PolicyViolationDetectedEvent, ProviderReasoningDeltaEvent,
    RunFinishedEvent, RunStartedEvent, StaleDetectedEvent, TaskCancelledEvent, TaskCompletedEvent,
    TaskCompletionMetadata, TaskLineageMetadata, TaskResultLateEvent, TaskScheduleState,
    TaskScheduledEvent, ToolCallFinishedEvent, ToolCallMetadata, ToolCallStartedEvent,
    ToolCallStatus, ToolIdentityMetadata, UserMessageSubmittedEvent,
};
use crate::perm::{
    permission_kind_for_tool, permission_kind_for_tool_call, PermissionDecision, PermissionKind,
    PermissionPolicy, PolicyDecision,
};
use crate::proj::{inspect_resume_plan, RecordedRuntimeContext, RunMetadata};
use crate::redact::Redactor;
use crate::sched::{
    ConcurrencyKey, ScheduleDecision, Scheduler, SchedulerLimits, TaskProgressSnapshot,
};
use crate::store::{EventEnvelopeWithoutSeqV1, EventStore, EventStoreError, JsonlFileEventStore};
use crate::tool::{
    canonical_tool_id_for, sanitize_tool_function_name, ToolContext, ToolRegistry, ToolResult,
};
use harness_providers::Provider;

const DEFAULT_COMMAND_BUFFER: usize = 64;
const DEFAULT_TOOL_CONCURRENCY: usize = 1;
const DEFAULT_PROVIDER_MODEL_CONCURRENCY: usize = 1;
const DEFAULT_STALE_TIMEOUT_MS: u64 = 15_000;
const DEFAULT_WATCHDOG_TICK_MS: u64 = 100;
const DEFAULT_SIMULATED_JOB_DURATION_MS: u64 = 10;
const DEFAULT_QUESTION_TIMEOUT_MS: u64 = 300_000;
const COORDINATOR_AGENT_ID: &str = "coordinator";
const HASHLINE_APPLY_TOOL_ID: &str = "edit.hashline_apply";

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
    pub plan_profiles: BTreeMap<String, PlanProfileConfig>,
    pub hook_runtime_config: HookRuntimeConfig,
    pub config_digest: String,
    pub harness_version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanProfileConfig {
    pub plan_mode: bool,
    pub exit_target_profile: Option<String>,
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
            plan_profiles: BTreeMap::new(),
            hook_runtime_config: registered_hook_runtime_config(),
            config_digest: "none".to_string(),
            harness_version: env!("CARGO_PKG_VERSION").to_string(),
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
    SpawnAgent {
        actor: EventActor,
        profile: String,
        parent_agent_id: Option<String>,
        respond_to: oneshot::Sender<Result<String, CoordinatorError>>,
    },
    SpawnAgentIdle {
        actor: EventActor,
        profile: String,
        parent_agent_id: Option<String>,
        respond_to: oneshot::Sender<Result<String, CoordinatorError>>,
    },
    RequestAgentTurn {
        actor: EventActor,
        agent_id: String,
        prompt: String,
        model_ref_override: Option<String>,
        model_settings_override: Option<AgentModelSettings>,
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
    },
    AgentTurnFinished {
        task_id: String,
        agent_id: String,
        request_id: String,
        outcome: AgentTurnTaskOutcome,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentTurnTaskOutcome {
    Succeeded { output: String },
    Failed { reason: String },
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
    #[error("lifecycle hook failed: {0}")]
    LifecycleHookFailed(String),
}

#[derive(Debug, Clone)]
pub struct CoordinatorHandle {
    tx: mpsc::Sender<Command>,
}

impl CoordinatorHandle {
    pub async fn start_run(
        &self,
        run_name: impl Into<String>,
        workspace_root: impl Into<PathBuf>,
    ) -> Result<RunInfo, CoordinatorError> {
        let (respond_to, response_rx) = oneshot::channel();
        self.tx
            .send(Command::StartRun {
                run_name: run_name.into(),
                workspace_root: workspace_root.into(),
                respond_to,
            })
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed)?;

        response_rx
            .await
            .map_err(|_| CoordinatorError::ResponseChannelClosed)?
    }

    pub async fn resume_run(
        &self,
        run_id: impl Into<String>,
        run_name: impl Into<String>,
    ) -> Result<RunInfo, CoordinatorError> {
        let (respond_to, response_rx) = oneshot::channel();
        self.tx
            .send(Command::ResumeRun {
                run_id: run_id.into(),
                run_name: run_name.into(),
                respond_to,
            })
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed)?;

        response_rx
            .await
            .map_err(|_| CoordinatorError::ResponseChannelClosed)?
    }

    pub async fn stop_run(&self) -> Result<(), CoordinatorError> {
        let (respond_to, response_rx) = oneshot::channel();
        self.tx
            .send(Command::StopRun { respond_to })
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed)?;

        response_rx
            .await
            .map_err(|_| CoordinatorError::ResponseChannelClosed)?
    }

    pub async fn event_store(&self) -> Result<Arc<dyn EventStore>, CoordinatorError> {
        let (respond_to, response_rx) = oneshot::channel();
        self.tx
            .send(Command::GetEventStore { respond_to })
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed)?;

        let store = response_rx
            .await
            .map_err(|_| CoordinatorError::ResponseChannelClosed)??;
        let store: Arc<dyn EventStore> = store;
        Ok(store)
    }

    pub async fn spawn_agent(
        &self,
        actor: EventActor,
        profile: impl Into<String>,
        parent_agent_id: Option<String>,
    ) -> Result<String, CoordinatorError> {
        let (respond_to, response_rx) = oneshot::channel();
        self.tx
            .send(Command::SpawnAgent {
                actor,
                profile: profile.into(),
                parent_agent_id,
                respond_to,
            })
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed)?;

        response_rx
            .await
            .map_err(|_| CoordinatorError::ResponseChannelClosed)?
    }

    pub async fn spawn_agent_idle(
        &self,
        actor: EventActor,
        profile: impl Into<String>,
        parent_agent_id: Option<String>,
    ) -> Result<String, CoordinatorError> {
        let (respond_to, response_rx) = oneshot::channel();
        self.tx
            .send(Command::SpawnAgentIdle {
                actor,
                profile: profile.into(),
                parent_agent_id,
                respond_to,
            })
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed)?;

        response_rx
            .await
            .map_err(|_| CoordinatorError::ResponseChannelClosed)?
    }

    pub async fn request_tool_call(
        &self,
        actor: EventActor,
        category: Option<String>,
        tool_id: impl Into<String>,
        args_json: Value,
    ) -> Result<String, CoordinatorError> {
        let (respond_to, response_rx) = oneshot::channel();
        self.tx
            .send(Command::RequestToolCall {
                actor,
                category,
                tool_id: tool_id.into(),
                args_json,
                respond_to,
            })
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed)?;

        response_rx
            .await
            .map_err(|_| CoordinatorError::ResponseChannelClosed)?
    }

    pub async fn execute_agent_tool_call(
        &self,
        actor: EventActor,
        category: Option<String>,
        tool_id: impl Into<String>,
        args_json: Value,
    ) -> Result<ToolResult, String> {
        let (respond_to, response_rx) = oneshot::channel();
        self.tx
            .send(Command::ExecuteAgentToolCall {
                actor,
                category,
                tool_id: tool_id.into(),
                args_json,
                respond_to,
            })
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed.to_string())?;

        response_rx
            .await
            .map_err(|_| CoordinatorError::ResponseChannelClosed.to_string())?
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
        let (respond_to, response_rx) = oneshot::channel();
        self.tx
            .send(Command::RequestAgentTurn {
                actor,
                agent_id: agent_id.into(),
                prompt: prompt.into(),
                model_ref_override,
                model_settings_override,
                respond_to,
            })
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed)?;

        response_rx
            .await
            .map_err(|_| CoordinatorError::ResponseChannelClosed)?
    }

    pub async fn resolve_permission(
        &self,
        permission_id: impl Into<String>,
        decision: PermissionDecision,
        reason: Option<String>,
    ) -> Result<(), CoordinatorError> {
        let (respond_to, response_rx) = oneshot::channel();
        self.tx
            .send(Command::ResolvePermission {
                permission_id: permission_id.into(),
                decision,
                reason,
                respond_to,
            })
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed)?;

        response_rx
            .await
            .map_err(|_| CoordinatorError::ResponseChannelClosed)?
    }

    pub async fn request_question(
        &self,
        actor: EventActor,
        tool_call_id: impl Into<String>,
        request_json: Value,
    ) -> Result<Vec<Vec<String>>, String> {
        let (respond_to, response_rx) = oneshot::channel();
        self.tx
            .send(Command::RequestQuestion {
                actor,
                tool_call_id: tool_call_id.into(),
                request_json,
                respond_to,
            })
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed.to_string())?;

        response_rx
            .await
            .map_err(|_| CoordinatorError::ResponseChannelClosed.to_string())?
    }

    pub async fn job_progress(
        &self,
        task_id: impl Into<String>,
        kind: JobProgressKind,
    ) -> Result<(), CoordinatorError> {
        self.tx
            .send(Command::JobProgress {
                task_id: task_id.into(),
                kind,
            })
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed)
    }

    pub async fn cancel_task(
        &self,
        task_id: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<(), CoordinatorError> {
        let (respond_to, response_rx) = oneshot::channel();
        self.tx
            .send(Command::CancelTask {
                task_id: task_id.into(),
                reason: reason.into(),
                respond_to,
            })
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed)?;

        response_rx
            .await
            .map_err(|_| CoordinatorError::ResponseChannelClosed)?
    }

    pub async fn job_finished(
        &self,
        task_id: impl Into<String>,
        outcome: JobOutcome,
    ) -> Result<(), CoordinatorError> {
        self.tx
            .send(Command::JobFinished {
                task_id: task_id.into(),
                outcome,
            })
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed)
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
            Command::SpawnAgent {
                actor,
                profile,
                parent_agent_id,
                respond_to,
            } => {
                let result = self
                    .spawn_agent_internal(actor, profile, parent_agent_id, true)
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "spawn_agent");
            }
            Command::SpawnAgentIdle {
                actor,
                profile,
                parent_agent_id,
                respond_to,
            } => {
                let result = self
                    .spawn_agent_internal(actor, profile, parent_agent_id, false)
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "spawn_agent_idle");
            }
            Command::RequestAgentTurn {
                actor,
                agent_id,
                prompt,
                model_ref_override,
                model_settings_override,
                respond_to,
            } => {
                let result = self
                    .request_agent_turn_internal(
                        actor,
                        agent_id,
                        prompt,
                        model_ref_override,
                        model_settings_override,
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
                respond_to,
            } => {
                let result = self
                    .resolve_permission_internal(permission_id, decision, reason)
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
                let result = self.cancel_task_internal(task_id, reason);
                warn_oneshot_send_failure(respond_to.send(result), "cancel_task");
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
            } => {
                let _ = self
                    .agent_provider_request_finished_internal(
                        task_id,
                        agent_id,
                        request_id,
                        finish_reason,
                        output_digest,
                        usage,
                    )
                    .await;
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
        let artifacts_dir = run_dir.join("artifacts");
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

        let mut run_state = RunState {
            info: run_info.clone(),
            event_store,
            next_event_seq: 1,
            next_agent_id: 1,
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
            pending_permissions: BTreeMap::new(),
            cancelled_running_tasks: BTreeSet::new(),
            queued_agent_turns: BTreeMap::new(),
            running_agent_turns: BTreeMap::new(),
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
            .map(str::trim)
            .filter(|value| !value.is_empty())
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
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);

            if let Some(parent_agent_id) = parent_agent_id.as_ref() {
                restored_subagent_parent_by_id.insert(agent_id.clone(), parent_agent_id.clone());
            }

            agents.insert(agent_id.clone(), profile_cfg);
            restored_agent_bindings.push((agent_id.clone(), profile_name.clone(), parent_agent_id));
        }

        let provider_context_by_agent =
            restore_provider_context_from_history(&self.config.session_dir, &run_id)?;

        let next_agent_id = checked_next_counter(max_agent_id, &run_id, "agent id")?;
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

        let artifacts_dir = run_dir.join("artifacts");
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
            pending_permissions: BTreeMap::new(),
            cancelled_running_tasks: BTreeSet::new(),
            queued_agent_turns: BTreeMap::new(),
            running_agent_turns: BTreeMap::new(),
            scheduler: Scheduler::new(SchedulerLimits {
                provider_model: self.config.provider_model_concurrency,
                tool: self.config.tool_concurrency,
            }),
            recorded_runtime_context: None,
            allow_initial_runtime_context_recording: false,
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

        let profile_cfg = self
            .config
            .agent_profiles
            .get(&profile)
            .cloned()
            .unwrap_or_else(|| AgentProfile::fallback(profile.clone()));
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
            let request_id = format!("req_{:06}", run_state.next_provider_request_id);
            run_state.next_provider_request_id += 1;

            let request = AgentRequest {
                agent_id: agent_id.clone(),
                prompt: if profile_cfg.system_prompt.is_empty() {
                    format!("execute one-shot turn for {}", profile_cfg.name)
                } else {
                    profile_cfg.system_prompt.clone()
                },
                model_ref: profile_cfg.model_ref.clone(),
                model_settings: default_model_settings_for_profile(&profile_cfg.name),
            };

            schedule_agent_turn(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                self.job_tx.clone(),
                run_state,
                self.config.hook_runtime_config.clone(),
                ScheduleAgentTurnArgs {
                    provider: self.config.provider.clone(),
                    tool_registry: self.config.tool_registry.clone(),
                    profile: profile_cfg,
                    request,
                    request_id,
                },
            )
            .await?;
        }

        Ok(agent_id)
    }

    async fn request_agent_turn_internal(
        &mut self,
        actor: EventActor,
        agent_id: String,
        prompt: String,
        model_ref_override: Option<String>,
        model_settings_override: Option<AgentModelSettings>,
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

        let request_id = format!("req_{:06}", run_state.next_provider_request_id);
        run_state.next_provider_request_id += 1;

        let request = AgentRequest {
            agent_id,
            prompt,
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

        schedule_agent_turn(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            self.job_tx.clone(),
            run_state,
            self.config.hook_runtime_config.clone(),
            ScheduleAgentTurnArgs {
                provider: self.config.provider.clone(),
                tool_registry: self.config.tool_registry.clone(),
                profile,
                request,
                request_id: request_id.clone(),
            },
        )
        .await?;

        Ok(request_id)
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
        let (plan_mode, plan_exit_target_profile) = resolve_plan_profile_context(
            effective_category.as_deref(),
            &self.config.plan_profiles,
            &self.config.agent_profiles,
        );

        let skip_outer_question_permission = canonical_tool_id_for(&tool_id) == Some("question");
        let maybe_kind = if skip_outer_question_permission {
            None
        } else {
            permission_kind_for_tool_call(&tool_id, capability)
        };
        let decision = maybe_kind.map(|kind| {
            self.config
                .permission_policy
                .evaluate(effective_category.as_deref(), kind)
        });
        let hashline_edit = hashline_edit_metadata(&tool_id, &args_json, &tool_call_id);

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
                    resolution: PendingPermissionResolution::ToolCall {
                        tool_id,
                        args_json,
                        actor,
                        category: effective_category.clone(),
                        plan_mode,
                        plan_exit_target_profile,
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

                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
                    let _ = job_tx
                        .send(Command::PermissionTimedOut { permission_id })
                        .await;
                });
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
                        plan_mode,
                        plan_exit_target_profile,
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
                permission_id: Some(permission_id),
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
                resolution:
                    PendingPermissionResolution::ToolCall {
                        tool_id,
                        args_json,
                        actor,
                        category,
                        plan_mode,
                        plan_exit_target_profile,
                        respond_to,
                    },
                ..
            } => {
                if decision == PermissionDecision::Allow && permission_hook_failure.is_none() {
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
                            plan_mode,
                            plan_exit_target_profile,
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
                            resolution: PendingPermissionResolution::ToolCall {
                                tool_id,
                                args_json,
                                actor,
                                category,
                                plan_mode,
                                plan_exit_target_profile,
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
                resolution:
                    PendingPermissionResolution::ToolCall {
                        tool_id,
                        args_json,
                        actor,
                        category,
                        plan_mode,
                        plan_exit_target_profile,
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
                        resolution: PendingPermissionResolution::ToolCall {
                            tool_id,
                            args_json,
                            actor,
                            category,
                            plan_mode,
                            plan_exit_target_profile,
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
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
                let _ = job_tx
                    .send(Command::PermissionTimedOut { permission_id })
                    .await;
            });

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

    fn cancel_task_internal(
        &mut self,
        task_id: String,
        reason: String,
    ) -> Result<(), CoordinatorError> {
        let Some(run_state) = self.run_state.as_mut() else {
            return Err(CoordinatorError::RunNotStarted);
        };

        if let Some(queued) = run_state.queued_agent_turns.remove(&task_id) {
            let _ = run_state.scheduler.cancel_queued(&task_id);
            append_payload_event_with_correlation(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                agent_actor(&queued.agent_id),
                Some(format!("task:{task_id}")),
                Some(queued.request_id),
                EventV1::TaskCancelled(TaskCancelledEvent { task_id, reason }),
            )?;
            return Ok(());
        }

        if let Some(running) = run_state.running_agent_turns.get(&task_id) {
            running.cancellation_token.cancel();
            run_state.cancelled_running_tasks.insert(task_id.clone());
            append_payload_event_with_correlation(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                agent_actor(&running.agent_id),
                Some(format!("task:{task_id}")),
                Some(running.request_id.clone()),
                EventV1::TaskCancelled(TaskCancelledEvent { task_id, reason }),
            )?;
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
            EventV1::TaskCancelled(TaskCancelledEvent { task_id, reason }),
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
        } = args;
        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(());
        };

        let Some(running) = run_state.running_agent_turns.get(&task_id) else {
            return Ok(());
        };
        let category = running.category.clone();
        let cancellation_token = running.cancellation_token.clone();
        let parent_agent_id = run_state.subagent_parent_by_id.get(&agent_id).cloned();

        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            agent_actor(&agent_id),
            Some(format!("agent:{agent_id}")),
            Some(request_id.clone()),
            EventV1::ProviderRequestStarted(crate::event::ProviderRequestStartedEvent {
                request_id: request_id.clone(),
                provider_id: provider_id.clone(),
                model_id: model_id.clone(),
                prompt_summary: prompt_summary.clone(),
                request_digest,
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
                request_id: Some(request_id.clone()),
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
                    Some(request_id),
                    EventV1::TaskCancelled(TaskCancelledEvent { task_id, reason }),
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

        if !run_state.running_agent_turns.contains_key(&task_id) {
            return Ok(());
        }

        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            agent_actor(&agent_id),
            Some(format!("agent:{agent_id}")),
            Some(request_id.clone()),
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

        if !run_state.running_agent_turns.contains_key(&task_id) {
            return Ok(());
        }

        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            agent_actor(&agent_id),
            Some(format!("agent:{agent_id}")),
            Some(request_id.clone()),
            EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent { request_id, delta }),
        )?;

        Ok(())
    }

    async fn agent_provider_request_finished_internal(
        &mut self,
        task_id: String,
        agent_id: String,
        request_id: String,
        finish_reason: String,
        output_digest: Option<String>,
        usage: Option<harness_providers::CompletionUsage>,
    ) -> Result<(), CoordinatorError> {
        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(());
        };

        let Some(running) = run_state.running_agent_turns.get(&task_id) else {
            return Ok(());
        };
        let category = running.category.clone();
        let cancellation_token = running.cancellation_token.clone();
        let parent_agent_id = run_state.subagent_parent_by_id.get(&agent_id).cloned();

        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            agent_actor(&agent_id),
            Some(format!("agent:{agent_id}")),
            Some(request_id.clone()),
            EventV1::ProviderRequestFinished(crate::event::ProviderRequestFinishedEvent {
                request_id: request_id.clone(),
                finish_reason: finish_reason.clone(),
                output_digest: output_digest.clone(),
                usage,
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
                request_id: Some(request_id.clone()),
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
                    Some(request_id),
                    EventV1::TaskCancelled(TaskCancelledEvent { task_id, reason }),
                )?;
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
        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(());
        };

        let Some(running) = run_state.running_agent_turns.remove(&task_id) else {
            return Ok(());
        };

        let was_cancelled = run_state.cancelled_running_tasks.remove(&task_id);
        let dequeued = run_state.scheduler.complete(&running.queue_key);
        let finished_mono_ms = self.clock.mono_ms();
        let subagent_parent_id = run_state
            .subagent_parent_by_id
            .get(&running.agent_id)
            .cloned();
        let (hook_outcome, hook_output_summary, hook_failure_reason) = match &outcome {
            AgentTurnTaskOutcome::Succeeded { output } => {
                ("succeeded".to_string(), Some(output.clone()), None)
            }
            AgentTurnTaskOutcome::Failed { reason } => {
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

        if !was_cancelled {
            match outcome {
                AgentTurnTaskOutcome::Succeeded { output } => {
                    run_state
                        .provider_context_by_agent
                        .entry(running.agent_id.clone())
                        .or_default()
                        .push(ProviderConversationTurn {
                            user_prompt: running.request_prompt,
                            assistant_response: output.clone(),
                        });
                    if let Some(reason) = critical_hook_failure.clone() {
                        append_payload_event_with_correlation(
                            self.clock.as_ref(),
                            self.redactor.as_ref(),
                            run_state,
                            agent_actor(&running.agent_id),
                            Some(format!("task:{task_id}")),
                            Some(request_id),
                            EventV1::TaskCancelled(TaskCancelledEvent { task_id, reason }),
                        )?;
                    } else {
                        append_payload_event_with_correlation(
                            self.clock.as_ref(),
                            self.redactor.as_ref(),
                            run_state,
                            agent_actor(&running.agent_id),
                            Some(format!("task:{task_id}")),
                            Some(request_id),
                            EventV1::TaskCompleted(TaskCompletedEvent {
                                task_id,
                                result_digest: digest12(output.as_bytes()),
                                result_summary: output,
                                metadata: Some(TaskCompletionMetadata {
                                    lineage: None,
                                    timing: Some(execution_timing_metadata(
                                        running.started_mono_ms,
                                        finished_mono_ms,
                                    )),
                                    hook_executions,
                                }),
                            }),
                        )?;
                    }
                }
                AgentTurnTaskOutcome::Failed { reason } => {
                    let reason = match critical_hook_failure.clone() {
                        Some(hook_reason) => {
                            format!("{reason}; critical lifecycle hook failed: {hook_reason}")
                        }
                        None => reason,
                    };
                    append_payload_event_with_correlation(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        agent_actor(&running.agent_id),
                        Some(format!("task:{task_id}")),
                        Some(request_id),
                        EventV1::TaskCancelled(TaskCancelledEvent { task_id, reason }),
                    )?;
                }
            }
        }

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
                    self.config.provider.clone(),
                    self.config.tool_registry.clone(),
                    queued,
                )
                .await?;
            }
        }

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
    provider_context_by_agent: BTreeMap<String, Vec<ProviderConversationTurn>>,
    tasks: BTreeMap<String, TaskState>,
    task_hook_state: BTreeMap<String, TaskHookState>,
    agent_hook_state: BTreeMap<String, Vec<HookExecutionMetadata>>,
    subagent_parent_by_id: BTreeMap<String, String>,
    pending_permissions: BTreeMap<String, PendingPermissionState>,
    cancelled_running_tasks: BTreeSet<String>,
    queued_agent_turns: BTreeMap<String, QueuedAgentTurn>,
    running_agent_turns: BTreeMap<String, RunningAgentTurn>,
    scheduler: Scheduler,
    recorded_runtime_context: Option<RecordedRuntimeContext>,
    allow_initial_runtime_context_recording: bool,
    shutdown_token: CancellationToken,
}

#[derive(Debug, Clone)]
struct QueuedAgentTurn {
    task_id: String,
    agent_id: String,
    request_id: String,
    profile: AgentProfile,
    request: AgentRequest,
    prior_turns: Vec<ProviderConversationTurn>,
    queue_key: ConcurrencyKey,
}

#[derive(Debug, Clone)]
struct RunningAgentTurn {
    agent_id: String,
    request_id: String,
    request_prompt: String,
    category: Option<String>,
    queue_key: ConcurrencyKey,
    cancellation_token: CancellationToken,
    started_mono_ms: u64,
    hook_executions: Vec<HookExecutionMetadata>,
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
    resolution: PendingPermissionResolution,
}

enum PendingPermissionResolution {
    ToolCall {
        tool_id: String,
        args_json: Value,
        actor: EventActor,
        category: Option<String>,
        plan_mode: bool,
        plan_exit_target_profile: Option<String>,
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
}

struct ToolCallExecutionArgs {
    tool_call_id: String,
    tool_id: String,
    args_json: Value,
    actor: EventActor,
    category: Option<String>,
    hook_executions: Vec<HookExecutionMetadata>,
    plan_mode: bool,
    plan_exit_target_profile: Option<String>,
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

struct ScheduleAgentTurnArgs {
    provider: Arc<dyn Provider>,
    tool_registry: Arc<ToolRegistry>,
    profile: AgentProfile,
    request: AgentRequest,
    request_id: String,
}

fn resolve_plan_profile_context(
    category: Option<&str>,
    plan_profiles: &BTreeMap<String, PlanProfileConfig>,
    agent_profiles: &BTreeMap<String, AgentProfile>,
) -> (bool, Option<String>) {
    let Some(category) = category else {
        return (false, None);
    };
    let Some(profile) = plan_profiles.get(category) else {
        return (false, None);
    };
    if !profile.plan_mode {
        return (false, None);
    }
    if let Some(target) = profile
        .exit_target_profile
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let target = target.to_string();
        let resolved = agent_profiles.contains_key(&target).then_some(target);
        return (true, resolved);
    }
    let fallback = agent_profiles
        .contains_key("build")
        .then(|| "build".to_string());
    (true, fallback)
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

        if runtime.suppress_execution {
            batch.hook_executions.push(HookExecutionMetadata {
                hook_name: hook_identifier(hook, index),
                status: HookExecutionStatus::Skipped,
                hook_event: Some(context.event.as_str().to_string()),
                command_digest: Some(digest12(hook.command.join("\u{0}").as_bytes())),
                output_digest: None,
                output_summary: Some("suppressed during deterministic execution".to_string()),
                duration_ms: Some(0),
            });
            continue;
        }

        let (metadata, failure) =
            execute_lifecycle_hook(clock, runtime, hook, index, &context).await;
        batch.hook_executions.push(metadata);
        if hook.critical {
            if let Some(failure) = failure {
                batch.critical_failure = Some(failure);
                break;
            }
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
        Ok((output_digest, output_summary)) => (
            HookExecutionMetadata {
                hook_name,
                status: HookExecutionStatus::Succeeded,
                hook_event: Some(context.event.as_str().to_string()),
                command_digest: Some(command_digest),
                output_digest: Some(output_digest),
                output_summary: Some(output_summary),
                duration_ms: Some(finished_mono_ms.saturating_sub(started_mono_ms)),
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
                    command_digest: Some(command_digest),
                    output_digest: Some(digest12(reason.as_bytes())),
                    output_summary: Some(output_summary),
                    duration_ms: Some(finished_mono_ms.saturating_sub(started_mono_ms)),
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
) -> Result<(String, String), (String, String)> {
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

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let output_summary = summarize_hook_output(&stdout, &stderr);
    let output_digest =
        digest12(format!("{}\u{0}{}\u{0}{:?}", stdout, stderr, output.status).as_bytes());

    if output.status.success() {
        Ok((output_digest, output_summary))
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
    let combined = if stderr.trim().is_empty() {
        stdout.trim()
    } else if stdout.trim().is_empty() {
        stderr.trim()
    } else {
        "stdout/stderr captured"
    };

    let mut summary = combined.chars().take(160).collect::<String>();
    if combined.chars().count() > 160 {
        summary.push('…');
    }
    if summary.is_empty() {
        "no output".to_string()
    } else {
        summary
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
        plan_mode,
        plan_exit_target_profile,
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
            plan_mode,
            plan_exit_target_profile,
            tool_call_id: tool_call_id.clone(),
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

async fn schedule_agent_turn<C, R>(
    clock: &C,
    redactor: &R,
    job_tx: mpsc::Sender<Command>,
    run_state: &mut RunState,
    hook_runtime_config: HookRuntimeConfig,
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
    } = args;
    let model = crate::agent::AgentModelRef::parse(&request.model_ref);
    let agent_id = request.agent_id.clone();
    let prior_turns = run_state
        .provider_context_by_agent
        .get(&agent_id)
        .cloned()
        .unwrap_or_default();
    let task_id = format!("task_{:06}", run_state.next_task_id);
    run_state.next_task_id += 1;

    let queue_key = ConcurrencyKey::ProviderModel {
        provider_id: model.provider_id,
        model_id: model.model_id,
    };

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
                provider,
                tool_registry,
                QueuedAgentTurn {
                    task_id,
                    agent_id,
                    request_id,
                    profile,
                    request,
                    prior_turns,
                    queue_key,
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
                    prior_turns,
                    queue_key,
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
    provider: Arc<dyn Provider>,
    tool_registry: Arc<ToolRegistry>,
    task: QueuedAgentTurn,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let cancellation_token = run_state.shutdown_token.child_token();

    let category = Some(task.profile.category.clone());
    let mut hook_executions = run_state
        .agent_hook_state
        .remove(&task.agent_id)
        .unwrap_or_default();
    let started_hook_batch = run_lifecycle_hooks(
        clock,
        &hook_runtime_config,
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
            category: category.clone(),
            queue_key: task.queue_key.clone(),
            cancellation_token: cancellation_token.clone(),
            started_mono_ms: clock.mono_ms(),
            hook_executions,
        },
    );

    if let Some(reason) = started_hook_batch.critical_failure {
        warn_command_send_failure(
            job_tx
                .send(Command::AgentTurnFinished {
                    task_id: task.task_id,
                    agent_id: task.agent_id,
                    request_id: task.request_id,
                    outcome: AgentTurnTaskOutcome::Failed { reason },
                })
                .await,
            "agent_turn_finished_from_hook_failure",
        );
        return Ok(());
    }

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
                    },
                }).await, "agent_turn_finished_from_cancellation");
            }
            outcome = run_multi_turn_streaming(
                MultiTurnStreamingRequest {
                    provider,
                    tool_registry,
                    profile: &task.profile,
                    request_id: task.request_id.clone(),
                    request: task.request,
                    prior_turns: &task.prior_turns,
                },
                {
                    let job_tx = job_tx.clone();
                    let agent_id = task.agent_id.clone();
                    let category = Some(task.profile.category.clone());
                    move |tool_id, args_json| {
                        let job_tx = job_tx.clone();
                        let agent_id = agent_id.clone();
                        let category = category.clone();
                        async move {
                            let (respond_to, response_rx) = oneshot::channel();
                            job_tx
                                .send(Command::ExecuteAgentToolCall {
                                    actor: EventActor::new(ActorKind::Worker, Some(agent_id)),
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
                    }
                },
                |event| {
                    let job_tx = job_tx.clone();
                    let task_id = task.task_id.clone();
                    let agent_id = task.agent_id.clone();
                    async move {
                        match event {
                            AgentRuntimeEvent::ProviderRequestStarted(started) => {
                                warn_command_send_failure(job_tx.send(Command::AgentProviderRequestStarted {
                                    task_id,
                                    agent_id,
                                    request_id: started.request_id,
                                    provider_id: started.provider_id,
                                    model_id: started.model_id,
                                    prompt_summary: started.prompt_summary,
                                    request_digest: started.request_digest,
                                }).await, "agent_provider_request_started");
                            }
                            AgentRuntimeEvent::ProviderStreamDelta { request_id, delta } => {
                                warn_command_send_failure(job_tx.send(Command::AgentProviderStreamDelta {
                                    task_id,
                                    agent_id,
                                    request_id,
                                    delta,
                                }).await, "agent_provider_stream_delta");
                            }
                            AgentRuntimeEvent::ProviderReasoningDelta { request_id, delta } => {
                                warn_command_send_failure(job_tx.send(Command::AgentProviderReasoningDelta {
                                    task_id,
                                    agent_id,
                                    request_id,
                                    delta,
                                }).await, "agent_provider_reasoning_delta");
                            }
                            AgentRuntimeEvent::ProviderRequestFinished(finished) => {
                                warn_command_send_failure(job_tx.send(Command::AgentProviderRequestFinished {
                                    task_id,
                                    agent_id,
                                    request_id: finished.request_id,
                                    finish_reason: finished.finish_reason,
                                    output_digest: finished.output_digest,
                                    usage: finished.usage,
                                }).await, "agent_provider_request_finished");
                            }
                        }
                    }
                }
            ) => {
                let outcome = match outcome {
                    AgentTurnOutcome::Succeeded { output } => AgentTurnTaskOutcome::Succeeded { output },
                    AgentTurnOutcome::Failed { reason } => AgentTurnTaskOutcome::Failed { reason },
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
    let Some(reason) = reason.map(str::trim).filter(|reason| !reason.is_empty()) else {
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

fn validate_question_answers(
    prompts: &[QuestionPromptSpec],
    answers: Vec<Vec<String>>,
) -> Result<Vec<Vec<String>>, String> {
    if answers.len() != prompts.len() {
        return Err(format!(
            "Expected {} answer group(s) for {} question(s); received {}.",
            prompts.len(),
            prompts.len(),
            answers.len()
        ));
    }

    prompts
        .iter()
        .zip(answers)
        .enumerate()
        .map(|(index, (prompt, answers))| normalize_question_answers(index, prompt, answers))
        .collect()
}

fn normalize_question_answers(
    index: usize,
    prompt: &QuestionPromptSpec,
    answers: Vec<String>,
) -> Result<Vec<String>, String> {
    let answers = answers
        .into_iter()
        .map(|answer| answer.trim().to_string())
        .filter(|answer| !answer.is_empty())
        .collect::<Vec<_>>();
    if answers.is_empty() {
        return Ok(Vec::new());
    }

    if !prompt.multiple.unwrap_or(false) && answers.len() != 1 {
        return Err(format!(
            "Question {} ({}) accepts only one answer.",
            index + 1,
            prompt.header
        ));
    }

    Ok(answers
        .into_iter()
        .map(|answer| canonicalize_question_answer(prompt, answer))
        .collect())
}

fn canonicalize_question_answer(prompt: &QuestionPromptSpec, answer: String) -> String {
    prompt
        .options
        .iter()
        .find(|option| option.label.eq_ignore_ascii_case(&answer))
        .map(|option| option.label.clone())
        .unwrap_or(answer)
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
    };

    let meta_path = run_state.info.run_dir.join("meta.json");
    let body = serde_json::to_string_pretty(&metadata)?;
    fs::write(&meta_path, body).map_err(|source| CoordinatorError::WriteRunMetadata {
        path: meta_path.display().to_string(),
        source,
    })?;

    Ok(())
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

fn hashline_edit_metadata(
    tool_id: &str,
    args_json: &Value,
    tool_call_id: &str,
) -> Option<HashlineEditMetadata> {
    if tool_id != HASHLINE_APPLY_TOOL_ID {
        let canonical_tool_id = canonical_tool_id_for(tool_id)?;
        if canonical_tool_id != "write" && canonical_tool_id != "edit" {
            return None;
        }

        let path = args_json
            .get("path")
            .or_else(|| args_json.get("filePath"))
            .and_then(Value::as_str)?;
        let canonical_request = serde_json::to_vec(args_json).unwrap_or_else(|_| b"null".to_vec());

        let (edit_id, summary) = match canonical_tool_id {
            "write" => (
                format!("fs-write-{tool_call_id}"),
                "rewrite file through hashline workspace op".to_string(),
            ),
            "edit" => (
                format!("edit-{tool_call_id}"),
                "rewrite file through native edit tool".to_string(),
            ),
            _ => return None,
        };

        return Some(HashlineEditMetadata {
            edit_id,
            path: path.to_string(),
            summary,
            patch_digest: digest12(&canonical_request),
        });
    }

    let patch: HashlinePatch = serde_json::from_value(args_json.clone()).ok()?;
    let canonical_patch = serde_json::to_vec(&patch).unwrap_or_else(|_| b"null".to_vec());

    Some(HashlineEditMetadata {
        edit_id: patch.edit_id,
        path: patch.path,
        summary: format!("apply hashline patch with {} op(s)", patch.ops.len()),
        patch_digest: digest12(&canonical_patch),
    })
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
    tool_id: &str,
    result: &ToolResult,
    fallback: Option<&HashlineEditMetadata>,
) -> Vec<AppliedToolEditMetadata> {
    if canonical_tool_id_for(tool_id) == Some("apply_patch") {
        return result
            .structured_json
            .as_ref()
            .and_then(|value| value.get("edits"))
            .and_then(Value::as_array)
            .map(|edits| {
                edits
                    .iter()
                    .filter_map(|edit| {
                        let edit_id = edit.get("edit_id").and_then(Value::as_str)?.trim();
                        let path = edit.get("path").and_then(Value::as_str)?.trim();
                        if edit_id.is_empty() || path.is_empty() {
                            return None;
                        }
                        let summary = edit
                            .get("summary")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .unwrap_or("apply patch update");
                        Some(AppliedToolEditMetadata {
                            metadata: HashlineEditMetadata {
                                edit_id: edit_id.to_string(),
                                path: path.to_string(),
                                summary: summary.to_string(),
                                patch_digest: digest12(edit_id.as_bytes()),
                            },
                            diff_rel_path: edit
                                .get("diff_rel_path")
                                .and_then(Value::as_str)
                                .map(ToOwned::to_owned),
                            diff_digest: edit
                                .get("diff_digest")
                                .and_then(Value::as_str)
                                .map(ToOwned::to_owned),
                            deleted: edit
                                .get("deleted")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
    }

    let Some(metadata) = fallback else {
        return Vec::new();
    };
    let (diff_rel_path, diff_digest) = hashline_diff_refs(result);
    vec![AppliedToolEditMetadata {
        metadata: metadata.clone(),
        diff_rel_path,
        diff_digest,
        deleted: false,
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
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
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

fn sanitize_mcp_tool_segment(name: &str) -> String {
    sanitize_tool_function_name(name).replace('-', "_")
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
        parent_session_id: task.owner_actor.agent_id.clone(),
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
    })
}

fn extract_object_string(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = object
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
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
    let relative = Path::new(relative_path);
    if relative.is_absolute() {
        return Err("path must be relative to workspace root".to_string());
    }

    let canonical_workspace = workspace_root
        .canonicalize()
        .map_err(|err| format!("failed to resolve workspace root: {err}"))?;
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
    agent_id: Option<String>,
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

    let Some(prompt_summary) = prompt_summary
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
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
) -> Result<BTreeMap<String, Vec<ProviderConversationTurn>>, CoordinatorError> {
    let events_path = session_dir.join(run_id).join("events.jsonl");
    let file =
        fs::File::open(&events_path).map_err(|source| CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!(
                "failed to open historical events {}: {source}",
                events_path.display()
            ),
        })?;

    let mut histories: BTreeMap<String, Vec<ProviderConversationTurn>> = BTreeMap::new();
    let mut requests: BTreeMap<String, HistoricalRequestState> = BTreeMap::new();
    let mut expected_seq = 1_u64;

    for (line_number, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|source| CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!(
                "failed to read historical event line {} in {}: {source}",
                line_number + 1,
                events_path.display()
            ),
        })?;

        if line.trim().is_empty() {
            continue;
        }

        let event: EventEnvelopeV1 = serde_json::from_str(&line).map_err(|source| {
            CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: format!(
                    "invalid historical event line {} in {}: {source}",
                    line_number + 1,
                    events_path.display()
                ),
            }
        })?;

        if event.seq != expected_seq {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: format!(
                    "historical sequence mismatch at {}: expected {expected_seq}, got {}",
                    events_path.display(),
                    event.seq
                ),
            });
        }
        expected_seq = expected_seq.saturating_add(1);

        match &event.payload {
            EventV1::UserMessageSubmitted(payload) => {
                requests
                    .entry(payload.request_id.clone())
                    .or_default()
                    .user_text = Some(payload.text.clone());
            }
            EventV1::ProviderRequestStarted(payload) => {
                requests
                    .entry(payload.request_id.clone())
                    .or_default()
                    .prompt_summary = Some(payload.prompt_summary.clone());
                if let Some(agent_id) = event
                    .actor
                    .agent_id
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                {
                    requests
                        .entry(payload.request_id.clone())
                        .or_default()
                        .agent_id = Some(agent_id.to_string());
                }
            }
            EventV1::ProviderStreamDelta(payload) => {
                requests
                    .entry(payload.request_id.clone())
                    .or_default()
                    .assistant_output
                    .push_str(&payload.delta);
            }
            EventV1::TaskCompleted(payload) => {
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };

                let Some(agent_id) = event
                    .actor
                    .agent_id
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
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
                    request_state.user_text,
                    request_state.prompt_summary,
                )?;

                let assistant_response = if payload.result_summary.is_empty() {
                    request_state.assistant_output
                } else {
                    payload.result_summary.clone()
                };

                histories
                    .entry(request_state.agent_id.unwrap_or(agent_id))
                    .or_default()
                    .push(ProviderConversationTurn {
                        user_prompt,
                        assistant_response,
                    });
            }
            _ => {}
        }
    }

    Ok(histories)
}

fn parse_prefixed_counter(id: &str, expected_prefix: &str) -> Option<u64> {
    let tail = id.strip_prefix(expected_prefix)?;
    if tail.is_empty() {
        return None;
    }

    tail.parse::<u64>().ok()
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
    Ok(appended)
}

fn system_actor() -> EventActor {
    EventActor::new(ActorKind::System, Some(COORDINATOR_AGENT_ID.to_string()))
}

fn agent_actor(agent_id: &str) -> EventActor {
    EventActor::new(ActorKind::Worker, Some(agent_id.to_string()))
}

fn digest12(bytes: &[u8]) -> String {
    let digest = blake3::hash(bytes);
    digest.to_hex().chars().take(12).collect()
}
