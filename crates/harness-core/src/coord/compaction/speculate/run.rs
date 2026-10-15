//! The background pass: slot registration, the isolated worker, and the draft it prepares.
use super::*;
use crate::coord::compaction::commit::merge_preserve;
use crate::coord::compaction::generate::{SummaryInputs, Transport, WalkMode};
use crate::coord::compaction::maintain::methods::{draft::previous_preserve, hook_abort};
use crate::coord::compaction::maintain::TurnPhase;
use crate::coord::compaction::progress::Progress;
use harness_compaction::prompts;

impl Worker {
    pub(super) async fn start_speculation(
        &self,
        context: &Context,
        measure: &Measure,
        method: CompactionMethod,
        pending: Option<usize>,
    ) -> Result<(), CoordinatorError> {
        let agent = self.agent_id();
        let token = CancellationToken::new();
        let (slot_agent, slot_cancel, model) =
            (agent.clone(), token.clone(), self.turn.model.clone());
        let prefix: Vec<u64> = context.entries.iter().map(|e| e.seq).collect();
        let tokens = measure.tokens;
        let (settled, _) = tokio::sync::watch::channel(false);
        let generation = self
            .handle
            .call(move |s| {
                cancel(s, &slot_agent);
                s.counter += 1;
                let generation = s.counter;
                s.speculation.insert(
                    slot_agent.clone(),
                    Speculation {
                        generation,
                        cancel: slot_cancel,
                        model,
                        tokens_at_start: tokens,
                        prefix,
                        armed: None,
                        settled,
                    },
                );
                s.refresh_speculation_status(&slot_agent);
                Ok(generation)
            })
            .await?;
        // The blacklist belongs to the live session; the isolated one is only for requests.
        let key = native_key(&self.session, &self.turn.model);
        let mut fork = self.fork(token.clone());
        // Speculative requests never share the live provider session or its cache state.
        fork.session = format!("{}:spec:{}", self.session, uuid::Uuid::now_v7().simple());
        let snapshot = context.clone();
        let handle = self.handle.clone();
        tokio::spawn(async move {
            let draft = fork.speculative_draft(&snapshot, method, pending).await;
            let aborted = token.is_cancelled();
            if let Err(error) = &draft
                && !aborted
            {
                tracing::debug!(%error, ?method, "speculative compaction failed");
            }
            let irrecoverable = method == CompactionMethod::Remote
                && !aborted
                && draft.as_ref().err().is_some_and(native_irrecoverable);
            let armed = match draft {
                Ok(Some(draft)) if !aborted => Some(Box::new(draft)),
                _ => None,
            };
            let _ = handle
                .call(move |s| {
                    let current = s.speculation.get(&agent).is_some_and(|slot| {
                        slot.generation == generation && !slot.cancel.is_cancelled()
                    });
                    if irrecoverable && current {
                        s.native_failed.insert(key);
                    }
                    arm(s, &agent, generation, armed);
                    Ok(())
                })
                .await;
        });
        Ok(())
    }

    /// The background summary itself; it never touches history.
    async fn speculative_draft(
        &self,
        context: &Context,
        method: CompactionMethod,
        pending: Option<usize>,
    ) -> Result<Option<Draft>, CoordinatorError> {
        let progress = Progress::silent(
            &self.handle,
            &self.actor,
            &self.turn.id,
            &self.agent_id(),
            self.cancellation.clone(),
        );
        let measure = self.measure(context)?;
        let Some((body, prep)) = self.prepare(context, pending) else {
            return Ok(None);
        };
        let model = crate::agent::AgentModelRef::parse(&self.turn.model);
        match method {
            CompactionMethod::Soft => {
                let hook = self
                    .run_summarizing_hook(&progress.cancel, &self.summarized_text(context, &prep))
                    .await
                    .map_err(hook_abort)?;
                let inputs = SummaryInputs {
                    prompt_override: hook.prompt.as_deref(),
                    extra_context: &hook.context,
                    focus: None,
                    reserve: policy::effective_reserve(
                        measure.window,
                        measure.settings.reserve_tokens,
                    ),
                };
                let output = self
                    .soft_summary(
                        context,
                        &prep,
                        &inputs,
                        WalkMode::Speculative,
                        Transport::Model,
                        method,
                        None,
                        &progress,
                    )
                    .await?;
                let mut draft = self.text_draft(
                    context,
                    body,
                    &prep,
                    output.summary,
                    output.short,
                    method,
                    &measure,
                    output.usage,
                    Some((output.provider_id, output.model_id)),
                );
                draft.state.preserve_data = merge_preserve(
                    &previous_preserve(context, body),
                    hook.preserve_data,
                    BTreeMap::new(),
                );
                Ok(Some(draft))
            }
            CompactionMethod::Handoff => {
                // The generator saves the document itself for an automatic trigger, so it is
                // kept on disk even when the draft is never committed.
                let document = self
                    .handoff_document(
                        context,
                        Some(prompts::auto_handoff_threshold_focus()),
                        true,
                        &progress,
                    )
                    .await?;
                if progress.cancel.is_cancelled() {
                    return Err(CoordinatorError::Cancelled(self.turn.id.clone()));
                }
                let Some(doc) = document else {
                    return Ok(None);
                };
                let saved_path = doc.saved_path.clone();
                let mut draft = self.text_draft(
                    context,
                    body,
                    &prep,
                    doc.text,
                    None,
                    method,
                    &measure,
                    doc.usage,
                    Some((model.provider_id, model.model_id)),
                );
                draft.saved_path = saved_path;
                Ok(Some(draft))
            }
            CompactionMethod::Remote => {
                let mut request = Request::automatic(Reason::Threshold, &[method], pending);
                request.phase = TurnPhase::Standalone;
                let mut draft = self
                    .remote_draft(
                        context,
                        body,
                        &prep,
                        &request,
                        &measure,
                        WalkMode::Speculative,
                        &progress,
                    )
                    .await?;
                if let Some(draft) = draft.as_mut() {
                    mark_replay_boundary(context, draft);
                }
                Ok(draft)
            }
            CompactionMethod::Archive | CompactionMethod::Shake => Ok(None),
        }
    }
}

/// A native Responses replacement history covers the snapshot up to the draft's cut; the
/// context that grows afterwards stays live on replay.
fn mark_replay_boundary(context: &Context, draft: &mut Draft) {
    let responses = draft
        .state
        .native
        .as_ref()
        .is_some_and(|meta| !meta.lane.starts_with("anthropic"));
    if responses && draft.replay_through_seq.is_none() {
        draft.replay_through_seq = draft
            .cut
            .checked_sub(1)
            .and_then(|last| context.entries.get(last))
            .map(|entry| entry.seq);
    }
}
