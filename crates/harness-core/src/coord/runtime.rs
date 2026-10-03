mod activity;
use super::context::Context;
use super::{handle::system, *};
use std::collections::VecDeque;
use tokio::{
    sync::Semaphore,
    task::{Id, JoinSet},
};

pub(super) struct Runtime {
    pub config: CoordinatorConfig,
    pub clock: Arc<dyn Clock + Send + Sync>,
    pub redactor: Arc<dyn Redactor + Send + Sync>,
    pub tx: mpsc::WeakSender<Command>,
    pub info: Option<RunInfo>,
    pub metadata: Option<crate::proj::RunMetadata>,
    pub store: Option<Arc<dyn EventStore>>,
    pub agents: BTreeMap<String, Agent>,
    pub profiles: BTreeMap<String, Arc<AgentProfile>>,
    pub children: BTreeMap<String, super::children::Child>,
    pub child_journals: BTreeMap<String, super::child_journals::ChildJournal>,
    pub running: BTreeMap<String, Job>,
    pub commands: BTreeMap<String, super::commands::CommandRun>,
    pub native_subagents: BTreeMap<String, super::public_subagents::NativeSubagent>,
    pub native_subagent_queue: VecDeque<String>,
    pub native_message_ingress: usize,
    pub native_model_policies: BTreeMap<String, super::public_subagents::SubagentModelPolicy>,
    pub active_contexts: BTreeMap<String, Context>,
    pub native_sampling: Arc<Semaphore>,
    pub pending: BTreeMap<String, super::tools::Pending>,
    pub grants: Vec<crate::perm::PermissionGrant>,
    pub last_tool: Option<(String, u32)>,
    pub jobs: JoinSet<Completion>,
    pub providers: Arc<Semaphore>,
    pub tools: Arc<Semaphore>,
    pub stopping: Option<(Option<String>, Reply<()>)>,
    pub fault: Option<String>,
    pub rewind: Option<(
        crate::conversation_rewind::RewindPoint,
        Reply<crate::conversation_rewind::RewindPoint>,
    )>,
    pub counter: u64,
    pub last_seq: u64,
    pub tool_state: crate::tool::ToolRunState,
    pub subagent_history: crate::subagent::SubagentHistory,
    pub projection_owner: Option<String>,
    pub raw_tool_results: BTreeMap<String, (String, Result<crate::tool::ToolResult, String>)>,
    pub stopped_sessions: std::collections::BTreeSet<String>,
    pub killed_agents: std::collections::BTreeSet<String>,
    pub compacting: BTreeMap<String, (u64, CancellationToken)>,
    pub edits: BTreeMap<String, super::edits::PendingEdit>,
    pub snapshots: BTreeMap<String, super::workspace::Snapshot>,
}
pub(super) struct Agent {
    pub info: AgentRuntimeInfo,
    pub profile: Arc<AgentProfile>,
    pub target: Option<ResolvedModelTarget>,
    pub settings: AgentModelSettings,
    pub policy: PermissionPolicy,
    pub messages: Context,
    pub queue: VecDeque<super::turn::Turn>,
    pub busy: bool,
    pub execution: crate::subagent::ResolvedSubagentContext,
    pub cwd: PathBuf,
    pub tool_state: crate::tool::ToolRunState,
    pub generation: u64,
    pub prompt_turns: u32,
    pub tool_calls: u32,
    pub pending_model_tools: BTreeMap<String, (String, bool)>,
    pub tools_used: Vec<String>,
    pub error_count: u32,
    pub native_context_tokens: Option<u64>,
    pub native_context_window: Option<u64>,
    pub attempt: Option<String>,
    pub attempt_started_ms: u64,
    pub notification_seq: u64,
    pub finalized: Option<crate::subagent::FinalizedAgentStateReferenceV1>,
    pub source_reference: Option<Box<crate::subagent::FinalizedAgentStateReferenceV1>>,
    pub skill_startup: Option<Arc<crate::config::SkillStartupSnapshot>>,
    pub skill_preloads: Option<Vec<(String, String)>>,
}

pub(super) struct Job {
    pub join_id: Option<Id>,
    pub actor: EventActor,
    pub kind: JobKind,
    pub parent: Option<String>,
    pub cancellation: CancellationToken,
    pub reason: Option<String>,
    pub hooks: Vec<HookExecutionMetadata>,
}
pub(super) enum JobKind {
    SubagentPreparation {
        agent: String,
    },
    Command,
    Turn {
        agent: String,
    },
    Tool {
        tool_id: String,
        reply: Option<Reply<ToolResult>>,
        capability: crate::tool::ToolCapability,
        paths: Vec<PathBuf>,
    },
}
pub(super) enum Completion {
    SubagentPrepared {
        id: String,
        result: Result<super::public_subagents::PreparedSubagent, CoordinatorError>,
    },
    Command {
        id: String,
        result: Result<super::commands::CommandExit, CoordinatorError>,
    },
    Turn {
        id: String,
        messages: Context,
        result: Result<String, CoordinatorError>,
    },
    Tool {
        id: String,
        result: Result<ToolResult, CoordinatorError>,
    },
}
impl Runtime {
    pub fn new(
        mut config: CoordinatorConfig,
        clock: Arc<dyn Clock + Send + Sync>,
        redactor: Arc<dyn Redactor + Send + Sync>,
        tx: mpsc::WeakSender<Command>,
    ) -> Self {
        let fault = config
            .secret_registry
            .register(
                std::mem::take(&mut config.secret_values)
                    .into_iter()
                    .chain(config.tool_registry.secret_values())
                    .chain(
                        config
                            .hook_runtime_config
                            .hooks
                            .lifecycle
                            .iter()
                            .flat_map(|h| h.env.values().cloned()),
                    )
                    .chain(
                        config
                            .formatter
                            .overrides
                            .values()
                            .filter_map(|f| f.environment.as_ref())
                            .flat_map(|env| env.values().cloned()),
                    ),
            )
            .err()
            .map(str::to_owned);
        let fault = fault.or_else(|| {
            if config.hook_runtime_config.hooks.lifecycle.len() > 64 {
                return Some("at most 64 lifecycle hooks are allowed".into());
            }
            config
                .hook_runtime_config
                .hooks
                .lifecycle
                .iter()
                .find_map(|h| h.validate().err().map(|e| e.to_string()))
        });
        let redactor = Arc::new(crate::redact::SecretRedactor::new(
            redactor,
            Arc::clone(&config.secret_registry),
        ));
        Self {
            native_sampling: Arc::new(Semaphore::new(config.subagents.sampling_limit.max(1))),
            profiles: std::mem::take(&mut config.agent_profiles)
                .into_iter()
                .map(|(name, profile)| (name, Arc::new(profile)))
                .collect(),
            tools: Arc::new(Semaphore::new(config.tool_concurrency.max(1))),
            providers: Arc::new(Semaphore::new(config.provider_model_concurrency.max(1))),
            config,
            clock,
            redactor,
            tx,
            info: None,
            metadata: None,
            store: None,
            agents: BTreeMap::new(),
            children: BTreeMap::new(),
            child_journals: BTreeMap::new(),
            running: BTreeMap::new(),
            commands: BTreeMap::new(),
            native_subagents: BTreeMap::new(),
            native_subagent_queue: VecDeque::new(),
            native_message_ingress: 0,
            native_model_policies: BTreeMap::new(),
            active_contexts: BTreeMap::new(),
            pending: BTreeMap::new(),
            grants: Vec::new(),
            last_tool: None,
            jobs: JoinSet::new(),
            stopping: None,
            fault,
            rewind: None,
            counter: 0,
            last_seq: 0,
            tool_state: Default::default(),
            subagent_history: Default::default(),
            projection_owner: None,
            raw_tool_results: BTreeMap::new(),
            stopped_sessions: Default::default(),
            killed_agents: Default::default(),
            edits: BTreeMap::new(),
            snapshots: BTreeMap::new(),
            compacting: BTreeMap::new(),
        }
    }
    pub async fn run(mut self, mut rx: mpsc::Receiver<Command>, shutdown: CancellationToken) {
        let mut closing = false;
        loop {
            if closing && self.jobs.is_empty() {
                break;
            }
            let deadline = self.pending.values().filter_map(|p| p.deadline).min();
            let progress_deadline = self.native_progress_deadline();
            tokio::select! {
                () = shutdown.cancelled(), if !closing => {
                    closing = true;
                    let (reply, _) = oneshot::channel();
                    let _ = self.stop(Some("coordinator owner disconnected".into()), reply);
                }
                command = rx.recv(), if !closing => match command {
                    Some(command) => command(&mut self),
                    None => {
                        closing = true;
                        let (reply, _) = oneshot::channel();
                        let _ = self.stop(Some("coordinator command channel disconnected".into()), reply);
                    },
                },
                done = self.jobs.join_next_with_id(), if !self.jobs.is_empty() => {
                    match done {
                        Some(Ok((_, result))) => { let _ = self.finished(result); }
                        Some(Err(error)) => self.worker_failed(error),
                        None => {},
                    }
                }
                () = async {
                    if let Some(deadline) = deadline { tokio::time::sleep_until(deadline).await; }
                    else { std::future::pending::<()>().await; }
                } => self.expire_permissions(),
                () = async {
                    if let Some(deadline) = progress_deadline { tokio::time::sleep_until(deadline).await; }
                    else { std::future::pending::<()>().await; }
                } => self.publish_native_progress(),
            }
            self.finish_stop().await;
            self.finish_rewind();
        }
        self.jobs.shutdown().await;
        if let Some(info) = &self.info {
            let _ = self
                .config
                .tool_registry
                .close_run(info.run_id.as_str())
                .await;
        }
        if let Some(store) = &self.store {
            let _ = store.close_writer();
        }
    }
    pub fn info(&self) -> Result<&RunInfo, CoordinatorError> {
        self.info.as_ref().ok_or(CoordinatorError::RunNotStarted)
    }
    pub fn accepting(&self) -> Result<(), CoordinatorError> {
        self.info()?;
        if let Some(fault) = &self.fault {
            return Err(CoordinatorError::Invalid(fault.clone()));
        }
        if self.stopping.is_some() || self.rewind.is_some() {
            Err(CoordinatorError::Stopping)
        } else {
            Ok(())
        }
    }
    pub fn handle(&self) -> Result<CoordinatorHandle, CoordinatorError> {
        Ok(CoordinatorHandle {
            tx: self.tx.upgrade().ok_or(CoordinatorError::Closed)?,
            _owner: None,
        })
    }
    pub fn id(&mut self, prefix: &str) -> Result<String, CoordinatorError> {
        self.counter = self
            .counter
            .checked_add(1)
            .ok_or_else(|| CoordinatorError::Invalid("session identifiers exhausted".into()))?;
        Ok(format!("{prefix}-{}", self.counter))
    }
    pub fn emit(
        &mut self,
        actor: EventActor,
        correlation: Option<String>,
        payload: EventV1,
    ) -> Result<EventEnvelopeV1, CoordinatorError> {
        self.emit_applied(actor, correlation, payload, |_, _| {})
    }
    pub(super) fn emit_applied(
        &mut self,
        actor: EventActor,
        correlation: Option<String>,
        mut payload: EventV1,
        apply: impl FnOnce(&mut Self, &EventEnvelopeV1),
    ) -> Result<EventEnvelopeV1, CoordinatorError> {
        self.child_lineage(&actor, &mut payload);
        let payload = crate::redact::redact_event_payload(self.redactor.as_ref(), payload)?;
        let event = crate::store::EventEnvelopeWithoutSeqV1 {
            schema_version: SCHEMA_VERSION,
            event_id: self.id("event")?,
            run_id: self.info()?.run_id.clone(),
            mono_ms: self.clock.mono_ms(),
            ts: self.clock.system_time_rfc3339_millis(),
            actor,
            correlation_id: correlation,
            causation_id: None,
            stream_key: None,
            payload,
        };
        let store = self.store.clone().ok_or(CoordinatorError::RunNotStarted)?;
        let mut apply = Some(apply);
        let appended = store.append_applied(event, &mut |event| {
            if let Some(apply) = apply.take() {
                apply(self, event);
            }
            if let Some(agent) = event
                .actor
                .agent_id
                .as_ref()
                .and_then(|id| self.agents.get_mut(id))
            {
                agent.observe_activity(event);
            }
        });
        match appended {
            Ok(event) => {
                self.last_seq = event.seq;
                if let Err(error) = self.publish_child_event(&event) {
                    self.storage_failed(&error);
                    return Err(error.into());
                }
                Ok(event)
            }
            Err(error) => {
                self.storage_failed(&error);
                Err(error.into())
            }
        }
    }
    pub fn storage_failed(&mut self, error: &EventStoreError) {
        let message = self
            .redactor
            .redact_text(&format!("session storage failed: {error}"));
        self.fault = Some(message.clone());
        self.fail_child_waiters(&message);
        for agent in self.agents.values_mut() {
            agent.queue.clear();
        }
        self.pending.clear();
        self.running.retain(|_, job| {
            job.cancellation.cancel();
            if job.join_id.is_some() {
                return true;
            }
            let reply = match &mut job.kind {
                JobKind::Tool { reply, .. } => reply.take(),
                JobKind::Turn { .. } | JobKind::Command | JobKind::SubagentPreparation { .. } => {
                    None
                }
            };
            if let Some(reply) = reply {
                let _ = reply.send(Err(CoordinatorError::Invalid(message.clone())));
            }
            false
        });
        self.fail_command_waiters(&message);
        if let (Some(store), Some(info)) = (&self.store, &self.info) {
            store.publish_live(LiveEventEnvelope {
                event_id: "storage-failure".into(),
                run_id: info.run_id.clone(),
                mono_ms: self.clock.mono_ms(),
                ts: None,
                actor: system(),
                correlation_id: None,
                causation_id: None,
                stream_key: None,
                payload: LiveEventV1::RuntimeWarning { message },
            });
        }
    }
    pub fn live(
        &mut self,
        actor: EventActor,
        task: String,
        payload: LiveEventV1,
    ) -> Result<(), CoordinatorError> {
        if !matches!(
            payload,
            LiveEventV1::CompactionProgress { preview: None, .. }
        ) {
            self.check_task(&task)?;
        }
        let event = LiveEventEnvelope {
            event_id: self.id("live")?,
            run_id: self.info()?.run_id.clone(),
            mono_ms: self.clock.mono_ms(),
            ts: None,
            actor,
            correlation_id: Some(task),
            causation_id: None,
            stream_key: None,
            payload,
        };
        if let Some(store) = &self.store {
            let mut value = serde_json::to_value(event)?;
            crate::redact::redact_in_place(self.redactor.as_ref(), &mut value);
            store.publish_live(serde_json::from_value(value)?);
        }
        Ok(())
    }
    pub fn check_task(&self, id: &str) -> Result<(), CoordinatorError> {
        let task = self
            .running
            .get(id)
            .ok_or_else(|| CoordinatorError::UnknownTask(id.into()))?;
        if task.cancellation.is_cancelled() {
            Err(CoordinatorError::Cancelled(id.into()))
        } else {
            Ok(())
        }
    }
    fn worker_failed(&mut self, error: tokio::task::JoinError) {
        let Some((id, job)) = self
            .running
            .iter()
            .find(|(_, j)| j.join_id == Some(error.id()))
        else {
            return;
        };
        let lost_context = matches!(job.kind, JobKind::Turn { .. });
        let message = "worker stopped unexpectedly";
        let failure = CoordinatorError::Invalid(message.into());
        let completion = match job.kind {
            JobKind::Turn { .. } => Completion::Turn {
                id: id.clone(),
                messages: Context::default(),
                result: Err(failure),
            },
            JobKind::Tool { .. } => Completion::Tool {
                id: id.clone(),
                result: Err(failure),
            },
            JobKind::Command => Completion::Command {
                id: id.clone(),
                result: Err(failure),
            },
            JobKind::SubagentPreparation { .. } => Completion::SubagentPrepared {
                id: id.clone(),
                result: Err(failure),
            },
        };
        if lost_context {
            self.fault = Some(message.into());
        }
        let _ = self.finished(completion);
        if lost_context && self.stopping.is_none() {
            let (reply, _) = oneshot::channel();
            let _ = self.stop(Some(message.into()), reply);
        }
    }
}
pub(super) fn digest(value: &str) -> String {
    blake3::hash(value.as_bytes()).to_hex().to_string()
}
