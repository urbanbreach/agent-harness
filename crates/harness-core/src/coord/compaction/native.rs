//! The remote method: provider-native compaction lanes, or a configured summary endpoint.
use super::super::{context::Context, turn::Worker, *};
use super::commit::Draft;
use super::generate::{into_coordinator, is_cancelled, NativeBoundary, WalkMode};
use super::maintain::methods::hook_abort;
use super::maintain::{Measure, Request};
use super::progress::Progress;
use crate::event::{CompactionPhase, NativeCompactionMeta};
use harness_compaction::{policy, prepare::Preparation, prompts, request, CompactionMethod};
use harness_providers::{NativeCompactionLane, NativeCompactionRequest};

mod anthropic;
mod custom;
mod retained;
use anthropic::{anthropic_cut, files_due};
use retained::retained_messages;

const NATIVE_SUMMARY: &str =
    "Remote compaction preserved provider-native history for this session.";

fn lane_name(lane: NativeCompactionLane) -> &'static str {
    match lane {
        NativeCompactionLane::OpenAiV2 => "openai_v2",
        NativeCompactionLane::OpenAiV1 => "openai_v1",
        NativeCompactionLane::Anthropic => "anthropic",
    }
}

impl Worker {
    #[expect(
        clippy::too_many_arguments,
        reason = "The remote method needs the whole preparation context"
    )]
    pub(super) async fn remote_draft(
        &self,
        context: &Context,
        body: usize,
        prep: &Preparation,
        req: &Request,
        measure: &Measure,
        mode: WalkMode,
        progress: &Progress,
    ) -> Result<Option<Draft>, CoordinatorError> {
        let live = self.request(context);
        let settings = self.native_lane_settings(&live);
        let lanes = self.provider.native_compaction_lanes(&live, &settings);
        let model = crate::agent::AgentModelRef::parse(&self.turn.model);
        if lanes.is_empty() {
            return self
                .endpoint_draft(context, body, prep, req, measure, mode, progress)
                .await;
        }
        progress
            .phase(Some(CompactionMethod::Remote), CompactionPhase::Remote)
            .await?;
        let anthropic = lanes.contains(&NativeCompactionLane::Anthropic);
        let pending = req.pending_from.unwrap_or(context.entries.len());
        // OpenAI replacement history carries the retained turns itself; Anthropic summarizes
        // only the discarded prefix and the kept tail stays as ordinary messages.
        let end = if anthropic {
            anthropic_cut(context, body + prep.recent.start, pending)
        } else {
            pending
        }
        .min(context.entries.len());
        let compacted = Context {
            entries: context.entries[..end].to_vec(),
            ..context.clone()
        };
        let mut native_request = self.request(&compacted);
        native_request.context.main_turn = false;
        let mut retained_files = Vec::new();
        if anthropic {
            let due = files_due(&context.entries, end);
            let times = std::mem::take(&mut native_request.message_times_ms);
            for (index, message) in native_request.messages.iter_mut().enumerate() {
                if let Some(replay) = message.continuation.as_mut() {
                    let summary_at = times.get(index).copied().flatten();
                    retained_files.extend(
                        replay
                            .retain_anthropic_files(summary_at, due, index, end, pending)
                            .map_err(|error| CoordinatorError::Invalid(error.into()))?,
                    );
                }
            }
            native_request.message_times_ms = times;
        }
        let retained = retained_messages(&context.entries[body..end]);
        let reserve = policy::effective_reserve(measure.window, measure.settings.reserve_tokens);
        let hook = if anthropic {
            let text = self.summarized_text(context, prep);
            Some(
                self.run_summarizing_hook(&progress.cancel, &text)
                    .await
                    .map_err(hook_abort)?,
            )
        } else {
            None
        };
        let focus = req.internal_guidance.as_deref().or(req.focus.as_deref());
        let instructions = match &hook {
            Some(hook) => prompts::anthropic_compaction_instructions(
                &hook.context,
                hook.prompt.as_deref(),
                focus,
            ),
            None => prompts::summarization_system_prompt().to_owned(),
        };
        // Same-provider native-capable models only; an Anthropic signed summary is bound to
        // the model that wrote its prefix, so it keeps that model alone.
        let boundary = NativeBoundary {
            provider: model.provider_id.clone(),
            model: anthropic.then(|| model.model_id.clone()),
        };
        let candidates = self
            .summary_candidates(Some(&boundary))
            .await
            .map_err(|error| into_coordinator(error, &self.turn.id))?;
        let snapshot_through = compacted
            .entries
            .iter()
            .map(|entry| entry.seq)
            .max()
            .filter(|seq| *seq > 0);
        let result = self
            .walk_candidates(mode, &progress.cancel, candidates, |candidate, _| {
                let mut attempt = native_request.clone();
                attempt.model_id.clone_from(&candidate.model);
                let attempt_settings = self.native_lane_settings(&attempt);
                let attempt_lanes = self
                    .provider
                    .native_compaction_lanes(&attempt, &attempt_settings);
                let input = NativeCompactionRequest {
                    request: attempt,
                    lanes: attempt_lanes,
                    instructions: instructions.clone(),
                    retained_message_budget: self.compaction.v2_retained_message_budget,
                    context_window: candidate
                        .window
                        .or_else(|| measure.window_known.then_some(measure.window)),
                    max_tokens: request::summary_max_tokens(reserve),
                    retained: retained.clone(),
                    settings: attempt_settings,
                    saved_request: context.model_request.clone(),
                    ..Default::default()
                };
                let provider = std::sync::Arc::clone(&self.provider);
                let cancel = progress.cancel.clone();
                let used_model = candidate.model.clone();
                async move {
                    provider
                        .native_compact(input, cancel)
                        .await
                        .map(|result| (used_model, result))
                        .map_err(super::CompactionError::from)
                }
            })
            .await;
        let (used_model, mut result) = match result {
            Ok(done) => done,
            Err(error) if progress.cancel.is_cancelled() || is_cancelled(&error) => {
                return Err(CoordinatorError::Cancelled(self.turn.id.clone()))
            }
            // A failed native lane falls back to a configured summary endpoint, never to the
            // ordinary summarizer.
            Err(_) if self.compaction.remote_endpoint.is_some() => {
                return self
                    .endpoint_draft(context, body, prep, req, measure, mode, progress)
                    .await;
            }
            Err(error) => return Err(into_coordinator(error, &self.turn.id)),
        };
        result.snapshot_through_seq = snapshot_through;
        let mut adjusted = prep.clone();
        if anthropic {
            harness_compaction::files::collect_file_ops(
                &context.items_with(&self.counter)[prep.recent.start..end - body],
                &mut adjusted.file_ops,
            );
            adjusted.recent.start = end - body;
        }
        let summary = result.summary.clone().unwrap_or_else(|| {
            if result.used_tokens > 0 {
                format!(
                    "{NATIVE_SUMMARY} Compaction processed {} input tokens.",
                    result.used_tokens
                )
            } else {
                NATIVE_SUMMARY.to_owned()
            }
        });
        let mut draft = self.text_draft(
            context,
            body,
            &adjusted,
            summary,
            Some("Remote compaction".into()),
            CompactionMethod::Remote,
            measure,
            result.usage.clone(),
            Some((model.provider_id.clone(), used_model.clone())),
        );
        draft.cut = end;
        draft.replay_through_seq = result.snapshot_through_seq;
        // The commit stores the payload privately and fills in the reference and protocol.
        draft.state.native = Some(NativeCompactionMeta {
            lane: lane_name(result.lane).into(),
            provider_id: Some(model.provider_id),
            model_id: Some(used_model),
            api: None,
            payload: None,
        });
        if let Some(hook) = hook {
            draft.state.preserve_data = super::commit::merge_preserve(
                &draft.state.preserve_data,
                hook.preserve_data,
                std::collections::BTreeMap::new(),
            );
        }
        if result.lane == NativeCompactionLane::Anthropic {
            let block = harness_compaction::files::render_files_block(
                &draft.state.read_files,
                &draft.state.modified_files,
                &adjusted.file_ops.read,
            );
            let block = self
                .handle
                .call(move |runtime| Ok(runtime.redactor.redact_text(&block)))
                .await?;
            result
                .replay
                .set_anthropic_files(block, pending.saturating_sub(end), retained_files)
                .map_err(|error| CoordinatorError::Invalid(error.into()))?;
        }
        draft.continuation = Some(result.replay);
        Ok(Some(draft))
    }
}

#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;
