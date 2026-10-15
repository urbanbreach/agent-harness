//! Construction of the drafts text methods produce.
use super::super::*;
use super::{hook_abort, HookCompaction};
use crate::coord::compaction::commit::merge_preserve;
use crate::coord::compaction::generate::{SummaryInputs, WalkMode};

/// Occupancy recorded as `tokens_before`: the trigger occupancy for automatic passes (a
/// supplied trigger wins), the preparation's own usage figure for manual and handoff passes.
pub(super) fn tokens_before(request: &Request, prep: &Preparation, measure: &Measure) -> u32 {
    if request.reason == Reason::Manual || request.handoff_requested {
        prep.tokens_before
    } else {
        request.trigger_tokens.unwrap_or(measure.tokens)
    }
}

impl Worker {
    /// The summarized messages as plain text, for the summarizing hook.
    pub(in crate::coord::compaction) fn summarized_text(
        &self,
        context: &Context,
        prep: &Preparation,
    ) -> String {
        let items = context.items_with(&self.counter);
        let mut text = String::new();
        for item in items[prep.history.clone()]
            .iter()
            .chain(&items[prep.turn_prefix.clone()])
        {
            text.push_str(&format!("[{:?}]: {}\n", item.role(), item.text()));
        }
        text
    }

    /// Summary through the candidate chain with the hook's extra context and prompt.
    #[expect(
        clippy::too_many_arguments,
        reason = "A draft gathers every committed field"
    )]
    pub(super) async fn soft_draft(
        &self,
        context: &Context,
        body: usize,
        prep: &Preparation,
        request: &Request,
        measure: &Measure,
        mode: WalkMode,
        progress: &Progress,
    ) -> Result<Draft, CoordinatorError> {
        let hook = self
            .run_summarizing_hook(&progress.cancel, &self.summarized_text(context, prep))
            .await
            .map_err(hook_abort)?;
        let focus = request
            .internal_guidance
            .as_deref()
            .or(request.focus.as_deref());
        let inputs = SummaryInputs {
            prompt_override: hook.prompt.as_deref(),
            extra_context: &hook.context,
            focus,
            reserve: policy::effective_reserve(measure.window, measure.settings.reserve_tokens),
        };
        let output = self
            .soft_summary(
                context,
                prep,
                &inputs,
                mode,
                Transport::Model,
                CompactionMethod::Soft,
                None,
                progress,
            )
            .await?;
        let mut draft = self.text_draft(
            context,
            body,
            prep,
            output.summary,
            output.short,
            CompactionMethod::Soft,
            measure,
            output.usage,
            Some((output.provider_id, output.model_id)),
        );
        draft.state.preserve_data = merge_preserve(
            &previous_preserve(context, body),
            hook.preserve_data,
            BTreeMap::new(),
        );
        Ok(draft)
    }

    /// A compaction the before hook supplied: its cut point, token count and details are
    /// trusted as given.
    pub(in crate::coord::compaction) fn hook_draft(
        &self,
        context: &Context,
        body: usize,
        prep: &Preparation,
        custom: HookCompaction,
        method: CompactionMethod,
    ) -> Draft {
        let first = body + usize::from(prep.summary_index.is_some());
        let cut = context.entries[first.min(context.entries.len())..]
            .iter()
            .position(|entry| entry.seq >= custom.first_kept_seq)
            .map_or(context.entries.len(), |offset| first + offset);
        let (read_files, modified_files) = files::file_lists(&prep.file_ops);
        Draft {
            state: crate::coord::context::CompactionState {
                summary: custom.summary,
                short_summary: custom.short_summary,
                method,
                tokens_before: custom.tokens_before,
                tokens_after: None,
                read_files,
                modified_files,
                warning: None,
                archive: None,
                native: None,
                from_hook: true,
                details: custom.details,
                preserve_data: custom.preserve_data,
                commit_seq: 0,
                history_rewrite_at_ms: None,
            },
            continuation: None,
            frames: Vec::new(),
            cut,
            usage: None,
            model: None,
            replay_through_seq: None,
            omit_method: false,
            supersedes_seq: None,
            saved_path: None,
            after_summary: Vec::new(),
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "A text draft gathers every committed field"
    )]
    pub(in crate::coord::compaction) fn text_draft(
        &self,
        context: &Context,
        body: usize,
        prep: &Preparation,
        summary: String,
        short: Option<String>,
        method: CompactionMethod,
        measure: &Measure,
        usage: Option<harness_providers::CompletionUsage>,
        model: Option<(String, String)>,
    ) -> Draft {
        let (read_files, modified_files) = files::file_lists(&prep.file_ops);
        let block = files::render_files_block(&read_files, &modified_files, &prep.file_ops.read);
        let summary = files::upsert_files_block(&summary, &block);
        Draft {
            state: crate::coord::context::CompactionState {
                summary,
                short_summary: short,
                method,
                tokens_before: measure.tokens,
                tokens_after: None,
                read_files,
                modified_files,
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
            usage,
            model,
            replay_through_seq: None,
            omit_method: false,
            supersedes_seq: None,
            saved_path: None,
            after_summary: Vec::new(),
        }
    }

    /// The handoff method: the live model writes a document that becomes the summary.
    #[expect(
        clippy::too_many_arguments,
        reason = "A draft gathers every committed field"
    )]
    pub(super) async fn handoff_draft(
        &self,
        context: &Context,
        body: usize,
        prep: &Preparation,
        request: &Request,
        measure: &Measure,
        progress: &Progress,
    ) -> Result<Result<Draft, MethodResult>, CoordinatorError> {
        progress
            .phase(Some(CompactionMethod::Handoff), CompactionPhase::Handoff)
            .await?;
        let focus = request
            .internal_guidance
            .clone()
            .or_else(|| request.focus.clone())
            .or_else(|| {
                (request.reason == Reason::Threshold)
                    .then(|| prompts::auto_handoff_threshold_focus().to_owned())
            });
        let document = self
            .handoff_document(
                context,
                focus.as_deref(),
                request.reason.automatic(),
                progress,
            )
            .await?;
        // A cancellation that arrived during generation must not accept the document.
        if progress.cancel.is_cancelled() {
            return Err(CoordinatorError::Cancelled(self.turn.id.clone()));
        }
        let Some(doc) = document else {
            if request.reason == Reason::Manual {
                return Err(CoordinatorError::Invalid(
                    "Handoff generation produced no content".into(),
                ));
            }
            return Ok(Err(MethodResult::Unavailable(Some(
                "Auto-handoff returned no document; trying the next preferred compaction method."
                    .into(),
            ))));
        };
        let saved_path = doc.saved_path.clone();
        let model = crate::agent::AgentModelRef::parse(&self.turn.model);
        let mut draft = self.text_draft(
            context,
            body,
            prep,
            doc.text,
            None,
            CompactionMethod::Handoff,
            measure,
            doc.usage,
            Some((model.provider_id, model.model_id)),
        );
        draft.saved_path = saved_path;
        Ok(Ok(draft))
    }
}

/// Preserved data of the active summary, which a new compaction carries forward.
pub(in crate::coord::compaction) fn previous_preserve(
    context: &Context,
    body: usize,
) -> BTreeMap<String, serde_json::Value> {
    context
        .entries
        .get(body)
        .and_then(|entry| entry.compaction.as_ref())
        .map(|state| state.preserve_data.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measure(tokens: u32) -> Measure {
        Measure {
            window: 100_000,
            window_known: true,
            threshold: 80_000,
            tokens,
            provider: tokens,
            local: tokens,
            fixed: 0,
            input_cap: u32::MAX,
            settings: Default::default(),
        }
    }

    #[test]
    fn tokens_before_follows_the_pass_kind() {
        let prep = Preparation {
            summary_index: None,
            history: 0..1,
            turn_prefix: 1..1,
            recent: 1..2,
            is_split: false,
            tokens_before: 42,
            keep_recent_tokens: 1,
            file_ops: Default::default(),
            previous_summary: None,
            previous_summary_at_ms: None,
            previous_details: None,
            previous_preserve: Default::default(),
            last_original_seq: 0,
        };
        let automatic = |trigger| Request {
            trigger_tokens: trigger,
            ..Request::automatic(Reason::Threshold, &[CompactionMethod::Soft], None)
        };
        let manual = Request::automatic(Reason::Manual, &[CompactionMethod::Soft], None);
        let handoff = Request {
            handoff_requested: true,
            ..automatic(Some(9))
        };
        for (request, expected) in [
            (automatic(Some(90_000)), 90_000),
            (automatic(None), 85_000),
            (manual, 42),
            (handoff, 42),
        ] {
            assert_eq!(tokens_before(&request, &prep, &measure(85_000)), expected);
        }
    }
}
