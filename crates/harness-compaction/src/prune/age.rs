//! Age-based tool-output pruning.
use super::{
    aged_notice, collect, counts_toward_suffix, eligible, lookback_floor, AgePruneConfig, Calls,
    PruneEdit, PruneKind, PrunePlan, ToolMatcher, MIN_PRUNE_TOKENS, SUPERSEDED_NOTICE,
    USELESS_NOTICE,
};
use crate::item::Item;

/// Replaces tool output older than the protected window, superseded results and useless
/// results with short notices. All or nothing: below the minimum savings nothing changes.
pub fn plan_age_prune(
    items: &[Item<'_>],
    boundary: usize,
    cfg: &AgePruneConfig<'_>,
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
    let floor = cfg.cache_warm_suffix.map_or(boundary, |_| {
        lookback_floor(items, boundary, cfg.cache_lookback)
    });
    let mut suffix = 0_u32;
    let mut newer_tools = 0_u32;
    let mut edits = Vec::new();
    let mut savings = 0_u32;
    for index in (boundary..items.len()).rev() {
        let item = &items[index];
        if cfg
            .cache_warm_suffix
            .is_some_and(|limit| suffix > limit || index < floor)
        {
            break;
        }
        if item.is_tool_result() {
            let candidate = candidates.get(&index).copied();
            let aged = candidate.is_none()
                && newer_tools >= cfg.protect_tokens
                && item.tokens >= MIN_PRUNE_TOKENS
                && eligible(item, calls.paired(item), protected);
            if candidate.is_some() || aged {
                let kind = candidate.unwrap_or(PruneKind::Aged);
                let replacement = match kind {
                    PruneKind::Superseded => SUPERSEDED_NOTICE.to_owned(),
                    PruneKind::Useless => USELESS_NOTICE.to_owned(),
                    PruneKind::Aged => aged_notice(item.tokens),
                };
                let saved = collect::savings(item, &replacement);
                savings = savings.saturating_add(saved);
                edits.push(PruneEdit {
                    index,
                    replacement,
                    kind,
                    savings: saved,
                });
            }
            newer_tools = newer_tools.saturating_add(item.tokens);
        }
        if counts_toward_suffix(item) {
            suffix = suffix.saturating_add(item.tokens);
        }
    }
    if edits.is_empty() || savings < cfg.minimum_savings {
        return PrunePlan::default();
    }
    edits.reverse();
    PrunePlan { edits, savings }
}
