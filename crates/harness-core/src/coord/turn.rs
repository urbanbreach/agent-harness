use super::{runtime::*, *};
use harness_providers::MessageRole;
use std::mem;
mod request;
mod skills;
mod worker;

pub(super) struct Turn {
    pub id: String,
    pub(super) prompt: super::prompt::Prompt,
    pub(super) model: String,
    pub(super) settings: AgentModelSettings,
    pub(super) target: Option<ResolvedModelTarget>,
    seq: u64,
    inherited_selection: bool,
    user_initiated: bool,
    pub manual: Option<super::compaction::Manual>,
}
pub(super) struct Worker {
    pub(super) tool_scope: Option<String>,
    pub(super) handle: CoordinatorHandle,
    pub(super) provider: Arc<dyn harness_providers::Provider>,
    pub(super) permits: Arc<tokio::sync::Semaphore>,
    pub(super) profile: Arc<AgentProfile>,
    prompt_source: Option<Arc<crate::model_resolution::PromptSource>>,
    workspace: PathBuf,
    pub(super) compaction: crate::config::CompactionSettings,
    pub(super) tools: Vec<harness_providers::ToolDef>,
    native_schema: Value,
    native: bool,
    skill_metadata: Option<String>,
    skill_preload_names: Vec<String>,
    skill_preloads: Option<Vec<(String, String)>>,
    registry: Arc<crate::tool::ToolRegistry>,
    permissions: (PermissionPolicy, PermissionPolicy),
    pub(super) actor: EventActor,
    pub(super) session: String,
    pub(super) cancellation: CancellationToken,
    pub(super) turn: Turn,
    fallbacks: std::collections::VecDeque<ResolvedModelTarget>,
    retry: crate::config::ProviderRetryRuntimeConfig,
}
impl Runtime {
    pub fn queue_turn(
        &mut self,
        mut actor: EventActor,
        agent_id: &str,
        mut prompt: super::prompt::Prompt,
        model: Option<String>,
        settings: Option<AgentModelSettings>,
        target: Option<ResolvedModelTarget>,
    ) -> Result<String, CoordinatorError> {
        self.accepting()?;
        self.execution_cwd(&EventActor::new(ActorKind::Worker, Some(agent_id.into())))?;
        self.ensure_skill_startup(agent_id)?;
        if self.killed_agents.contains(agent_id)
            || self.stopped_sessions.contains(agent_id)
            || self.stopped_sessions.contains(self.info()?.run_id.as_str())
        {
            return Err(CoordinatorError::Stopping);
        }
        let user_initiated = actor.kind == ActorKind::User;
        prompt.validate(self.redactor.as_ref())?;
        let agent = self
            .agents
            .get(agent_id)
            .ok_or_else(|| CoordinatorError::UnknownAgent(agent_id.into()))?;
        self.check_turn_capacity(agent_id)?;
        let inherited_selection = model.is_none() && settings.is_none() && target.is_none();
        let explicit_model = model.is_some();
        let target = target.or_else(|| {
            if model.is_none() {
                agent.target.clone()
            } else {
                None
            }
        });
        let model = model
            .or_else(|| target.as_ref().map(|t| t.model_ref.clone()))
            .unwrap_or_else(|| agent.info.model_ref.clone());
        let settings = settings
            .or_else(|| target.as_ref().map(AgentModelSettings::from))
            .unwrap_or_else(|| {
                if explicit_model {
                    AgentModelSettings::default()
                } else {
                    agent.settings.clone()
                }
            });
        let id = match prompt.reserved_id.take() {
            Some(id) => id,
            None => self.id("turn")?,
        };
        if let Some(child) = self.children.get_mut(agent_id) {
            child.request.clone_from(&id);
            child.complete = false;
        }
        actor.agent_id = Some(agent_id.into());
        let _ = self.persist_attachments(&actor, None, &prompt.attachments)?;
        // Completion wakes resolve their digest when admitted to the worker.
        // A queued wake may be consumed by a tool result before it starts.
        let submitted_seq = if prompt.child_completion.is_some() {
            0
        } else {
            self.emit(
                actor.clone(),
                Some(id.clone()),
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: id.clone().into(),
                    text: prompt.text.clone(),
                }),
            )?
            .seq
        };
        if !prompt.attachments.is_empty() {
            self.emit(
                actor.clone(),
                Some(id.clone()),
                EventV1::PromptAttachmentsSubmitted(PromptAttachmentsSubmittedEvent {
                    request_id: id.clone().into(),
                    attachments: prompt.attachments.clone(),
                }),
            )?;
        }
        self.emit(
            actor,
            Some(id.clone()),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: id.clone().into(),
                state: TaskScheduleState::Queued,
                queue_key: Some(agent_id.into()),
                metadata: None,
            }),
        )?;
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.queue.push_back(Turn {
                id: id.clone(),
                prompt,
                model,
                settings,
                target,
                seq: submitted_seq,
                manual: None,
                inherited_selection,
                user_initiated,
            });
        }
        self.start_next(agent_id)?;
        Ok(id)
    }
    pub fn queue_compaction(
        &mut self,
        agent_id: &str,
        manual: super::compaction::Manual,
    ) -> Result<(), CoordinatorError> {
        self.accepting()?;
        let agent = self
            .agents
            .get(agent_id)
            .ok_or_else(|| CoordinatorError::UnknownAgent(agent_id.into()))?;
        if !agent.busy
            && agent.queue.is_empty()
            && (!self.config.compaction.enabled
                || super::compaction::plan::Plan::new(
                    &agent.messages,
                    &self.config.compaction,
                    self.config.compaction.keep_recent_tokens,
                    manual.through.as_deref(),
                )?
                .is_none())
        {
            let _ = manual.reply.send(Ok(ManualCompactionOutcome::NoOp));
            return Ok(());
        }
        self.check_turn_capacity(agent_id)?;
        let (model, settings, target) = (
            agent.info.model_ref.clone(),
            agent.settings.clone(),
            agent.target.clone(),
        );
        let id = self.id("compact")?;
        self.emit(
            EventActor::new(ActorKind::Worker, Some(agent_id.into())),
            Some(id.clone()),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: id.clone().into(),
                state: TaskScheduleState::Queued,
                queue_key: Some(agent_id.into()),
                metadata: None,
            }),
        )?;
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.queue.push_back(Turn {
                id,
                model,
                settings,
                target,
                prompt: super::prompt::Prompt::default(),
                user_initiated: false,
                seq: 0,
                manual: Some(manual),
                inherited_selection: true,
            });
        }
        self.start_next(agent_id)
    }
    pub fn start_next(&mut self, agent_id: &str) -> Result<(), CoordinatorError> {
        if self.stopping.is_some() || self.rewind.is_some() || self.fault.is_some() {
            return Ok(());
        }
        let handle = self.handle()?;
        let session = self.info()?.run_id.to_string();
        let native_schema = self.native_subagent_schema(agent_id)?;
        let native = self.native_subagents.contains_key(agent_id);
        let tool_scope = self.tool_scope(Some(agent_id));
        let workspace =
            self.execution_cwd(&EventActor::new(ActorKind::Worker, Some(agent_id.into())))?;
        if self
            .agents
            .get(agent_id)
            .is_some_and(|a| !a.busy && !a.queue.is_empty())
        {
            let attempt = self.agents[agent_id]
                .queue
                .front()
                .map(|t| t.id.clone())
                .ok_or_else(|| CoordinatorError::Invalid("attempt queue disappeared".into()))?;
            self.begin_agent_attempt(agent_id, &attempt)?;
        }
        let agent = self
            .agents
            .get_mut(agent_id)
            .ok_or_else(|| CoordinatorError::UnknownAgent(agent_id.into()))?;
        if agent.busy {
            return Ok(());
        }
        let Some(mut turn) = agent.queue.pop_front() else {
            return Ok(());
        };
        let session = if agent.info.parent_agent_id.is_some() {
            agent.info.agent_id.clone()
        } else {
            session
        };
        if turn.inherited_selection {
            turn.model = agent.info.model_ref.clone();
            turn.target = agent.target.clone();
            turn.settings = agent.settings.clone();
        }
        let fallbacks = self
            .config
            .agent_model_fallbacks
            .get(&agent.profile.name)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let after = fallbacks
            .iter()
            .position(|t| t.model_ref == turn.model && t.variant == turn.settings.variant)
            .map_or_else(
                || {
                    if turn.model == agent.profile.model_ref {
                        0
                    } else {
                        fallbacks.len()
                    }
                },
                |index| index + 1,
            );
        let fallbacks = fallbacks[after..].iter().cloned().collect();
        agent.busy = true;
        agent.info.model_ref = turn.model.clone();
        agent.target = turn.target.clone();
        agent.settings = turn.settings.clone();
        let messages = mem::take(&mut agent.messages);
        let actor = EventActor::new(ActorKind::Worker, Some(agent_id.into()));
        let cancellation = CancellationToken::new();
        let worker = Worker {
            tool_scope: tool_scope.clone(),
            handle,
            provider: Arc::clone(&self.config.provider),
            permits: Arc::clone(if native {
                &self.native_sampling
            } else {
                &self.providers
            }),
            native_schema: native_schema.clone(),
            native,
            skill_preload_names: self
                .native_subagents
                .get(agent_id)
                .map(super::public_subagents::NativeSubagent::preload_skills)
                .unwrap_or_default(),
            skill_preloads: agent.skill_preloads.clone(),
            skill_metadata: agent
                .skill_startup
                .as_ref()
                .filter(|snapshot| native || !snapshot.catalog.entries.is_empty())
                .map(|snapshot| {
                    let entries: Vec<_> = snapshot
                        .catalog
                        .entries
                        .iter()
                        .map(|entry| {
                            serde_json::json!({
                                "stable_id": entry.stable_id,
                                "name": entry.name,
                                "description": entry.description,
                                "source_scope": entry.source_scope,
                                "loadable": entry.loadable,
                                "permission_mode": entry.permission_mode,
                                "status": entry.status.as_str(),
                                "reason": entry.reason,
                                "argument_hint": entry.argument_hint,
                                "allowed_tools": entry.allowed_tools,
                                "body_loaded": entry.body_loaded,
                            })
                        })
                        .collect();
                    crate::redact::redact_value(
                        self.redactor.as_ref(),
                        &serde_json::json!({"available_skills": entries}),
                    )
                    .to_string()
                }),
            tools: self.config.tool_registry.definitions(
                &agent.profile,
                tool_scope.as_deref(),
                (&self.config.permission_policy, &agent.policy),
            ),
            registry: Arc::clone(&self.config.tool_registry),
            permissions: (self.config.permission_policy.clone(), agent.policy.clone()),
            profile: Arc::clone(&agent.profile),
            prompt_source: self
                .config
                .agent_prompt_sources
                .get(&agent.profile.name)
                .cloned(),
            workspace,
            compaction: self.config.compaction.clone(),
            actor: actor.clone(),
            session,
            cancellation: cancellation.clone(),
            turn,
            fallbacks,
            retry: self.config.provider_retry.clone(),
        };
        let id = worker.turn.id.clone();
        self.record_selection(agent_id)?;
        let mut worker = worker;
        super::public_subagent_hooks::apply_native_schema_hints(
            &mut worker.tools,
            &native_schema,
            native,
        );
        let join = self.jobs.spawn(worker.run(messages));
        self.running.insert(
            id.clone(),
            Job {
                join_id: Some(join.id()),
                actor,
                kind: JobKind::Turn {
                    agent: agent_id.into(),
                },
                parent: None,
                cancellation,
                reason: None,
                hooks: Vec::new(),
            },
        );
        self.native_subagent_started(agent_id, &id)?;
        Ok(())
    }
}
