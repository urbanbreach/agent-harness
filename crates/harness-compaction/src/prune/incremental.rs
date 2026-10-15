//! Cache-aware incremental pruning of superseded and useless results.
use super::{
    collect, counts_toward_suffix, lookback_floor, Calls, IncrementalPruneConfig, PruneEdit,
    PruneKind, PrunePlan, ToolMatcher, SUPERSEDED_NOTICE, USELESS_NOTICE,
};
use crate::item::{Item, ItemKind};

/// Replaces superseded (and optionally useless) results.
///
/// A pass that finds the provider cache cold (the newest message is older than the idle gap)
/// flushes every candidate. Otherwise only candidates inside the warm suffix budget and the
/// lookback window change, so the cached prefix stays valid. There is no savings gate.
pub fn plan_incremental_prune(
    items: &[Item<'_>],
    boundary: usize,
    cfg: &IncrementalPruneConfig<'_>,
    protected: &[ToolMatcher],
) -> PrunePlan {
    let boundary = boundary.min(items.len());
    let calls = Calls::new(items, boundary);
    let candidates = collect::candidates(
        items,
        boundary,
        &calls,
        protected,
        cfg.supersede_key,
        cfg.supersede_complete,
        cfg.prune_useless,
    );
    if candidates.is_empty() {
        return PrunePlan::default();
    }
    let cold = items
        .iter()
        .rev()
        .find(|item| item.kind == ItemKind::Message)
        .and_then(|item| item.at_ms)
        .and_then(|at| cfg.now_ms.checked_sub(at))
        .is_some_and(|elapsed| elapsed >= cfg.idle_flush_ms);
    let floor = lookback_floor(items, boundary, cfg.cache_lookback);
    let mut suffix = 0_u32;
    let mut edits = Vec::new();
    let mut savings = 0_u32;
    for index in (boundary..items.len()).rev() {
        if !cold && (suffix > cfg.suffix_token_limit || index < floor) {
            break;
        }
        if let Some(kind) = candidates.get(&index) {
            let replacement = if *kind == PruneKind::Superseded {
                SUPERSEDED_NOTICE
            } else {
                USELESS_NOTICE
            };
            let saved = collect::savings(&items[index], replacement);
            savings = savings.saturating_add(saved);
            edits.push(PruneEdit {
                index,
                kind: *kind,
                replacement: replacement.to_owned(),
                savings: saved,
            });
        }
        if counts_toward_suffix(&items[index]) {
            suffix = suffix.saturating_add(items[index].tokens);
        }
    }
    edits.reverse();
    PrunePlan { edits, savings }
}
