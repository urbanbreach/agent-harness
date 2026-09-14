use crate::agent::AgentModelRef;
use crate::context_budget::RequestBudgetSnapshot;
use crate::coord::compaction::{
    build_active_path_compaction_snapshot, find_safe_cut_point, ActivePathCompactionSnapshotInput,
    CompactionOwner, CurrentCompactionModel, LegacySourceSequences,
};
use crate::coord::provider_context::event_belongs_to_agent;
use crate::event::{EventEnvelopeV1, EventV1};
use crate::ids::EntryId;
use crate::session::{CanonicalSessionProjection, EventIdentityNamespace, SessionEntryPayload};

use super::super::CoordinatorError;
use super::budget::{CompactionBudget, CompactionBudgetPlanInput, CompleteRequestBudget};

pub(super) struct TypedCompactionPreparation {
    pub(super) first_kept_entry_id: EntryId,
    pub(super) first_kept_event_seq: u64,
    pub(super) first_kept_request_id: Option<String>,
    pub(super) is_split_turn: bool,
    pub(super) turn_start_seq: Option<u64>,
    pub(super) request_budget: CompleteRequestBudget,
}

pub(super) struct TypedCompactionPreparationRequest<'a> {
    pub(super) events: &'a [EventEnvelopeV1],
    pub(super) agent_id: &'a str,
    pub(super) model: &'a AgentModelRef,
    pub(super) request_budget: RequestBudgetSnapshot,
    pub(super) keep_recent_tokens: u32,
    pub(super) force_progress: bool,
    pub(super) context_window: u32,
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
        force_progress,
        context_window,
    } = request;
    let projected =
        CanonicalSessionProjection::from_event_history(events).map_err(compaction_error)?;
    let active_path = projected.session.active_path().map_err(compaction_error)?;
    let run_id = events.first().map(|event| &event.run_id).ok_or_else(|| {
        CoordinatorError::CompactionFailed("canonical compaction requires a run event".to_string())
    })?;
    let namespace = EventIdentityNamespace::new(run_id);
    let active_entry_ids = active_path
        .iter()
        .map(|entry| entry.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let source_sequences = LegacySourceSequences::new(events.iter().filter_map(|event| {
        namespace
            .source_entry_id(event)
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

    let mut cut = match find_safe_cut_point(&snapshot, keep_recent_tokens, force_progress) {
        Ok(cut) => cut,
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
    cut.retained_tokens = super::preparation::build_agent_conversation_messages(events, agent_id)
        .iter()
        .filter(|message| {
            !matches!(
                message,
                crate::conversation::ConversationMessage::Checkpoint(_)
            ) && super::preparation::message_seq(message) >= first_kept_event_seq
        })
        .map(|message| {
            super::super::compaction::estimate_admitted_message_tokens(message, context_window)
        })
        .fold(0_u32, u32::saturating_add);
    let request_budget =
        match CompactionBudget::resolve_for_snapshot(request_budget, events, &snapshot)
            .complete_request_plan(CompactionBudgetPlanInput {
                snapshot: &snapshot,
                cut: &cut,
                keep_recent_tokens: keep_recent_tokens.max(cut.retained_tokens),
            }) {
            Ok(budget) => budget,
            Err(_) => return Ok(None),
        };
    let first_kept_request_id = events
        .iter()
        .find(|event| event.seq == first_kept_event_seq)
        .and_then(|event| event.correlation_id.clone());
    Ok(Some(TypedCompactionPreparation {
        first_kept_entry_id: cut.first_kept_entry_id,
        first_kept_event_seq,
        first_kept_request_id,
        is_split_turn: !matches!(
            boundary.entry.payload,
            SessionEntryPayload::UserMessage { .. }
        ),
        turn_start_seq: snapshot
            .entries
            .iter()
            .rev()
            .find(|entry| {
                entry
                    .legacy_source_sequence
                    .is_some_and(|seq| seq < first_kept_event_seq)
                    && matches!(entry.entry.payload, SessionEntryPayload::UserMessage { .. })
            })
            .and_then(|entry| entry.legacy_source_sequence),
        request_budget,
    }))
}

fn compaction_error(error: impl std::fmt::Display) -> CoordinatorError {
    CoordinatorError::CompactionFailed(error.to_string())
}
