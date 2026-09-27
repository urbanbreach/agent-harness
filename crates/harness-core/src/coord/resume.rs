use super::{handle::system, runtime::*, *};
use crate::conversation_rewind::RewindPoint;
use std::collections::VecDeque;

impl CoordinatorHandle {
    pub async fn resume_run(
        &self,
        id: impl Into<String>,
        name: impl Into<String>,
    ) -> Result<RunInfo, CoordinatorError> {
        let (id, name) = (id.into(), name.into());
        self.call(move |s| s.resume(&id, name)).await
    }
    pub async fn rewind_points(&self) -> Result<Option<Vec<RewindPoint>>, CoordinatorError> {
        self.call(|s| {
            if s.info.is_none() {
                return Ok(None);
            }
            Ok(Some(crate::conversation_rewind::rewind_points(
                &crate::store::read_events(&s.info()?.events_path)?,
            )))
        })
        .await
    }
    pub async fn rewind_conversation(
        &self,
        request: impl Into<String>,
    ) -> Result<RewindPoint, CoordinatorError> {
        let request = request.into();
        let (tx, rx) = oneshot::channel();
        self.call(move |s| s.request_rewind(&request, tx)).await?;
        rx.await.map_err(|_| CoordinatorError::Closed)?
    }
}
impl Runtime {
    fn resume(&mut self, id: &str, name: String) -> Result<RunInfo, CoordinatorError> {
        if let Some(error) = &self.fault {
            return Err(CoordinatorError::Invalid(error.clone()));
        }
        if self.info.is_some() {
            return Err(CoordinatorError::RunAlreadyStarted);
        }
        crate::store::validate_session_id(id)?;
        let dir = self.config.session_dir.join(id);
        if std::fs::symlink_metadata(&dir)?.file_type().is_symlink() {
            return Err(CoordinatorError::Invalid(
                "session directory cannot be a symlink".into(),
            ));
        }
        let metadata = crate::proj::read_run_metadata(&dir)?;
        if metadata.as_ref().is_some_and(|m| {
            m.run_id != id
                || matches!(
                    m.mode_source,
                    Some(
                        crate::proj::SessionModeSource::ReplayOnly
                            | crate::proj::SessionModeSource::ScenarioFixture
                    )
                )
        }) {
            return Err(CoordinatorError::Invalid(
                "session metadata does not permit continuation".into(),
            ));
        }
        let journal = crate::store::Journal::open_existing(
            &self.config.session_dir,
            id,
            self.config.deterministic_store,
        )?;
        let mut events = crate::store::read_events(journal.file_path())?;
        crate::proj::checked_history(&events)
            .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
        let mut interrupted = crate::proj::InFlight::default();
        for event in &events {
            interrupted.apply(event);
        }
        let start = events
            .iter()
            .find_map(|e| match &e.payload {
                EventV1::RunStarted(e) => Some(e),
                _ => None,
            })
            .ok_or_else(|| CoordinatorError::Invalid("journal has no run start".into()))?;
        let root = PathBuf::from(&start.workspace_root).canonicalize()?;
        if !root.is_dir() {
            return Err(CoordinatorError::Invalid(
                "workspace is no longer a directory".into(),
            ));
        }
        let mut agents = self.restore_agents(
            &events,
            &dir,
            metadata
                .as_ref()
                .and_then(|m| m.recorded_runtime_context.as_ref()),
        )?;
        let grants = super::grants::load_grants(&root, &events)?;
        for (kind, request) in interrupted.open.keys() {
            if *kind == "user" {
                for agent in agents.values_mut() {
                    agent.messages.discard_turn(request);
                }
            }
        }
        self.counter = events
            .iter()
            .map(|e| id_number(&e.event_id).max(e.seq))
            .max()
            .unwrap_or(0);
        for event in &events {
            if let Some(id) = recorded_id(&event.payload) {
                self.counter = self.counter.max(id_number(id));
            }
        }
        let info = RunInfo {
            run_id: id.into(),
            run_name: name.clone().into(),
            workspace_root: root,
            run_dir: dir.clone(),
            artifacts_dir: dir.join("artifacts"),
            events_path: journal.file_path().into(),
        };
        self.info = Some(info.clone());
        self.store = Some(Arc::new(journal));
        self.agents = agents;
        self.grants = grants;
        self.fault = None;
        self.metadata = Some(metadata.unwrap_or(self.new_metadata()?));
        self.last_seq = events.last().map_or(0, |event| event.seq);
        if let Err(error) = self.restore_child_journals(&events) {
            self.child_journals.clear();
            self.store = None;
            self.info = None;
            self.metadata = None;
            self.agents.clear();
            self.grants.clear();
            return Err(error.into());
        }
        let was_active = interrupted.status == Some(crate::proj::RunStatus::Running);
        let terminals = interrupted.terminals(super::history::INTERRUPTED_TOOL_RESULT);
        for event in terminals {
            events.push(self.emit(event.actor, event.correlation_id, event.payload)?);
        }
        self.restore_children(&events);
        if was_active {
            self.emit(
                system(),
                None,
                EventV1::RunFailed(RunFailedEvent {
                    error: "session owner disconnected before the run finished".into(),
                }),
            )?;
        }
        self.emit(
            system(),
            None,
            EventV1::RunStarted(RunStartedEvent {
                run_name: name.into(),
                workspace_root: info.workspace_root.to_string_lossy().into(),
            }),
        )?;
        self.start_hooks()?;
        self.write_metadata()?;
        Ok(info)
    }
    fn restore_agents(
        &self,
        events: &[EventEnvelopeV1],
        run_dir: &std::path::Path,
        recorded: Option<&crate::proj::RecordedRuntimeContext>,
    ) -> Result<BTreeMap<String, Agent>, CoordinatorError> {
        let active = crate::conversation_rewind::active_events(events);
        let mut agents = BTreeMap::new();
        let primary = active.iter().find_map(|e| match &e.payload {
            EventV1::AgentSpawned(e) if e.parent_agent_id.is_none() => Some(e.agent_id.as_str()),
            _ => None,
        });
        for event in active.iter() {
            if let EventV1::AgentSpawned(e) = &event.payload {
                let profile = self
                    .profiles
                    .get(&e.profile)
                    .cloned()
                    .ok_or_else(|| CoordinatorError::UnknownProfile(e.profile.clone()))?;
                let policy = PermissionPolicy::from_rules(profile.permission_ruleset.clone())
                    .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
                let mut target = self.config.agent_model_targets.get(&e.profile).cloned();
                if let Some(context) = recorded.filter(|_| primary == Some(e.agent_id.as_str())) {
                    target = Some(recorded_target(context, target.as_ref(), &self.config)?);
                }
                let info = AgentRuntimeInfo {
                    agent_id: e.agent_id.clone(),
                    profile_name: e.profile.clone(),
                    model_ref: target
                        .as_ref()
                        .map_or_else(|| profile.model_ref.clone(), |t| t.model_ref.clone()),
                    model_ref_explicit: profile.model_ref_explicit,
                    toolset: profile.toolset.clone(),
                    parent_agent_id: e.parent_agent_id.clone(),
                };
                let messages = if e.parent_agent_id.is_some() {
                    super::context::Context::default()
                } else {
                    super::history::messages(
                        events,
                        &e.agent_id,
                        primary == Some(e.agent_id.as_str()),
                        &profile.system_prompt,
                        run_dir,
                    )?
                };
                agents.insert(
                    e.agent_id.clone(),
                    Agent {
                        info,
                        settings: target
                            .as_ref()
                            .map(AgentModelSettings::from)
                            .unwrap_or_default(),
                        target,
                        profile,
                        policy,
                        messages,
                        queue: VecDeque::new(),
                        busy: false,
                    },
                );
            }
            let EventV1::ProviderRequestStarted(e) = &event.payload else {
                continue;
            };
            let Some(agent) = event
                .actor
                .agent_id
                .as_ref()
                .and_then(|id| agents.get_mut(id))
            else {
                continue;
            };
            agent.info.model_ref = format!("{}:{}", e.provider_id, e.model_id);
            if let Some(selection) = e
                .metadata
                .as_ref()
                .and_then(|m| m.runtime_selection.as_deref())
            {
                let target = restored_target(selection, agent.target.as_ref(), &self.config);
                agent.settings = (&target).into();
                agent.target = Some(target);
            } else if agent
                .target
                .as_ref()
                .is_some_and(|t| t.provider != e.provider_id || t.model != e.model_id)
            {
                agent.target = None;
                agent.settings = AgentModelSettings::default();
            }
        }
        Ok(agents)
    }
    fn request_rewind(
        &mut self,
        request: &str,
        reply: Reply<RewindPoint>,
    ) -> Result<(), CoordinatorError> {
        self.accepting()?;
        let events = crate::store::read_events(&self.info()?.events_path)?;
        let point = crate::conversation_rewind::rewind_points(&events)
            .into_iter()
            .find(|p| p.request_id == request)
            .ok_or_else(|| {
                CoordinatorError::Invalid("rewind target is not an active user message".into())
            })?;
        self.rewind = Some((point, reply));
        let queued: Vec<_> = self
            .agents
            .values()
            .flat_map(|a| a.queue.iter().map(|t| t.id.clone()))
            .collect();
        for id in queued {
            self.cancel(&id, "conversation rewind")?;
        }
        let tasks: Vec<_> = self.running.keys().cloned().collect();
        for id in tasks {
            if self.running.contains_key(&id) {
                self.cancel(&id, "conversation rewind")?;
            }
        }
        Ok(())
    }
    pub fn finish_rewind(&mut self) {
        if !self.jobs.is_empty() || !self.running.is_empty() {
            return;
        }
        let Some((point, reply)) = self.rewind.take() else {
            return;
        };
        let result = (|| {
            self.accepting()?;
            self.emit(
                system(),
                Some(point.request_id.clone()),
                EventV1::ConversationRewound(
                    crate::conversation_rewind::ConversationRewoundEvent {
                        target_seq: point.seq,
                        request_id: point.request_id.clone(),
                    },
                ),
            )?;
            let events = crate::store::read_events(&self.info()?.events_path)?;
            self.agents = self.restore_agents(
                &events,
                &self.info()?.run_dir,
                self.metadata
                    .as_ref()
                    .and_then(|m| m.recorded_runtime_context.as_ref()),
            )?;
            self.restore_children(&events);
            Ok(point)
        })();
        let _ = reply.send(result);
    }
}
fn recorded_target(
    context: &crate::proj::RecordedRuntimeContext,
    configured: Option<&ResolvedModelTarget>,
    config: &CoordinatorConfig,
) -> Result<ResolvedModelTarget, CoordinatorError> {
    if context.provider.trim().is_empty() || context.model.trim().is_empty() {
        return Err(CoordinatorError::Invalid(
            "recorded model selection is incomplete".into(),
        ));
    }
    let selection = crate::session::CanonicalRuntimeSelection {
        profile: Some(context.profile.clone()),
        provider_id: context.provider.clone(),
        model_id: context.model.clone(),
        variant: context.variant.clone(),
        reasoning_effort: context.reasoning_effort.clone(),
        text_verbosity: context.text_verbosity.clone(),
        thinking: context.thinking.clone(),
        resolved_limits: context.effective_model_limits(),
        ..Default::default()
    };
    selection
        .resolved_limits
        .validate("recorded model")
        .map_err(|error| CoordinatorError::Invalid(error.to_string()))?;
    Ok(restored_target(&selection, configured, config))
}
fn id_number(id: &str) -> u64 {
    id.rsplit_once('-')
        .and_then(|(_, n)| n.parse().ok())
        .unwrap_or(0)
}
fn recorded_id(event: &EventV1) -> Option<&str> {
    match event {
        EventV1::AgentSpawned(e) => Some(e.agent_id.as_str()),
        EventV1::UserMessageSubmitted(e) => Some(e.request_id.as_str()),
        EventV1::ProviderRequestStarted(e) => Some(e.request_id.as_str()),
        EventV1::ToolCallRequested(e) => Some(e.tool_call_id.as_str()),
        EventV1::PermissionRequested(e) => Some(e.permission_id.as_str()),
        EventV1::TaskScheduled(e) => Some(e.task_id.as_str()),
        _ => None,
    }
}

pub(super) fn restored_target(
    selection: &crate::session::CanonicalRuntimeSelection,
    configured: Option<&ResolvedModelTarget>,
    config: &CoordinatorConfig,
) -> ResolvedModelTarget {
    let configured = configured
        .into_iter()
        .chain(config.agent_model_targets.values())
        .chain(config.agent_model_fallbacks.values().flatten())
        .find(|t| {
            t.provider == selection.provider_id
                && t.model == selection.model_id
                && t.variant == selection.variant
        });
    let entry = configured
        .and_then(|t| t.catalog_entry.as_deref())
        .or_else(|| {
            config.model_catalog.iter().find(|entry| {
                entry.provider == selection.provider_id
                    && entry.model == selection.model_id
                    && entry.variant == selection.variant
            })
        });
    let resolution = configured
        .map(|t| &t.resolution)
        .or_else(|| entry.map(|e| &e.resolution))
        .map_or_else(
            || {
                crate::model_resolution::resolve_model(
                    crate::model_resolution::ModelResolutionInput {
                        provider: &selection.provider_id,
                        model: &selection.model_id,
                        metadata_family: None,
                        input_modalities: &[],
                        supports_tool_calls: None,
                        supports_reasoning_summaries: None,
                    },
                )
            },
            Clone::clone,
        );
    ResolvedModelTarget {
        model_ref: format!("{}:{}", selection.provider_id, selection.model_id),
        provider: selection.provider_id.clone(),
        model: selection.model_id.clone(),
        variant: selection.variant.clone(),
        reasoning_effort: selection.reasoning_effort.clone(),
        text_verbosity: selection.text_verbosity.clone(),
        reasoning_summary: selection.reasoning_summary.clone(),
        thinking: selection.thinking.clone(),
        limits: selection.resolved_limits.clone(),
        resolution,
        catalog_entry: entry.cloned().map(Box::new),
    }
}
