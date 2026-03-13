use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

use crate::agent::{
    default_provider, run_multi_turn_streaming, AgentProfile, AgentRequest, AgentRuntimeEvent,
    AgentTurnOutcome, MultiTurnStreamingRequest, ProviderConversationTurn,
};
use crate::clock::Clock;
use crate::edit::hashline::HashlinePatch;
use crate::event::{
    ActorKind, AgentSpawnedEvent, ArtifactWrittenEvent, EditAppliedEvent, EditProposedEvent,
    EditRejectedEvent, EventActor, EventBuildError, EventBuilder, EventContext, EventEnvelopeV1,
    EventV1, PermissionDecision as EventPermissionDecision, PermissionResolvedEvent,
    PolicyViolationDetectedEvent, RunFinishedEvent, RunStartedEvent, StaleDetectedEvent,
    TaskCancelledEvent, TaskCompletedEvent, TaskResultLateEvent, TaskScheduleState,
    TaskScheduledEvent, ToolCallFinishedEvent, ToolCallStartedEvent, ToolCallStatus,
    UserMessageSubmittedEvent,
};
use crate::perm::{
    permission_kind_for_capability, PermissionDecision, PermissionKind, PermissionPolicy,
    PolicyDecision,
};
use crate::proj::inspect_resume_plan;
use crate::redact::Redactor;
use crate::sched::{
    ConcurrencyKey, ScheduleDecision, Scheduler, SchedulerLimits, TaskProgressSnapshot,
};
use crate::store::{EventEnvelopeWithoutSeqV1, EventStore, EventStoreError, JsonlFileEventStore};
use crate::tool::{ToolContext, ToolRegistry, ToolResult};
use harness_providers::Provider;

const DEFAULT_COMMAND_BUFFER: usize = 64;
const DEFAULT_TOOL_CONCURRENCY: usize = 1;
const DEFAULT_PROVIDER_MODEL_CONCURRENCY: usize = 1;
const DEFAULT_STALE_TIMEOUT_MS: u64 = 15_000;
const DEFAULT_WATCHDOG_TICK_MS: u64 = 100;
const DEFAULT_SIMULATED_JOB_DURATION_MS: u64 = 10;
const COORDINATOR_AGENT_ID: &str = "coordinator";
const HASHLINE_APPLY_TOOL_ID: &str = "edit.hashline_apply";

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
    pub config_digest: String,
    pub harness_version: String,
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
        respond_to: oneshot::Sender<Result<String, CoordinatorError>>,
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
    ResolvePermission {
        permission_id: String,
        decision: PermissionDecision,
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
}

#[derive(Clone)]
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

    pub async fn request_agent_turn(
        &self,
        actor: EventActor,
        agent_id: impl Into<String>,
        prompt: impl Into<String>,
    ) -> Result<String, CoordinatorError> {
        let (respond_to, response_rx) = oneshot::channel();
        self.tx
            .send(Command::RequestAgentTurn {
                actor,
                agent_id: agent_id.into(),
                prompt: prompt.into(),
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
    ) -> Result<(), CoordinatorError> {
        let (respond_to, response_rx) = oneshot::channel();
        self.tx
            .send(Command::ResolvePermission {
                permission_id: permission_id.into(),
                decision,
                respond_to,
            })
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed)?;

        response_rx
            .await
            .map_err(|_| CoordinatorError::ResponseChannelClosed)?
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
                    let _ =
                        self.stop_run_internal("coordinator command channel closed".to_string());
                } else {
                    break;
                }
            }

            tokio::select! {
                command = self.command_rx.recv(), if !command_channel_closed => {
                    match command {
                        Some(command) => self.handle_command(command),
                        None => command_channel_closed = true,
                    }
                }
                command = self.job_rx.recv() => {
                    if let Some(command) = command {
                        self.handle_command(command);
                    }
                }
                _ = watchdog.tick() => {
                    let _ = self.watchdog_tick_internal();
                }
            }
        }
    }

    fn handle_command(&mut self, command: Command) {
        match command {
            Command::StartRun {
                run_name,
                workspace_root,
                respond_to,
            } => {
                let result = self.start_run_internal(run_name, workspace_root);
                let _ = respond_to.send(result);
            }
            Command::ResumeRun {
                run_id,
                run_name,
                respond_to,
            } => {
                let result = self.resume_run_internal(run_id, run_name);
                let _ = respond_to.send(result);
            }
            Command::StopRun { respond_to } => {
                let result = self.stop_run_internal("run stopped".to_string());
                let _ = respond_to.send(result);
            }
            Command::GetEventStore { respond_to } => {
                let result = self.get_event_store_internal();
                let _ = respond_to.send(result);
            }
            Command::SpawnAgent {
                actor,
                profile,
                parent_agent_id,
                respond_to,
            } => {
                let result = self.spawn_agent_internal(actor, profile, parent_agent_id, true);
                let _ = respond_to.send(result);
            }
            Command::SpawnAgentIdle {
                actor,
                profile,
                parent_agent_id,
                respond_to,
            } => {
                let result = self.spawn_agent_internal(actor, profile, parent_agent_id, false);
                let _ = respond_to.send(result);
            }
            Command::RequestAgentTurn {
                actor,
                agent_id,
                prompt,
                respond_to,
            } => {
                let result = self.request_agent_turn_internal(actor, agent_id, prompt);
                let _ = respond_to.send(result);
            }
            Command::RequestToolCall {
                actor,
                category,
                tool_id,
                args_json,
                respond_to,
            } => {
                let result =
                    self.request_tool_call_internal(actor, category, tool_id, args_json, None);
                let _ = respond_to.send(result);
            }
            Command::ExecuteAgentToolCall {
                actor,
                category,
                tool_id,
                args_json,
                respond_to,
            } => {
                let _ = self.request_tool_call_internal(
                    actor,
                    category,
                    tool_id,
                    args_json,
                    Some(respond_to),
                );
            }
            Command::ResolvePermission {
                permission_id,
                decision,
                respond_to,
            } => {
                let result = self.resolve_permission_internal(permission_id, decision);
                let _ = respond_to.send(result);
            }
            Command::PermissionTimedOut { permission_id } => {
                self.resolve_permission_timeout_internal(permission_id);
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
                let _ = respond_to.send(result);
            }
            Command::JobFinished { task_id, outcome } => {
                let _ = self.job_finished_internal(task_id, outcome);
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
                let _ = self.agent_provider_request_started_internal(
                    task_id,
                    agent_id,
                    request_id,
                    provider_id,
                    model_id,
                    prompt_summary,
                    request_digest,
                );
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
            Command::AgentProviderRequestFinished {
                task_id,
                agent_id,
                request_id,
                finish_reason,
                output_digest,
            } => {
                let _ = self.agent_provider_request_finished_internal(
                    task_id,
                    agent_id,
                    request_id,
                    finish_reason,
                    output_digest,
                );
            }
            Command::AgentTurnFinished {
                task_id,
                agent_id,
                request_id,
                outcome,
            } => {
                let _ = self.agent_turn_finished_internal(task_id, agent_id, request_id, outcome);
            }
        }
    }

    fn start_run_internal(
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
            pending_permissions: BTreeMap::new(),
            cancelled_running_tasks: BTreeSet::new(),
            queued_agent_turns: BTreeMap::new(),
            running_agent_turns: BTreeMap::new(),
            scheduler: Scheduler::new(SchedulerLimits {
                provider_model: self.config.provider_model_concurrency,
                tool: self.config.tool_concurrency,
            }),
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
            agents.insert(agent_id.clone(), profile_cfg);
            restored_agent_bindings.push((agent_id.clone(), profile_name.clone()));
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
            pending_permissions: BTreeMap::new(),
            cancelled_running_tasks: BTreeSet::new(),
            queued_agent_turns: BTreeMap::new(),
            running_agent_turns: BTreeMap::new(),
            scheduler: Scheduler::new(SchedulerLimits {
                provider_model: self.config.provider_model_concurrency,
                tool: self.config.tool_concurrency,
            }),
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

        for (agent_id, profile) in restored_agent_bindings {
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                &mut run_state,
                system_actor(),
                Some(format!("agent:{agent_id}")),
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id,
                    profile,
                    parent_agent_id: None,
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

    fn stop_run_internal(&mut self, summary: String) -> Result<(), CoordinatorError> {
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
            EventV1::RunFinished(RunFinishedEvent { summary }),
        )?;

        Ok(())
    }

    fn spawn_agent_internal(
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

        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("agent:{agent_id}")),
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: agent_id.clone(),
                profile: profile.clone(),
                parent_agent_id,
            }),
        )?;

        let profile_cfg = self
            .config
            .agent_profiles
            .get(&profile)
            .cloned()
            .unwrap_or_else(|| AgentProfile::fallback(profile.clone()));
        run_state
            .agents
            .insert(agent_id.clone(), profile_cfg.clone());

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
            };

            schedule_agent_turn(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                self.job_tx.clone(),
                run_state,
                self.config.provider.clone(),
                self.config.tool_registry.clone(),
                profile_cfg,
                request,
                request_id,
            )?;
        }

        Ok(agent_id)
    }

    fn request_agent_turn_internal(
        &mut self,
        actor: EventActor,
        agent_id: String,
        prompt: String,
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
            model_ref: profile.model_ref.clone(),
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
            self.config.provider.clone(),
            self.config.tool_registry.clone(),
            profile,
            request,
            request_id.clone(),
        )?;

        Ok(request_id)
    }

    fn request_tool_call_internal(
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

        append_tool_call_requested_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            ToolCallRequestedEventArgs {
                actor: actor.clone(),
                tool_call_id: &tool_call_id,
                tool_id: &tool_id,
                args_json: &args_json,
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

            Some(worker_profile.category.as_str())
        } else {
            category.as_deref()
        };

        let maybe_kind = permission_kind_for_capability(capability);
        let decision = maybe_kind.map(|kind| {
            self.config
                .permission_policy
                .evaluate(effective_category, kind)
        });
        let hashline_edit = hashline_edit_metadata(&tool_id, &args_json);

        match decision {
            Some(PolicyDecision::Deny) => {
                finalize_permission_denied(
                    clock.as_ref(),
                    redactor.as_ref(),
                    run_state,
                    PermissionDeniedArgs {
                        tool_call_id: &tool_call_id,
                        hashline_edit: hashline_edit.as_ref(),
                        kind: maybe_kind
                            .expect("permission kind exists when policy decision exists"),
                        reason: "policy denied request",
                        request_correlation_id: request_correlation_id.as_deref(),
                    },
                )?;
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

                append_permission_requested_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    &permission_id,
                    &tool_call_id,
                    maybe_kind.expect("permission kind exists when policy decision exists"),
                    summary,
                    digest,
                    timeout_ms,
                    event_permission_decision(default_decision),
                    request_correlation_id.as_deref(),
                )?;

                run_state.pending_permissions.insert(
                    permission_id.clone(),
                    PendingPermissionState {
                        tool_call_id: tool_call_id.clone(),
                        tool_id,
                        args_json,
                        actor,
                        request_correlation_id,
                        respond_to,
                    },
                );

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
                    tool_call_id.clone(),
                    tool_id,
                    args_json,
                    actor,
                    self.config.tool_registry.clone(),
                    request_correlation_id,
                    respond_to,
                )?;
            }
        }

        Ok(tool_call_id)
    }

    fn resolve_permission_internal(
        &mut self,
        permission_id: String,
        decision: PermissionDecision,
    ) -> Result<(), CoordinatorError> {
        let clock = self.clock.clone();
        let redactor = self.redactor.clone();
        let job_tx = self.job_tx.clone();

        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;

        let Some(pending) = run_state.pending_permissions.remove(&permission_id) else {
            return Err(CoordinatorError::UnknownPermission(permission_id));
        };

        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            system_actor(),
            Some(format!("permission:{permission_id}")),
            EventV1::PermissionResolved(PermissionResolvedEvent {
                permission_id,
                decision: event_permission_decision(decision),
                reason: None,
            }),
        )?;

        match decision {
            PermissionDecision::Allow => {
                start_tool_call_execution(
                    clock.as_ref(),
                    redactor.as_ref(),
                    job_tx,
                    run_state,
                    pending.tool_call_id,
                    pending.tool_id,
                    pending.args_json,
                    pending.actor,
                    self.config.tool_registry.clone(),
                    pending.request_correlation_id,
                    pending.respond_to,
                )?;
            }
            PermissionDecision::Deny => {
                if let Some(metadata) = hashline_edit_metadata(&pending.tool_id, &pending.args_json)
                {
                    append_edit_rejected_event(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        &pending.tool_call_id,
                        &metadata,
                        "permission denied".to_string(),
                        pending.request_correlation_id.as_deref(),
                    )?;
                }

                append_failed_tool_call_finished_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    &pending.tool_call_id,
                    "permission denied",
                    pending.request_correlation_id.as_deref(),
                )?;
                if let Some(respond_to) = pending.respond_to {
                    let _ = respond_to.send(Err("tool call denied: permission denied".to_string()));
                }
            }
        }

        Ok(())
    }

    fn resolve_permission_timeout_internal(&mut self, permission_id: String) {
        let Some(run_state) = self.run_state.as_mut() else {
            return;
        };

        let Some(pending) = run_state.pending_permissions.remove(&permission_id) else {
            return;
        };

        let _ = append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            system_actor(),
            Some(format!("permission:{permission_id}")),
            EventV1::PermissionResolved(PermissionResolvedEvent {
                permission_id,
                decision: EventPermissionDecision::Deny,
                reason: Some("permission request timed out".to_string()),
            }),
        );

        if let Some(metadata) = hashline_edit_metadata(&pending.tool_id, &pending.args_json) {
            let _ = append_edit_rejected_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                &pending.tool_call_id,
                &metadata,
                "permission denied by timeout".to_string(),
                pending.request_correlation_id.as_deref(),
            );
        }

        let _ = append_failed_tool_call_finished_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            &pending.tool_call_id,
            "permission denied by timeout",
            pending.request_correlation_id.as_deref(),
        );
        if let Some(respond_to) = pending.respond_to {
            let _ = respond_to.send(Err(
                "tool call timed out: permission request timed out".to_string()
            ));
        }
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

    fn job_finished_internal(
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

        match outcome {
            JobOutcome::Succeeded { result } => {
                let result_for_response = result.clone();
                if let Some(metadata) = task.hashline_edit.as_ref() {
                    match workspace_file_digest(&run_state.info.workspace_root, &metadata.path) {
                        Ok(new_file_digest) => {
                            let (diff_rel_path, diff_digest) = hashline_diff_refs(&result);
                            append_edit_applied_event(
                                self.clock.as_ref(),
                                self.redactor.as_ref(),
                                run_state,
                                &task.tool_call_id,
                                metadata,
                                new_file_digest,
                                diff_rel_path,
                                diff_digest,
                                request_correlation_id.as_deref(),
                            )?;
                        }
                        Err(reason) => {
                            append_edit_rejected_event(
                                self.clock.as_ref(),
                                self.redactor.as_ref(),
                                run_state,
                                &task.tool_call_id,
                                metadata,
                                format!("failed to compute file digest: {reason}"),
                                request_correlation_id.as_deref(),
                            )?;
                        }
                    }
                }

                let result_summary = result.display_text;
                for artifact in &result.artifacts {
                    append_artifact_written_event(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        &task.tool_call_id,
                        artifact,
                        request_correlation_id.as_deref(),
                    )?;
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
                    }),
                )?;

                append_tool_call_finished_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    &task.tool_call_id,
                    ToolCallStatus::Succeeded,
                    Some(result_summary),
                    request_correlation_id.as_deref(),
                )?;
                if let Some(respond_to) = task.respond_to {
                    let _ = respond_to.send(Ok(result_for_response));
                }
            }
            JobOutcome::Failed { error } => {
                let error_for_response = error.clone();
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

                append_payload_event_with_correlation(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    task.owner_actor.clone(),
                    Some(format!("task:{task_id}")),
                    request_correlation_id.clone(),
                    EventV1::TaskCancelled(TaskCancelledEvent {
                        task_id,
                        reason: error,
                    }),
                )?;

                append_failed_tool_call_finished_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    &task.tool_call_id,
                    "tool execution failed",
                    request_correlation_id.as_deref(),
                )?;
                if let Some(respond_to) = task.respond_to {
                    let _ = respond_to
                        .send(Err(format!("tool execution failed: {error_for_response}")));
                }
            }
            JobOutcome::Cancelled { reason } => {
                let reason_for_response = reason.clone();
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

                append_payload_event_with_correlation(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    task.owner_actor.clone(),
                    Some(format!("task:{task_id}")),
                    request_correlation_id.clone(),
                    EventV1::TaskCancelled(TaskCancelledEvent { task_id, reason }),
                )?;

                append_failed_tool_call_finished_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    &task.tool_call_id,
                    "tool execution cancelled",
                    request_correlation_id.as_deref(),
                )?;
                if let Some(respond_to) = task.respond_to {
                    let _ =
                        respond_to.send(Err(format!("tool call cancelled: {reason_for_response}")));
                }
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn agent_provider_request_started_internal(
        &mut self,
        task_id: String,
        agent_id: String,
        request_id: String,
        provider_id: String,
        model_id: String,
        prompt_summary: String,
        request_digest: String,
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
            EventV1::ProviderRequestStarted(crate::event::ProviderRequestStartedEvent {
                request_id,
                provider_id,
                model_id,
                prompt_summary,
                request_digest,
            }),
        )?;

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

    fn agent_provider_request_finished_internal(
        &mut self,
        task_id: String,
        agent_id: String,
        request_id: String,
        finish_reason: String,
        output_digest: Option<String>,
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
            EventV1::ProviderRequestFinished(crate::event::ProviderRequestFinishedEvent {
                request_id,
                finish_reason,
                output_digest,
            }),
        )?;

        Ok(())
    }

    fn agent_turn_finished_internal(
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
                        }),
                    )?;
                }
                AgentTurnTaskOutcome::Failed { reason } => {
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
                    self.config.provider.clone(),
                    self.config.tool_registry.clone(),
                    queued,
                )?;
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
    pending_permissions: BTreeMap<String, PendingPermissionState>,
    cancelled_running_tasks: BTreeSet<String>,
    queued_agent_turns: BTreeMap<String, QueuedAgentTurn>,
    running_agent_turns: BTreeMap<String, RunningAgentTurn>,
    scheduler: Scheduler,
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
    queue_key: ConcurrencyKey,
    cancellation_token: CancellationToken,
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

struct TaskState {
    tool_call_id: String,
    owner_actor: EventActor,
    request_correlation_id: Option<String>,
    queue_key: ConcurrencyKey,
    state: TaskExecutionState,
    cancellation_token: CancellationToken,
    last_progress_mono_ms: u64,
    last_progress_kind: JobProgressKind,
    hashline_edit: Option<HashlineEditMetadata>,
    respond_to: Option<oneshot::Sender<Result<ToolResult, String>>>,
}

struct PendingPermissionState {
    tool_call_id: String,
    tool_id: String,
    args_json: Value,
    actor: EventActor,
    request_correlation_id: Option<String>,
    respond_to: Option<oneshot::Sender<Result<ToolResult, String>>>,
}

struct AgentTurnTaskScheduledEventArgs<'a> {
    task_id: &'a str,
    agent_id: &'a str,
    request_id: &'a str,
    queue_key: &'a ConcurrencyKey,
    state: TaskScheduleState,
}

struct PermissionDeniedArgs<'a> {
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
    request_correlation_id: Option<&'a str>,
}

#[allow(clippy::too_many_arguments)]
fn start_tool_call_execution<C, R>(
    clock: &C,
    redactor: &R,
    job_tx: mpsc::Sender<Command>,
    run_state: &mut RunState,
    tool_call_id: String,
    tool_id: String,
    args_json: Value,
    actor: EventActor,
    tool_registry: Arc<ToolRegistry>,
    request_correlation_id: Option<String>,
    respond_to: Option<oneshot::Sender<Result<ToolResult, String>>>,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
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
        )?;
        return Err(CoordinatorError::PolicyViolation(
            "tool capability forbidden for actor".to_string(),
        ));
    }

    let hashline_edit = hashline_edit_metadata(&tool_id, &args_json);

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
    run_state.tasks.insert(
        task_id.clone(),
        TaskState {
            tool_call_id: tool_call_id.clone(),
            owner_actor: actor.clone(),
            request_correlation_id,
            queue_key,
            state: TaskExecutionState::Running,
            cancellation_token: cancellation_token.clone(),
            last_progress_mono_ms: clock.mono_ms(),
            last_progress_kind: JobProgressKind::Heartbeat,
            hashline_edit,
            respond_to,
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
            tool_call_id: tool_call_id.clone(),
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

#[allow(clippy::too_many_arguments)]
fn schedule_agent_turn<C, R>(
    clock: &C,
    redactor: &R,
    job_tx: mpsc::Sender<Command>,
    run_state: &mut RunState,
    provider: Arc<dyn Provider>,
    tool_registry: Arc<ToolRegistry>,
    profile: AgentProfile,
    request: AgentRequest,
    request_id: String,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
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
            )?;
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

fn start_agent_turn_execution<C, R>(
    _clock: &C,
    _redactor: &R,
    job_tx: mpsc::Sender<Command>,
    run_state: &mut RunState,
    provider: Arc<dyn Provider>,
    tool_registry: Arc<ToolRegistry>,
    task: QueuedAgentTurn,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let cancellation_token = run_state.shutdown_token.child_token();

    run_state.running_agent_turns.insert(
        task.task_id.clone(),
        RunningAgentTurn {
            agent_id: task.agent_id.clone(),
            request_id: task.request_id.clone(),
            request_prompt: task.request.prompt.clone(),
            queue_key: task.queue_key.clone(),
            cancellation_token: cancellation_token.clone(),
        },
    );

    tokio::spawn(async move {
        let task_id = task.task_id.clone();
        let agent_id = task.agent_id.clone();
        let request_id = task.request_id.clone();

        tokio::select! {
            _ = cancellation_token.cancelled() => {
                let _ = job_tx.send(Command::AgentTurnFinished {
                    task_id,
                    agent_id,
                    request_id,
                    outcome: AgentTurnTaskOutcome::Failed {
                        reason: "job cancelled".to_string(),
                    },
                }).await;
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
                                let _ = job_tx.send(Command::AgentProviderRequestStarted {
                                    task_id,
                                    agent_id,
                                    request_id: started.request_id,
                                    provider_id: started.provider_id,
                                    model_id: started.model_id,
                                    prompt_summary: started.prompt_summary,
                                    request_digest: started.request_digest,
                                }).await;
                            }
                            AgentRuntimeEvent::ProviderStreamDelta { request_id, delta } => {
                                let _ = job_tx.send(Command::AgentProviderStreamDelta {
                                    task_id,
                                    agent_id,
                                    request_id,
                                    delta,
                                }).await;
                            }
                            AgentRuntimeEvent::ProviderRequestFinished(finished) => {
                                let _ = job_tx.send(Command::AgentProviderRequestFinished {
                                    task_id,
                                    agent_id,
                                    request_id: finished.request_id,
                                    finish_reason: finished.finish_reason,
                                    output_digest: finished.output_digest,
                                }).await;
                            }
                        }
                    }
                }
            ) => {
                let outcome = match outcome {
                    AgentTurnOutcome::Succeeded { output } => AgentTurnTaskOutcome::Succeeded { output },
                    AgentTurnOutcome::Failed { reason } => AgentTurnTaskOutcome::Failed { reason },
                };
                let _ = job_tx.send(Command::AgentTurnFinished {
                    task_id: task.task_id,
                    agent_id: task.agent_id,
                    request_id: task.request_id,
                    outcome,
                }).await;
            }
        }
    });

    Ok(())
}

fn finalize_permission_denied<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    args: PermissionDeniedArgs<'_>,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let PermissionDeniedArgs {
        tool_call_id,
        hashline_edit,
        kind,
        reason,
        request_correlation_id,
    } = args;

    let permission_id = format!("perm_{:06}", run_state.next_permission_id);
    run_state.next_permission_id += 1;

    append_payload_event(
        clock,
        redactor,
        run_state,
        system_actor(),
        Some(format!("permission:{permission_id}")),
        EventV1::PermissionResolved(PermissionResolvedEvent {
            permission_id,
            decision: EventPermissionDecision::Deny,
            reason: Some(format!("{} ({})", reason, kind.as_str())),
        }),
    )?;

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
    )?;
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
        request_correlation_id,
    } = args;

    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.clone());
    let mut context = EventContext::new(run_state.next_event_seq, actor);
    context.correlation_id = request_correlation_id
        .map(ToOwned::to_owned)
        .or_else(|| Some(tool_call_id.to_string()));
    context.stream_key = Some(format!("tool_call:{tool_call_id}"));
    let envelope = builder.tool_call_requested(context, tool_call_id, tool_id, args_json)?;
    append_built_event(run_state, envelope)
}

#[allow(clippy::too_many_arguments)]
fn append_permission_requested_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    permission_id: &str,
    tool_call_id: &str,
    kind: PermissionKind,
    summary: String,
    request_digest: String,
    timeout_ms: u64,
    default_decision: EventPermissionDecision,
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
    context.stream_key = Some(format!("permission:{permission_id}"));

    let envelope = builder.permission_requested(
        context,
        permission_id,
        kind.as_str(),
        Some(tool_call_id.to_string()),
        summary,
        request_digest,
        timeout_ms,
        default_decision,
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
    tool_call_id: &str,
    status: ToolCallStatus,
    output_summary: Option<String>,
    request_correlation_id: Option<&str>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
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

#[allow(clippy::too_many_arguments)]
fn append_edit_applied_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    tool_call_id: &str,
    metadata: &HashlineEditMetadata,
    new_file_digest: String,
    diff_rel_path: Option<String>,
    diff_digest: Option<String>,
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

fn append_failed_tool_call_finished_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    tool_call_id: &str,
    reason: &str,
    request_correlation_id: Option<&str>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    append_tool_call_finished_event(
        clock,
        redactor,
        run_state,
        tool_call_id,
        ToolCallStatus::Failed,
        Some(reason.to_string()),
        request_correlation_id,
    )
}

fn append_artifact_written_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    tool_call_id: &str,
    artifact: &crate::tool::ArtifactRef,
    request_correlation_id: Option<&str>,
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
            metadata: BTreeMap::new(),
        }),
    )?;

    append_built_event(run_state, envelope)
}

#[derive(Debug, Clone, Serialize)]
struct RunMetadata {
    run_id: String,
    run_name: String,
    workspace_root: String,
    created_at: Option<String>,
    config_digest: String,
    harness_version: String,
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

fn hashline_edit_metadata(tool_id: &str, args_json: &Value) -> Option<HashlineEditMetadata> {
    if tool_id != HASHLINE_APPLY_TOOL_ID {
        return None;
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use serde_json::json;
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    use crate::clock::FakeClock;
    use crate::config::PermissionMode;
    use crate::event::{
        ActorKind, AgentSpawnedEvent, EventActor, EventEnvelopeV1, EventV1,
        ProviderRequestStartedEvent, RunFinishedEvent, RunStartedEvent, TaskCompletedEvent,
        ToolCallStatus, SCHEMA_VERSION,
    };
    use crate::perm::{PermissionDecision, PermissionPolicy};
    use crate::redact::DefaultRedactor;
    use crate::sched::{ConcurrencyKey, ScheduleDecision};
    use crate::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};

    use super::{
        restore_provider_context_from_history, spawn_coordinator, Coordinator, CoordinatorConfig,
        JobOutcome, JobProgressKind, TaskExecutionState, TaskState,
    };

    struct TestShellTool;

    #[async_trait]
    impl Tool for TestShellTool {
        fn id(&self) -> &str {
            "shell.run"
        }

        fn capability(&self) -> ToolCapability {
            ToolCapability::Shell
        }

        async fn call(
            &self,
            _ctx: ToolContext,
            args_json: serde_json::Value,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::text(format!("ok {args_json}")))
        }
    }

    fn test_tool_registry() -> Arc<ToolRegistry> {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(TestShellTool));
        Arc::new(registry)
    }

    fn test_config(session_dir: &Path) -> CoordinatorConfig {
        let mut config = CoordinatorConfig::new(session_dir);
        config.deterministic_store = true;
        config.tool_registry = test_tool_registry();
        config
    }

    #[tokio::test]
    async fn perm_allow_path_proceeds() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut config = test_config(temp_dir.path());
        config.permission_policy = PermissionPolicy::new(
            PermissionMode::Deny,
            PermissionMode::Allow,
            PermissionMode::Deny,
        );

        let handle = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );

        let run = handle
            .start_run("perm_allow", temp_dir.path())
            .await
            .expect("start run");

        let tool_call_id = handle
            .request_tool_call(
                EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string())),
                Some("deep".to_string()),
                "shell.run",
                json!({"cmd": "true"}),
            )
            .await
            .expect("request tool call");

        tokio::time::sleep(Duration::from_millis(80)).await;
        handle.stop_run().await.expect("stop run");

        let events = read_events(&run.events_path);
        assert!(events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallRequested(data)
                    if data.tool_call_id == tool_call_id
                        && event.correlation_id.as_deref() == Some(tool_call_id.as_str())
            )
        }));
        assert!(events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallStarted(data)
                    if data.tool_call_id == tool_call_id
                        && event.correlation_id.as_deref() == Some(tool_call_id.as_str())
            )
        }));
        assert!(events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallFinished(data)
                    if data.tool_call_id == tool_call_id
                        && data.status == ToolCallStatus::Succeeded
                        && event.correlation_id.as_deref() == Some(tool_call_id.as_str())
            )
        }));
    }

    #[tokio::test]
    async fn perm_ask_path_blocks_until_resolved() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut config = test_config(temp_dir.path());
        config.permission_policy = PermissionPolicy::new(
            PermissionMode::Deny,
            PermissionMode::Ask,
            PermissionMode::Deny,
        )
        .with_ask_timeout_ms(1_000);

        let handle = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );

        let run = handle
            .start_run("perm_ask", temp_dir.path())
            .await
            .expect("start run");

        let tool_call_id = handle
            .request_tool_call(
                EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string())),
                Some("deep".to_string()),
                "shell.run",
                json!({"cmd": "echo blocked"}),
            )
            .await
            .expect("request tool call");

        tokio::time::sleep(Duration::from_millis(40)).await;
        let before_resolve = read_events(&run.events_path);
        assert!(
            !before_resolve.iter().any(|event| {
                matches!(
                    &event.payload,
                    EventV1::ToolCallStarted(data) if data.tool_call_id == tool_call_id
                )
            }),
            "tool call must not start before permission resolution"
        );

        let permission_id = before_resolve
            .iter()
            .find_map(|event| match &event.payload {
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_deref() == Some(tool_call_id.as_str()) =>
                {
                    Some(data.permission_id.clone())
                }
                _ => None,
            })
            .expect("permission requested event");

        handle
            .resolve_permission(permission_id, PermissionDecision::Allow)
            .await
            .expect("resolve permission");

        tokio::time::sleep(Duration::from_millis(80)).await;
        handle.stop_run().await.expect("stop run");

        let events = read_events(&run.events_path);
        let requested_idx = events
            .iter()
            .position(|event| {
                matches!(
                    &event.payload,
                    EventV1::PermissionRequested(data)
                        if data.tool_call_id.as_deref() == Some(tool_call_id.as_str())
                )
            })
            .expect("permission requested index");
        let resolved_idx = events
            .iter()
            .position(|event| {
                matches!(
                    &event.payload,
                    EventV1::PermissionResolved(data)
                        if data.decision == crate::event::PermissionDecision::Allow
                )
            })
            .expect("permission resolved index");
        let started_idx = events
            .iter()
            .position(|event| {
                matches!(
                    &event.payload,
                    EventV1::ToolCallStarted(data) if data.tool_call_id == tool_call_id
                )
            })
            .expect("tool started index");

        assert!(requested_idx < resolved_idx);
        assert!(resolved_idx < started_idx);
    }

    #[tokio::test]
    async fn perm_timeout_path_denies_deterministically() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut config = test_config(temp_dir.path());
        config.permission_policy = PermissionPolicy::new(
            PermissionMode::Deny,
            PermissionMode::Ask,
            PermissionMode::Deny,
        )
        .with_ask_timeout_ms(25);

        let handle = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );

        let run = handle
            .start_run("perm_timeout", temp_dir.path())
            .await
            .expect("start run");

        let tool_call_id = handle
            .request_tool_call(
                EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string())),
                Some("deep".to_string()),
                "shell.run",
                json!({"cmd": "sleep 1"}),
            )
            .await
            .expect("request tool call");

        tokio::time::sleep(Duration::from_millis(90)).await;
        handle.stop_run().await.expect("stop run");

        let events = read_events(&run.events_path);
        assert!(events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::PermissionResolved(data)
                    if data.decision == crate::event::PermissionDecision::Deny
                        && data.reason.as_deref() == Some("permission request timed out")
            )
        }));
        assert!(events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallFinished(data)
                    if data.tool_call_id == tool_call_id && data.status == ToolCallStatus::Failed
            )
        }));
        assert!(!events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallStarted(data) if data.tool_call_id == tool_call_id
            )
        }));
    }

    #[test]
    fn stale_tool_task_late_result_preserves_owner_actor() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut config = test_config(temp_dir.path());
        config.stale_timeout_ms = 20;
        let clock = Arc::new(FakeClock::new());
        let redactor = Arc::new(DefaultRedactor::default());
        let (_command_tx, command_rx) = mpsc::channel(1);
        let (job_tx, job_rx) = mpsc::channel(1);
        let mut coordinator =
            Coordinator::new(config, clock.clone(), redactor, command_rx, job_tx, job_rx);

        let run = coordinator
            .start_run_internal("stale_owner".to_string(), temp_dir.path().to_path_buf())
            .expect("start run");
        let task_id = "task_000001".to_string();
        let queue_key = ConcurrencyKey::Tool {
            tool_id: "shell.run".to_string(),
        };
        let owner_actor = EventActor::new(ActorKind::Worker, Some("agent_000001".to_string()));
        let request_correlation_id = Some("req_000001".to_string());

        {
            let run_state = coordinator.run_state.as_mut().expect("run state");
            assert!(matches!(
                run_state
                    .scheduler
                    .schedule(task_id.clone(), queue_key.clone()),
                ScheduleDecision::Started(_)
            ));
            run_state.tasks.insert(
                task_id.clone(),
                TaskState {
                    tool_call_id: "toolcall_000001".to_string(),
                    owner_actor: owner_actor.clone(),
                    request_correlation_id: request_correlation_id.clone(),
                    queue_key,
                    state: TaskExecutionState::Running,
                    cancellation_token: CancellationToken::new(),
                    last_progress_mono_ms: 0,
                    last_progress_kind: JobProgressKind::Heartbeat,
                    hashline_edit: None,
                    respond_to: None,
                },
            );
        }

        clock.advance(25);
        coordinator
            .watchdog_tick_internal()
            .expect("detect stale tool task");
        coordinator
            .job_finished_internal(
                task_id.clone(),
                JobOutcome::Cancelled {
                    reason: "job cancelled".to_string(),
                },
            )
            .expect("record late result");

        let events = read_events(&run.events_path);
        assert!(events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::StaleDetected(data)
                    if data.task_id == task_id
                        && event.actor == owner_actor
                        && event.correlation_id.as_deref() == request_correlation_id.as_deref()
            )
        }));
        assert!(events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskResultLate(data)
                    if data.task_id == task_id
                        && event.actor == owner_actor
                        && event.correlation_id.as_deref() == request_correlation_id.as_deref()
            )
        }));
    }

    #[test]
    fn restore_provider_context_uses_task_completed_summary_for_iterative_history() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let run_id = "run_iterative_restore";
        write_restore_history_fixture(
            temp_dir.path(),
            run_id,
            &[
                restore_fixture_event(
                    run_id,
                    1,
                    EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                    None,
                    EventV1::RunStarted(RunStartedEvent {
                        run_name: "interactive".to_string(),
                        workspace_root: "/workspace/project".to_string(),
                    }),
                ),
                restore_fixture_event(
                    run_id,
                    2,
                    EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                    None,
                    EventV1::AgentSpawned(AgentSpawnedEvent {
                        agent_id: "agent_000001".to_string(),
                        profile: "alpha".to_string(),
                        parent_agent_id: None,
                    }),
                ),
                restore_fixture_event(
                    run_id,
                    3,
                    EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                    Some("req_000001"),
                    EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                        request_id: "req_000001".to_string(),
                        provider_id: "mock".to_string(),
                        model_id: "model-1".to_string(),
                        prompt_summary: "first question".to_string(),
                        request_digest: "digest-1".to_string(),
                    }),
                ),
                restore_fixture_event(
                    run_id,
                    4,
                    EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                    Some("req_000001"),
                    EventV1::ProviderStreamDelta(crate::event::ProviderStreamDeltaEvent {
                        request_id: "req_000001".to_string(),
                        delta: "calling tool".to_string(),
                    }),
                ),
                restore_fixture_event(
                    run_id,
                    5,
                    EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                    Some("req_000001_iter_02"),
                    EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                        request_id: "req_000001_iter_02".to_string(),
                        provider_id: "mock".to_string(),
                        model_id: "model-1".to_string(),
                        prompt_summary: "tool result follow-up".to_string(),
                        request_digest: "digest-2".to_string(),
                    }),
                ),
                restore_fixture_event(
                    run_id,
                    6,
                    EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                    Some("req_000001_iter_02"),
                    EventV1::ProviderStreamDelta(crate::event::ProviderStreamDeltaEvent {
                        request_id: "req_000001_iter_02".to_string(),
                        delta: "final answer".to_string(),
                    }),
                ),
                restore_fixture_event(
                    run_id,
                    7,
                    EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                    Some("req_000001"),
                    EventV1::TaskCompleted(TaskCompletedEvent {
                        task_id: "task_000001".to_string(),
                        result_summary: "final answer".to_string(),
                        result_digest: "digest-task".to_string(),
                    }),
                ),
                restore_fixture_event(
                    run_id,
                    8,
                    EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                    None,
                    EventV1::RunFinished(RunFinishedEvent {
                        summary: "done".to_string(),
                    }),
                ),
            ],
        );

        let restored = restore_provider_context_from_history(temp_dir.path(), run_id)
            .expect("restore provider context");
        let turns = restored
            .get("agent_000001")
            .expect("agent should have restored history");
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].user_prompt, "first question");
        assert_eq!(turns[0].assistant_response, "final answer");
    }

    fn write_restore_history_fixture(session_dir: &Path, run_id: &str, events: &[EventEnvelopeV1]) {
        let run_dir = session_dir.join(run_id);
        fs::create_dir_all(&run_dir).expect("create run directory");

        let mut body = String::new();
        for event in events {
            let line = serde_json::to_string(event).expect("serialize event");
            body.push_str(&line);
            body.push('\n');
        }

        fs::write(run_dir.join("events.jsonl"), body).expect("write events");
    }

    fn restore_fixture_event(
        run_id: &str,
        seq: u64,
        actor: EventActor,
        correlation_id: Option<&str>,
        payload: EventV1,
    ) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-{seq:04}"),
            seq,
            run_id: run_id.to_string(),
            mono_ms: seq,
            ts: None,
            actor,
            correlation_id: correlation_id.map(str::to_string),
            causation_id: None,
            stream_key: Some(format!("run:{run_id}")),
            payload,
        }
    }

    fn read_events(path: &Path) -> Vec<EventEnvelopeV1> {
        let text = fs::read_to_string(path).expect("read events");
        text.lines()
            .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).expect("valid event"))
            .collect()
    }
}
