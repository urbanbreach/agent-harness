use serde_json::Value;

use crate::agent::{ProviderContext, ProviderConversationTurn};
use crate::conversation::ConversationMessage;
use crate::text::truncate_with_ellipsis;

use super::PROVIDER_CONTEXT_COMPACTION_TURN_EXCERPT_MAX_CHARS;

pub(super) fn summarize_compaction_text(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_with_ellipsis(
        &normalized,
        PROVIDER_CONTEXT_COMPACTION_TURN_EXCERPT_MAX_CHARS,
    )
}

pub(super) fn approximate_turn_tokens(turn: &ProviderConversationTurn) -> u32 {
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
                        .map(Value::to_string)
                        .as_deref()
                        .map(approximate_text_tokens)
                        .unwrap_or(0),
                )
        }
    }
}

pub(in crate::coord) fn approximate_text_tokens(text: &str) -> u32 {
    (u32::try_from(text.chars().count()).unwrap_or(u32::MAX) / 4).max(1)
}

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

pub(super) fn preserved_tokens_estimate(turns: &[ProviderConversationTurn]) -> u32 {
    turns.iter().map(approximate_turn_tokens).sum::<u32>()
}
