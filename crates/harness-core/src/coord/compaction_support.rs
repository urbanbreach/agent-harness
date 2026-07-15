//! Residual helpers from the old provider-context compaction module.
//!
//! These types and functions are still referenced by the coordinator's
//! turn-completion, runtime, state, and session-restore code paths.
//! The old checkpoint-based compaction flow has been replaced by
//! [`crate::coord::session_compaction::compact_session`]; these helpers
//! remain only to support the existing call sites until they are migrated.

use crate::agent::{ProviderContext, ProviderConversationTurn};
use crate::conversation::ConversationMessage;
use crate::text::truncate_with_ellipsis;

use super::CoordinatorError;

const FAILURE_REASON_MAX_CHARS: usize = 240;

/// Trigger metadata for a compaction request.
///
/// Used by the coordinator's turn-completion flow to pass context to
/// lifecycle hooks and to record compaction failures.
#[derive(Debug, Clone)]
pub(in crate::coord) struct ProviderCompactionTrigger {
    pub(in crate::coord) agent_id: String,
    pub(in crate::coord) profile_name: String,
    pub(in crate::coord) model_ref: String,
    pub(in crate::coord) provider_id: Option<String>,
    pub(in crate::coord) model_id: Option<String>,
    pub(in crate::coord) through_request_id: Option<String>,
    pub(in crate::coord) trigger_reason: String,
    pub(in crate::coord) tokens_before: Option<u32>,
    pub(in crate::coord) prompt_tokens_estimate: Option<u32>,
    pub(in crate::coord) estimate_source: Option<String>,
}

/// Check whether a provider error reason indicates context overflow.
pub(in crate::coord) fn is_provider_context_overflow_reason(reason: &str) -> bool {
    let normalized = reason.to_ascii_lowercase();
    [
        "context length",
        "context window",
        "too many tokens",
        "prompt token count",
        "maximum context",
        "input token",
        "reduce the length",
        "token count of",
        "exceeds the limit",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

/// Trim and truncate a failure reason for persistence.
pub(in crate::coord) fn truncated_failure_reason(reason: &str) -> Option<String> {
    let reason = reason.trim();
    if reason.is_empty() {
        None
    } else {
        Some(truncate_with_ellipsis(reason, FAILURE_REASON_MAX_CHARS))
    }
}

/// Rough token estimate: chars / 4 (minimum 1).
pub(in crate::coord) fn approximate_text_tokens(text: &str) -> u32 {
    (u32::try_from(text.chars().count()).unwrap_or(u32::MAX) / 4).max(1)
}

/// Estimate total tokens in a [`ProviderContext`].
pub(in crate::coord) fn approximate_provider_context_tokens(context: &ProviderContext) -> u32 {
    let summary_tokens = context
        .compacted_summary
        .as_deref()
        .map(approximate_text_tokens)
        .unwrap_or(0);
    summary_tokens.saturating_add(
        context
            .preserved_turns
            .iter()
            .map(approximate_turn_tokens)
            .sum::<u32>(),
    )
}

fn approximate_turn_tokens(turn: &ProviderConversationTurn) -> u32 {
    if !turn.messages.is_empty() {
        return turn
            .messages
            .iter()
            .map(approximate_conversation_message_tokens)
            .sum();
    }
    approximate_text_tokens(&turn.user_prompt)
        .saturating_add(approximate_text_tokens(&turn.assistant_response))
}

fn approximate_conversation_message_tokens(message: &ConversationMessage) -> u32 {
    match message {
        ConversationMessage::Checkpoint(checkpoint) => approximate_text_tokens(&checkpoint.summary),
        ConversationMessage::User(user) => approximate_text_tokens(&user.text),
        ConversationMessage::Assistant(assistant) => assistant.tool_calls.iter().fold(
            approximate_text_tokens(&assistant.text),
            |tokens, tool_call| {
                tokens
                    .saturating_add(approximate_text_tokens(tool_call.tool_call_id.as_str()))
                    .saturating_add(approximate_text_tokens(&tool_call.tool_id))
                    .saturating_add(approximate_text_tokens(&tool_call.args_summary))
            },
        ),
        ConversationMessage::ToolResult(tool_result) => {
            approximate_text_tokens(tool_result.tool_call_id.as_str())
                .saturating_add(
                    tool_result
                        .tool_id
                        .as_deref()
                        .map(approximate_text_tokens)
                        .unwrap_or(0),
                )
                .saturating_add(
                    tool_result
                        .output_summary
                        .as_deref()
                        .map(approximate_text_tokens)
                        .unwrap_or(0),
                )
                .saturating_add(
                    tool_result
                        .output_json
                        .as_ref()
                        .map(|v| approximate_text_tokens(&v.to_string()))
                        .unwrap_or(0),
                )
        }
    }
}
