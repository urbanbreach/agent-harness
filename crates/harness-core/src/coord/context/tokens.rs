//! Entry pricing: one counter prices every entry, with a per-entry cache keyed by the counter
//! domain and the entry's mutation version.
use super::{Context, Entry};
use harness_compaction::{
    tokenizer::{Counter, ImageDetail, ImageSize},
    tokens::{estimate_item, summary_price, ItemTokens},
    Item, ItemKind, Stop,
};
use harness_providers::MessageRole;
use std::sync::Mutex;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
    domain: u64,
    version: u32,
    content_len: usize,
}

/// The last price computed for an entry. Clones start empty so no two entries share a memo.
#[derive(Default)]
pub(in crate::coord) struct PriceCache(Mutex<Option<(Key, ItemTokens)>>);

impl Clone for PriceCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PriceCache {
    fn get(&self, key: Key) -> Option<ItemTokens> {
        let slot = self.0.lock().ok()?;
        slot.filter(|(cached, _)| *cached == key)
            .map(|(_, tokens)| tokens)
    }

    fn put(&self, key: Key, tokens: ItemTokens) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some((key, tokens));
        }
    }
}

/// Non-image attachments have no price; every image prices by its dimensions and detail.
fn attachment_image_tokens(counter: &Counter, entry: &Entry) -> (u32, u32) {
    entry
        .attachments
        .iter()
        .filter(|attachment| attachment.mime.starts_with("image/"))
        .fold((0u32, 0u32), |(count, tokens), attachment| {
            let size = attachment.dimensions.map(|d| ImageSize {
                width: d.width,
                height: d.height,
            });
            let detail = attachment
                .detail
                .as_deref()
                .and_then(ImageDetail::from_name);
            (
                count.saturating_add(1),
                tokens.saturating_add(counter.image_tokens(size, detail)),
            )
        })
}

/// Entries that are cacheable: everything but an assistant whose response did not settle with
/// reported usage.
fn cacheable(entry: &Entry) -> bool {
    entry.message.role != MessageRole::Assistant
        || (!matches!(entry.stop, Stop::Aborted | Stop::Error)
            && entry
                .usage
                .as_ref()
                .is_some_and(|usage| usage.total_tokens > 0))
}

fn price_uncached(counter: &Counter, entry: &Entry) -> ItemTokens {
    // Items never carry the system prompt; whole-context totals still count it as text.
    if entry.message.role == MessageRole::System {
        let total = counter.count_text(&entry.message.content);
        return ItemTokens {
            normal: total,
            floor: total,
        };
    }
    if let Some(state) = &entry.compaction {
        let record = state.archive.as_ref();
        let blocks = record
            .into_iter()
            .flat_map(|record| [record.text_head.as_str(), record.text_tail.as_str()]);
        let frames = record
            .into_iter()
            .flat_map(|record| record.frames.iter().map(|f| (f.width, f.height)));
        let total = summary_price(counter, &state.summary, blocks, frames);
        return ItemTokens {
            normal: total,
            floor: total,
        };
    }
    if let Some(branch) = &entry.branch_summary {
        let total = counter.count_text(&branch.summary);
        return ItemTokens {
            normal: total,
            floor: total,
        };
    }
    let (_, derived) = attachment_image_tokens(counter, entry);
    let mut item = Item::new(&entry.message);
    item.reasoning = &entry.settled_reasoning;
    item.parts = &entry.parts;
    item.images = &entry.images;
    item.image_tokens = derived;
    item.usage = entry.usage.as_ref();
    item.stop = entry.stop;
    item.elided_output_tokens = entry.elided_output_tokens;
    estimate_item(counter, &item)
}

/// Price of one entry in both estimate domains.
pub(in crate::coord) fn entry_price(counter: &Counter, entry: &Entry) -> ItemTokens {
    if !cacheable(entry) {
        return price_uncached(counter, entry);
    }
    let key = Key {
        domain: counter.domain_id(),
        version: entry.version,
        content_len: entry.message.content.len(),
    };
    if let Some(tokens) = entry.price.get(key) {
        return tokens;
    }
    let tokens = price_uncached(counter, entry);
    entry.price.put(key, tokens);
    tokens
}

/// Estimated tokens of every entry, system messages included.
pub(in crate::coord) fn tokens_with(entries: &[Entry], counter: &Counter) -> u32 {
    entries.iter().fold(0u32, |total, entry| {
        total.saturating_add(entry_price(counter, entry).normal)
    })
}

/// The native child counter excludes request framing and tool schemas.
/// Unrepresentable attachment projections remain unknown.
pub(in crate::coord) fn native_tokens_with(entries: &[Entry], counter: &Counter) -> Option<u64> {
    entries.iter().try_fold(0u64, |total, entry| {
        if entry
            .attachments
            .iter()
            .any(|a| !a.mime.starts_with("image/"))
        {
            return None;
        }
        Some(total.saturating_add(u64::from(entry_price(counter, entry).normal)))
    })
}

impl Context {
    /// The compaction view of the body entries, priced by `counter`.
    pub fn items_with(&self, counter: &Counter) -> Vec<Item<'_>> {
        let summary = self
            .entries
            .iter()
            .enumerate()
            .rfind(|(_, e)| e.compaction.is_some())
            .and_then(|(index, e)| Some((index, e.compaction.as_ref()?)));
        // Inline imaging rewrites tool results without touching the entry, so its stamps fold
        // into the prune time that staleness checks read. One lock covers the whole view.
        let stamps = self.render_caches.inline.lock().ok();
        self.entries
            .iter()
            .enumerate()
            .skip_while(|(_, e)| super::is_prefix(e))
            .map(|(index, entry)| {
                let (image_count, image_tokens) = if entry.images.is_empty() {
                    attachment_image_tokens(counter, entry)
                } else {
                    (u32::try_from(entry.images.len()).unwrap_or(u32::MAX), 0)
                };
                let kind = if entry.compaction.is_some() {
                    ItemKind::Summary
                } else if entry.branch_summary.is_some() {
                    ItemKind::BranchSummary
                } else if entry.note_kind.is_some() {
                    ItemKind::Note
                } else {
                    ItemKind::Message
                };
                let price = entry_price(counter, entry);
                Item {
                    message: &entry.message,
                    kind,
                    seq: entry.seq,
                    turn: entry.turn.as_deref(),
                    reasoning: &entry.settled_reasoning,
                    tool: entry.tool,
                    image_count,
                    image_tokens,
                    usage: entry.usage.as_ref(),
                    stop: entry.stop,
                    at_ms: entry.at_ms,
                    tokens: price.normal,
                    tokens_floor: price.floor,
                    origin: entry.origin,
                    note_kind: entry.note_kind,
                    parts: &entry.parts,
                    images: &entry.images,
                    pruned_at_ms: entry.pruned_at_ms.max(
                        stamps
                            .as_ref()
                            .filter(|_| entry.message.role == MessageRole::Tool)
                            .and_then(|cache| {
                                cache.stamp_for(entry.seq, entry.message.tool_call_id.as_deref())
                            }),
                    ),
                    elided_output_tokens: entry.elided_output_tokens,
                    version: entry.version,
                    from_hook: entry.compaction.as_ref().is_some_and(|c| c.from_hook),
                    retained: summary.is_some_and(|(at, state)| {
                        index > at && state.commit_seq > 0 && entry.seq <= state.commit_seq
                    }) && kind == ItemKind::Message,
                }
            })
            .collect()
    }

    pub fn tokens_with(&self, counter: &Counter) -> u32 {
        tokens_with(&self.entries, counter)
    }

    /// Native child context estimate under `counter`, extended from the last provider report.
    pub fn native_estimated_tokens_with(&self, counter: &Counter) -> Option<u64> {
        let usage = self.native_context_usage?;
        Some(
            usage.total_tokens.saturating_add(
                native_tokens_with(&self.entries, counter)?
                    .saturating_sub(usage.estimate_after_last_response?),
            ),
        )
    }
}

#[cfg(test)]
mod tests;
