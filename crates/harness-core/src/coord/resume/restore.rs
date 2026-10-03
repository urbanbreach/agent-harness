use super::*;
use std::collections::VecDeque;

impl Runtime {
    pub(super) fn restore_agents(
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
        let root = events
            .iter()
            .find_map(|event| match &event.payload {
                EventV1::RunStarted(start) => Some(start.workspace_root.clone()),
                _ => None,
            })
            .ok_or_else(|| CoordinatorError::Invalid("run context missing".into()))?;
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
                let mut messages = if e.parent_agent_id.is_some() {
                    super::super::context::Context::default()
                } else {
                    super::super::history::messages(
                        events,
                        &e.agent_id,
                        primary == Some(e.agent_id.as_str()),
                        &profile.system_prompt,
                        run_dir,
                    )?
                };
                messages.unavailable =
                    Some(crate::subagent::FinalizedStateUnavailable::LegacySummaryOnly);
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
                        execution: crate::subagent::ResolvedSubagentContext {
                            effective_cwd: root.clone(),
                            policy_roots: vec![root.clone()],
                            isolation: crate::subagent::ResolvedSubagentIsolation::SharedWorkspace,
                        },
                        cwd: PathBuf::from(&root),
                        tool_state: self.tool_state.fresh_owner(),
                        generation: 0,
                        prompt_turns: 0,
                        tool_calls: 0,
                        pending_model_tools: BTreeMap::new(),
                        attempt: None,
                        attempt_started_ms: 0,
                        notification_seq: 0,
                        finalized: None,
                        source_reference: None,
                        skill_startup: None,
                        skill_preloads: None,
                    },
                );
            }
            self.restore_owned_event(&mut agents, event, events, primary, run_dir)?;
            if let Some(agent) = event
                .actor
                .agent_id
                .as_ref()
                .and_then(|id| agents.get_mut(id))
            {
                agent.observe_activity(event);
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

    fn restore_owned_event(
        &self,
        agents: &mut BTreeMap<String, Agent>,
        event: &EventEnvelopeV1,
        events: &[EventEnvelopeV1],
        primary: Option<&str>,
        run_dir: &std::path::Path,
    ) -> Result<(), CoordinatorError> {
        use crate::subagent::*;
        let reference = match &event.payload {
            EventV1::FinalizedAgentState(reference) => Some(reference),
            EventV1::SubagentTransition(transition)
                if transition.transition == SubagentTransitionKind::Finished =>
            {
                transition.finalized_state.as_ref()
            }
            _ => None,
        };
        if let Some(reference) = reference {
            let Some(agent) = agents.get_mut(&reference.state.owner.0) else {
                return Ok(());
            };
            agent.finalized = Some(reference.clone());
            agent.generation = agent.generation.max(reference.generation);
            agent.attempt = Some(reference.attempt_id.clone());
            match resolve_finalized_state(
                run_dir,
                reference,
                self.projection_owner
                    .as_deref()
                    .filter(|owner| *owner == reference.owner_run_id)
                    .unwrap_or(event.run_id.as_str()),
                &reference.state.owner,
                &reference.attempt_id,
            ) {
                FinalizedStateResult::Available { mut state } => {
                    let historical = super::super::history::messages(
                        events,
                        &reference.state.owner.0,
                        primary == Some(reference.state.owner.0.as_str()),
                        &agent.profile.system_prompt,
                        run_dir,
                    )?;
                    agent.messages = super::super::subagents::current_context(
                        &mut state,
                        historical,
                        &agent.profile.system_prompt,
                        run_dir,
                    )?;
                    agent.execution = state.execution_context;
                    agent.cwd = PathBuf::from(&agent.execution.effective_cwd);
                    agent.tool_state = self
                        .tool_state
                        .with_read_snapshot(state.read_state)
                        .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
                    agent.source_reference = state.source_reference;
                }
                FinalizedStateResult::Unavailable { .. } => {
                    if agent.info.parent_agent_id.is_none() {
                        agent.messages = super::super::history::messages(
                            events,
                            &reference.state.owner.0,
                            primary == Some(reference.state.owner.0.as_str()),
                            &agent.profile.system_prompt,
                            run_dir,
                        )?;
                    }
                    agent.messages.unavailable = reference
                        .unavailable
                        .or(Some(FinalizedStateUnavailable::Incomplete));
                }
            }
        }
        match &event.payload {
            EventV1::AgentExecutionContextChanged(e) if e.payload_version == 1 => {
                let Some(agent) = agents.get_mut(&e.agent_id.0) else {
                    return Ok(());
                };
                agent.execution = e.context.clone();
                agent.cwd = PathBuf::from(&e.context.effective_cwd);
                if let Some(prompt) = &e.system_prompt {
                    Arc::make_mut(&mut agent.profile)
                        .system_prompt
                        .clone_from(prompt);
                }
            }
            EventV1::SubagentTransition(e) if e.payload_version == 1 => {
                let Some(agent) = agents.get_mut(&e.child_id.0) else {
                    return Ok(());
                };
                agent.generation = agent.generation.max(e.generation);
                agent.attempt.clone_from(&e.attempt_id);
                agent.notification_seq =
                    agent.notification_seq.max(e.notification_seq.unwrap_or(0));
            }
            EventV1::AgentContextInitialized(initialized) if initialized.payload_version == 1 => {
                let Some(agent) = agents.get_mut(&initialized.agent_id.0) else {
                    return Ok(());
                };
                let FinalizedStateResult::Available { mut state } = resolve_finalized_state(
                    run_dir,
                    &initialized.source,
                    self.projection_owner
                        .as_deref()
                        .filter(|owner| *owner == initialized.source.owner_run_id)
                        .unwrap_or(event.run_id.as_str()),
                    &initialized.source.state.owner,
                    &initialized.source.attempt_id,
                ) else {
                    return Err(CoordinatorError::Invalid(
                        "initialized agent source is unavailable".into(),
                    ));
                };
                let historical = super::super::history::messages(
                    events,
                    &initialized.agent_id.0,
                    primary == Some(initialized.agent_id.0.as_str()),
                    &agent.profile.system_prompt,
                    run_dir,
                )?;
                agent.messages = super::super::subagents::current_context(
                    &mut state,
                    historical,
                    &agent.profile.system_prompt,
                    run_dir,
                )?;
                agent.tool_state = self
                    .tool_state
                    .with_read_snapshot(state.read_state)
                    .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
                agent.source_reference = Some(Box::new(initialized.source.clone()));
                if initialized.mode != FinalizedContextCopy::Fork {
                    agent.execution = state.execution_context;
                    agent.cwd = PathBuf::from(&agent.execution.effective_cwd);
                    agent.info.model_ref = state.source_model;
                    agent.settings = state.model_settings;
                    agent.target = state.model_target;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
