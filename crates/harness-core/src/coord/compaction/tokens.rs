//! Token estimation and context usage calculation ported from Pi's compaction.
//!
//! These are pure heuristic functions — no provider calls, no I/O.
//! The chars/4 heuristic mirrors Pi's `Math.ceil(TextEncoder().encode(text).length / 4)`.

use serde_json::Value;

use crate::conversation::ConversationMessage;

/// Estimate token count from text using Pi's chars/4 heuristic.
///
/// Uses byte length (matching Pi's `TextEncoder().encode(text).length`)
/// divided by 4 with ceiling. Empty text returns 0.
pub fn estimate_text_tokens(text: &str) -> u32 {
    let byte_len = u32::try_from(text.len()).unwrap_or(u32::MAX);
    byte_len.div_ceil(4)
}

/// Calculate total context tokens from provider usage components.
///
/// Ports Pi's `calculateContextTokens`: `input + output + cacheRead + cacheWrite`.
pub const fn calculate_context_tokens(
    input: u32,
    output: u32,
    cache_read: u32,
    cache_write: u32,
) -> u32 {
    input
        .saturating_add(output)
        .saturating_add(cache_read)
        .saturating_add(cache_write)
}

/// Estimate token count for a single conversation message.
///
/// Ports Pi's `estimateTokens`:
/// - User: `ceil(text_bytes / 4)`
/// - Assistant: `ceil((text + tool_call_names + tool_call_args) / 4)`
/// - ToolResult: `ceil(output_content_bytes / 4)`
/// - Checkpoint: `ceil(summary_bytes / 4)`
pub fn estimate_message_tokens(message: &ConversationMessage) -> u32 {
    match message {
        ConversationMessage::User(user) => estimate_text_tokens(&user.text),
        ConversationMessage::Assistant(assistant) => {
            let mut chars = assistant.text.len();
            for tool_call in &assistant.tool_calls {
                chars += tool_call.tool_id.len();
                chars += tool_call.args_summary.len();
            }
            estimate_text_tokens_len(chars)
        }
        ConversationMessage::ToolResult(tool_result) => {
            let chars = tool_result
                .output_summary
                .as_deref()
                .map(str::len)
                .unwrap_or_else(|| {
                    tool_result
                        .output_json
                        .as_ref()
                        .map(Value::to_string)
                        .map(|s| s.len())
                        .unwrap_or(0)
                });
            estimate_text_tokens_len(chars)
        }
        ConversationMessage::Checkpoint(checkpoint) => estimate_text_tokens(&checkpoint.summary),
    }
}

/// Estimate total tokens for a slice of messages.
pub fn estimate_messages_tokens(messages: &[ConversationMessage]) -> u32 {
    messages.iter().map(estimate_message_tokens).sum()
}

/// Estimated context token usage derived from conversation messages.
///
/// Ports Pi's `ContextUsageEstimate`. When provider usage data is available
/// (from `ProviderRequestFinishedEvent`), `last_assistant_usage` holds the
/// token count from the last non-zero assistant usage and `estimated` is
/// `false`. When no usage anchor is available, all tokens are estimated
/// and `estimated` is `true`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextUsageEstimate {
    /// Total estimated context tokens (usage anchor + trailing estimates, or all estimates).
    pub total_tokens: u32,
    /// Token count from the last non-zero assistant usage, if available.
    pub last_assistant_usage: Option<u32>,
    /// `true` when the total is purely estimated (no provider usage anchor).
    pub estimated: bool,
}

/// Estimate context tokens from conversation messages.
///
/// Ports Pi's `estimateContextTokens`. Since `ConversationMessage` does not
/// carry provider usage data, this function always estimates all messages.
/// When usage data is available from events, the caller should compute the
/// anchor separately and add trailing estimates.
pub fn estimate_context_tokens(messages: &[ConversationMessage]) -> ContextUsageEstimate {
    let total_tokens = estimate_messages_tokens(messages);
    ContextUsageEstimate {
        total_tokens,
        last_assistant_usage: None,
        estimated: true,
    }
}

/// Ceiling division of byte length by 4 — internal helper matching Pi's `Math.ceil(len / 4)`.
fn estimate_text_tokens_len(byte_len: usize) -> u32 {
    let byte_len = u32::try_from(byte_len).unwrap_or(u32::MAX);
    byte_len.div_ceil(4)
}
