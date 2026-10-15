//! Native Responses reasoning price.
//!
//! A replayable Responses payload prices its reasoning by the provider-reported
//! `reasoning_tokens`, never by ciphertext bytes. Which reasoning survives replay follows the
//! same sanitation the request encoder applies.
use crate::{Item, Stop};
use harness_providers::Protocol;
use harness_tokenizer::{Counter, ReaderApi};
use serde_json::Value;
use std::collections::HashSet;

/// Reasoning items that survive replay sanitation.
struct Sanitized<'a> {
    reasoning: Vec<(&'a Value, usize)>,
    /// Segments (spans between client-input boundaries) that hold a computer call with an id.
    computer_segments: HashSet<usize>,
}

fn is_valid_name(name: &Value) -> bool {
    name.as_str().is_some_and(|name| {
        !name.is_empty()
            && name.encode_utf16().count() <= 128
            && !name.chars().any(|c| c.is_whitespace() || c.is_control())
    })
}

fn arguments_parse(item: &Value) -> bool {
    item["arguments"]
        .as_str()
        .is_some_and(|text| !text.trim().is_empty() && serde_json::from_str::<Value>(text).is_ok())
}

fn has_visible_output(item: &Value) -> bool {
    match &item["content"] {
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(parts) => parts.iter().any(|part| {
            let text = match part["type"].as_str() {
                Some("output_text") => part["text"].as_str(),
                Some("refusal") => part["refusal"].as_str().or_else(|| part["text"].as_str()),
                _ => None,
            };
            text.is_some_and(|text| !text.trim().is_empty())
        }),
        _ => false,
    }
}

fn is_boundary(item: &Value) -> bool {
    let kind = item["type"].as_str().unwrap_or("message");
    if kind == "message" {
        return item["role"].as_str() != Some("assistant");
    }
    kind.ends_with("_output")
        || kind.starts_with("compaction")
        || matches!(kind, "mcp_approval_response" | "item_reference")
        || kind.starts_with("input_")
}

fn sanitize(items: &[Value]) -> Sanitized<'_> {
    let mut retained = Vec::new();
    let mut pending: Vec<(&Value, usize)> = Vec::new();
    let mut computer_segments = HashSet::new();
    let mut segment = 0usize;
    let mut removed_empty = false;
    let mut has_output = false;
    for item in items {
        if is_boundary(item) {
            pending.clear();
            removed_empty = false;
            segment += 1;
            continue;
        }
        let kind = item["type"].as_str().unwrap_or("message");
        let output = match kind {
            "reasoning" => {
                if removed_empty {
                    pending.clear();
                    removed_empty = false;
                }
                pending.push((item, segment));
                continue;
            }
            "message" => {
                if has_visible_output(item) {
                    true
                } else {
                    removed_empty = true;
                    continue;
                }
            }
            "function_call" => is_valid_name(&item["name"]) && arguments_parse(item),
            "custom_tool_call" => is_valid_name(&item["name"]),
            "image_generation_call" => {
                item["id"].is_string()
                    && item["result"]
                        .as_str()
                        .is_some_and(|result| !result.is_empty())
            }
            "computer_call" => {
                if item["id"].is_string() {
                    computer_segments.insert(segment);
                }
                true
            }
            "item_reference" => false,
            _ => true,
        };
        if output {
            has_output = true;
            removed_empty = false;
            retained.append(&mut pending);
        }
    }
    if !has_output {
        retained.clear();
    }
    Sanitized {
        reasoning: retained,
        computer_segments,
    }
}

/// Whether every surviving reasoning item replays from plain text alone: no ids, no
/// ciphertext, and a `reasoning_text` content part each.
fn cold_replayable(sanitized: &Sanitized<'_>) -> bool {
    !sanitized.reasoning.is_empty()
        && sanitized.reasoning.iter().all(|(item, segment)| {
            let keeps_id = item["id"].is_string() && sanitized.computer_segments.contains(segment);
            let plain = !item["encrypted_content"].is_string();
            let text = item["content"]
                .as_array()
                .is_some_and(|parts| parts.iter().any(|part| part["type"] == "reasoning_text"));
            !keeps_id && plain && text
        })
}

/// Numeric price of the reasoning in a Responses payload on this entry, or `None` when no
/// valid matching payload exists (the signature price then applies). Zero means the reasoning
/// does not replay.
pub(super) fn native_price(counter: &Counter, item: &Item<'_>) -> Option<u32> {
    let continuation = item.message.continuation.as_deref()?;
    let reader = counter.reasoning_reader()?;
    if continuation.protocol() != Protocol::Responses
        || reader.api != ReaderApi::OpenAiResponses
        || continuation.model_id() != reader.model_id
        || continuation.provider_id().unwrap_or(&reader.provider) != reader.provider
    {
        return None;
    }
    let data = continuation.data();
    if data["type"] != "openaiResponsesHistory" {
        return None;
    }
    let items = data["items"].as_array()?;
    let thinking_only = item.message.content.is_empty()
        && item
            .message
            .assistant_tool_calls
            .as_ref()
            .is_none_or(Vec::is_empty);
    if thinking_only && item.stop != Stop::Complete {
        return Some(0);
    }
    let sanitized = sanitize(items);
    if sanitized.reasoning.is_empty() || (!counter.history_replay() && !cold_replayable(&sanitized))
    {
        return Some(0);
    }
    item.usage.and_then(|usage| usage.reasoning_tokens)
}

/// Encrypted reasoning strings of a Responses payload that falls back to signature pricing.
pub(super) fn encrypted_fragments(items: &Value) -> Vec<&str> {
    items["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["type"] == "reasoning")
        .filter_map(|item| item["encrypted_content"].as_str())
        .filter(|text| !text.is_empty())
        .collect()
}
