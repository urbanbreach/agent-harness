use super::{
    super::context::{Context, Entry},
    *,
};
use std::path::Path;

pub(in crate::coord) fn restored_context(
    state: &mut RawFinalizedState,
    system_prompt: &str,
    run_dir: &Path,
) -> Result<Context, CoordinatorError> {
    let mut context = Context::new(system_prompt);
    context.semantic = state.fidelity == FinalizedStateFidelity::Semantic;
    let model = crate::agent::AgentModelRef::parse(&state.source_model);
    let request = harness_providers::CompletionRequest {
        provider_id: Some(model.provider_id),
        model_id: model.model_id,
        ..Default::default()
    };
    for item in &state.conversation_items {
        let mut entry = Entry::new(item.message.clone(), item.event_seq, item.turn_id.clone());
        entry.attachments = item.attachments.clone();
        entry.settled_reasoning = item.settled_reasoning.clone();
        entry.raw_tool_result = item.raw_tool_result.clone();
        if let Some(meta) = &item.meta {
            super::meta::restore(&mut entry, meta, run_dir, &request);
        }
        context.entries.push(entry);
    }
    super::meta::settle_rewrite_marks(&mut context.entries);
    super::super::history::derive_images(&mut context.entries);
    context.usage = state.usage.clone();
    context.native_context_usage = state.native_context_usage;
    context.model_request = state.model_request.clone();
    context.unavailable = state.unavailable;
    super::prompt::restore_content(&mut context, run_dir, &std::collections::HashMap::new())?;
    Ok(context)
}

/// A reusable completed prefix is not the whole current conversation.
/// Later presentation-only turns remain visible without acquiring exact fidelity.
pub(in crate::coord) fn current_context(
    state: &mut RawFinalizedState,
    historical: Context,
    system_prompt: &str,
    run_dir: &Path,
) -> Result<Context, CoordinatorError> {
    if historical.entries.iter().any(|entry| {
        entry.message.role != harness_providers::MessageRole::System
            && entry.turn.is_none()
            && entry.compaction.is_none()
            && entry.branch_summary.is_none()
    }) {
        return Ok(historical);
    }
    let covered: std::collections::BTreeSet<_> = state
        .conversation_items
        .iter()
        .filter_map(|item| item.turn_id.clone())
        .collect();
    let mut context = restored_context(state, system_prompt, run_dir)?;
    let suffix: Vec<_> = historical
        .entries
        .into_iter()
        .filter(|entry| {
            entry.message.role != harness_providers::MessageRole::System
                && entry
                    .turn
                    .as_ref()
                    .is_some_and(|turn| !covered.contains(turn))
        })
        .collect();
    if !suffix.is_empty() {
        context.unavailable = Some(FinalizedStateUnavailable::LegacySummaryOnly);
        context.model_request = None;
        context.entries.extend(suffix);
    }
    Ok(context)
}
