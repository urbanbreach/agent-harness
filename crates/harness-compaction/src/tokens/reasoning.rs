//! Opaque reasoning pricing: signatures, redacted data, server-tool blocks and output-size
//! uplift for signed reasoning.
use super::price::ItemTokens;
use super::{output_tokens, responses};
use crate::{Item, ItemPart, Stop};
use harness_providers::Protocol;
use harness_tokenizer::{Counter, ReaderApi, ReasoningPricing};
use serde_json::Value;

/// Anthropic content blocks of the entry's continuation, when it carries any.
fn anthropic_blocks<'a>(item: &'a Item<'_>) -> &'a [Value] {
    item.message
        .continuation
        .as_deref()
        .filter(|continuation| continuation.protocol() == Protocol::Anthropic)
        .and_then(|continuation| continuation.data().as_array())
        .map_or(&[], Vec::as_slice)
}

fn is_server_tool_block(block: &Value) -> bool {
    block["type"]
        .as_str()
        .is_some_and(|kind| kind == "server_tool_use" || kind.ends_with("_tool_result"))
}

/// Compact JSON of native server-tool blocks; counted in the normal estimate only.
fn server_tool_fragments(item: &Item<'_>) -> Vec<String> {
    anthropic_blocks(item)
        .iter()
        .filter(|block| is_server_tool_block(block))
        .map(|block| serde_json::to_string(block).unwrap_or_else(|_| "null".into()))
        .collect()
}

/// Opaque strings the provider would replay for this entry: signatures, redacted data and
/// Chat thought signatures.
fn signature_fragments(item: &Item<'_>) -> Vec<String> {
    let Some(continuation) = item.message.continuation.as_deref() else {
        return Vec::new();
    };
    let mut fragments = Vec::new();
    match continuation.protocol() {
        Protocol::Anthropic => {
            for block in anthropic_blocks(item) {
                let text = match block["type"].as_str() {
                    Some("thinking") => block["signature"].as_str(),
                    Some("redacted_thinking") => block["data"].as_str(),
                    _ => None,
                };
                fragments.extend(text.filter(|text| !text.is_empty()).map(str::to_owned));
            }
        }
        Protocol::Chat => collect_signatures(continuation.data(), &mut fragments),
        Protocol::Responses => fragments.extend(
            responses::encrypted_fragments(continuation.data())
                .into_iter()
                .map(str::to_owned),
        ),
    }
    fragments
}

fn collect_signatures(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                match child.as_str() {
                    Some(text) if key == "thought_signature" && !text.is_empty() => {
                        out.push(text.to_owned());
                    }
                    _ => collect_signatures(child, out),
                }
            }
        }
        Value::Array(items) => items
            .iter()
            .for_each(|child| collect_signatures(child, out)),
        _ => {}
    }
}

/// Whether the entry carries signed reasoning the provider will accept back. A thinking
/// signature is void when its block ends an aborted or failed turn, or when the turn made
/// tool calls but stopped for any reason other than a tool use. Redacted thinking always
/// counts.
fn signed_reasoning(item: &Item<'_>) -> bool {
    if item
        .parts
        .iter()
        .any(|part| matches!(part, ItemPart::RedactedReasoning))
    {
        return true;
    }
    let blocks = anthropic_blocks(item);
    let has_calls = item
        .message
        .assistant_tool_calls
        .as_ref()
        .is_some_and(|calls| !calls.is_empty());
    let abandoned = has_calls && item.stop == Stop::Truncated;
    let failed = matches!(item.stop, Stop::Aborted | Stop::Error);
    blocks
        .iter()
        .enumerate()
        .any(|(index, block)| match block["type"].as_str() {
            Some("redacted_thinking") => true,
            Some("thinking") => {
                block["signature"]
                    .as_str()
                    .is_some_and(|signature| !signature.is_empty())
                    && !abandoned
                    && !(failed && index + 1 == blocks.len())
            }
            _ => false,
        })
}

/// The assistant's provider scope matches the reader, so its signed reasoning replays.
fn matches_reader(counter: &Counter, item: &Item<'_>) -> bool {
    let (Some(reader), Some(continuation)) = (
        counter.reasoning_reader(),
        item.message.continuation.as_deref(),
    ) else {
        return false;
    };
    continuation.protocol() == Protocol::Anthropic
        && reader.api == ReaderApi::Anthropic
        && continuation.provider_id() == Some(reader.provider.as_str())
        && (continuation.model_id() == reader.model_id || counter.deployment_signatures())
}

/// Prices an assistant entry given its visible text, thinking, calls and images.
pub(super) fn price(counter: &Counter, item: &Item<'_>, visible: u32) -> ItemTokens {
    if let Some(native) = responses::native_price(counter, item) {
        let total = visible.saturating_add(native);
        return ItemTokens {
            normal: total,
            floor: total,
        };
    }
    let server = server_tool_fragments(item);
    let server_tokens = counter.count_fragments(server.iter().map(String::as_str));
    let local = visible.saturating_add(server_tokens);
    let normal = match counter.reasoning_pricing() {
        ReasoningPricing::Signature => {
            let signatures = signature_fragments(item);
            local.saturating_add(counter.count_fragments(signatures.iter().map(String::as_str)))
        }
        ReasoningPricing::OutputSize => match item.usage {
            Some(usage) if signed_reasoning(item) && matches_reader(counter, item) => {
                local.max(output_tokens(usage).saturating_sub(item.elided_output_tokens))
            }
            _ => local,
        },
    };
    ItemTokens {
        normal,
        floor: visible,
    }
}

/// Sum of numeric native reasoning prices across `items`. A replayable payload with no
/// reported number contributes its opaque overhead (normal minus floor).
pub fn replayed_reasoning_tokens(counter: &Counter, items: &[Item<'_>]) -> u32 {
    items.iter().fold(0u32, |total, item| {
        let price = match responses::native_price(counter, item) {
            Some(price) => price,
            None if is_replayable_payload(counter, item) => {
                item.tokens.saturating_sub(item.tokens_floor)
            }
            None => 0,
        };
        total.saturating_add(price)
    })
}

/// A matching Responses payload whose reasoning survives but has no reported number.
fn is_replayable_payload(counter: &Counter, item: &Item<'_>) -> bool {
    item.message
        .continuation
        .as_deref()
        .is_some_and(|continuation| {
            continuation.protocol() == Protocol::Responses
                && counter
                    .reasoning_reader()
                    .is_some_and(|reader| reader.api == ReaderApi::OpenAiResponses)
        })
        && item
            .usage
            .is_some_and(|usage| usage.reasoning_tokens.is_none())
}
