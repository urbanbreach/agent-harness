//! Token estimation and context usage calculation ported from Senpi's compaction.
//!
//! These are pure heuristic functions — no provider calls, no I/O.

use crate::conversation::ConversationMessage;
/// Senpi's UTF-16 chars/4 estimate, with long base64-like runs weighted 4x.
pub fn estimate_text_tokens(text: &str) -> u32 {
    let mut weighted = text.encode_utf16().count();
    let mut run = 0_usize;
    for byte in text.bytes().chain(std::iter::once(b' ')) {
        if byte.is_ascii_alphanumeric() || b"+/=_-".contains(&byte) {
            run += 1;
        } else {
            if run >= 512 {
                weighted = weighted.saturating_add(run.saturating_mul(3));
            }
            run = 0;
        }
    }
    u32::try_from(weighted.div_ceil(4)).unwrap_or(u32::MAX)
}

/// Project oversized tool output on the request copy; durable output is untouched.
pub fn admit_tool_result(text: &str, window: u32) -> String {
    let budget = (window / 20).clamp(8192, 50_000);
    let total = estimate_text_tokens(text);
    if total <= budget {
        return text.to_string();
    }
    let mut chars = usize::try_from(
        u64::from(budget) * u64::try_from(text.len()).unwrap_or(u64::MAX) / u64::from(total),
    )
    .unwrap_or(usize::MAX);
    loop {
        let head = &text[..text.floor_char_boundary(chars.saturating_mul(3) / 5)];
        let tail = &text[text.ceil_char_boundary(text.len().saturating_sub(chars / 5))..];
        let kept = estimate_text_tokens(head).saturating_add(estimate_text_tokens(tail));
        let excerpt =
            format!("{head}\n[tool result projected: kept {kept} of ~{total} tokens]\n{tail}");
        if estimate_text_tokens(&excerpt) <= budget {
            return excerpt;
        }
        if chars == 0 {
            return String::new();
        }
        chars = chars.saturating_mul(4) / 5;
    }
}

/// Calculate total context tokens from provider usage components.
///
/// Ports Senpi's `calculateContextTokens`: `input + output + cacheRead + cacheWrite`.
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
/// Ports Senpi's `estimateTokens`:
/// - User: weighted text and image estimate
/// - Assistant: weighted text and tool calls
/// - ToolResult: weighted output
/// - Checkpoint: weighted summary
pub fn estimate_message_tokens(message: &ConversationMessage) -> u32 {
    match message {
        ConversationMessage::User(user) => estimate_text_tokens(&user.text),
        ConversationMessage::Assistant(assistant) => {
            let mut text = assistant.text.clone();
            for call in &assistant.tool_calls {
                text.push_str(&call.tool_id);
                text.push_str(&call.args_summary);
            }
            estimate_text_tokens(&text)
        }
        ConversationMessage::ToolResult(result) => result.output_summary.as_deref().map_or_else(
            || {
                result
                    .output_json
                    .as_ref()
                    .map_or(0, |value| estimate_text_tokens(&value.to_string()))
            },
            estimate_text_tokens,
        ),
        ConversationMessage::Checkpoint(checkpoint) => estimate_text_tokens(&checkpoint.summary),
    }
}

/// Cost the retained request projection, including tool-result admission.
pub fn estimate_admitted_message_tokens(message: &ConversationMessage, window: u32) -> u32 {
    if let ConversationMessage::ToolResult(result) = message {
        let text = result
            .output_summary
            .clone()
            .or_else(|| result.output_json.as_ref().map(ToString::to_string))
            .unwrap_or_default();
        estimate_text_tokens(&admit_tool_result(&text, window))
    } else {
        estimate_message_tokens(message)
    }
}

/// Estimate total tokens for a slice of messages.
pub fn estimate_messages_tokens(messages: &[ConversationMessage]) -> u32 {
    messages
        .iter()
        .map(estimate_message_tokens)
        .fold(0, u32::saturating_add)
}

/// Estimated context token usage derived from conversation messages.
///
/// Ports Senpi's `ContextUsageEstimate`. When provider usage data is available
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
/// Ports Senpi's `estimateContextTokens`. Since `ConversationMessage` does not
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
