//! Method execution: hooks, summarizer dispatch and the draft each method produces.
use super::super::hooks::{HookCompaction, PreparationFacts};
use super::*;

pub(in crate::coord::compaction) mod draft;

/// A failed hook ends the pass instead of starting another method; a typed cancellation
/// keeps its identity.
pub(in crate::coord::compaction) fn hook_abort(error: CoordinatorError) -> CoordinatorError {
    match error {
        CoordinatorError::CompactionCancelled { .. }
        | CoordinatorError::Compaction(_)
        | CoordinatorError::Cancelled(_) => error,
        other => CoordinatorError::Compaction(CompactionError::new(
            CompactionErrorKind::Hook,
            other.to_string(),
        )),
    }
}

/// What the before-compaction hook decided.
pub(in crate::coord::compaction) enum Hooked {
    /// The hook supplied the whole compaction.
    Custom(Box<HookCompaction>),
    /// Summarize normally.
    Proceed,
}

impl Worker {
    pub(super) async fn run_method(
        &mut self,
        method: CompactionMethod,
        context: &mut Context,
        request: &Request,
        measure: &Measure,
        mode: WalkMode,
        progress: &Progress,
    ) -> Result<MethodResult, CoordinatorError> {
        if method == CompactionMethod::Shake {
            return self
                .run_shake_method(context, request, measure, progress)
                .await;
        }
        // An opaque provider-native boundary only serves the lane that produced it; any other
        // method summarizes the original history recovered from the journal.
        let live = self.request(context);
        let reusable = method == CompactionMethod::Remote
            && !self
                .provider
                .native_compaction_lanes(&live, &self.native_lane_settings(&live))
                .is_empty();
        if let Some(mut expanded) = self.reexpand_native(context, reusable).await? {
            let pending = request
                .pending_from
                .and_then(|index| context.entries.get(index))
                .and_then(|entry| expanded.entries.iter().position(|e| e.seq == entry.seq));
            let remapped = Request {
                pending_from: pending,
                ..request.clone()
            };
            let result = Box::pin(self.run_method(
                method,
                &mut expanded,
                &remapped,
                measure,
                mode,
                progress,
            ))
            .await?;
            if matches!(result, MethodResult::Committed(_)) {
                *context = expanded;
            }
            return Ok(result);
        }
        let Some((body, prep)) = self.prepare(context, request.pending_from) else {
            return Ok(MethodResult::NothingToCompact);
        };
        progress
            .update(|stage| {
                let items = context.items_with(&self.counter);
                let sum = |range: std::ops::Range<usize>| {
                    items[range]
                        .iter()
                        .fold(0u32, |n, i| n.saturating_add(i.tokens))
                };
                stage.summarized_tokens = Some(sum(
                    prep.history.start..prep.turn_prefix.end.max(prep.history.end)
                ));
                stage.kept_tokens = Some(sum(prep.recent.clone()));
                stage.first_kept_seq = context.entries.get(body + prep.recent.start).map(|e| e.seq);
            })
            .await?;
        let draft = match self
            .before_hook(context, body, &prep, request, measure, progress)
            .await?
        {
            Hooked::Custom(custom) => Some(self.hook_draft(context, body, &prep, *custom, method)),
            Hooked::Proceed => match method {
                CompactionMethod::Handoff => {
                    match self
                        .handoff_draft(context, body, &prep, request, measure, progress)
                        .await?
                    {
                        Ok(draft) => Some(draft),
                        Err(result) => return Ok(result),
                    }
                }
                CompactionMethod::Archive => {
                    match self
                        .archive_draft(context, body, &prep, request, measure, progress)
                        .await?
                    {
                        Ok(draft) => Some(draft),
                        Err(blocked) => return self.archive_blocked(request, blocked).await,
                    }
                }
                CompactionMethod::Soft => Some(
                    self.soft_draft(context, body, &prep, request, measure, mode, progress)
                        .await?,
                ),
                CompactionMethod::Remote => {
                    self.remote_draft(context, body, &prep, request, measure, mode, progress)
                        .await?
                }
                CompactionMethod::Shake => None,
            },
        };
        let Some(mut draft) = draft else {
            return Ok(MethodResult::Unavailable(None));
        };
        if !draft.state.from_hook {
            draft.state.tokens_before = draft::tokens_before(request, &prep, measure);
        }
        let committed = self.finish_draft(context, draft, request, progress).await?;
        Ok(MethodResult::Committed(committed))
    }

    /// Automatic shake: a mechanical reduction judged against the recovery target.
    async fn run_shake_method(
        &mut self,
        context: &mut Context,
        request: &Request,
        measure: &Measure,
        progress: &Progress,
    ) -> Result<MethodResult, CoordinatorError> {
        progress
            .phase(Some(CompactionMethod::Shake), CompactionPhase::Shaking)
            .await?;
        let shake = self
            .auto_shake(context, harness_compaction::shake::AUTO, progress)
            .await?;
        let Some(freed) = shake else {
            return Ok(MethodResult::Unavailable((request.reason != Reason::Idle).then(|| {
                "Auto-shake found nothing eligible to drop; trying the next preferred compaction method.".to_owned()
            })));
        };
        // A finite trigger is corrected by what was freed; without one the live occupancy
        // decides.
        let corrected = request
            .trigger_tokens
            .map_or(measure.tokens, |trigger| trigger.saturating_sub(freed));
        let enough = measure.window == 0 || corrected <= policy::recovery_target(measure.threshold);
        if !enough && request.reason != Reason::Idle {
            self.warn(format!(
                "Auto-shake reclaimed ~{freed} tokens but context is still above the threshold; trying the next preferred compaction method."
            ))
            .await?;
        }
        Ok(MethodResult::Reduced { enough })
    }

    /// Emits the before hook with the public focus only and returns its decision. A veto is
    /// a typed cancellation; any failure ends the pass.
    pub(in crate::coord::compaction) async fn before_hook(
        &self,
        context: &Context,
        body: usize,
        prep: &Preparation,
        request: &Request,
        measure: &Measure,
        progress: &Progress,
    ) -> Result<Hooked, CoordinatorError> {
        let (read_files, modified_files) = files::file_lists(&prep.file_ops);
        let entries = &context.entries;
        let seq_at = |index: usize| entries.get(index).map_or(0, |e| e.seq);
        let facts = PreparationFacts {
            reason: request.reason.as_str().to_owned(),
            history_messages: prep.history.len(),
            prefix_messages: prep.turn_prefix.len(),
            retained_messages: prep.recent.len(),
            is_split_turn: prep.is_split,
            tokens_before: prep.tokens_before,
            keep_recent_tokens: prep.keep_recent_tokens,
            reserve_tokens: policy::effective_reserve(
                measure.window,
                measure.settings.reserve_tokens,
            ),
            window_tokens: measure.window_known.then_some(measure.window),
            first_kept_seq: seq_at(body + prep.recent.start),
            branch_from_seq: seq_at(body + prep.history.start.min(prep.turn_prefix.start)),
            branch_through_seq: seq_at(entries.len().saturating_sub(1)),
            previous_summary: prep.previous_summary.clone(),
            read_files,
            modified_files,
        };
        let before = self
            .run_before_hook(&progress.cancel, &facts, request.focus.as_deref())
            .await
            .map_err(hook_abort)?;
        if before.cancel {
            let agent = self.actor.agent_id.clone().unwrap_or_default();
            return Err(super::super::manual::hook_veto(&agent));
        }
        Ok(before
            .compaction
            .map_or(Hooked::Proceed, |custom| Hooked::Custom(Box::new(custom))))
    }

    /// A blocked archive: automatic passes walk on, manual ones warn then fail.
    async fn archive_blocked(
        &self,
        request: &Request,
        blocked: super::super::archive::ArchiveBlocked,
    ) -> Result<MethodResult, CoordinatorError> {
        if request.reason == Reason::Manual {
            self.warn(blocked.warning).await?;
            return Err(CoordinatorError::Invalid(blocked.error));
        }
        let warning = blocked.warning.trim_end_matches('.');
        Ok(MethodResult::Unavailable(Some(format!(
            "{warning}; trying the next preferred compaction method."
        ))))
    }
}
