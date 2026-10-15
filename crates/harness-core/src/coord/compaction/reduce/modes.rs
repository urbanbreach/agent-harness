//! Manual `/shake` modes: elide, images and thinking.
use super::*;
use harness_compaction::{shake, ShakeMode};
use harness_providers::MessageRole;

impl Worker {
    /// Removes image attachments from user, tool and custom entries across the whole
    /// context. Assistant entries and compaction summaries are not image-strip roles.
    /// Entries left empty get a placeholder when the edit applies.
    pub(super) fn drop_images(&self, context: &mut Context) -> Rewrite {
        let mut rewrite = Rewrite::default();
        let body = context.body_start();
        for index in body..context.entries.len() {
            let entry = &context.entries[index];
            let images = entry
                .attachments
                .iter()
                .filter(|a| a.mime.starts_with("image/"))
                .count();
            if images == 0
                || entry.compaction.is_some()
                || entry.branch_summary.is_some()
                || entry.message.role == MessageRole::Assistant
            {
                continue;
            }
            let edit = ContextEdit::DropAttachments {
                seq: entry.seq,
                tool_call_id: entry.message.tool_call_id.clone(),
            };
            rewrite.images += images;
            rewrite.remember(context, index);
            rewrite.edits.push(edit);
        }
        apply(context, &rewrite.edits);
        rewrite
    }

    /// Removes visible and opaque reasoning from every assistant entry. Counts are exact;
    /// opaque signatures never inflate the tokens freed.
    fn drop_thinking(&self, context: &mut Context) -> Rewrite {
        let mut rewrite = Rewrite::default();
        let mut savings = Vec::new();
        let body = context.body_start();
        for index in body..context.entries.len() {
            let entry = &context.entries[index];
            if entry.message.role != MessageRole::Assistant {
                continue;
            }
            let (blocks, tokens) = self.reasoning_blocks(entry);
            if blocks == 0 {
                continue;
            }
            let edit = ContextEdit::DropReasoning {
                seq: entry.seq,
                elided_output_tokens: tokens,
            };
            rewrite.thinking += blocks;
            rewrite.tokens_saved = rewrite.tokens_saved.saturating_add(tokens);
            savings.push((index - body, tokens));
            rewrite.remember(context, index);
            rewrite.edits.push(edit);
        }
        apply(context, &rewrite.edits);
        self.correct_anchor(context, &mut rewrite, &savings);
        rewrite
    }

    /// Real reasoning blocks of an entry and their visible tokens. Ordered parts are the
    /// authority; a flat entry falls back to its settled reasoning, and an Anthropic
    /// continuation with neither is one opaque block.
    fn reasoning_blocks(&self, entry: &Entry) -> (usize, u32) {
        use harness_compaction::ItemPart;
        let mut blocks = 0;
        let mut tokens = 0_u32;
        for part in &entry.parts {
            match part {
                ItemPart::Reasoning(text) => {
                    blocks += 1;
                    tokens = tokens.saturating_add(self.counter.count_text(text));
                }
                ItemPart::RedactedReasoning => blocks += 1,
                ItemPart::Text(_) | ItemPart::Call(_) => {}
            }
        }
        if blocks > 0 {
            return (blocks, tokens);
        }
        for text in &entry.settled_reasoning {
            blocks += 1;
            tokens = tokens.saturating_add(self.counter.count_text(text));
        }
        let signed = entry
            .message
            .continuation
            .as_ref()
            .is_some_and(|c| c.protocol() == harness_providers::Protocol::Anthropic);
        (blocks.max(usize::from(signed)), tokens)
    }

    /// `/shake [elide|images|thinking]`.
    pub(in crate::coord::compaction) async fn manual_shake(
        &self,
        context: &mut Context,
        mode: ShakeMode,
        progress: &Progress,
    ) -> Result<(String, u32), CoordinatorError> {
        let (kind, reason, mut rewrite) = match mode {
            ShakeMode::Elide => {
                let options = ElideOptions::new(progress.cancel.clone(), progress.generation);
                let rewrite = self
                    .elide(context, &shake::MANUAL, &self.skill_matchers(), &options)
                    .await?;
                (PruneKindRecord::Shake, RewriteReason::Shake, rewrite)
            }
            ShakeMode::Images => (
                PruneKindRecord::Images,
                RewriteReason::Images,
                self.drop_images(context),
            ),
            ShakeMode::Thinking => (
                PruneKindRecord::Thinking,
                RewriteReason::Thinking,
                self.drop_thinking(context),
            ),
        };
        if mode == ShakeMode::Elide {
            self.commit_rewrite(context, kind, &mut rewrite, reason)
                .await?;
        } else {
            // Image and reasoning removal keep their weaker failure semantics: a journal
            // failure is reported but the live edit is not rolled back.
            self.journal(kind, &rewrite, None).await?;
            if !rewrite.edits.is_empty() {
                rebase_native_usage(context, &self.counter);
                self.after_rewrite(reason);
            }
        }
        let summary = shake::summarize_shake(
            mode,
            rewrite.tool_results,
            rewrite.blocks,
            rewrite.images,
            rewrite.thinking,
            rewrite.tokens_saved,
        );
        let freed = rewrite.tokens_saved;
        progress
            .update(|stage| {
                stage.tokens_after = stage
                    .tokens_before
                    .map(|before| before.saturating_sub(freed));
            })
            .await?;
        Ok((summary, freed))
    }
}
