//! Keeps provider-billed usage honest after history before it shrank.
use super::*;
use harness_compaction::tokens;

impl Worker {
    /// Lowers the usage the latest transcript anchor reports by the tokens `savings` removed
    /// from entries strictly before it. `savings` pairs item indexes with saved tokens.
    /// When a native compaction replaced provider history, entries at or before it are
    /// ignored: the replacement is not billed twice. The anchor entry is remembered in
    /// `rewrite` so a rollback restores its usage.
    pub(super) fn correct_anchor(
        &self,
        context: &mut Context,
        rewrite: &mut Rewrite,
        savings: &[(usize, u32)],
    ) {
        let body = context.body_start();
        let (anchor, replaced_through) = {
            let items = context.items_with(&self.counter);
            let summary = items.iter().rposition(|i| i.kind == ItemKind::Summary);
            let start = summary.map_or(0, |index| index + 1);
            let native = summary.is_some_and(|index| {
                context
                    .entries
                    .get(body + index)
                    .and_then(|entry| entry.compaction.as_ref())
                    .is_some_and(|state| state.native.is_some())
            });
            (
                tokens::usage_anchor(&items, start, false),
                summary.filter(|_| native),
            )
        };
        let Some(anchor) = anchor else {
            return;
        };
        let saved = tokens::anchored_savings(anchor, savings, replaced_through);
        if saved == 0 {
            return;
        }
        let index = body + anchor.index;
        rewrite.remember(context, index);
        if let Some(usage) = context
            .entries
            .get_mut(index)
            .and_then(|entry| entry.usage.as_mut())
        {
            tokens::correct_usage(usage, saved);
        }
    }
}
