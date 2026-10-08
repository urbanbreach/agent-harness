use super::{handle::system, runtime::*, *};
use crate::conversation_rewind::RewindPoint;
mod restore;

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
        self.projection_owner = checked_projection_owner(&self.config.session_dir, id, &events)?;
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
        self.restore_native_profiles(&events)?;
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
            self.todos.apply(event);
            self.apply_guidance_state(event);
        }
        self.rebuild_instruction_state(&events);
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
        self.restore_native_subagents(&events)?;
        for event in &events {
            if let EventV1::SubagentCancelRequested(intent) = &event.payload {
                match &intent.command {
                    crate::subagent::SubagentCommandRequest::ExplicitChildKill { child_id }
                        if self.agents.contains_key(&child_id.0) =>
                    {
                        self.killed_agents.insert(child_id.0.clone());
                    }
                    crate::subagent::SubagentCommandRequest::ParentSessionStop { session_id }
                        if self.agents.contains_key(session_id) =>
                    {
                        self.stopped_sessions.insert(session_id.clone());
                    }
                    _ => {}
                }
            }
        }
        self.restore_command_notices();
        self.subagent_history = crate::subagent::SubagentHistory::from_events(&events);
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
        self.reconcile_subagents()?;
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
        if self.config.yolo_on_start
            || events
                .iter()
                .rev()
                .find_map(|event| match event.payload {
                    EventV1::YoloModeChanged { enabled } => Some(enabled),
                    _ => None,
                })
                .unwrap_or(false)
        {
            self.set_yolo_mode(true)?;
        }
        self.start_hooks()?;
        self.write_metadata()?;
        Ok(info)
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
            self.rebuild_instruction_state(&events);
            self.agents = self.restore_agents(
                &events,
                &self.info()?.run_dir,
                self.metadata
                    .as_ref()
                    .and_then(|m| m.recorded_runtime_context.as_ref()),
            )?;
            self.restore_children(&events);
            self.subagent_history = crate::subagent::SubagentHistory::from_events(&events);
            for session_id in self.provider_sessions() {
                self.config.provider.session_event(
                    &harness_providers::ProviderSessionEvent::Rewound { session_id },
                );
            }
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
/// Original owners are allowed only for checked child projections whose reference
/// records agree with the immutable authoritative parent journal.
fn checked_projection_owner(
    sessions: &std::path::Path,
    id: &str,
    events: &[EventEnvelopeV1],
) -> Result<Option<String>, CoordinatorError> {
    let Some(metadata) = crate::proj::read_metadata_value(&sessions.join(id))? else {
        return Ok(None);
    };
    let lineage = &metadata["harness_lineage"];
    if lineage["relationship"] != "task_child_session" {
        return Ok(None);
    }
    if metadata["run_id"] != id || lineage["child_session_id"] != id {
        return Err(CoordinatorError::Invalid(
            "child projection identity mismatch".into(),
        ));
    }
    let owner = lineage["parent_run_id"]
        .as_str()
        .ok_or_else(|| CoordinatorError::Invalid("child projection owner missing".into()))?;
    crate::store::validate_session_id(owner)?;
    let parent_dir = sessions.join(owner);
    if owner == id
        || std::fs::symlink_metadata(&parent_dir)?
            .file_type()
            .is_symlink()
    {
        return Err(CoordinatorError::Invalid(
            "invalid child projection owner".into(),
        ));
    }
    let original = crate::store::read_events(&parent_dir.join("events.jsonl"))?;
    crate::proj::checked_history(&original)
        .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
    if original.iter().any(|event| event.run_id.as_str() != owner) {
        return Err(CoordinatorError::Invalid(
            "original projection owner mismatch".into(),
        ));
    }
    let spawn = original
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::AgentSpawned(spawn)
                if spawn.agent_id == id && spawn.parent_agent_id.is_some() =>
            {
                Some(spawn)
            }
            _ => None,
        })
        .ok_or_else(|| CoordinatorError::Invalid("original child creation is missing".into()))?;
    let parent = spawn
        .parent_agent_id
        .as_deref()
        .ok_or_else(|| CoordinatorError::Invalid("original child parent is missing".into()))?;
    let parent_is_root = original.iter().any(|event| {
        matches!(&event.payload, EventV1::AgentSpawned(spawn) if spawn.agent_id == parent && spawn.parent_agent_id.is_none())
    });
    if lineage["parent_session_id"] != if parent_is_root { owner } else { parent }
        || lineage["profile"] != spawn.profile
        || !events.iter().any(|event| {
            matches!(&event.payload, EventV1::AgentSpawned(projected) if projected.agent_id == id
                && projected.parent_agent_id.is_none() && projected.profile == spawn.profile)
        })
    {
        return Err(CoordinatorError::Invalid(
            "child projection ancestry mismatch".into(),
        ));
    }
    let mut checked = std::collections::BTreeSet::new();
    for event in events {
        let reference = match &event.payload {
            EventV1::FinalizedAgentState(reference) => Some(reference),
            EventV1::SubagentTransition(transition) => transition.finalized_state.as_ref(),
            EventV1::AgentContextInitialized(initialized) => Some(&initialized.source),
            _ => None,
        };
        let Some(reference) = reference.filter(|reference| reference.owner_run_id != id) else {
            continue;
        };
        let key = serde_json::to_string(reference)?;
        let source = event
            .event_id
            .strip_prefix("source:")
            .and_then(|value| value.rsplit_once(':'));
        let anchored = source.is_some_and(|(run, seq)| {
            run == owner
                && seq.parse::<u64>().is_ok_and(|seq| {
                    original.iter().any(|saved| {
                        saved.seq == seq
                            && saved.payload == event.payload
                            && saved.actor == event.actor
                            && saved.correlation_id == event.correlation_id
                    })
                })
        });
        if reference.owner_run_id != owner || (!anchored && !checked.contains(&key)) {
            return Err(CoordinatorError::Invalid(
                "finalized projection source is not authoritative".into(),
            ));
        }
        checked.insert(key);
    }
    Ok(Some(owner.into()))
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
