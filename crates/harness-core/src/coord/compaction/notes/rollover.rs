//! The rollover pass: an ordinary preparation and keep-recent cut, committed as a normal
//! compaction whose summary is a fixed notice. No summarizer, catalog, rescue or method
//! fallback is involved; the old transcript stays canonical in the journal.
use super::super::commit::{merge_preserve, Committed, Draft};
use super::super::maintain::{
    dead_end_message,
    methods::{draft::previous_preserve, hook_abort, Hooked},
    Measure, Outcome, Reason, Request,
};
use super::super::progress::Progress;
use super::{
    inject_notebook, latest_notebook, recover_authoritative_request, reminder, rollover_details,
    rollover_summary, NotebookRevision,
};
use crate::coord::{
    context::{CompactionState, Context, Entry},
    turn::Worker,
    *,
};
use harness_compaction::{policy, prepare::Preparation, CompactionMethod};
use std::collections::BTreeMap;

/// The typed no-op of a manual rollover with nothing to roll.
const NOTHING_TO_COMPACT: &str = "Nothing to compact (session too small or already rolled over)";

/// What must stay the same from preparation to commit.
struct Snapshot {
    model: String,
    rewinds: usize,
}

/// The journal facts a rolled-over window is built from.
struct Material {
    recovered: Option<Entry>,
    notebook: Option<NotebookRevision>,
}

impl Worker {
    /// Whether the rollover, not the configured method walk, serves this request: the
    /// experiment is usable and the request is not an explicit mode, a focused or guided
    /// manual pass, or a handoff.
    pub(in crate::coord) fn notes_routes(&self, request: &Request) -> bool {
        if !self.notes_usable() {
            return false;
        }
        match request.reason {
            Reason::Manual => {
                !request.forced
                    && !request.handoff_requested
                    && request.internal_guidance.is_none()
                    && request
                        .focus
                        .as_deref()
                        .is_none_or(|focus| focus.trim().is_empty())
            }
            Reason::Threshold | Reason::Overflow | Reason::Incomplete | Reason::Idle => true,
        }
    }

    /// The no-op of a rollover with nothing to roll over, decided before any progress or
    /// lifecycle event exists: manual callers get the typed message, automatic ones a
    /// blocked continuation, and nobody gets a start or end event, rescue or warning.
    pub(in crate::coord) fn notes_unprepared(
        &self,
        context: &Context,
        request: &Request,
    ) -> Result<Option<Outcome>, CoordinatorError> {
        if !self.notes_routes(request) {
            return Ok(None);
        }
        if self.prepare(context, request.pending_from).is_some() {
            return Ok(None);
        }
        Ok(Some(Outcome {
            skipped: true,
            blocked: request.checks_progress(),
            fits: self.measure(context)?.fits(),
            noop: Some(NOTHING_TO_COMPACT.to_owned()),
            ..Outcome::default()
        }))
    }

    /// Commits one rollover. A pass with nothing to roll over is a no-op: manual callers
    /// get the typed message, automatic ones a blocked continuation without a warning.
    pub(in crate::coord) async fn notes_rollover(
        &mut self,
        context: &mut Context,
        request: &Request,
        progress: &Progress,
    ) -> Result<Outcome, CoordinatorError> {
        let measure = self.measure(context)?;
        let Some((body, prep)) = self.prepare(context, request.pending_from) else {
            return Ok(Outcome {
                skipped: true,
                blocked: request.checks_progress(),
                fits: measure.fits(),
                noop: Some(NOTHING_TO_COMPACT.to_owned()),
                ..Outcome::default()
            });
        };
        let agent = self.actor.agent_id.clone().unwrap_or_default();
        let snapshot = Snapshot {
            model: self.turn.model.clone(),
            rewinds: self.rewinds().await?,
        };
        // Whatever was being summarized ahead of time no longer applies.
        let speculating = agent.clone();
        self.handle
            .call(move |s| {
                super::super::speculate::cancel(s, &speculating);
                Ok(())
            })
            .await?;
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

        let (mut draft, custom) = match self
            .before_hook(context, body, &prep, request, &measure, progress)
            .await?
        {
            Hooked::Custom(custom) => (
                self.hook_draft(context, body, &prep, *custom, CompactionMethod::Soft),
                true,
            ),
            Hooked::Proceed => (
                self.rollover_draft(context, body, &prep, request, &measure),
                false,
            ),
        };
        if !self.still_eligible(&snapshot, progress).await? {
            return Ok(self.stale(&measure));
        }
        // The summarizing hook only contributes preserved data: with no model summary there
        // is nothing for its context or prompt to steer.
        if !custom {
            let hook = self
                .run_summarizing_hook(&progress.cancel, &self.summarized_text(context, &prep))
                .await
                .map_err(hook_abort)?;
            draft.state.preserve_data = merge_preserve(
                &previous_preserve(context, body),
                hook.preserve_data,
                BTreeMap::new(),
            );
            if !self.still_eligible(&snapshot, progress).await? {
                return Ok(self.stale(&measure));
            }
        } else if request.reason.automatic()
            && let Some(trigger) = request.trigger_tokens
        {
            // A supplied trigger occupancy overrides a hook's own count.
            draft.state.tokens_before = trigger;
        }
        draft.state.details = Some(rollover_details(draft.state.details.take()));
        draft.omit_method = true;

        let mut work = context.clone();
        self.build_window(&mut work, &mut draft).await?;
        progress
            .phase(None, crate::event::CompactionPhase::Committing)
            .await?;
        let mut committed = self.commit(&mut work, draft, request, progress).await?;
        *context = work;
        self.handle
            .call(move |s| {
                reminder::clear(s, &agent);
                Ok(())
            })
            .await?;
        self.judge(context, request, &mut committed).await
    }

    /// The draft of a rollover nobody supplied: a fixed notice, no files, no method.
    fn rollover_draft(
        &self,
        context: &Context,
        body: usize,
        prep: &Preparation,
        request: &Request,
        measure: &Measure,
    ) -> Draft {
        let tokens_before = if request.reason == Reason::Manual {
            prep.tokens_before
        } else {
            request.trigger_tokens.unwrap_or(measure.tokens)
        };
        Draft {
            state: CompactionState {
                summary: rollover_summary(),
                short_summary: None,
                // The record names no method; this only fills the field.
                method: CompactionMethod::Soft,
                tokens_before,
                tokens_after: None,
                read_files: Vec::new(),
                modified_files: Vec::new(),
                warning: None,
                archive: None,
                native: None,
                from_hook: false,
                details: None,
                preserve_data: merge_preserve(
                    &previous_preserve(context, body),
                    BTreeMap::new(),
                    BTreeMap::new(),
                ),
                commit_seq: 0,
                history_rewrite_at_ms: None,
            },
            continuation: None,
            frames: Vec::new(),
            cut: (body + prep.recent.start).min(context.entries.len()),
            usage: None,
            model: None,
            replay_through_seq: None,
            omit_method: true,
            supersedes_seq: None,
            saved_path: None,
            after_summary: Vec::new(),
        }
    }

    /// Prepares the working context: the latest notebook leads it, and the newest
    /// authoritative request that falls before the kept window follows the summary.
    async fn build_window(
        &self,
        work: &mut Context,
        draft: &mut Draft,
    ) -> Result<(), CoordinatorError> {
        let kept = work.entries.get(draft.cut).map(|entry| {
            (
                entry.seq,
                entry.message.role,
                entry.message.tool_call_id.clone(),
            )
        });
        let material = self.window_material(kept).await?;
        if let Some(request) = material.recovered {
            draft.after_summary = vec![request];
        }
        // The notebook sits in the standing prefix, which moves every later index.
        let before = work.body_start();
        inject_notebook(
            work,
            &material.notebook.unwrap_or(NotebookRevision {
                seq: 0,
                text: String::new(),
                at_ms: None,
            }),
        );
        draft.cut = (draft.cut + work.body_start()).saturating_sub(before);
        Ok(())
    }

    /// The latest notebook and the newest authoritative request before the first kept
    /// entry, both read from the original journal rather than from the live context.
    async fn window_material(
        &self,
        kept: Option<(u64, harness_providers::MessageRole, Option<String>)>,
    ) -> Result<Material, CoordinatorError> {
        let agent = self.actor.agent_id.clone().unwrap_or_default();
        self.handle
            .call(move |s| {
                let (active, _) = s.notes_branch()?;
                let primary = s
                    .agents
                    .get(&agent)
                    .is_some_and(|a| a.info.parent_agent_id.is_none());
                let originals = crate::coord::history::original_entries(&active, &agent, primary);
                let first_kept = kept.map_or(originals.len(), |(seq, role, call)| {
                    originals
                        .iter()
                        .position(|e| {
                            e.seq == seq && e.message.role == role && e.message.tool_call_id == call
                        })
                        .or_else(|| originals.iter().position(|e| e.seq >= seq))
                        .unwrap_or(originals.len())
                });
                let recovered = match recover_authoritative_request(&originals, first_kept) {
                    Some(index) => {
                        let mut request = originals[index].clone();
                        crate::coord::history::hydrate_original(
                            &mut request,
                            &active,
                            &s.info()?.run_dir,
                        )?;
                        // The copy belongs to no turn, so a failed turn cannot discard it.
                        request.turn = None;
                        Some(request)
                    }
                    None => None,
                };
                Ok(Material {
                    notebook: latest_notebook(&active, &agent),
                    recovered,
                })
            })
            .await
    }

    async fn rewinds(&self) -> Result<usize, CoordinatorError> {
        self.handle
            .call(|s| s.notes_branch().map(|(_, rewinds)| rewinds))
            .await
    }

    /// The pass may still commit: not cancelled, the experiment still usable, and the
    /// model and branch it prepared against unchanged.
    async fn still_eligible(
        &self,
        snapshot: &Snapshot,
        progress: &Progress,
    ) -> Result<bool, CoordinatorError> {
        if progress.cancel.is_cancelled() {
            return Err(CoordinatorError::Cancelled(self.turn.id.clone()));
        }
        Ok(self.notes_usable()
            && self.turn.model == snapshot.model
            && self.rewinds().await? == snapshot.rewinds)
    }

    /// A pass whose eligibility vanished ends skipped, never as a fallback to another method.
    fn stale(&self, measure: &Measure) -> Outcome {
        Outcome {
            skipped: true,
            fits: measure.fits(),
            ..Outcome::default()
        }
    }

    /// Judges the committed window. A retry must leave a context that fits the window,
    /// other non-idle work must reach the ordinary recovery band; idle always passes. A
    /// window that is still too large is stamped with the dead-end warning and blocks
    /// automatic continuation. Nothing else is tried.
    async fn judge(
        &mut self,
        context: &mut Context,
        request: &Request,
        committed: &mut Committed,
    ) -> Result<Outcome, CoordinatorError> {
        let after = self.measure(context)?;
        let mut outcome = Outcome {
            rewritten: true,
            fits: after.fits(),
            ..Outcome::default()
        };
        if request.checks_progress() {
            let ok = if request.will_retry() {
                after.fits()
            } else {
                after.window == 0 || after.tokens <= policy::recovery_target(after.threshold)
            };
            if !ok {
                let message = dead_end_message(true, 0);
                self.stamp_warning(context, committed.seq, &message).await?;
                committed.warning = Some(message.clone());
                self.warn(message).await?;
                outcome.blocked = true;
                self.park_context();
            }
        }
        outcome.committed = Some(committed.clone());
        Ok(outcome)
    }
}
