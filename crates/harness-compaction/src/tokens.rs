//! Local token estimates and provider-usage accounting.
//!
//! Estimates are priced by a [`Counter`]: every fragment of a message is counted
//! independently and no fixed per-message overhead is added. Provider usage is an anchor that
//! local counts only extend, never replace.
use crate::{Item, Stop};
use harness_providers::CompletionUsage;

mod anchor;
mod price;
mod reasoning;
mod responses;

pub use anchor::{
    anchored_savings, correct_usage, request_anchor, transcript_estimate, usage_anchor, Anchor,
    RewriteMarker,
};
pub use price::{estimate_item, summary_price, ItemTokens};
pub use reasoning::replayed_reasoning_tokens;

/// Output tokens the provider reported for a response.
fn output_tokens(usage: &CompletionUsage) -> u32 {
    usage.output_tokens.max(usage.completion_tokens)
}

/// Prompt tokens the provider reported, cache reads and writes included.
fn split_prompt_tokens(usage: &CompletionUsage) -> u32 {
    usage
        .input_tokens
        .saturating_add(usage.cache_read_tokens)
        .saturating_add(usage.cache_write_tokens)
}

/// Context occupancy of the request that produced `usage`: the explicit value (zero kept),
/// else the reported total, else the sum of its parts.
pub fn context_tokens(usage: &CompletionUsage) -> u32 {
    if let Some(explicit) = usage.context_tokens {
        return explicit;
    }
    if usage.total_tokens > 0 {
        return usage.total_tokens;
    }
    let split = split_prompt_tokens(usage).saturating_add(usage.output_tokens);
    let aggregate = usage.prompt_tokens.saturating_add(usage.completion_tokens);
    split.max(aggregate)
}

/// Prompt tokens of the request that produced `usage`: the explicit context value, else the
/// reported prompt parts, else the context.
pub fn prompt_tokens(usage: &CompletionUsage) -> u32 {
    if let Some(explicit) = usage.context_tokens {
        return explicit;
    }
    let prompt = usage.prompt_tokens.max(split_prompt_tokens(usage));
    if prompt > 0 {
        prompt
    } else {
        context_tokens(usage)
    }
}

/// Whether `usage` can anchor a transcript: some prompt was reported, or the context exceeds
/// the output.
pub fn has_context_usage(usage: &CompletionUsage) -> bool {
    usage.context_tokens.is_some_and(|tokens| tokens > 0)
        || prompt_tokens_reported(usage) > 0
        || context_tokens(usage) > output_tokens(usage)
}

fn prompt_tokens_reported(usage: &CompletionUsage) -> u32 {
    usage.prompt_tokens.max(split_prompt_tokens(usage))
}

/// The compaction floor: the larger of the provider-anchored usage and the local count.
pub fn compaction_context_tokens(provider: u32, local: u32) -> u32 {
    provider.max(local)
}

/// An assistant entry whose provider usage may anchor the transcript.
fn anchors(item: &Item<'_>) -> Option<u32> {
    if !item.is_assistant() || matches!(item.stop, Stop::Error | Stop::Aborted) {
        return None;
    }
    let usage = item.usage?;
    has_context_usage(usage).then(|| context_tokens(usage))
}

#[cfg(test)]
mod tests;
