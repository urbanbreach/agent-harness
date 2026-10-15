use super::super::super::{context::Context, turn::Worker, *};
use super::super::commit::merge_preserve;
use super::super::commit::Draft;
use super::super::generate::{SummaryInputs, Transport, WalkMode};
use super::super::maintain::methods::draft::previous_preserve;
use super::super::maintain::methods::hook_abort;
use super::super::maintain::{Measure, Request};
use super::super::progress::Progress;
use harness_compaction::{policy, prepare::Preparation, CompactionMethod};
use std::collections::BTreeMap;

impl Worker {
    /// The remote method without a native lane: a readable summary written by the configured
    /// endpoint. A native boundary in the context is expanded back to its journal history
    /// first, because an opaque replacement history cannot be summarized.
    #[expect(
        clippy::too_many_arguments,
        reason = "Endpoint fallback shares the remote preparation"
    )]
    pub(super) async fn endpoint_draft(
        &self,
        context: &Context,
        body: usize,
        prep: &Preparation,
        req: &Request,
        measure: &Measure,
        mode: WalkMode,
        progress: &Progress,
    ) -> Result<Option<Draft>, CoordinatorError> {
        let Some(endpoint) = self.compaction.remote_endpoint.as_deref() else {
            return Ok(None);
        };
        let expanded = self.reexpand_native(context, false).await?;
        let source = expanded.as_ref().unwrap_or(context);
        let pending = req
            .pending_from
            .and_then(|index| context.entries.get(index))
            .and_then(|pending| {
                source.entries.iter().position(|entry| {
                    entry.seq == pending.seq && entry.message.role == pending.message.role
                })
            });
        let prepared = if expanded.is_some() {
            self.prepare(source, pending)
        } else {
            None
        };
        let source_prep = prepared.as_ref().map_or(prep, |(_, prep)| prep);
        let hook = self
            .run_summarizing_hook(&progress.cancel, &self.summarized_text(source, source_prep))
            .await
            .map_err(hook_abort)?;
        let inputs = SummaryInputs {
            prompt_override: hook.prompt.as_deref(),
            extra_context: &hook.context,
            focus: req.internal_guidance.as_deref().or(req.focus.as_deref()),
            reserve: policy::effective_reserve(measure.window, measure.settings.reserve_tokens),
        };
        let output = self
            .soft_summary(
                source,
                source_prep,
                &inputs,
                mode,
                Transport::Endpoint(endpoint),
                CompactionMethod::Remote,
                None,
                progress,
            )
            .await?;
        let mut display_prep = prep.clone();
        display_prep.file_ops.clone_from(&source_prep.file_ops);
        let mut draft = self.text_draft(
            context,
            body,
            &display_prep,
            output.summary,
            output.short,
            CompactionMethod::Remote,
            measure,
            output.usage,
            Some((output.provider_id, output.model_id)),
        );
        draft.state.preserve_data = merge_preserve(
            &previous_preserve(context, body),
            hook.preserve_data,
            BTreeMap::new(),
        );
        if let Some((source_body, prepared)) = prepared {
            draft.cut = source
                .entries
                .get(source_body + prepared.recent.start)
                .and_then(|kept| {
                    context.entries.iter().position(|entry| {
                        entry.seq == kept.seq && entry.message.role == kept.message.role
                    })
                })
                .unwrap_or(context.entries.len());
        }
        Ok(Some(draft))
    }
}
