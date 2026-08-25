//! Context projection from events, handling compaction and branch summaries.
//!
//! Ports Pi's `buildSessionContext` and `buildSessionContextWithCompaction`
//! into the harness event model. These are pure functions that derive the
//! provider-facing message list from the append-only event log, respecting
//! the latest compaction boundary and injecting branch summaries.

use crate::conversation::{
    compaction_first_kept_sequence, ConversationMessage, ConversationUserMessage,
};
use crate::event::{EventEnvelopeV1, EventV1};
use crate::ids::RequestId;

use super::super::provider_context::project_committed_context;
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
    let Ok(projection) = project_committed_context(events, agent_id) else {
        return Vec::new();
    };
    projection
        .messages
        .into_iter()
        .map(|message| match message {
            ConversationMessage::Checkpoint(checkpoint) => {
                ConversationMessage::User(ConversationUserMessage {
                    request_id: RequestId::new(&checkpoint.checkpoint_id),
                    text: checkpoint.summary,
                    seq: Some(checkpoint.through_seq.saturating_add(1)),
                    agent_id: Some(agent_id.to_string()),
                })
            }
            ConversationMessage::User(_)
            | ConversationMessage::Assistant(_)
            | ConversationMessage::ToolResult(_) => message,
        })
        .collect()
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

    let first_kept_seq = events.iter().rev().find_map(|event| match &event.payload {
        EventV1::SessionCompaction(compaction) if compaction.agent_id == agent_id => {
            compaction_first_kept_sequence(events, compaction)
        }
        _ => None,
    });
    for event in events {
        if first_kept_seq.is_some_and(|first_kept_seq| event.seq < first_kept_seq) {
            continue;
        }
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
