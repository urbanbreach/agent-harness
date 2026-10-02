use super::{runtime::*, *};
use harness_providers::{AssistantToolCall, CompletionMessage, MessageRole};
use std::mem;
mod request;

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
    pub(super) handle: CoordinatorHandle,
    pub(super) provider: Arc<dyn harness_providers::Provider>,
    pub(super) permits: Arc<tokio::sync::Semaphore>,
    pub(super) profile: Arc<AgentProfile>,
    prompt_source: Option<Arc<crate::model_resolution::PromptSource>>,
    workspace: PathBuf,
    pub(super) compaction: crate::config::CompactionSettings,
    pub(super) tools: Vec<harness_providers::ToolDef>,
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
        let submitted = self.emit(
            actor.clone(),
            Some(id.clone()),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: id.clone().into(),
                text: prompt.text.clone(),
            }),
        )?;
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
                seq: submitted.seq,
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
            handle,
            provider: Arc::clone(&self.config.provider),
            permits: Arc::clone(&self.providers),
            tools: self.config.tool_registry.definitions(
                &agent.profile,
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
        let join = self.jobs.spawn(worker.run(messages));
        self.running.insert(
            id,
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
        Ok(())
    }
}
impl Worker {
    async fn run(mut self, mut messages: super::context::Context) -> Completion {
        let (id, actor) = (self.turn.id.clone(), self.actor.clone());
        let started = self
            .handle
            .call(move |s| {
                s.check_task(&id)?;
                s.emit_hooked(
                    actor.clone(),
                    Some(id.clone()),
                    EventV1::TaskScheduled(TaskScheduledEvent {
                        task_id: id.into(),
                        state: TaskScheduleState::Started,
                        queue_key: actor.agent_id,
                        metadata: None,
                    }),
                )
                .map(|_| ())
            })
            .await;
        if let Err(error) = started {
            if let Some(manual) = self.turn.manual.take() {
                let _ = manual
                    .reply
                    .send(Err(CoordinatorError::Invalid(error.to_string())));
            }
            return Completion::Turn {
                id: self.turn.id,
                messages,
                result: Err(error),
            };
        }
        self.update_prompt(&mut messages);
        if let Some(manual) = self.turn.manual.take() {
            let outcome = self
                .compact(
                    &mut messages,
                    manual.through.as_deref(),
                    &manual.reason,
                    manual.instructions.as_deref(),
                    None,
                )
                .await;
            let result = outcome
                .as_ref()
                .map(|_| "Context compaction finished.".into())
                .map_err(|e| CoordinatorError::Invalid(e.to_string()));
            let _ = manual.reply.send(outcome);
            return Completion::Turn {
                id: self.turn.id,
                messages,
                result,
            };
        }
        let mut dispatched = false;
        let result = async {
            let context = self.prompt_context(&self.turn.prompt.tags).await?;
            let mut text = self.turn.prompt.text.clone();
            if !context.is_empty() {
                text.push_str(&format!("\n\nSelected context:\n{context}"));
            }
            messages.push(
                CompletionMessage::text(MessageRole::User, text),
                self.turn.seq,
                Some(&self.turn.id),
            );
            if let Some(entry) = messages.entries.last_mut() {
                entry.attachments = mem::take(&mut self.turn.prompt.attachments);
            }
            self.converse(&mut messages, &mut dispatched).await
        }
        .await;
        if result.is_err() && !dispatched {
            messages.discard_turn(&self.turn.id);
        }
        Completion::Turn {
            id: self.turn.id,
            messages,
            result,
        }
    }
    async fn converse(
        &mut self,
        messages: &mut super::context::Context,
        dispatched: &mut bool,
    ) -> Result<String, CoordinatorError> {
        let mut iteration = 0;
        loop {
            if self.cancellation.is_cancelled() {
                return Err(CoordinatorError::Cancelled(self.turn.id.clone()));
            }
            if self
                .profile
                .max_iters
                .is_some_and(|limit| iteration >= limit)
            {
                return Err(CoordinatorError::Invalid(
                    "agent iteration limit reached".into(),
                ));
            }
            iteration += 1;
            let (request_id, response) = loop {
                match self.dispatch(messages, dispatched).await {
                    Err(error @ CoordinatorError::Provider { .. }) => {
                        let next = self.fallbacks.pop_front().ok_or(error)?;
                        self.turn.model = next.model_ref.clone();
                        self.turn.settings = (&next).into();
                        self.turn.target = Some(next);
                        self.update_prompt(messages);
                    }
                    result => break result?,
                }
            };
            let calls = response.calls;
            if let Some(request) = response.logical_request {
                messages.model_request = Some(Box::new(request));
            }
            if response.settled_reasoning.is_none() {
                messages.unavailable =
                    Some(crate::subagent::FinalizedStateUnavailable::UnsupportedReasoning);
            } else if !response.usage_complete {
                messages.unavailable = Some(crate::subagent::FinalizedStateUnavailable::Incomplete);
            }
            messages
                .usage
                .push(crate::subagent::FinalizedProviderUsage {
                    request_id: request_id.clone(),
                    attempt_id: self.turn.id.clone(),
                    model_ref: self.turn.model.clone(),
                    usage: response.usage,
                    thinking: response.thinking,
                    usage_complete: response.usage_complete,
                    settled_reasoning: response.settled_reasoning.clone(),
                });
            messages.push(
                CompletionMessage {
                    role: MessageRole::Assistant,
                    content: response.text.clone(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: (!calls.is_empty()).then(|| calls.clone()),
                },
                response.event_seq,
                Some(&self.turn.id),
            );
            if let Some(entry) = messages.entries.last_mut() {
                entry.settled_reasoning = response.settled_reasoning.unwrap_or_default();
            }
            if calls.is_empty() {
                return Ok(response.text);
            }
            self.run_tools(&request_id, response.event_seq, calls, messages)
                .await?;
            self.tools = self
                .registry
                .definitions(&self.profile, (&self.permissions.0, &self.permissions.1));
        }
    }
    async fn run_tools(
        &self,
        request: &str,
        message_seq: u64,
        calls: Vec<AssistantToolCall>,
        messages: &mut super::context::Context,
    ) -> Result<(), CoordinatorError> {
        let mut workers = tokio::task::JoinSet::new();
        for (index, call) in calls.iter().enumerate() {
            let (handle, actor, parent) = (
                self.handle.clone(),
                self.actor.clone(),
                self.turn.id.clone(),
            );
            let id = format!("{request}-tool-{index}");
            let call = call.clone();
            workers.spawn(async move {
                let result = match serde_json::from_str(&call.arguments_json) {
                    Ok(args) => {
                        handle
                            .execute_tool(actor, Some(parent), Some(id), call.function_name, args)
                            .await
                    }
                    Err(error) => Err(CoordinatorError::Json(error)),
                };
                (index, result)
            });
        }
        let mut outputs = BTreeMap::new();
        while let Some(result) = workers.join_next().await {
            let (index, output) = result.map_err(|_| {
                CoordinatorError::Invalid("tool dispatch stopped unexpectedly".into())
            })?;
            outputs.insert(index, output);
        }
        let mut failed = false;
        for (index, call) in calls.into_iter().enumerate() {
            let output = outputs
                .remove(&index)
                .ok_or_else(|| CoordinatorError::Invalid("tool output missing".into()))?;
            let (text, attachments) = match output {
                Ok(output) => {
                    failed |= output.is_error();
                    (
                        crate::tool::provider_text(output.display_text, output.structured_json),
                        output.attachments,
                    )
                }
                Err(error) => {
                    failed = true;
                    (format!("Tool error: {error}"), Vec::new())
                }
            };
            let raw_id = format!("{request}-tool-{index}");
            let owner = self.actor.agent_id.clone();
            let raw = self
                .handle
                .call(move |s| match s.raw_tool_results.remove(&raw_id) {
                    Some((agent, output)) if owner.as_deref() == Some(&agent) => {
                        let (output, error) = match output {
                            Ok(output) => (Some(output), None),
                            Err(error) => (None, Some(error)),
                        };
                        Ok(Some(crate::subagent::FinalizedToolResult {
                            tool_call_id: raw_id,
                            provider_tool_call_id: None,
                            output,
                            error,
                        }))
                    }
                    Some(_) => Err(CoordinatorError::PermissionDenied(
                        "raw tool result owner mismatch".into(),
                    )),
                    None => Ok(None),
                })
                .await?;
            if raw.is_none() {
                messages.unavailable = Some(crate::subagent::FinalizedStateUnavailable::Incomplete);
            }
            let provider_call_id = call.tool_call_id.clone();
            messages.push(
                CompletionMessage {
                    role: MessageRole::Tool,
                    content: text,
                    name: Some(call.function_name),
                    tool_call_id: Some(call.tool_call_id),
                    assistant_tool_calls: None,
                },
                message_seq,
                Some(&self.turn.id),
            );
            if let Some(entry) = messages.entries.last_mut() {
                entry.attachments = attachments;
                entry.raw_tool_result = raw.map(|mut raw| {
                    raw.provider_tool_call_id = Some(provider_call_id);
                    raw
                });
            }
        }
        if self.cancellation.is_cancelled() {
            return Err(CoordinatorError::Cancelled(self.turn.id.clone()));
        }
        if failed && self.profile.tool_failure_mode == crate::config::ToolFailureMode::FailTurn {
            return Err(CoordinatorError::Invalid("tool execution failed".into()));
        }
        Ok(())
    }
}
