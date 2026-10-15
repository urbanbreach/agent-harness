//! Provider-usage anchors over a transcript.
use super::anchors;
use crate::{Item, ItemKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    pub index: usize,
    pub tokens: u32,
}

/// A user-side rewrite: the entry at `index` rewrote history at `at_ms`, so usage reported
/// before that time no longer describes the context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RewriteMarker {
    pub index: usize,
    pub at_ms: u64,
}

/// The newest assistant at or after `from_index` whose usage is trustworthy. With
/// `skip_pruned`, anchors stamped no later than the newest tool-result prune anywhere in
/// `items` are stale and skipped.
pub fn usage_anchor(items: &[Item<'_>], from_index: usize, skip_pruned: bool) -> Option<Anchor> {
    let newest_prune = skip_pruned
        .then(|| items.iter().filter_map(|item| item.pruned_at_ms).max())
        .flatten();
    items
        .iter()
        .enumerate()
        .skip(from_index)
        .rev()
        .find_map(|(index, item)| {
            let tokens = anchors(item)?;
            let stale = newest_prune.is_some_and(|pruned| item.at_ms.is_none_or(|at| at <= pruned));
            (!stale).then_some(Anchor { index, tokens })
        })
}

/// Provider usage extended by the local count of what follows it.
///
/// Without an anchor the local count starts at `count_from`; `anchor_from` only gates the anchor search. With
/// `exclude_encrypted` the local count leaves out opaque reasoning bytes and output-size uplift.
pub fn transcript_estimate(
    items: &[Item<'_>],
    anchor_from: usize,
    count_from: usize,
    exclude_encrypted: bool,
    skip_pruned: bool,
) -> u32 {
    let (tokens, start) = usage_anchor(items, anchor_from, skip_pruned)
        .map_or((0, count_from), |anchor| (anchor.tokens, anchor.index + 1));
    items.iter().skip(start).fold(tokens, |total, item| {
        total.saturating_add(if exclude_encrypted {
            item.tokens_floor
        } else {
            item.tokens
        })
    })
}

/// The latest trustworthy anchor in a forward scan.
///
/// The rewrite time grows with summary
/// commit times, rewrite markers and tool-result prune times; an assistant anchors only when
/// stamped strictly after it. A later rewrite does not clear an anchor already saved.
pub fn request_anchor(items: &[Item<'_>], markers: &[RewriteMarker]) -> Option<Anchor> {
    let mut rewritten = 0u64;
    let mut saved = None;
    for (index, item) in items.iter().enumerate() {
        if item.kind == ItemKind::Summary
            && let Some(at) = item.at_ms
        {
            rewritten = rewritten.max(at);
        }
        for marker in markers.iter().filter(|marker| marker.index == index) {
            rewritten = rewritten.max(marker.at_ms);
        }
        if let Some(pruned) = item.pruned_at_ms {
            rewritten = rewritten.max(pruned);
        }
        if let (Some(tokens), Some(at)) = (anchors(item), item.at_ms)
            && at > rewritten
        {
            saved = Some(Anchor { index, tokens });
        }
    }
    saved
}

/// Tokens a reducer saved that the provider's usage for `anchor` still counts.
///
/// Only entries strictly before the anchor, and, when provider history was replaced remotely through
/// `replaced_through`, only entries after that boundary (the replacement is not billed twice).
pub fn anchored_savings(
    anchor: Anchor,
    savings: &[(usize, u32)],
    replaced_through: Option<usize>,
) -> u32 {
    savings
        .iter()
        .filter(|(index, _)| {
            *index < anchor.index && replaced_through.is_none_or(|boundary| *index > boundary)
        })
        .fold(0u32, |total, (_, saved)| total.saturating_add(*saved))
}

/// Lowers a usage record by `saved` tokens of rewritten history. The reduction comes off the
/// context figures and the uncached prompt parts so every derived number agrees.
pub fn correct_usage(usage: &mut harness_providers::CompletionUsage, saved: u32) {
    usage.total_tokens = usage.total_tokens.saturating_sub(saved);
    usage.prompt_tokens = usage.prompt_tokens.saturating_sub(saved);
    usage.context_tokens = usage
        .context_tokens
        .map(|tokens| tokens.saturating_sub(saved));
    let mut remaining = saved;
    for part in [
        &mut usage.input_tokens,
        &mut usage.cache_read_tokens,
        &mut usage.cache_write_tokens,
    ] {
        let take = remaining.min(*part);
        *part -= take;
        remaining -= take;
    }
}
