//! Progress guard: the real compaction is committed first, then rescue tiers run against
//! the committed context, each appended as its own durable record.
use super::super::reduce::{RescueGoal, Rewrite};
use super::*;
use crate::event::{CompactionPatch, CompactionPatchedEvent, NoticeSeverity};

/// The info notice for a reducer rescue that made progress.
fn rescue_notice(rewrite: &Rewrite) -> Option<String> {
    let mut parts = Vec::new();
    let blocks = rewrite.tool_results + rewrite.blocks;
    if blocks > 0 {
        parts.push(format!(
            "elided {blocks} heavy block{} (~{} tokens) to {}",
            if blocks == 1 { "" } else { "s" },
            harness_compaction::payload::thousands(u64::from(rewrite.tokens_saved)),
            if rewrite.artifact.is_some() {
                "an artifact"
            } else {
                "placeholders"
            }
        ));
    }
    if rewrite.images > 0 {
        parts.push(format!(
            "dropped {} attached image{}",
            rewrite.images,
            if rewrite.images == 1 { "" } else { "s" }
        ));
    }
    (!parts.is_empty()).then(|| {
        format!(
            "Compaction dead-end recovery: {} so maintenance could make progress.",
            parts.join(" and ")
        )
    })
}

impl Worker {
    /// Commits a draft, then judges the committed context and rescues it when it does not
    /// leave room. Nothing after the append starts another method: late failures are
    /// reported and the compaction stands.
    pub(super) async fn finish_draft(
        &mut self,
        context: &mut Context,
        draft: Draft,
        request: &Request,
        progress: &Progress,
    ) -> Result<Committed, CoordinatorError> {
        progress
            .phase(Some(draft.state.method), CompactionPhase::Committing)
            .await?;
        let mut committed = self.commit(context, draft, request, progress).await?;
        if request.checks_progress()
            && let Err(error) = self
                .post_commit(context, request, progress, &mut committed)
                .await
            && !progress.cancel.is_cancelled()
        {
            let _ = self
                .warn(format!(
                    "Compaction finished, but recovery work failed: {error}"
                ))
                .await;
        }
        Ok(committed)
    }

    fn progress_ok(request: &Request, measure: &Measure) -> bool {
        if request.will_retry() {
            measure.fits()
        } else {
            measure.has_headroom()
        }
    }

    /// Rescue tiers after the initial compaction: archive frames, elide, drop images. The
    /// notices go out only when the final predicate passes; otherwise the dead-end warning
    /// is stamped on the latest divider.
    async fn post_commit(
        &mut self,
        context: &mut Context,
        request: &Request,
        progress: &Progress,
        committed: &mut Committed,
    ) -> Result<(), CoordinatorError> {
        if Self::progress_ok(request, &self.measure(context)?) {
            return Ok(());
        }
        let mut notices = Vec::new();
        if let Some((next, notice)) = self.frame_rescue(context, request, progress).await? {
            committed.seq = next.seq;
            committed.tokens_after = next.tokens_after;
            notices.push(notice);
        }
        if !Self::progress_ok(request, &self.measure(context)?) && !progress.cancel.is_cancelled() {
            let goal = if request.will_retry() {
                RescueGoal::Fits
            } else {
                RescueGoal::Headroom
            };
            if let Some(notice) = self
                .reducer_rescue(context, goal, request, progress)
                .await?
            {
                notices.push(notice);
            }
        }
        let measure = self.measure(context)?;
        if Self::progress_ok(request, &measure) {
            for notice in notices {
                self.notice(NoticeSeverity::Info, notice).await?;
            }
            committed.tokens_after = measure.tokens;
            return Ok(());
        }
        let message = dead_end_message(true, held_frames(context));
        self.stamp_warning(context, committed.seq, &message).await?;
        committed.warning = Some(message.clone());
        self.warn(message).await
    }

    /// Elide heavy blocks, then drop images, on a working copy; the result is journaled and
    /// swapped in. A failure other than cancellation is not fatal: the next tier still runs.
    async fn reducer_rescue(
        &self,
        context: &mut Context,
        goal: RescueGoal,
        request: &Request,
        progress: &Progress,
    ) -> Result<Option<String>, CoordinatorError> {
        let mut working = context.clone();
        let rewrite = match self
            .rescue(&mut working, goal, progress, request.fallback_from_shake)
            .await
        {
            Ok(rewrite) => rewrite,
            Err(error) if progress.cancel.is_cancelled() => return Err(error),
            Err(_) => return Ok(None),
        };
        if rewrite.edits.is_empty() {
            return Ok(None);
        }
        self.emit_rescue(&rewrite, None).await?;
        *context = working;
        Ok(rescue_notice(&rewrite))
    }

    /// Rebuilds the trailing archive with fewer frames and appends it as a replacement
    /// compaction that names the record it supersedes.
    async fn frame_rescue(
        &mut self,
        context: &mut Context,
        request: &Request,
        progress: &Progress,
    ) -> Result<Option<(Committed, String)>, CoordinatorError> {
        let body = context.body_start();
        let Some(state) = context
            .entries
            .get(body)
            .and_then(|e| e.compaction.clone())
            .filter(|state| state.archive.is_some())
        else {
            return Ok(None);
        };
        let measure = self.measure(context)?;
        let tail = tail_tokens(context, &self.counter);
        let rebuilt = match self
            .rerender_archive(context, &state, tail, &measure, progress)
            .await
        {
            Ok(rebuilt) => rebuilt,
            Err(error) if progress.cancel.is_cancelled() => return Err(error),
            Err(_) => None,
        };
        let Some((rebuilt, frames)) = rebuilt else {
            return Ok(None);
        };
        let old = state.archive.as_ref().map_or(0, |a| a.frames.len());
        let notice = format!(
            "Compaction dead-end recovery: rebuilt the trailing archive at a smaller frame budget ({old} → {} frames) so maintenance could make progress.",
            frames.len()
        );
        let draft = Draft {
            state: crate::coord::context::CompactionState {
                warning: None,
                commit_seq: 0,
                ..rebuilt
            },
            continuation: None,
            frames,
            cut: body + 1,
            usage: None,
            model: None,
            replay_through_seq: None,
            omit_method: false,
            supersedes_seq: Some(state.commit_seq).filter(|seq| *seq > 0),
            saved_path: None,
            after_summary: Vec::new(),
        };
        let next = self.commit(context, draft, request, progress).await?;
        Ok(Some((next, notice)))
    }

    /// Records a warning on an already committed compaction as an append-only patch and
    /// mirrors it in the live context.
    pub(in crate::coord::compaction) async fn stamp_warning(
        &self,
        context: &mut Context,
        seq: u64,
        text: &str,
    ) -> Result<(), CoordinatorError> {
        let (actor, task, agent, warning) = (
            self.actor.clone(),
            self.turn.id.clone(),
            self.actor.agent_id.clone().unwrap_or_default(),
            text.to_owned(),
        );
        self.handle
            .call(move |s| {
                s.emit(
                    actor,
                    Some(task),
                    EventV1::CompactionPatched(CompactionPatchedEvent {
                        agent_id: agent,
                        compaction_seq: seq,
                        patch: CompactionPatch::Warning { text: warning },
                    }),
                )
                .map(|_| ())
            })
            .await?;
        let body = context.body_start();
        if let Some(entry) = context.entries.get_mut(body)
            && let Some(state) = entry.compaction.as_ref()
        {
            let mut state = (**state).clone();
            state.warning = Some(text.to_owned());
            entry.compaction = Some(Arc::new(state));
        }
        Ok(())
    }

    /// After a rescue made a cut point appear, runs the normal pass again. The rescue stays
    /// reported as a rewrite even when the pass itself finds nothing more to do.
    async fn summarize_after_rescue(
        &mut self,
        context: &mut Context,
        request: &Request,
        progress: &Progress,
        rescued: &mut Outcome,
        notices: &[String],
    ) -> Result<Option<Outcome>, CoordinatorError> {
        if self.prepare(context, request.pending_from).is_none() {
            return Ok(None);
        }
        let mut next = Box::pin(self.maintain_inner(context, request, progress)).await?;
        next.rewritten = true;
        if next.committed.is_none() {
            next.committed = rescued.committed.take();
        }
        if !next.blocked {
            for notice in notices {
                self.notice(NoticeSeverity::Info, notice.clone()).await?;
            }
        }
        Ok(Some(next))
    }

    /// Automatic pass with no older history to summarize: rescue reducers on the whole
    /// context, re-prepare, else pause with a dead-end warning. A rewrite stays reported
    /// through the recursion.
    pub(super) async fn dead_end_without_preparation(
        &mut self,
        context: &mut Context,
        request: &Request,
        progress: &Progress,
    ) -> Result<Outcome, CoordinatorError> {
        let mut outcome = Outcome::default();
        let mut notices = Vec::new();
        if let Some((committed, notice)) = self.frame_rescue(context, request, progress).await? {
            outcome.committed = Some(committed);
            outcome.rewritten = true;
            notices.push(notice);
            let measure = self.measure(context)?;
            if Self::progress_ok(request, &measure) {
                outcome.fits = measure.fits();
                for notice in notices {
                    self.notice(NoticeSeverity::Info, notice).await?;
                }
                return Ok(outcome);
            }
        }
        if !progress.cancel.is_cancelled()
            && let Some(notice) = self
                .reducer_rescue(
                    context,
                    RescueGoal::Preparation(request.pending_from),
                    request,
                    progress,
                )
                .await?
        {
            outcome.rewritten = true;
            notices.push(notice);
            if let Some(next) = self
                .summarize_after_rescue(context, request, progress, &mut outcome, &notices)
                .await?
            {
                return Ok(next);
            }
        }
        let message = dead_end_message(false, held_frames(context));
        if outcome.rewritten
            && let Some(seq) = context
                .entries
                .get(context.body_start())
                .and_then(|e| e.compaction.as_ref())
                .map(|state| state.commit_seq)
                .filter(|seq| *seq > 0)
        {
            self.stamp_warning(context, seq, &message).await?;
        }
        self.warn(message.clone()).await?;
        outcome.fits = self.measure(context)?.fits();
        outcome.blocked = true;
        outcome.noop = Some(message);
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rescue_notice_composes_elide_and_image_fragments() {
        for (tool_results, blocks, images, artifact, expected) in [
            (1, 0, 0, true, Some("elided 1 heavy block (~1,500 tokens) to an artifact")),
            (2, 1, 0, false, Some("elided 3 heavy blocks (~1,500 tokens) to placeholders")),
            (0, 0, 2, false, Some("dropped 2 attached images")),
            (
                1,
                0,
                1,
                true,
                Some("elided 1 heavy block (~1,500 tokens) to an artifact and dropped 1 attached image"),
            ),
            (0, 0, 0, false, None),
        ] {
            let mut rewrite = Rewrite::default();
            rewrite.tool_results = tool_results;
            rewrite.blocks = blocks;
            rewrite.images = images;
            rewrite.tokens_saved = 1_500;
            rewrite.artifact = artifact.then(|| "artifacts/x".to_owned());
            assert_eq!(
                rescue_notice(&rewrite),
                expected.map(|part| format!(
                    "Compaction dead-end recovery: {part} so maintenance could make progress."
                ))
            );
        }
    }
}
