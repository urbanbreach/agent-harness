use super::super::*;

pub(in crate::coord) struct CompactionStartContext {
    pub(in crate::coord) existing_context: ProviderContext,
    pub(in crate::coord) trigger: ProviderCompactionTrigger,
    pub(in crate::coord) hook_context: HookInvocationContext,
}

pub(in crate::coord) fn compaction_start_context(
    run_state: &RunState,
    request: &CompactAgentContextRequest,
) -> Result<CompactionStartContext, CoordinatorError> {
    let existing_context = run_state
        .provider_context_by_agent
        .get(&request.agent_id)
        .cloned()
        .unwrap_or_default();
    let manual_tokens_before = (request.trigger_reason == "manual")
        .then(|| approximate_provider_context_tokens(&existing_context));
    let running_turn = request
        .task_id
        .as_deref()
        .and_then(|task_id| run_state.running_agent_turns.get(task_id))
        .or_else(|| {
            run_state.running_agent_turns.values().find(|running| {
                running.agent_id == request.agent_id
                    && request
                        .through_request_id
                        .as_deref()
                        .is_none_or(|request_id| running.request_id == request_id)
            })
        });
    let trigger = if let Some(running) = running_turn {
        ProviderCompactionTrigger {
            agent_id: request.agent_id.clone(),
            profile_name: running.profile_name.clone(),
            model_ref: running.model_ref.clone(),
            provider_id: running.latest_provider_id.clone(),
            model_id: running.latest_model_id.clone(),
            through_request_id: request.through_request_id.clone(),
            trigger_reason: request.trigger_reason.clone(),
            tokens_before: request
                .evidence
                .usage
                .as_ref()
                .map(|usage| usage.prompt_tokens)
                .or(manual_tokens_before),
            estimate_source: None,
        }
    } else {
        let profile = run_state
            .agents
            .get(&request.agent_id)
            .ok_or_else(|| CoordinatorError::UnknownAgent(request.agent_id.clone()))?;
        ProviderCompactionTrigger {
            agent_id: request.agent_id.clone(),
            profile_name: profile.name.clone(),
            model_ref: profile.model_ref.clone(),
            provider_id: None,
            model_id: None,
            through_request_id: request.through_request_id.clone(),
            trigger_reason: request.trigger_reason.clone(),
            tokens_before: request
                .evidence
                .usage
                .as_ref()
                .map(|usage| usage.prompt_tokens)
                .or(manual_tokens_before),
            estimate_source: None,
        }
    };
    let hook_context = HookInvocationContext {
        event: HookLifecycleEvent::CompactionRequested,
        run_id: run_state.info.run_id.to_string(),
        workspace_root: run_state.info.workspace_root.clone(),
        artifacts_dir: run_state.info.artifacts_dir.clone(),
        actor: Some(agent_actor(&request.agent_id)),
        agent_id: Some(request.agent_id.clone()),
        request_id: trigger.through_request_id.clone(),
        permission_id: None,
        task_id: request.task_id.clone(),
        tool_call_id: None,
        tool_id: None,
        provider_id: trigger.provider_id.clone(),
        model_id: trigger.model_id.clone(),
        parent_agent_id: run_state
            .subagent_parent_by_id
            .get(&request.agent_id)
            .cloned(),
        profile: Some(trigger.profile_name.clone()),
        outcome: Some(trigger.trigger_reason.clone()),
        output_summary: trigger.tokens_before.map(|tokens| tokens.to_string()),
        failure_reason: None,
    };
    Ok(CompactionStartContext {
        existing_context,
        trigger,
        hook_context,
    })
}
