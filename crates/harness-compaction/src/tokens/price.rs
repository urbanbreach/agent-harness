//! Per-entry price: every original block counted as its own fragment.
use super::reasoning;
use crate::{ImageOrigin, Item, ItemKind, ItemPart};
use harness_providers::{AssistantToolCall, MessageRole};
use harness_tokenizer::{Counter, ImageSize};

/// What one entry costs in the two estimate domains.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ItemTokens {
    /// Everything the provider would receive, opaque reasoning included.
    pub normal: u32,
    /// Trustworthy content only: no guessed opaque bytes and no output-size uplift.
    pub floor: u32,
}

/// Compact JSON of a tool call's arguments. Empty or whitespace-only arguments serialize as
/// `null`; text that does not parse is priced as written.
fn compact_arguments(call: &AssistantToolCall) -> String {
    let text = call.arguments_json.trim();
    if text.is_empty() {
        return "null".into();
    }
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| serde_json::to_string(&value).ok())
        .unwrap_or_else(|| text.to_owned())
}

fn call_tokens(counter: &Counter, call: &AssistantToolCall) -> u32 {
    let arguments = compact_arguments(call);
    counter.count_fragments([call.function_name.as_str(), arguments.as_str()])
}

fn image_tokens(counter: &Counter, item: &Item<'_>) -> u32 {
    if item.images.is_empty() {
        return item.image_tokens;
    }
    item.images.iter().fold(0u32, |total, image| {
        let size = match (image.width, image.height) {
            (Some(width), Some(height)) if width > 0 && height > 0 => {
                Some(ImageSize { width, height })
            }
            _ => None,
        };
        let price = match image.origin {
            ImageOrigin::ArchiveFrame | ImageOrigin::InlineFrame => {
                let (width, height) = size.map_or((0, 0), |size| (size.width, size.height));
                counter.frame_tokens(width, height)
            }
            ImageOrigin::User | ImageOrigin::Tool => counter.image_tokens(size, image.detail),
        };
        total.saturating_add(price)
    })
}

/// Visible assistant content: text blocks, visible thinking and tool calls.
fn assistant_tokens(counter: &Counter, item: &Item<'_>) -> u32 {
    let calls = item.message.assistant_tool_calls.as_deref().unwrap_or(&[]);
    if item.parts.is_empty() {
        let text = counter.count_fragments(
            std::iter::once(item.text())
                .filter(|text| !text.is_empty())
                .chain(item.reasoning.iter().map(String::as_str)),
        );
        return calls.iter().fold(text, |total, call| {
            total.saturating_add(call_tokens(counter, call))
        });
    }
    let mut total = 0u32;
    let mut priced = vec![false; calls.len()];
    for part in item.parts {
        let tokens = match part {
            ItemPart::Text(text) | ItemPart::Reasoning(text) if text.is_empty() => 0,
            ItemPart::Text(text) | ItemPart::Reasoning(text) => counter.count_text(text),
            ItemPart::RedactedReasoning => 0,
            ItemPart::Call(index) => match calls.get(*index) {
                Some(call) if !priced[*index] => {
                    priced[*index] = true;
                    call_tokens(counter, call)
                }
                _ => 0,
            },
        };
        total = total.saturating_add(tokens);
    }
    // Calls the ordered blocks do not mention still reach the provider.
    calls
        .iter()
        .zip(&priced)
        .filter(|(_, priced)| !**priced)
        .fold(total, |total, (call, _)| {
            total.saturating_add(call_tokens(counter, call))
        })
}

/// Prices one entry. Summary and branch-summary entries priced here count their provider text;
/// the coordinator prices raw summaries with [`summary_price`] instead.
pub fn estimate_item(counter: &Counter, item: &Item<'_>) -> ItemTokens {
    let images = image_tokens(counter, item);
    match item.role() {
        MessageRole::Assistant if item.kind == ItemKind::Message => {
            let visible = assistant_tokens(counter, item).saturating_add(images);
            reasoning::price(counter, item, visible)
        }
        // The system prompt is priced with the fixed request prefix, and the reference prices
        // an unrecognized role at zero.
        MessageRole::System => ItemTokens::default(),
        MessageRole::User | MessageRole::Tool | MessageRole::Assistant => {
            let text = counter.count_text(item.text());
            let total = text.saturating_add(images);
            ItemTokens {
                normal: total,
                floor: total,
            }
        }
    }
}

/// Price of a compaction summary: the raw summary, each ordered block and every archive frame
/// as its own fragment, with frames priced from their dimensions.
pub fn summary_price<'a>(
    counter: &Counter,
    summary: &'a str,
    blocks: impl IntoIterator<Item = &'a str>,
    frames: impl IntoIterator<Item = (u32, u32)>,
) -> u32 {
    let text = counter.count_fragments(std::iter::once(summary).chain(blocks));
    frames.into_iter().fold(text, |total, (width, height)| {
        total.saturating_add(counter.frame_tokens(width, height))
    })
}
