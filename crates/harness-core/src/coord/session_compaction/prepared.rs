use crate::agent::AgentModelRef;
use crate::config::CompactionSettings;
use crate::context_budget::RequestBudgetSnapshot;
use crate::conversation::ConversationMessage;
use crate::event::{EventEnvelopeV1, EventV1, UiIntentReceivedEvent};
use crate::ids::EntryId;

use super::super::compaction::{
    build_summarization_prompt, compute_file_lists, estimate_context_tokens, estimate_text_tokens,
    CutPointResult,
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
    pub(super) summary_messages: Vec<harness_providers::CompletionMessage>,
    pub(super) summary_tools: Option<Vec<harness_providers::ToolDef>>,
    pub(super) context_window: u32,
    pub(super) required: bool,
    pub(super) warm_max_growth: u32,
    pub(super) within_grace: bool,
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
    if trigger_reason != "manual" && (!settings.enabled || settings.suppress_auto_compaction) {
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
    let latest_visible = all_events.iter().rev().find(|event| {
        super::super::provider_context::event_belongs_to_agent(
            event,
            agent_id,
            &format!("agent:{agent_id}"),
        ) && matches!(
            event.payload,
            EventV1::UserMessageSubmitted(_)
                | EventV1::AssistantMessageFinished(_)
                | EventV1::ToolCallFinished(_)
                | EventV1::SessionCompaction(_)
        )
    });
    if latest_visible.is_some_and(|event| matches!(event.payload, EventV1::SessionCompaction(_))) {
        return Ok(None);
    }
    let effective_first_seq = latest_compaction.map_or(0, |event| event.first_kept_event_seq);
    let events = all_events
        .iter()
        .filter(|event| event.seq >= effective_first_seq)
        .cloned()
        .collect::<Vec<EventEnvelopeV1>>();

    let mut context_messages = build_agent_conversation_messages(&events, agent_id);
    context_messages.retain(|message| !matches!(message, ConversationMessage::Checkpoint(_)));
    let mut total_tokens = estimate_context_tokens(&context_messages).total_tokens;
    if let Some(summary) = run_state
        .provider_context_by_agent
        .get(agent_id)
        .and_then(|context| context.compacted_summary.as_deref())
    {
        total_tokens = total_tokens.saturating_add(estimate_text_tokens(summary));
    }

    let force_compact = matches!(trigger_reason, "manual" | "overflow");
    let Some(mut request_budget) = context_budget.request_snapshot() else {
        return Ok(None);
    };
    if request_budget.compaction_threshold_tokens.is_none() && trigger_reason != "manual" {
        return Ok(None);
    }
    // The canonical snapshot already owns the submitted prompt, including during pre-prompt compaction.
    let pending_request_id = run_state
        .running_agent_turns
        .values()
        .find(|turn| turn.agent_id == agent_id)
        .map(|turn| turn.request_id.as_str())
        .or_else(|| {
            all_events
                .iter()
                .rev()
                .find_map(|event| match &event.payload {
                    EventV1::ProviderRequestStarted(started)
                        if event.actor.agent_id.as_deref() == Some(agent_id) =>
                    {
                        Some(
                            event
                                .correlation_id
                                .as_deref()
                                .unwrap_or(started.request_id.as_str()),
                        )
                    }
                    _ => None,
                })
        });
    if pending_request_id.is_some_and(|request_id| context_messages.iter().any(|message| matches!(message, ConversationMessage::User(user) if user.request_id.as_str() == request_id))) {
        request_budget.components.pending_prompt_tokens = 0;
    }
    let model = AgentModelRef::parse(&determine_model_ref(run_state, agent_id));
    let limits = run_state
        .cached_canonical_provider_view(agent_id)
        .filter(|view| {
            view.runtime_selection.provider_id == model.provider_id
                && view.runtime_selection.model_id == model.model_id
        })
        .map(|view| &view.runtime_selection.resolved_limits);
    let context_window = limits
        .and_then(|limits| limits.context_window_tokens())
        .or_else(|| {
            request_budget.maximum_input_tokens.map(|tokens| {
                tokens.saturating_add(request_budget.reserved_output_tokens.unwrap_or(0))
            })
        })
        .unwrap_or(settings.fallback_input_tokens);
    let summary_max_tokens = 32_768.min(context_window / 2).min(
        limits
            .and_then(|limits| limits.max_output_tokens())
            .unwrap_or(32_768),
    );
    let last_compaction_seq = all_events.iter().rev().find(|event| matches!(&event.payload, EventV1::SessionCompaction(data) if data.agent_id == agent_id)).map(|event| event.seq);
    let occupied = context_budget
        .anchored_context_tokens(&context_messages, last_compaction_seq)
        .unwrap_or(total_tokens);
    let threshold_hundredths = settings
        .threshold_override(
            run_state
                .agents
                .get(agent_id)
                .map_or("", |profile| profile.name.as_str()),
            &format!("{}:{}", model.provider_id, model.model_id),
        )
        .map_or_else(
            || {
                u64::from(context_window)
                    * u64::from(super::policy::threshold_percent(
                        context_window,
                        super::policy::previous_yield(&all_events, agent_id),
                    ))
            },
            |threshold| threshold.token_hundredths(context_window),
        );
    let threshold = super::policy::threshold_tokens(threshold_hundredths);
    let lead = super::policy::lead_tokens(threshold_hundredths);
    let hard_limit = context_window.saturating_sub(super::policy::reserve_tokens(
        settings.reserve_tokens,
        context_window,
    ));
    if let Some(limit) = &mut request_budget.compaction_threshold_tokens {
        *limit = (*limit).min(hard_limit);
    }
    let required = force_compact
        || occupied >= threshold
        || context_budget.requires_compaction()
        || occupied >= hard_limit;
    let lead_threshold = if trigger_reason == "idle" {
        context_window.div_ceil(2)
    } else {
        threshold.saturating_sub(lead)
    };
    if !required && occupied < lead_threshold {
        return Ok(None);
    }
    let history_allowance = CompactionBudget::resolve(Some(request_budget), &all_events, agent_id)
        .history_allowance(super::policy::keep_recent_tokens(
            settings.keep_recent_tokens,
            context_window,
            threshold_hundredths,
        ));
    let Some(typed) = prepare_typed_compaction(TypedCompactionPreparationRequest {
        events: &all_events,
        agent_id,
        model: &model,
        request_budget,
        keep_recent_tokens: history_allowance,
        force_progress: trigger_reason == "overflow",
        context_window,
    })?
    else {
        return Ok(None);
    };
    let cut_point = CutPointResult {
        first_kept_event_seq: typed.first_kept_event_seq,
        first_kept_request_id: typed.first_kept_request_id.clone(),
        is_split_turn: typed.is_split_turn && typed.turn_start_seq.is_some(),
        turn_start_seq: typed.turn_start_seq,
        tokens_before: typed.request_budget.pre_input_tokens,
    };
    let (mut messages_to_summarize, turn_prefix_messages, _) =
        split_messages_at_cut_point(&context_messages, &cut_point);
    if messages_to_summarize.is_empty() && turn_prefix_messages.is_empty() {
        return Ok(None);
    }
    let file_ops = extract_file_ops_from_messages(&messages_to_summarize, &turn_prefix_messages);
    let previous_summary = find_previous_summary(&all_events, agent_id);
    let summary_prompt = if previous_summary.is_none() && cut_point.is_split_turn {
        super::super::compaction::TURN_PREFIX_SUMMARIZATION_PROMPT.to_string()
    } else {
        build_summarization_prompt(previous_summary.as_deref(), &file_ops)
    };
    messages_to_summarize.extend(turn_prefix_messages);
    messages_to_summarize.retain(|message| !matches!(message, ConversationMessage::Checkpoint(_)));
    let profile = run_state
        .agents
        .get(agent_id)
        .ok_or_else(|| CoordinatorError::UnknownAgent(agent_id.to_string()))?;
    let summary_messages =
        crate::agent::transform_context_for_provider(crate::agent::ProviderBoundaryInput {
            profile,
            model: model.clone(),
            model_settings: Default::default(),
            context: crate::agent::ProviderBoundaryContext::ProjectedHarness {
                messages: &messages_to_summarize,
                checkpoint: None,
            },
            tools: None,
            tool_choice: Some(harness_providers::ToolChoice::None),
        })
        .messages;
    let (read_files, modified_files) = compute_file_lists(&file_ops);
    let durable_state = durable_compaction_state(&all_events, agent_id, read_files, modified_files);
    Ok(Some(PreparedSessionCompaction {
        agent_id: agent_id.to_string(),
        trigger_reason: trigger_reason.to_string(),
        model,
        summary_prompt,
        summary_messages,
        summary_tools: None,
        context_window,
        required,
        warm_max_growth: history_allowance.max(8192),
        within_grace: !force_compact
            && occupied < threshold.saturating_add(lead).min(hard_limit)
            && !context_budget.requires_compaction(),
        first_kept_event_seq: cut_point.first_kept_event_seq,
        first_kept_request_id: cut_point.first_kept_request_id,
        first_kept_entry_id: Some(typed.first_kept_entry_id),
        tokens_before: typed
            .request_budget
            .pre_input_tokens
            .max(request_budget.occupied_input_tokens)
            .max(total_tokens),
        preserved_message_tokens: typed.request_budget.retained_history_tokens,
        summary_max_tokens: if context_window == 0 {
            typed.request_budget.summary_allowance_tokens
        } else {
            summary_max_tokens
        },
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
