//! Context projection from events, handling compaction and branch summaries.
//!
//! Ports Pi's `buildSessionContext` and `buildSessionContextWithCompaction`
//! into the harness event model. These are pure functions that derive the
//! provider-facing message list from the append-only event log, respecting
//! the latest compaction boundary and injecting branch summaries.

use crate::conversation::{project_conversation, ConversationMessage, ConversationUserMessage};
use crate::event::{EventEnvelopeV1, EventV1, SessionCompactionEvent};
use crate::ids::RequestId;

use super::branch_summary::{BRANCH_SUMMARY_PREFIX, BRANCH_SUMMARY_SUFFIX};
use super::tokens::{estimate_context_tokens, ContextUsageEstimate};

/// Build the session context messages for an agent from the event log.
///
/// Finds the latest [`EventV1::SessionCompaction`] for the agent and builds
/// context as: `[summary_message] + [messages from first_kept_event_seq onwards]`.
///
/// If no compaction exists, returns all projected conversation messages.
///
/// Ports Pi's `buildSessionContext`.
pub fn build_session_context(
    events: &[EventEnvelopeV1],
    agent_id: &str,
) -> Vec<ConversationMessage> {
    let latest_compaction = find_latest_session_compaction(events, agent_id);

    let agent_events: Vec<EventEnvelopeV1> = events
        .iter()
        .filter(|event| {
            let actor_matches = event
                .actor
                .agent_id
                .as_deref()
                .is_some_and(|id| id == agent_id);
            let stream_matches = event
                .stream_key
                .as_deref()
                .is_some_and(|key| key == format!("agent:{agent_id}"));
            actor_matches || stream_matches
        })
        .cloned()
        .collect();

    let projection = project_conversation(&agent_events, &[]).unwrap_or_default();
    let mut messages = projection.messages;

    if let Some((compaction_event, compaction_payload)) = latest_compaction {
        let first_kept_seq = compaction_payload.first_kept_event_seq;
        messages.retain(|m| message_seq(m) >= first_kept_seq);

        let summary_message = ConversationMessage::User(ConversationUserMessage {
            request_id: RequestId::new(&format!("compaction-summary-{}", compaction_event.seq)),
            text: compaction_payload.summary.clone(),
            seq: Some(compaction_event.seq),
            agent_id: Some(agent_id.to_string()),
        });
        messages.insert(0, summary_message);
    }

    messages
}

/// Build the session context messages with branch summaries injected.
///
/// Same as [`build_session_context`], but also injects [`EventV1::BranchSummary`]
/// events as user messages wrapped in `<summary>` tags at their chronological
/// position in the event sequence.
///
/// Ports Pi's `buildSessionContextWithCompaction` with branch summary support.
pub fn build_session_context_with_branch_summaries(
    events: &[EventEnvelopeV1],
    agent_id: &str,
) -> Vec<ConversationMessage> {
    let mut messages = build_session_context(events, agent_id);

    for event in events {
        if let EventV1::BranchSummary(payload) = &event.payload {
            if payload.agent_id != agent_id {
                continue;
            }

            let summary_text = format!(
                "{BRANCH_SUMMARY_PREFIX}{}{BRANCH_SUMMARY_SUFFIX}",
                payload.summary
            );
            let branch_msg = ConversationMessage::User(ConversationUserMessage {
                request_id: RequestId::new(&format!("branch-summary-{}", event.seq)),
                text: summary_text,
                seq: Some(event.seq),
                agent_id: Some(agent_id.to_string()),
            });

            let insert_pos = messages
                .iter()
                .position(|m| message_seq(m) > event.seq)
                .unwrap_or(messages.len());
            messages.insert(insert_pos, branch_msg);
        }
    }

    messages
}

/// Estimate the token usage of the session context for an agent.
///
/// Convenience wrapper that builds the session context and estimates
/// its token count using the chars/4 heuristic.
pub fn estimate_session_context_tokens(
    events: &[EventEnvelopeV1],
    agent_id: &str,
) -> ContextUsageEstimate {
    let messages = build_session_context(events, agent_id);
    estimate_context_tokens(&messages)
}

/// Find the latest [`EventV1::SessionCompaction`] event for the specified agent.
fn find_latest_session_compaction<'a>(
    events: &'a [EventEnvelopeV1],
    agent_id: &str,
) -> Option<(&'a EventEnvelopeV1, &'a SessionCompactionEvent)> {
    events.iter().rev().find_map(|event| match &event.payload {
        EventV1::SessionCompaction(payload) if payload.agent_id == agent_id => {
            Some((event, payload))
        }
        _ => None,
    })
}

/// Get the chronological position (seq) of a conversation message.
///
/// Mirrors the helper in [`branch_summary`] for consistent seq-based ordering.
fn message_seq(msg: &ConversationMessage) -> u64 {
    match msg {
        ConversationMessage::User(m) => m.seq.unwrap_or(0),
        ConversationMessage::Assistant(m) => m.last_seq.or(m.first_seq).unwrap_or(0),
        ConversationMessage::ToolResult(m) => m.seq.unwrap_or(0),
        ConversationMessage::Checkpoint(m) => m.through_seq,
    }
}
