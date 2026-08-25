use crate::agent::AgentModelRef;
use crate::config::CompactionSettings;
use crate::context_budget::RequestBudgetSnapshot;
use crate::conversation::ConversationMessage;
use crate::event::{EventEnvelopeV1, EventV1, UiIntentReceivedEvent};
use crate::ids::EntryId;

use super::super::compaction::{
    build_summarization_prompt, build_turn_prefix_prompt, compute_file_lists,
    estimate_context_tokens, estimate_messages_tokens, estimate_text_tokens, CutPointResult,
};
use super::super::{CoordinatorError, RunState};
use super::budget::CompactionBudget;
use super::preparation::{
    build_agent_conversation_messages, collect_events, determine_model_ref,
    durable_compaction_state, extract_file_ops_from_messages, find_previous_summary,
    split_messages_at_cut_point,
};
use super::typed_preparation::{prepare_typed_compaction, TypedCompactionPreparationRequest};

pub(in crate::coord) struct SessionCompactionPreparationRequest<'a> {
    pub(in crate::coord) run_state: &'a RunState,
    pub(in crate::coord) agent_id: &'a str,
    pub(in crate::coord) trigger_reason: &'a str,
    pub(in crate::coord) settings: &'a CompactionSettings,
    pub(in crate::coord) prepared_budget: Option<RequestBudgetSnapshot>,
}

#[derive(Debug)]
pub(super) struct PreparedSessionCompaction {
    pub(super) agent_id: String,
    pub(super) trigger_reason: String,
    pub(super) model: AgentModelRef,
    pub(super) summary_prompt: String,
    pub(super) turn_prefix_prompt: Option<String>,
    pub(super) first_kept_event_seq: u64,
    pub(super) first_kept_request_id: Option<String>,
    pub(super) first_kept_entry_id: Option<EntryId>,
    pub(super) tokens_before: u32,
    pub(super) preserved_message_tokens: u32,
    pub(super) summary_max_tokens: u32,
    pub(super) request_budget: RequestBudgetSnapshot,
    pub(super) durable_agent_tail_seq: Option<u64>,
    pub(super) read_files: Vec<String>,
    pub(super) modified_files: Vec<String>,
    pub(super) current_intent: Option<UiIntentReceivedEvent>,
    pub(super) committed_events: Vec<EventEnvelopeV1>,
}

pub(in crate::coord) async fn prepare_session_compaction(
    request: SessionCompactionPreparationRequest<'_>,
) -> Result<Option<PreparedSessionCompaction>, CoordinatorError> {
    let SessionCompactionPreparationRequest {
        run_state,
        agent_id,
        trigger_reason,
        settings,
        prepared_budget,
    } = request;
    if !settings.enabled {
        return Ok(None);
    }

    let all_events = collect_events(run_state).await?;
    let context_budget = CompactionBudget::resolve(prepared_budget, &all_events, agent_id);
    let latest_compaction = all_events
        .iter()
        .rev()
        .find_map(|event| match &event.payload {
            EventV1::SessionCompaction(payload) if payload.agent_id == agent_id => Some(payload),
            _ => None,
        });
    let effective_first_seq = latest_compaction.map_or(0, |event| event.first_kept_event_seq);
    let events = all_events
        .iter()
        .filter(|event| event.seq >= effective_first_seq)
        .cloned()
        .collect::<Vec<EventEnvelopeV1>>();

    let completed_turns = events
        .iter()
        .filter(|event| {
            event.actor.agent_id.as_deref() == Some(agent_id)
                && matches!(event.payload, EventV1::AssistantMessageFinished(_))
        })
        .count();
    if completed_turns <= 1 && matches!(trigger_reason, "manual" | "pre_prompt" | "proactive") {
        return Ok(None);
    }
    if completed_turns == 0 && matches!(trigger_reason, "aborted_response" | "failed_response") {
        return Ok(None);
    }

    let context_messages = build_agent_conversation_messages(&events, agent_id);
    let mut total_tokens = estimate_context_tokens(&context_messages).total_tokens;
    if let Some(summary) = run_state
        .provider_context_by_agent
        .get(agent_id)
        .and_then(|context| context.compacted_summary.as_deref())
    {
        total_tokens = total_tokens.saturating_add(estimate_text_tokens(summary));
    }

    let force_compact = matches!(
        trigger_reason,
        "manual" | "overflow" | "aborted_response" | "failed_response"
    );
    if !force_compact && !context_budget.requires_compaction() {
        return Ok(None);
    }
    if force_compact && trigger_reason != "manual" && total_tokens < 100 {
        return Ok(None);
    }

    let Some(request_budget) = context_budget.request_snapshot() else {
        return Ok(None);
    };
    if request_budget.compaction_threshold_tokens.is_none() && trigger_reason != "manual" {
        return Ok(None);
    }
    let model = AgentModelRef::parse(&determine_model_ref(run_state, agent_id));
    let history_allowance =
        context_budget.history_allowance(settings.keep_recent_tokens, trigger_reason != "manual");
    let retained_tokens = if trigger_reason == "manual" {
        context_messages
            .iter()
            .rposition(|message| {
                matches!(message, crate::conversation::ConversationMessage::User(_))
            })
            .map_or(history_allowance, |index| {
                estimate_messages_tokens(&context_messages[index..])
                    .max(1)
                    .min(history_allowance)
            })
    } else {
        history_allowance
    };
    let Some(typed) = prepare_typed_compaction(TypedCompactionPreparationRequest {
        events: &all_events,
        agent_id,
        model: &model,
        request_budget,
        keep_recent_tokens: retained_tokens,
        preserve_latest_completed_turn: trigger_reason == "manual",
    })?
    else {
        return Ok(None);
    };
    let text_split = typed.text_split;
    let is_text_split = text_split.is_some();
    let cut_point = CutPointResult {
        first_kept_event_seq: typed.first_kept_event_seq,
        first_kept_request_id: typed.first_kept_request_id.clone(),
        is_split_turn: false,
        turn_start_seq: None,
        tokens_before: typed.request_budget.pre_input_tokens,
    };
    let (mut messages_to_summarize, _, _) =
        split_messages_at_cut_point(&context_messages, &cut_point);
    let mut turn_prefix_messages = match text_split {
        Some(split) => vec![split_prefix_message(
            &context_messages,
            cut_point.first_kept_event_seq,
            split.byte_index,
        )?],
        None => Vec::new(),
    };
    if messages_to_summarize.is_empty() && !turn_prefix_messages.is_empty() {
        messages_to_summarize = std::mem::take(&mut turn_prefix_messages);
    }
    if messages_to_summarize.is_empty() {
        return Ok(None);
    }

    let file_ops = extract_file_ops_from_messages(&messages_to_summarize, &turn_prefix_messages);
    let previous_summary = find_previous_summary(&events, agent_id);
    let summary_prompt = build_summarization_prompt(
        &messages_to_summarize,
        previous_summary.as_deref(),
        None,
        &file_ops,
    );
    let turn_prefix_prompt = (is_text_split && !turn_prefix_messages.is_empty())
        .then(|| build_turn_prefix_prompt(&turn_prefix_messages));
    let (read_files, modified_files) = compute_file_lists(&file_ops);
    let durable_state = durable_compaction_state(&all_events, agent_id, read_files, modified_files);
    Ok(Some(PreparedSessionCompaction {
        agent_id: agent_id.to_string(),
        trigger_reason: trigger_reason.to_string(),
        model,
        summary_prompt,
        turn_prefix_prompt,
        first_kept_event_seq: cut_point.first_kept_event_seq,
        first_kept_request_id: cut_point.first_kept_request_id,
        first_kept_entry_id: Some(typed.first_kept_entry_id),
        tokens_before: typed.request_budget.pre_input_tokens,
        preserved_message_tokens: typed.request_budget.retained_history_tokens,
        summary_max_tokens: typed.request_budget.summary_allowance_tokens,
        request_budget,
        durable_agent_tail_seq: super::super::provider_context::latest_agent_event_seq(
            &all_events,
            agent_id,
        ),
        read_files: durable_state.read_files,
        modified_files: durable_state.modified_files,
        current_intent: durable_state.current_intent,
        committed_events: all_events,
    }))
}

fn split_prefix_message(
    messages: &[ConversationMessage],
    sequence: u64,
    byte_index: usize,
) -> Result<ConversationMessage, CoordinatorError> {
    let message = messages
        .iter()
        .find(|message| match message {
            ConversationMessage::User(message) => message.seq == Some(sequence),
            ConversationMessage::Assistant(message) => {
                message.first_seq == Some(sequence) || message.last_seq == Some(sequence)
            }
            ConversationMessage::ToolResult(_) | ConversationMessage::Checkpoint(_) => false,
        })
        .cloned()
        .ok_or_else(|| {
            CoordinatorError::CompactionFailed(
                "typed split entry is absent from projected conversation".to_string(),
            )
        })?;
    match message {
        ConversationMessage::User(mut message) => {
            message.text = message
                .text
                .get(..byte_index)
                .ok_or_else(|| {
                    CoordinatorError::CompactionFailed(
                        "typed user split is not a UTF-8 boundary".to_string(),
                    )
                })?
                .to_string();
            Ok(ConversationMessage::User(message))
        }
        ConversationMessage::Assistant(mut message) => {
            message.text = message
                .text
                .get(..byte_index)
                .ok_or_else(|| {
                    CoordinatorError::CompactionFailed(
                        "typed assistant split is not a UTF-8 boundary".to_string(),
                    )
                })?
                .to_string();
            Ok(ConversationMessage::Assistant(message))
        }
        ConversationMessage::ToolResult(_) | ConversationMessage::Checkpoint(_) => {
            Err(CoordinatorError::CompactionFailed(
                "typed split targeted an atomic protocol entry".to_string(),
            ))
        }
    }
}
