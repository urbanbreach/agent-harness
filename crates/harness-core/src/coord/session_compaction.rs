//! Coordinator-side session compaction.
//!
//! Pi-style session compaction: find a cut point in the agent's events,
//! summarize the compacted window via an LLM call, append a single
//! `SessionCompaction` event, and update the provider context.
//!
//! This replaces the old checkpoint-based `compact_provider_context` flow.
//! Unlike the old flow, no checkpoint artifacts are written — the summary
//! lives entirely in the `SessionCompaction` event and the in-memory
//! `ProviderContext`.

use std::sync::Arc;

use harness_providers::{
    CompletionMessage, CompletionRequest, MessageRole, Provider, ProviderStreamEvent,
};
use tokio_stream::StreamExt;

use crate::agent::{AgentModelRef, ProviderContext};
use crate::clock::Clock;
use crate::config::CompactionSettings;
use crate::conversation::{project_conversation, ConversationMessage};
use crate::event::{EventEnvelopeV1, EventV1, SessionCompactionEvent};
use crate::redact::Redactor;
use crate::store::EventStore;

use super::compaction::{
    build_summarization_prompt, build_turn_prefix_prompt, compute_file_lists,
    estimate_context_tokens, estimate_messages_tokens, estimate_text_tokens,
    extract_file_ops_from_tool_call, find_cut_point, find_manual_cut_point, merge_file_operations,
    should_compact, CutPointResult, FileOperations, SUMMARIZATION_SYSTEM_PROMPT,
};
use super::{append_payload_event, system_actor, CoordinatorError, RunState};

/// Result of a successful session compaction.
#[derive(Debug, Clone)]
pub struct AppliedCompaction {
    /// The generated compaction summary (including appended file operation tags).
    pub summary: String,
    /// Sequence number of the first event kept after compaction.
    pub first_kept_event_seq: u64,
    /// Estimated token count before compaction.
    pub tokens_before: u32,
    /// Estimated token count after compaction.
    pub tokens_after: u32,
}

/// Default context window when model metadata is unavailable.
const DEFAULT_CONTEXT_WINDOW_TOKENS: u32 = 128_000;

/// Compact a session for the given agent.
///
/// This is the coordinator-side entry point for Pi-style session compaction.
/// It reads the agent's events from the event store, finds a cut point,
/// summarizes the compacted window via an LLM call, appends a single
/// `SessionCompaction` event, and updates the provider context.
///
/// Returns `Ok(None)` when compaction is disabled or not needed.
pub(in crate::coord) async fn compact_session<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    provider: Arc<dyn Provider>,
    agent_id: &str,
    trigger_reason: &str,
    settings: &CompactionSettings,
    prompt_tokens_estimate: Option<u32>,
) -> Result<Option<AppliedCompaction>, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    // 1. If compaction is disabled, no-op.
    if !settings.enabled {
        return Ok(None);
    }

    // 2. Read all events from the event store.
    let all_events = collect_events(run_state).await?;

    // 3. Check if the agent has any events.

    // If a previous SessionCompaction exists, only consider events at or after
    // its first_kept_event_seq — earlier events are already captured in the summary.
    let latest_compaction = all_events
        .iter()
        .rev()
        .find(|e| {
            matches!(&e.payload, EventV1::SessionCompaction(payload) if payload.agent_id == agent_id)
        })
        .and_then(|e| match &e.payload {
            EventV1::SessionCompaction(payload) => Some(payload),
            _ => None,
        });
    let effective_first_seq = latest_compaction
        .map(|c| c.first_kept_event_seq)
        .unwrap_or(0);
    let events: Vec<EventEnvelopeV1> = all_events
        .iter()
        .filter(|e| e.seq >= effective_first_seq)
        .cloned()
        .collect();

    // History triggers need two completed turns to have prior history. Terminal
    // triggers summarize the current partial turn, so they only no-op with zero
    // completed history. Overflow can still compact a single oversized turn.
    let completed_turns = events
        .iter()
        .filter(|e| {
            e.actor.agent_id.as_deref() == Some(agent_id)
                && matches!(e.payload, EventV1::AssistantMessageFinished(_))
        })
        .count();
    if completed_turns <= 1 && matches!(trigger_reason, "manual" | "pre_prompt" | "proactive") {
        return Ok(None);
    }
    if completed_turns == 0 && matches!(trigger_reason, "aborted_response" | "failed_response") {
        return Ok(None);
    }

    // 4. Estimate context tokens from conversation messages.
    let context_messages = build_agent_conversation_messages(&events, agent_id);
    let context_usage = estimate_context_tokens(&context_messages);
    let mut total_tokens = context_usage.total_tokens;

    // Include tokens from a previous compaction summary if present.
    if let Some(context) = run_state.provider_context_by_agent.get(agent_id) {
        if let Some(summary) = &context.compacted_summary {
            total_tokens = total_tokens.saturating_add(estimate_text_tokens(summary));
        }
    }

    // 5. Determine context window from recorded runtime context or config fallback.
    // When no model reports a window but token-trigger estimation is enabled, use the
    // configured fallback budget so tests and small-model profiles trigger compaction.
    let context_window = run_state
        .recorded_runtime_context
        .as_ref()
        .and_then(|ctx| ctx.max_input_tokens.or(ctx.context_window_tokens))
        .or(settings
            .estimated_token_triggers
            .then_some(settings.fallback_input_tokens))
        .unwrap_or(DEFAULT_CONTEXT_WINDOW_TOKENS);

    // 6. Check if compaction should trigger.
    // For pre_prompt/proactive triggers, include the upcoming prompt estimate so
    // compaction fires before the window would be exceeded.
    let estimated_total = total_tokens.saturating_add(prompt_tokens_estimate.unwrap_or(0));
    let force_compact = matches!(
        trigger_reason,
        "manual" | "overflow" | "aborted_response" | "failed_response"
    );
    if !force_compact && !should_compact(estimated_total, context_window, settings) {
        return Ok(None);
    }

    // Skip compaction for non-manual forced triggers when the context is too
    // small to benefit — the trigger is likely caused by external overhead.
    if force_compact && trigger_reason != "manual" && total_tokens < 100 {
        return Ok(None);
    }

    // 7. Find cut point in the agent's events.
    // Manual and pre_prompt compaction preserve whole turns: manual keeps the
    // latest completed turn, while pre_prompt keeps the latest turn so the
    // upcoming prompt plus that turn fit under the model window. Overflow and
    // proactive triggers use a token-budget cut-point that may split a turn.
    let cut_point = if trigger_reason == "manual" || trigger_reason == "pre_prompt" {
        find_manual_cut_point(&events, agent_id)
    } else {
        let prompt_estimate = prompt_tokens_estimate.unwrap_or(0);
        let window_budget = context_window
            .saturating_sub(settings.reserve_tokens)
            .saturating_sub(prompt_estimate);
        let recent_budget = settings.keep_recent_tokens.min(window_budget);
        find_cut_point(&events, agent_id, recent_budget)
    };
    let Some(cut_point) = cut_point else {
        return Ok(None);
    };

    // 8. Split conversation messages at the cut point.
    let (messages_to_summarize, turn_prefix_messages, _preserved_messages) =
        split_messages_at_cut_point(&context_messages, &cut_point);

    // If there is nothing before the cut point, compaction would be a no-op.
    if messages_to_summarize.is_empty() && turn_prefix_messages.is_empty() {
        return Ok(None);
    }

    // 9. Extract file operations from the messages to summarize.
    let file_ops = extract_file_ops_from_messages(&messages_to_summarize, &turn_prefix_messages);

    // 10. Find previous summary from the latest SessionCompaction event.
    let previous_summary = find_previous_summary(&events, agent_id);

    // 11. Build the summarization prompt.
    let prompt = build_summarization_prompt(
        &messages_to_summarize,
        previous_summary.as_deref(),
        None,
        &file_ops,
    );

    // 12. Get model ref for the LLM call.
    let model_ref = determine_model_ref(run_state, agent_id);
    let model = AgentModelRef::parse(&model_ref);

    // 13. Call the LLM to generate the summary.
    let mut summary = call_summary_llm(&provider, &model.provider_id, &model.model_id, &prompt)
        .await
        .map_err(CoordinatorError::CompactionFailed)?;

    // 14. Handle split turn: generate turn prefix summary and combine.
    if cut_point.is_split_turn && !turn_prefix_messages.is_empty() {
        let turn_prefix_prompt = build_turn_prefix_prompt(&turn_prefix_messages);
        let turn_prefix_summary = call_summary_llm(
            &provider,
            &model.provider_id,
            &model.model_id,
            &turn_prefix_prompt,
        )
        .await
        .map_err(CoordinatorError::CompactionFailed)?;

        summary =
            format!("{summary}\n\n---\n\n**Turn Context (split turn):**\n\n{turn_prefix_summary}");
    }

    // 15. Append file operations to the summary.
    let (read_files, modified_files) = compute_file_lists(&file_ops);
    let file_ops_text = super::compaction::format_file_operations(&read_files, &modified_files);
    if !file_ops_text.is_empty() {
        summary.push_str(&file_ops_text);
    }

    // 16. Estimate tokens after compaction.
    let tokens_after = estimate_text_tokens(&summary)
        .saturating_add(estimate_messages_tokens(&_preserved_messages));

    // 17. Append exactly one SessionCompaction event.
    append_payload_event(
        clock,
        redactor,
        run_state,
        system_actor(),
        Some(format!("compaction:{agent_id}")),
        EventV1::SessionCompaction(SessionCompactionEvent {
            agent_id: agent_id.to_string(),
            summary: summary.clone(),
            first_kept_event_seq: cut_point.first_kept_event_seq,
            first_kept_request_id: cut_point.first_kept_request_id.clone(),
            tokens_before: total_tokens,
            read_files: read_files.clone(),
            modified_files: modified_files.clone(),
            trigger_reason: trigger_reason.to_string(),
            from_hook: false,
        }),
    )?;

    // 18. Update the provider context. For terminal compaction triggers
    // (aborted_response, failed_response), preserve incomplete/aborted turns
    // that were pushed before compaction (e.g. by push_incomplete_provider_turn).
    // For other triggers, clear preserved_turns since they are now captured in the summary.
    let preserve_terminal_turns = matches!(trigger_reason, "aborted_response" | "failed_response");
    let preserved_turns = if preserve_terminal_turns {
        run_state
            .provider_context_by_agent
            .get(agent_id)
            .map(|ctx| ctx.preserved_turns.clone())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let new_context = ProviderContext {
        compacted_summary: Some(summary.clone()),
        preserved_turns,
        checkpoint: None,
    };
    run_state
        .provider_context_by_agent
        .insert(agent_id.to_string(), new_context);

    Ok(Some(AppliedCompaction {
        summary,
        first_kept_event_seq: cut_point.first_kept_event_seq,
        tokens_before: total_tokens,
        tokens_after,
    }))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Collect all events from the event store.
async fn collect_events(run_state: &RunState) -> Result<Vec<EventEnvelopeV1>, CoordinatorError> {
    let stream = run_state.event_store.replay(1)?;
    let mut events = Vec::new();
    let mut stream = std::pin::pin!(stream);
    while let Some(result) = stream.next().await {
        events.push(result?);
    }
    Ok(events)
}

/// Build conversation messages from an agent's events.
fn build_agent_conversation_messages(
    events: &[EventEnvelopeV1],
    agent_id: &str,
) -> Vec<ConversationMessage> {
    let agent_events: Vec<EventEnvelopeV1> = events
        .iter()
        .filter(|e| {
            e.actor.agent_id.as_deref() == Some(agent_id)
                || matches!(
                    e.payload,
                    EventV1::UserMessageSubmitted(_) | EventV1::ProviderRequestStarted(_)
                )
        })
        .cloned()
        .collect();
    let projection = project_conversation(&agent_events, &[]).unwrap_or_default();
    projection.messages
}

/// Split conversation messages at the cut point into:
/// - messages to summarize (before the cut)
/// - turn prefix messages (for split turns, between turn start and cut)
/// - preserved messages (from the cut onward)
fn split_messages_at_cut_point(
    messages: &[ConversationMessage],
    cut_point: &CutPointResult,
) -> (
    Vec<ConversationMessage>,
    Vec<ConversationMessage>,
    Vec<ConversationMessage>,
) {
    if cut_point.is_split_turn {
        let turn_start_seq = cut_point.turn_start_seq.unwrap_or(0);
        let messages_to_summarize = messages
            .iter()
            .filter(|m| message_seq(m) < turn_start_seq)
            .cloned()
            .collect();
        let turn_prefix_messages = messages
            .iter()
            .filter(|m| {
                let seq = message_seq(m);
                seq >= turn_start_seq && seq < cut_point.first_kept_event_seq
            })
            .cloned()
            .collect();
        let preserved_messages = messages
            .iter()
            .filter(|m| message_seq(m) >= cut_point.first_kept_event_seq)
            .cloned()
            .collect();
        (
            messages_to_summarize,
            turn_prefix_messages,
            preserved_messages,
        )
    } else {
        let messages_to_summarize = messages
            .iter()
            .filter(|m| message_seq(m) < cut_point.first_kept_event_seq)
            .cloned()
            .collect();
        let preserved_messages = messages
            .iter()
            .filter(|m| message_seq(m) >= cut_point.first_kept_event_seq)
            .cloned()
            .collect();
        (messages_to_summarize, Vec::new(), preserved_messages)
    }
}

/// Get the chronological position (seq) of a conversation message.
fn message_seq(msg: &ConversationMessage) -> u64 {
    match msg {
        ConversationMessage::User(m) => m.seq.unwrap_or(0),
        ConversationMessage::Assistant(m) => m.last_seq.or(m.first_seq).unwrap_or(0),
        ConversationMessage::ToolResult(m) => m.seq.unwrap_or(0),
        ConversationMessage::Checkpoint(m) => m.through_seq,
    }
}

/// Extract file operations from conversation messages.
///
/// Tries to parse each tool call's `args_summary` as JSON and extract file
/// operations. Tool calls whose args cannot be parsed as JSON are skipped.
fn extract_file_ops_from_messages(
    messages_to_summarize: &[ConversationMessage],
    turn_prefix_messages: &[ConversationMessage],
) -> FileOperations {
    let mut ops = FileOperations::new();
    for msg in messages_to_summarize
        .iter()
        .chain(turn_prefix_messages.iter())
    {
        if let ConversationMessage::Assistant(assistant) = msg {
            for tool_call in &assistant.tool_calls {
                if let Ok(args) = serde_json::from_str::<serde_json::Value>(&tool_call.args_summary)
                {
                    if let Some(op) = extract_file_ops_from_tool_call(&tool_call.tool_id, &args) {
                        merge_file_operations(&mut ops, op);
                    }
                }
            }
        }
    }
    ops
}

/// Find the previous compaction summary from the latest `SessionCompaction` event.
///
/// Searches by the `agent_id` field inside the `SessionCompactionEvent` payload,
/// not the event actor (which is `System` for compaction events).
fn find_previous_summary(events: &[EventEnvelopeV1], agent_id: &str) -> Option<String> {
    events.iter().rev().find_map(|e| match &e.payload {
        EventV1::SessionCompaction(event) if event.agent_id == agent_id => {
            Some(event.summary.clone())
        }
        _ => None,
    })
}

/// Determine the model ref for an agent.
///
/// Checks running turns first, then falls back to the agent's profile.
fn determine_model_ref(run_state: &RunState, agent_id: &str) -> String {
    for running in run_state.running_agent_turns.values() {
        if running.agent_id == agent_id {
            return running.model_ref.clone();
        }
    }
    if let Some(profile) = run_state.agents.get(agent_id) {
        return profile.model_ref.clone();
    }
    "default:default".to_string()
}

/// Call the LLM to generate a summary from a prompt.
///
/// Streams the completion and collects text deltas until `Done`.
async fn call_summary_llm(
    provider: &Arc<dyn Provider>,
    provider_id: &str,
    model_id: &str,
    user_prompt: &str,
) -> Result<String, String> {
    let request = CompletionRequest {
        provider_id: Some(provider_id.to_string()),
        model_id: model_id.to_string(),
        messages: vec![
            CompletionMessage {
                role: MessageRole::System,
                content: SUMMARIZATION_SYSTEM_PROMPT.to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
            CompletionMessage {
                role: MessageRole::User,
                content: user_prompt.to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
        ],
        temperature: None,
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        tools: None,
        tool_choice: None,
        context: Default::default(),
        stream: true,
    };

    let mut stream = provider.stream_completion(request).await;
    let mut output = String::new();
    while let Some(event) = stream.next().await {
        match event {
            ProviderStreamEvent::TextDelta(delta) => output.push_str(&delta),
            ProviderStreamEvent::Error { message, .. } => return Err(message),
            ProviderStreamEvent::Done { .. } | ProviderStreamEvent::DoneWithMetadata { .. } => {
                break;
            }
            ProviderStreamEvent::Start
            | ProviderStreamEvent::Started { .. }
            | ProviderStreamEvent::ReasoningDelta(_)
            | ProviderStreamEvent::ToolCallDelta { .. }
            | ProviderStreamEvent::ToolCallComplete { .. } => {}
        }
    }

    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Err("LLM summary was empty".to_string());
    }
    Ok(trimmed.to_string())
}
