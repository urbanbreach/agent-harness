//! Recorded model selection and checked projection ownership.
use super::*;

pub(super) fn recorded_target(
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
pub(super) fn checked_projection_owner(
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

pub(super) fn id_number(id: &str) -> u64 {
    id.rsplit_once('-')
        .and_then(|(_, n)| n.parse().ok())
        .unwrap_or(0)
}
pub(super) fn recorded_id(event: &EventV1) -> Option<&str> {
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

pub(in crate::coord) fn restored_target(
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
