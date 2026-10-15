//! Superseded and useless candidate collection shared by both pruning passes.
use super::{
    eligible, protects, Calls, PruneKind, SupersedeComplete, SupersedeKey, ToolMatcher,
    USELESS_NOTICE,
};
use crate::item::Item;
use std::collections::BTreeMap;

/// Tokens a replacement saves: the whole message less the notice (`ceil(chars / 4)`).
pub(super) fn savings(item: &Item<'_>, replacement: &str) -> u32 {
    item.tokens.saturating_sub(notice_tokens(replacement))
}

/// `ceil(characters / 4)`: reducer notices are priced by length, not by tokenizer.
fn notice_tokens(notice: &str) -> u32 {
    u32::try_from(notice.encode_utf16().count().div_ceil(4)).unwrap_or(u32::MAX)
}

/// Results a newer same-key (or complete parent-key) result supersedes.
///
/// Scans newest to oldest. Unpaired, protected and already-pruned results and results
/// without a key are skipped entirely: they can neither be superseded nor supersede.
pub(super) fn superseded(
    items: &[Item<'_>],
    boundary: usize,
    calls: &Calls<'_>,
    protected: &[ToolMatcher],
    key: &SupersedeKey<'_>,
    complete: Option<&SupersedeComplete<'_>>,
) -> Vec<usize> {
    // Per key: whether the newest successful result exists, and whether it is complete.
    let mut newest = BTreeMap::<String, (bool, bool)>::new();
    let mut found = Vec::new();
    for index in (boundary..items.len()).rev() {
        let item = &items[index];
        let Some(call) = calls.paired(item) else {
            continue;
        };
        if !eligible(item, Some(call), protected) {
            continue;
        }
        let Some(name) = key(item, call) else {
            continue;
        };
        let same = newest.get(&name);
        let parent = name
            .split_once('\0')
            .and_then(|(parent, _)| newest.get(parent));
        let superseded = if item.tool.is_error {
            same.is_some() || parent.is_some_and(|(success, _)| *success)
        } else {
            same.is_some_and(|(success, _)| *success)
                || parent.is_some_and(|(success, complete)| *success && *complete)
        };
        if superseded {
            found.push(index);
        }
        let record = newest.entry(name).or_insert((false, false));
        if !item.tool.is_error && !record.0 {
            *record = (true, complete.is_none_or(|complete| complete(item)));
        }
    }
    found.reverse();
    found
}

/// Whether the result is a non-error result the tool flagged as carrying no information and
/// replacing it saves tokens.
pub(super) fn useless_candidate(
    item: &Item<'_>,
    call: Option<&harness_providers::AssistantToolCall>,
    protected: &[ToolMatcher],
) -> bool {
    item.is_tool_result()
        && item.tool.useless
        && !item.tool.is_error
        && !item.tool.pruned
        && !protects(protected, item, call)
        && savings(item, USELESS_NOTICE) > 0
}

/// Superseded then useless candidates, the supersede notice winning duplicates.
pub(super) fn candidates(
    items: &[Item<'_>],
    boundary: usize,
    calls: &Calls<'_>,
    protected: &[ToolMatcher],
    key: Option<&SupersedeKey<'_>>,
    complete: Option<&SupersedeComplete<'_>>,
    prune_useless: bool,
) -> BTreeMap<usize, PruneKind> {
    let mut found = BTreeMap::new();
    if let Some(key) = key {
        for index in superseded(items, boundary, calls, protected, key, complete) {
            found.insert(index, PruneKind::Superseded);
        }
    }
    if prune_useless {
        for (index, item) in items.iter().enumerate().skip(boundary) {
            if useless_candidate(item, calls.paired(item), protected) {
                found.entry(index).or_insert(PruneKind::Useless);
            }
        }
    }
    found
}
