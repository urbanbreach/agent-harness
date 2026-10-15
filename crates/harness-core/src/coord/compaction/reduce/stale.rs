//! Stale-result pruning: superseded reads, useless results and aged tool output.
use super::*;
use harness_compaction::prune::{
    self, AgePruneConfig, IncrementalPruneConfig, PruneKind, PrunePlan, SupersedeComplete,
    SupersedeKey,
};

impl Worker {
    /// The provider's prompt-cache lookback in content blocks, when it has a bounded one.
    fn cache_lookback(&self) -> Option<u32> {
        self.provider
            .cache_lookback_blocks(&self.request(&Context::default()))
    }

    /// Newest tokens pruning leaves alone. A model whose thinking is bound to the exact
    /// prefix cannot lose any of it, so nothing newer than the very last message is touched.
    fn warm_suffix_tokens(&self) -> u32 {
        if self
            .counter
            .reader()
            .is_some_and(|reader| reader.prefix_binding)
        {
            0
        } else {
            prune::WARM_SUFFIX_TOKENS
        }
    }

    /// Runs the independent pruning passes, each journaled before it is applied, and
    /// returns the tokens saved so threshold decisions can credit them.
    ///
    /// The stale pass (superseded reads, plus useless results when enabled) runs first and
    /// the age pass plans over the history it changed. Each pass has its own gate: reads
    /// need `supersede_reads`, useless results need `drop_useless`, and age pruning needs
    /// automatic compaction to be enabled.
    pub(in crate::coord) async fn prune_stale(
        &self,
        context: &mut Context,
    ) -> Result<u32, CoordinatorError> {
        let settings = &self.compaction;
        let stale = settings.supersede_reads || settings.drop_useless;
        if !stale && !settings.enabled {
            return Ok(0);
        }
        let now_ms = self.now_ms().await?;
        let (lookback, warm) = (self.cache_lookback(), self.warm_suffix_tokens());
        let protected = self.skill_matchers();
        let mut saved = 0_u32;
        if stale {
            let plan = {
                let items = context.items_with(&self.counter);
                let key: &SupersedeKey<'_> = &prune::read_key;
                let complete: &SupersedeComplete<'_> = &prune::read_is_complete;
                prune::plan_incremental_prune(
                    &items,
                    prune_boundary(&items),
                    &IncrementalPruneConfig {
                        prune_useless: settings.drop_useless,
                        suffix_token_limit: warm,
                        idle_flush_ms: prune::SESSION_IDLE_FLUSH_MS,
                        now_ms,
                        cache_lookback: lookback,
                        supersede_key: settings.supersede_reads.then_some(key),
                        supersede_complete: Some(complete),
                    },
                    &protected,
                )
            };
            saved = saved.saturating_add(self.apply_prune(context, plan, now_ms).await?);
        }
        if settings.enabled {
            let plan = {
                let items = context.items_with(&self.counter);
                prune::plan_age_prune(
                    &items,
                    prune_boundary(&items),
                    &AgePruneConfig {
                        prune_useless: settings.drop_useless,
                        cache_warm_suffix: Some(warm),
                        cache_lookback: lookback,
                        ..AgePruneConfig::default()
                    },
                    &protected,
                )
            };
            saved = saved.saturating_add(self.apply_prune(context, plan, now_ms).await?);
        }
        Ok(saved)
    }

    /// Journals then applies one pruning plan, one event per kind of replacement.
    async fn apply_prune(
        &self,
        context: &mut Context,
        plan: PrunePlan,
        now_ms: u64,
    ) -> Result<u32, CoordinatorError> {
        let body = context.body_start();
        let mut saved = 0_u32;
        for (kind, record) in [
            (PruneKind::Superseded, PruneKindRecord::Superseded),
            (PruneKind::Useless, PruneKindRecord::Useless),
            (PruneKind::Aged, PruneKindRecord::Aged),
        ] {
            let mut rewrite = Rewrite::default();
            for edit in plan.edits.iter().filter(|edit| edit.kind == kind) {
                let Some(entry) = context.entries.get(body + edit.index) else {
                    continue;
                };
                if entry.tool.pruned {
                    continue;
                }
                let Some(record) = tool_edit(entry, &edit.replacement, now_ms, true) else {
                    continue;
                };
                rewrite.tokens_saved = rewrite.tokens_saved.saturating_add(edit.savings);
                rewrite.tool_results += 1;
                rewrite.edits.push(record);
            }
            if rewrite.is_empty() {
                continue;
            }
            self.journal(record, &rewrite, None).await?;
            apply(context, &rewrite.edits);
            rebase_native_usage(context, &self.counter);
            self.after_rewrite(RewriteReason::Prune);
            saved = saved.saturating_add(rewrite.tokens_saved);
        }
        Ok(saved)
    }
}
