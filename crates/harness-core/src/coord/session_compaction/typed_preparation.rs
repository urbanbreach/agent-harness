use crate::agent::AgentModelRef;
use crate::context_budget::RequestBudgetSnapshot;
use crate::coord::compaction::{
    build_active_path_compaction_snapshot, estimate_typed_entries_tokens, find_safe_cut_point,
    ActivePathCompactionSnapshot, ActivePathCompactionSnapshotInput, CompactionOwner,
    CurrentCompactionModel, LegacySourceSequences,
};
use crate::coord::provider_context::event_belongs_to_agent;
use crate::event::{EventEnvelopeV1, EventV1};
use crate::ids::EntryId;
use crate::session::{
    legacy::LegacyIdentityNamespace, CanonicalSessionProjection, SessionEntryPayload,
};

use super::super::CoordinatorError;
use super::budget::{CompactionBudget, CompactionBudgetPlanInput, CompleteRequestBudget};

pub(super) struct TypedCompactionPreparation {
    pub(super) first_kept_entry_id: EntryId,
    pub(super) first_kept_event_seq: u64,
    pub(super) first_kept_request_id: Option<String>,
    pub(super) text_split: Option<crate::coord::compaction::TypedTextSplit>,
    pub(super) request_budget: CompleteRequestBudget,
}

pub(super) struct TypedCompactionPreparationRequest<'a> {
    pub(super) events: &'a [EventEnvelopeV1],
    pub(super) agent_id: &'a str,
    pub(super) model: &'a AgentModelRef,
    pub(super) request_budget: RequestBudgetSnapshot,
    pub(super) keep_recent_tokens: u32,
    pub(super) preserve_latest_completed_turn: bool,
}

pub(super) fn prepare_typed_compaction(
    request: TypedCompactionPreparationRequest<'_>,
) -> Result<Option<TypedCompactionPreparation>, CoordinatorError> {
    let TypedCompactionPreparationRequest {
        events,
        agent_id,
        model,
        request_budget,
        keep_recent_tokens,
        preserve_latest_completed_turn,
    } = request;
    let projected =
        CanonicalSessionProjection::from_event_history(events).map_err(compaction_error)?;
    let active_path = projected.session.active_path().map_err(compaction_error)?;
    let run_id = events.first().map(|event| &event.run_id).ok_or_else(|| {
        CoordinatorError::CompactionFailed("canonical compaction requires a run event".to_string())
    })?;
    let namespace = LegacyIdentityNamespace::new(run_id);
    let active_entry_ids = active_path
        .iter()
        .map(|entry| entry.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let source_sequences = LegacySourceSequences::new(events.iter().filter_map(|event| {
        source_entry_id(&namespace, event)
            .filter(|entry_id| active_entry_ids.contains(entry_id))
            .map(|entry_id| (entry_id, event.seq))
    }))
    .map_err(compaction_error)?;
    let owner = CompactionOwner::root(agent_id, projected.session.session_id().clone());
    let mut snapshot = build_active_path_compaction_snapshot(ActivePathCompactionSnapshotInput {
        session: &projected.session,
        owner,
        legacy_source_sequences: &source_sequences,
        pending_prompt: None,
        current_model: CurrentCompactionModel::new(&model.provider_id, &model.model_id),
    })
    .map_err(compaction_error)?;
    let stream_key = format!("agent:{agent_id}");
    let owned_entry_ids = snapshot
        .entries
        .iter()
        .filter(|entry| {
            entry
                .legacy_source_sequence
                .and_then(|sequence| events.iter().find(|event| event.seq == sequence))
                .is_some_and(|event| event_belongs_to_agent(event, agent_id, &stream_key))
        })
        .map(|entry| entry.entry.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let owned_tool_pair_entry_ids = snapshot
        .entries
        .iter()
        .flat_map(|entry| &entry.tool_pairs)
        .filter(|pair| owned_entry_ids.contains(&pair.assistant_entry_id))
        .flat_map(|pair| {
            [
                pair.assistant_entry_id.clone(),
                pair.result_entry_id.clone(),
            ]
        })
        .collect::<std::collections::BTreeSet<_>>();
    snapshot.entries.retain(|entry| {
        owned_entry_ids.contains(&entry.entry.id)
            || owned_tool_pair_entry_ids.contains(&entry.entry.id)
    });
    snapshot.prior_active_summary = snapshot.prior_active_summary.filter(|summary| {
        summary
            .legacy_source_sequence
            .and_then(|sequence| events.iter().find(|event| event.seq == sequence))
            .is_some_and(|event| event_belongs_to_agent(event, agent_id, &stream_key))
    });
    snapshot.active_branch.entry_ids = snapshot
        .entries
        .iter()
        .map(|entry| entry.entry.id.clone())
        .collect();
    snapshot.active_branch.leaf_entry_id = snapshot.active_branch.entry_ids.last().cloned();

    let keep_recent_tokens = if preserve_latest_completed_turn {
        latest_completed_turn_tokens(&snapshot).unwrap_or(keep_recent_tokens)
    } else {
        keep_recent_tokens
    };
    let cut = match find_safe_cut_point(&snapshot, keep_recent_tokens) {
        Ok(cut) => cut,
        Err(_) => return Ok(None),
    };
    let request_budget =
        match CompactionBudget::resolve_for_snapshot(request_budget, events, &snapshot)
            .complete_request_plan(CompactionBudgetPlanInput {
                snapshot: &snapshot,
                cut: &cut,
                keep_recent_tokens,
            }) {
            Ok(budget) => budget,
            Err(_) => return Ok(None),
        };
    let Some(boundary) = snapshot
        .entries
        .iter()
        .find(|entry| entry.entry.id == cut.first_kept_entry_id)
    else {
        return Ok(None);
    };
    let Some(first_kept_event_seq) = boundary.legacy_source_sequence else {
        return Ok(None);
    };
    let first_kept_request_id = events
        .iter()
        .find(|event| event.seq == first_kept_event_seq)
        .and_then(|event| event.correlation_id.clone());
    Ok(Some(TypedCompactionPreparation {
        first_kept_entry_id: cut.first_kept_entry_id,
        first_kept_event_seq,
        first_kept_request_id,
        text_split: cut.text_split,
        request_budget,
    }))
}

fn latest_completed_turn_tokens(snapshot: &ActivePathCompactionSnapshot) -> Option<u32> {
    let latest_turn_id =
        snapshot
            .entries
            .iter()
            .rev()
            .find_map(|entry| match &entry.entry.payload {
                SessionEntryPayload::AssistantMessage { .. } => entry.entry.turn_id.as_ref(),
                _ => None,
            })?;
    let first_entry = snapshot
        .entries
        .iter()
        .position(|entry| entry.entry.turn_id.as_ref() == Some(latest_turn_id))?;
    Some(estimate_typed_entries_tokens(&snapshot.entries[first_entry..]).max(1))
}

fn source_entry_id(
    namespace: &LegacyIdentityNamespace<'_>,
    event: &EventEnvelopeV1,
) -> Option<EntryId> {
    let semantic_kind = match event.payload {
        EventV1::SessionTitleUpdated(_) => "session_metadata",
        EventV1::UserMessageSubmitted(_) => "user_message",
        EventV1::ProviderRequestStarted(_) => "assistant_message",
        EventV1::SessionCompaction(_) => "compaction_summary",
        EventV1::BranchSummary(_) => "branch_summary",
        EventV1::ToolCallFinished(_) => "tool_result",
        _ => return None,
    };
    Some(namespace.entry_id(event.seq, &event.event_id, semantic_kind))
}

fn compaction_error(error: impl std::fmt::Display) -> CoordinatorError {
    CoordinatorError::CompactionFailed(error.to_string())
}
