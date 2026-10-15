//! Shake: heavy tool results and large fenced or tagged blocks become recoverable placeholders.
use super::*;
use crate::event::ContextEdit as Edit;
use harness_compaction::{
    prune::ToolMatcher,
    shake::{self, ShakeConfig, ShakeRegion},
    CompactionMethod,
};
use harness_providers::MessageRole;
use tokio_util::sync::CancellationToken;

/// A recovery id the exact-savings gate prices placeholders with before the real artifact
/// exists; artifact names are `<64 hex digits>.txt`.
const ARTIFACT_NAME_STAND_IN: &str =
    "0000000000000000000000000000000000000000000000000000000000000000.txt";

/// How one elision pass behaves.
#[derive(Clone)]
pub(in crate::coord) struct ElideOptions {
    /// Fail instead of falling back to artifact-less placeholders when the originals
    /// cannot be saved.
    pub require_artifact: bool,
    /// Elide only tool results, and gate on the exact (not estimated) savings.
    pub tool_results_only: bool,
    pub cancel: CancellationToken,
    /// The maintenance generation the pass belongs to; `0` means it is not tracked.
    pub generation: u64,
}

impl ElideOptions {
    pub(in crate::coord) fn new(cancel: CancellationToken, generation: u64) -> Self {
        Self {
            require_artifact: false,
            tool_results_only: false,
            cancel,
            generation,
        }
    }
}

impl Worker {
    /// Errors when the pass was cancelled or a newer maintenance generation took over.
    async fn ensure_current(&self, options: &ElideOptions) -> Result<(), CoordinatorError> {
        let agent = self.actor.agent_id.clone().unwrap_or_default();
        let reason = if options.cancel.is_cancelled() {
            CancelReason::Manual
        } else {
            let (id, generation) = (agent.clone(), options.generation);
            let current = options.generation == 0
                || self
                    .handle
                    .call(move |s| {
                        Ok(s.compacting
                            .get(&id)
                            .is_some_and(|(current, _)| *current == generation))
                    })
                    .await?;
            if current {
                return Ok(());
            }
            CancelReason::Superseded
        };
        Err(CoordinatorError::CompactionCancelled {
            agent_id: agent,
            reason,
        })
    }

    /// Elides shake regions, saving their originals as a recovery artifact first. Applies
    /// the edits to `context` and returns them unjournaled, with the touched entries
    /// remembered so a failed journal write can roll them back.
    pub(super) async fn elide(
        &self,
        context: &mut Context,
        cfg: &ShakeConfig,
        protected: &[ToolMatcher],
        options: &ElideOptions,
    ) -> Result<Rewrite, CoordinatorError> {
        self.ensure_current(options).await?;
        let body = context.body_start();
        let mut regions: Vec<ShakeRegion> = {
            let items = context.items_with(&self.counter);
            shake::detect_regions(
                &items,
                shake_boundary(&items),
                cfg,
                protected,
                &self.counter,
            )
            .regions
        };
        if options.tool_results_only {
            regions.retain(|region| matches!(region, ShakeRegion::ToolResult { .. }));
            if shake::exact_savings(&regions, Some(ARTIFACT_NAME_STAND_IN), &self.counter)
                < cfg.min_savings
            {
                regions.clear();
            }
        }
        if regions.is_empty() {
            return Ok(Rewrite::default());
        }
        let text = shake::artifact_text(&regions);
        let (owner, task) = (self.actor.clone(), self.turn.id.clone());
        let artifact = match self
            .handle
            .call(move |s| s.write_shake_artifact(&owner, Some(&task), &text))
            .await
        {
            Ok(artifact) => Some(artifact),
            Err(_) if !options.require_artifact => None,
            Err(_) => {
                return Err(CoordinatorError::Invalid(
                    "shake could not save a recovery artifact".into(),
                ))
            }
        };
        self.ensure_current(options).await?;
        let name = artifact
            .as_ref()
            .and_then(|a| a.path.rsplit('/').next().map(str::to_owned));
        let (tools, blocks) = shake::plan_edits(&regions, name.as_deref());
        let now_ms = self.now_ms().await?;
        let mut rewrite = Rewrite {
            artifact: artifact.map(|a| a.path),
            ..Rewrite::default()
        };
        let original = regions
            .iter()
            .fold(0_u32, |sum, region| sum.saturating_add(region.tokens()));
        let mut replaced = 0_u32;
        let mut savings = Vec::new();
        for edit in &tools {
            let index = body + edit.index;
            let Some(record) = context
                .entries
                .get(index)
                .and_then(|entry| tool_edit(entry, &edit.replacement, now_ms, false))
            else {
                continue;
            };
            rewrite.remember(context, index);
            let tokens = self.counter.count_text(&edit.replacement);
            replaced = replaced.saturating_add(tokens);
            savings.push((edit.index, edit.original_tokens.saturating_sub(tokens)));
            rewrite.tool_results += 1;
            rewrite.edits.push(record);
        }
        for edit in &blocks {
            let index = body + edit.index;
            let Some(entry) = context.entries.get(index) else {
                continue;
            };
            let tokens = self.counter.count_text(&edit.replacement);
            let elided_output_tokens = if entry.message.role == MessageRole::Assistant {
                edit.original_tokens.saturating_sub(tokens)
            } else {
                0
            };
            let seq = entry.seq;
            rewrite.remember(context, index);
            replaced = replaced.saturating_add(tokens);
            savings.push((edit.index, edit.original_tokens.saturating_sub(tokens)));
            rewrite.blocks += 1;
            rewrite.edits.push(Edit::TextRange {
                seq,
                start: u32::try_from(edit.start).unwrap_or(u32::MAX),
                end: u32::try_from(edit.end).unwrap_or(u32::MAX),
                text: edit.replacement.clone(),
                elided_output_tokens,
            });
        }
        rewrite.tokens_saved = original.saturating_sub(replaced);
        apply(context, &rewrite.edits);
        self.correct_anchor(context, &mut rewrite, &savings);
        Ok(rewrite)
    }

    /// Automatic shake during maintenance; journals and returns the tokens freed.
    pub(in crate::coord::compaction) async fn auto_shake(
        &self,
        context: &mut Context,
        cfg: ShakeConfig,
        progress: &Progress,
    ) -> Result<Option<u32>, CoordinatorError> {
        let options = ElideOptions::new(progress.cancel.clone(), progress.generation);
        let mut rewrite = self
            .elide(context, &cfg, &self.default_protected_matchers(), &options)
            .await?;
        if rewrite.tool_results + rewrite.blocks == 0 {
            return Ok(None);
        }
        self.commit_rewrite(
            context,
            PruneKindRecord::Shake,
            &mut rewrite,
            RewriteReason::Shake,
        )
        .await?;
        let freed = rewrite.tokens_saved;
        progress
            .update(|stage| {
                stage.tokens_after = stage
                    .tokens_before
                    .map(|before| before.saturating_sub(freed));
            })
            .await?;
        Ok(Some(freed))
    }

    /// Request-body-read-timeout recovery: one mechanical, tool-results-only reduction that
    /// must save a recovery artifact. Does nothing unless automatic compaction is enabled
    /// and shake is among the configured methods. Returns the tokens freed.
    pub(in crate::coord::compaction) async fn recover_request_body(
        &self,
        context: &mut Context,
        cancel: &CancellationToken,
        generation: u64,
    ) -> Result<u32, CoordinatorError> {
        if !self.compaction.enabled
            || !self
                .compaction
                .method_order
                .contains(&CompactionMethod::Shake)
        {
            return Ok(0);
        }
        let options = ElideOptions {
            require_artifact: true,
            tool_results_only: true,
            cancel: cancel.clone(),
            generation,
        };
        let mut rewrite = self
            .elide(
                context,
                &shake::AUTO,
                &self.default_protected_matchers(),
                &options,
            )
            .await?;
        if rewrite.tool_results == 0 {
            return Ok(0);
        }
        self.commit_rewrite(
            context,
            PruneKindRecord::Shake,
            &mut rewrite,
            RewriteReason::Shake,
        )
        .await?;
        Ok(rewrite.tokens_saved)
    }
}
