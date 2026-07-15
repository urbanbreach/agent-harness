//! Cut-point detection for context compaction.
//!
//! Ports Pi's `findCutPoint` and `shouldCompact` to the Rust event model.
//! Walks backward through an agent's events, accumulating token estimates,
//! and finds the first valid cut point that keeps approximately `keep_recent_tokens`.

use crate::config::CompactionSettings;
use crate::event::{EventEnvelopeV1, EventV1};

use super::tokens::estimate_text_tokens;

/// Result of finding a compaction cut point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutPointResult {
    /// Seq of the first event to keep.
    pub first_kept_event_seq: u64,
    /// Request ID of the first kept message, if applicable.
    pub first_kept_request_id: Option<String>,
    /// `true` when the cut lands mid-turn (at an assistant message, not a user message).
    pub is_split_turn: bool,
    /// Seq of the user message that started the split turn, if `is_split_turn`.
    pub turn_start_seq: Option<u64>,
    /// Total estimated context tokens before compaction.
    pub tokens_before: u32,
}

/// Check if compaction should trigger based on context usage.
///
/// Ports Pi's `shouldCompact`: returns `false` when disabled, otherwise
/// `context_tokens > context_window.saturating_sub(reserve_tokens)`.
pub fn should_compact(
    context_tokens: u32,
    context_window: u32,
    settings: &CompactionSettings,
) -> bool {
    if !settings.enabled {
        return false;
    }
    context_tokens > context_window.saturating_sub(settings.reserve_tokens)
}

/// Find a cut point that preserves the agent's last complete turn and summarizes
/// everything before it.
///
/// Used for explicit manual compaction requests, where the operator expects the
/// most recent turn to remain intact and all prior turns to be rolled into a
/// summary. Returns `None` if the agent has fewer than two completed turns.
pub fn find_manual_cut_point(events: &[EventEnvelopeV1], agent_id: &str) -> Option<CutPointResult> {
    let turn_ids: std::collections::HashSet<&str> = events
        .iter()
        .filter(|e| e.actor.agent_id.as_deref() == Some(agent_id))
        .filter_map(|e| {
            e.correlation_id
                .as_deref()
                .and_then(|s| if s.is_empty() { None } else { Some(s) })
        })
        .collect();

    let agent_events: Vec<&EventEnvelopeV1> = events
        .iter()
        .filter(|e| {
            e.actor.agent_id.as_deref() == Some(agent_id)
                || matches!(
                    &e.payload,
                    EventV1::UserMessageSubmitted(payload)
                        if turn_ids.contains(payload.request_id.as_str())
                )
        })
        .collect();

    if agent_events.is_empty() {
        return None;
    }

    // Find the last completed turn, then the nearest user message before it.
    let last_assistant_idx = agent_events
        .iter()
        .rposition(|e| matches!(e.payload, EventV1::AssistantMessageFinished(_)))?;
    let last_user_idx = agent_events[..last_assistant_idx]
        .iter()
        .rposition(|e| matches!(e.payload, EventV1::UserMessageSubmitted(_)))?;

    // If the user message is the very first agent event, there is no prior history.
    if last_user_idx == 0 {
        return None;
    }

    let cut_event = agent_events[last_user_idx];
    let tokens_before: u32 = agent_events
        .iter()
        .map(|e| estimate_event_tokens(&e.payload))
        .sum();

    Some(CutPointResult {
        first_kept_event_seq: cut_event.seq,
        first_kept_request_id: extract_request_id(&cut_event.payload),
        is_split_turn: false,
        turn_start_seq: None,
        tokens_before,
    })
}

/// Find the cut point in an agent's events that keeps approximately `keep_recent_tokens`.
///
/// Walks backward through the agent's events, accumulating token estimates.
/// Valid cut points are `UserMessageSubmitted` (turn start) or
/// `AssistantMessageFinished` (assistant barrier). Never cuts at a tool result.
///
/// Returns `None` if the agent has no events.
pub fn find_cut_point(
    events: &[EventEnvelopeV1],
    agent_id: &str,
    keep_recent_tokens: u32,
) -> Option<CutPointResult> {
    let turn_ids: std::collections::HashSet<&str> = events
        .iter()
        .filter(|e| e.actor.agent_id.as_deref() == Some(agent_id))
        .filter_map(|e| {
            e.correlation_id
                .as_deref()
                .and_then(|s| if s.is_empty() { None } else { Some(s) })
        })
        .collect();

    let agent_events: Vec<&EventEnvelopeV1> = events
        .iter()
        .filter(|e| {
            e.actor.agent_id.as_deref() == Some(agent_id)
                || matches!(
                    &e.payload,
                    EventV1::UserMessageSubmitted(payload)
                        if turn_ids.contains(payload.request_id.as_str())
                )
        })
        .collect();

    if agent_events.is_empty() {
        return None;
    }

    let tokens_before: u32 = agent_events
        .iter()
        .map(|e| estimate_event_tokens(&e.payload))
        .sum();

    // Collect indices of valid cut points.
    let cut_point_indices: Vec<usize> = agent_events
        .iter()
        .enumerate()
        .filter(|(_, e)| is_valid_cut_point(&e.payload))
        .map(|(i, _)| i)
        .collect();

    if cut_point_indices.is_empty() {
        // No valid cut points — keep everything from the first event.
        return Some(CutPointResult {
            first_kept_event_seq: agent_events[0].seq,
            first_kept_request_id: extract_request_id(&agent_events[0].payload),
            is_split_turn: false,
            turn_start_seq: None,
            tokens_before,
        });
    }

    // Walk backward, accumulating tokens. Default: keep the most recent whole
    // turn (last user-message cut point) so there is prior context to summarize.
    let mut accumulated_tokens = 0u32;
    let mut cut_idx = *cut_point_indices
        .iter()
        .rfind(|&&i| matches!(agent_events[i].payload, EventV1::UserMessageSubmitted(_)))
        .unwrap_or(&cut_point_indices[0]);

    for i in (0..agent_events.len()).rev() {
        let event_tokens = estimate_event_tokens(&agent_events[i].payload);
        if event_tokens == 0 {
            continue;
        }
        accumulated_tokens = accumulated_tokens.saturating_add(event_tokens);

        if accumulated_tokens >= keep_recent_tokens {
            // Find the closest valid cut point at or after index i.
            for &cp in &cut_point_indices {
                if cp >= i {
                    cut_idx = cp;
                    break;
                }
            }
            break;
        }
    }

    let cut_event = agent_events[cut_idx];
    let is_user_message = matches!(cut_event.payload, EventV1::UserMessageSubmitted(_));

    let (is_split_turn, turn_start_seq) = if is_user_message {
        (false, None)
    } else {
        // Cut at assistant message — find the turn start (nearest user message before cut).
        let turn_start = agent_events[..cut_idx]
            .iter()
            .rev()
            .find(|e| matches!(e.payload, EventV1::UserMessageSubmitted(_)));
        match turn_start {
            Some(ts) => (true, Some(ts.seq)),
            None => (false, None),
        }
    };

    Some(CutPointResult {
        first_kept_event_seq: cut_event.seq,
        first_kept_request_id: extract_request_id(&cut_event.payload),
        is_split_turn,
        turn_start_seq,
        tokens_before,
    })
}

/// Whether an event is a valid compaction cut point.
///
/// Valid cut points: `ProviderRequestStarted` (turn start; the event log may
/// not emit a separate `UserMessageSubmitted` for every turn), or
/// `AssistantMessageFinished` (assistant barrier). Tool results are never
/// valid cut points — they must follow their tool call.
fn is_valid_cut_point(payload: &EventV1) -> bool {
    matches!(
        payload,
        EventV1::UserMessageSubmitted(_) | EventV1::AssistantMessageFinished(_)
    )
}

/// Extract the request ID from an event payload, if present.
fn extract_request_id(payload: &EventV1) -> Option<String> {
    match payload {
        EventV1::UserMessageSubmitted(e) => Some(e.request_id.as_str().to_string()),
        EventV1::AssistantMessageFinished(e) => Some(e.request_id.as_str().to_string()),
        EventV1::ProviderRequestStarted(e) => Some(e.request_id.as_str().to_string()),
        EventV1::ProviderRequestFinished(e) => Some(e.request_id.as_str().to_string()),
        EventV1::ProviderStreamDelta(e) => Some(e.request_id.as_str().to_string()),
        EventV1::ProviderReasoningDelta(e) => Some(e.request_id.as_str().to_string()),
        EventV1::ToolCallFinished(_) => {
            // Tool results don't have a request_id field; correlation_id
            // links them to the originating request.
            None
        }
        _ => None,
    }
}

/// Estimate the token contribution of a single event to the context window.
fn estimate_event_tokens(payload: &EventV1) -> u32 {
    match payload {
        EventV1::UserMessageSubmitted(e) => estimate_text_tokens(&e.text),
        EventV1::ProviderStreamDelta(e) => estimate_text_tokens(&e.delta),
        EventV1::ToolCallRequested(e) => {
            estimate_text_tokens(&e.tool_id).saturating_add(estimate_text_tokens(&e.args_summary))
        }
        EventV1::ToolCallFinished(e) => e
            .output_summary
            .as_deref()
            .map(estimate_text_tokens)
            .unwrap_or(0),
        _ => 0,
    }
}
