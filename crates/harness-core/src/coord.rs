use std::collections::{BTreeMap, BTreeSet};
use std::fs;
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
    default_provider, run_single_turn_streaming, AgentProfile, AgentRequest, AgentRuntimeEvent,
    AgentTurnOutcome,
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
};
use crate::perm::{
    permission_kind_for_capability, PermissionDecision, PermissionKind, PermissionPolicy,
    PolicyDecision,
};
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
                let result = self.request_tool_call_internal(actor, category, tool_id, args_json);
                let _ = respond_to.send(result);
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

        schedule_agent_turn(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            self.job_tx.clone(),
            run_state,
            self.config.provider.clone(),
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

        append_tool_call_requested_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor.clone(),
            &tool_call_id,
            &tool_id,
            &args_json,
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
                        detail: format!(
                            "worker agent_id `{worker_agent_id}` is not registered"
                        ),
                    }),
                )?;

                return Err(CoordinatorError::PolicyViolation(format!(
                    "worker agent_id `{worker_agent_id}` is not registered"
                )));
            };

            if !worker_profile.toolset.iter().any(|allowed| allowed == &tool_id) {
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
                    tool_call_id.clone(),
                    hashline_edit.as_ref(),
                    maybe_kind.expect("permission kind exists when policy decision exists"),
                    "policy denied request",
                )?;
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
                )?;

                run_state.pending_permissions.insert(
                    permission_id.clone(),
                    PendingPermissionState {
                        tool_call_id: tool_call_id.clone(),
                        tool_id,
                        args_json,
                        actor,
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
                    )?;
                }

                append_failed_tool_call_finished_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    &pending.tool_call_id,
                    "permission denied",
                )?;
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
            );
        }

        let _ = append_failed_tool_call_finished_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            &pending.tool_call_id,
            "permission denied by timeout",
        );
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

        if run_state.queued_agent_turns.remove(&task_id).is_some() {
            let _ = run_state.scheduler.cancel_queued(&task_id);
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                system_actor(),
                Some(format!("task:{task_id}")),
                EventV1::TaskCancelled(TaskCancelledEvent { task_id, reason }),
            )?;
            return Ok(());
        }

        if let Some(running) = run_state.running_agent_turns.get(&task_id) {
            running.cancellation_token.cancel();
            run_state.cancelled_running_tasks.insert(task_id.clone());
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                system_actor(),
                Some(format!("task:{task_id}")),
                EventV1::TaskCancelled(TaskCancelledEvent { task_id, reason }),
            )?;
            return Ok(());
        }

        let Some(task) = run_state.tasks.get(&task_id) else {
            return Ok(());
        };

        task.cancellation_token.cancel();
        run_state.cancelled_running_tasks.insert(task_id.clone());

        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            system_actor(),
            Some(format!("task:{task_id}")),
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

            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                system_actor(),
                Some(format!("task:{task_id}")),
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
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                system_actor(),
                Some(format!("task:{task_id}")),
                EventV1::TaskResultLate(TaskResultLateEvent {
                    task_id,
                    result_digest: digest12(format!("{:?}", outcome).as_bytes()),
                }),
            )?;
            return Ok(());
        }

        let _ = run_state.scheduler.complete(&task.queue_key);

        match outcome {
            JobOutcome::Succeeded { result } => {
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
                    )?;
                }
                append_payload_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    system_actor(),
                    Some(format!("task:{task_id}")),
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
                )?;
            }
            JobOutcome::Failed { error } => {
                if let Some(metadata) = task.hashline_edit.as_ref() {
                    append_edit_rejected_event(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        &task.tool_call_id,
                        metadata,
                        error.clone(),
                    )?;
                }

                append_payload_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    system_actor(),
                    Some(format!("task:{task_id}")),
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
                )?;
            }
            JobOutcome::Cancelled { reason } => {
                if let Some(metadata) = task.hashline_edit.as_ref() {
                    append_edit_rejected_event(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        &task.tool_call_id,
                        metadata,
                        reason.clone(),
                    )?;
                }

                append_payload_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    system_actor(),
                    Some(format!("task:{task_id}")),
                    EventV1::TaskCancelled(TaskCancelledEvent { task_id, reason }),
                )?;

                append_failed_tool_call_finished_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    &task.tool_call_id,
                    "tool execution cancelled",
                )?;
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

        let dequeued = run_state.scheduler.complete(&running.queue_key);

        match outcome {
            AgentTurnTaskOutcome::Succeeded { output } => {
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

        for task in dequeued {
            if let Some(queued) = run_state.queued_agent_turns.remove(&task.task_id) {
                start_agent_turn_execution(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    self.job_tx.clone(),
                    run_state,
                    self.config.provider.clone(),
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
    queue_key: ConcurrencyKey,
}

#[derive(Debug, Clone)]
struct RunningAgentTurn {
    agent_id: String,
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
    queue_key: ConcurrencyKey,
    state: TaskExecutionState,
    cancellation_token: CancellationToken,
    last_progress_mono_ms: u64,
    last_progress_kind: JobProgressKind,
    hashline_edit: Option<HashlineEditMetadata>,
}

struct PendingPermissionState {
    tool_call_id: String,
    tool_id: String,
    args_json: Value,
    actor: EventActor,
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
        )?;
        return Err(CoordinatorError::PolicyViolation(
            "tool capability forbidden for actor".to_string(),
        ));
    }

    let hashline_edit = hashline_edit_metadata(&tool_id, &args_json);

    append_tool_call_started_event(clock, redactor, run_state, &tool_call_id)?;

    if let Some(metadata) = hashline_edit.as_ref() {
        append_edit_proposed_event(clock, redactor, run_state, &tool_call_id, metadata)?;
    }

    let task_id = format!("task_{:06}", run_state.next_task_id);
    run_state.next_task_id += 1;

    let queue_key = ConcurrencyKey::Tool {
        tool_id: tool_id.clone(),
    };

    append_payload_event(
        clock,
        redactor,
        run_state,
        system_actor(),
        Some(format!("task:{task_id}")),
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
            queue_key,
            state: TaskExecutionState::Running,
            cancellation_token: cancellation_token.clone(),
            last_progress_mono_ms: clock.mono_ms(),
            last_progress_kind: JobProgressKind::Heartbeat,
            hashline_edit,
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
    profile: AgentProfile,
    request: AgentRequest,
    request_id: String,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let model = crate::agent::AgentModelRef::parse(&request.model_ref);
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
            append_payload_event(
                clock,
                redactor,
                run_state,
                system_actor(),
                Some(format!("task:{task_id}")),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: task_id.clone(),
                    state: TaskScheduleState::Started,
                    queue_key: Some(queue_key.queue_key()),
                }),
            )?;

            start_agent_turn_execution(
                clock,
                redactor,
                job_tx,
                run_state,
                provider,
                QueuedAgentTurn {
                    task_id,
                    agent_id: request.agent_id.clone(),
                    request_id,
                    profile,
                    request,
                    queue_key,
                },
            )?;
        }
        ScheduleDecision::Queued(_) => {
            append_payload_event(
                clock,
                redactor,
                run_state,
                system_actor(),
                Some(format!("task:{task_id}")),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: task_id.clone(),
                    state: TaskScheduleState::Queued,
                    queue_key: Some(queue_key.queue_key()),
                }),
            )?;

            run_state.queued_agent_turns.insert(
                task_id.clone(),
                QueuedAgentTurn {
                    task_id,
                    agent_id: request.agent_id.clone(),
                    request_id,
                    profile,
                    request,
                    queue_key,
                },
            );
        }
    }

    Ok(())
}

fn start_agent_turn_execution<C, R>(
    _clock: &C,
    _redactor: &R,
    job_tx: mpsc::Sender<Command>,
    run_state: &mut RunState,
    provider: Arc<dyn Provider>,
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
            outcome = run_single_turn_streaming(
                provider,
                &task.profile,
                task.request_id.clone(),
                task.request,
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
    tool_call_id: String,
    hashline_edit: Option<&HashlineEditMetadata>,
    kind: PermissionKind,
    reason: &str,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
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
            &tool_call_id,
            metadata,
            reason.to_string(),
        )?;
    }

    append_failed_tool_call_finished_event(clock, redactor, run_state, &tool_call_id, reason)?;
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
    actor: EventActor,
    tool_call_id: &str,
    tool_id: &str,
    args_json: &Value,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.clone());
    let mut context = EventContext::new(run_state.next_event_seq, actor);
    context.correlation_id = Some(tool_call_id.to_string());
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
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.clone());
    let mut context = EventContext::new(run_state.next_event_seq, system_actor());
    context.correlation_id = Some(tool_call_id.to_string());
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
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.clone());
    let mut context = EventContext::new(run_state.next_event_seq, system_actor());
    context.correlation_id = Some(tool_call_id.to_string());
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
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let output_digest = output_summary.as_ref().map(|s| digest12(s.as_bytes()));
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.clone());
    let mut context = EventContext::new(run_state.next_event_seq, system_actor());
    context.correlation_id = Some(tool_call_id.to_string());
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
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.clone());
    let mut context = EventContext::new(run_state.next_event_seq, system_actor());
    context.correlation_id = Some(tool_call_id.to_string());
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
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.clone());
    let mut context = EventContext::new(run_state.next_event_seq, system_actor());
    context.correlation_id = Some(tool_call_id.to_string());
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
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.clone());
    let mut context = EventContext::new(run_state.next_event_seq, system_actor());
    context.correlation_id = Some(tool_call_id.to_string());
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
    )
}

fn append_artifact_written_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    tool_call_id: &str,
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

    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.clone());
    let mut context = EventContext::new(run_state.next_event_seq, system_actor());
    context.correlation_id = Some(tool_call_id.to_string());
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

    use crate::clock::FakeClock;
    use crate::config::PermissionMode;
    use crate::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1, ToolCallStatus};
    use crate::perm::{PermissionDecision, PermissionPolicy};
    use crate::redact::DefaultRedactor;
    use crate::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};

    use super::{spawn_coordinator, CoordinatorConfig};

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

    fn read_events(path: &Path) -> Vec<EventEnvelopeV1> {
        let text = fs::read_to_string(path).expect("read events");
        text.lines()
            .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).expect("valid event"))
            .collect()
    }
}
