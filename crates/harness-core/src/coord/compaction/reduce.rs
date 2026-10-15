//! Mechanical reducers: stale-result pruning, shake, image and reasoning removal, rescue.
use super::super::{
    context::{Context, Entry},
    turn::Worker,
    *,
};
use super::progress::Progress;
use crate::event::{ContextEdit, ContextPrunedEvent, PruneKindRecord};
use harness_compaction::{Item, ItemKind};
use std::collections::HashMap;

mod after;
mod anchor;
mod elide;
mod modes;
mod protect;
mod rescue;
mod stale;

pub(super) use after::{rebase_native_usage, RewriteReason};
pub(super) use elide::ElideOptions;

/// Edits a reducer applied, ready to journal.
#[derive(Default)]
pub(super) struct Rewrite {
    pub edits: Vec<ContextEdit>,
    pub tokens_saved: u32,
    pub artifact: Option<String>,
    pub tool_results: usize,
    pub blocks: usize,
    pub images: usize,
    pub thinking: usize,
    /// Entries as they were before the edits touched them, for rollback.
    touched: Vec<(usize, Entry)>,
}

/// When a dead-end rescue may stop before dropping images.
#[derive(Clone, Copy)]
pub(super) enum RescueGoal {
    /// The context fits the prompt budget (recoveries that retry the request).
    Fits,
    /// The context is under the recovery band.
    Headroom,
    /// A cut point exists again for the given pending boundary.
    Preparation(Option<usize>),
}

impl Rewrite {
    fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    /// Whether the rewrite actually removed something.
    pub(super) fn reclaimed(&self) -> bool {
        self.tokens_saved > 0 || self.images > 0 || self.thinking > 0
    }

    /// Remembers the entry at `index` the first time an edit is about to touch it.
    fn remember(&mut self, context: &Context, index: usize) {
        if self.touched.iter().all(|(seen, _)| *seen != index)
            && let Some(entry) = context.entries.get(index)
        {
            self.touched.push((index, entry.clone()));
        }
    }

    /// Puts every touched entry back as it was.
    pub(super) fn restore(&mut self, context: &mut Context) {
        for (index, entry) in self.touched.drain(..) {
            if let Some(slot) = context.entries.get_mut(index) {
                *slot = entry;
            }
        }
    }
}

/// First item after the latest compaction summary; prune reducers never touch earlier items.
fn prune_boundary(items: &[Item<'_>]) -> usize {
    items
        .iter()
        .rposition(|item| item.kind == ItemKind::Summary)
        .map_or(0, |index| index + 1)
}

/// First item after the first compaction summary; shake never touches earlier items.
fn shake_boundary(items: &[Item<'_>]) -> usize {
    items
        .iter()
        .position(|item| item.kind == ItemKind::Summary)
        .map_or(0, |index| index + 1)
}

fn tool_edit(
    entry: &Entry,
    replacement: &str,
    now_ms: u64,
    drop_attachments: bool,
) -> Option<ContextEdit> {
    Some(ContextEdit::ToolResult {
        tool_call_id: entry.message.tool_call_id.clone()?,
        seq: Some(entry.seq),
        text: replacement.to_owned(),
        pruned_at_ms: now_ms,
        drop_attachments,
    })
}

fn apply(context: &mut Context, edits: &[ContextEdit]) {
    super::super::history::apply_edits(&mut context.entries, edits, &HashMap::new());
}

impl Worker {
    async fn now_ms(&self) -> Result<u64, CoordinatorError> {
        self.handle
            .call(|s| {
                Ok(s.clock
                    .system_time_rfc3339_millis()
                    .as_deref()
                    .and_then(crate::clock::rfc3339_unix_ms)
                    .and_then(|ms| u64::try_from(ms).ok())
                    .unwrap_or(0))
            })
            .await
    }

    async fn journal(
        &self,
        kind: PruneKindRecord,
        rewrite: &Rewrite,
        warning: Option<String>,
    ) -> Result<(), CoordinatorError> {
        if rewrite.is_empty() && warning.is_none() {
            return Ok(());
        }
        let agent = self.actor.agent_id.clone().unwrap_or_default();
        let (task, owner) = (self.turn.id.clone(), self.actor.clone());
        let event = ContextPrunedEvent {
            agent_id: agent,
            kind,
            edits: rewrite.edits.clone(),
            tokens_saved: rewrite.tokens_saved,
            artifact_path: rewrite.artifact.clone(),
            warning,
        };
        self.handle
            .call(move |s| {
                s.check_task(&task)?;
                s.emit(owner, Some(task), EventV1::ContextPruned(event))
                    .map(|_| ())
            })
            .await
    }

    /// Journals an applied rewrite. When the journal write fails the touched entries are
    /// restored so the live context never runs ahead of the durable one.
    async fn commit_rewrite(
        &self,
        context: &mut Context,
        kind: PruneKindRecord,
        rewrite: &mut Rewrite,
        reason: RewriteReason,
    ) -> Result<(), CoordinatorError> {
        if let Err(error) = self.journal(kind, rewrite, None).await {
            rewrite.restore(context);
            return Err(error);
        }
        if !rewrite.is_empty() {
            rebase_native_usage(context, &self.counter);
            self.after_rewrite(reason);
        }
        Ok(())
    }

    pub(super) async fn emit_rescue(
        &self,
        rewrite: &Rewrite,
        warning: Option<String>,
    ) -> Result<(), CoordinatorError> {
        self.journal(PruneKindRecord::Rescue, rewrite, warning)
            .await?;
        if !rewrite.is_empty() {
            self.after_rewrite(RewriteReason::Rescue);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
