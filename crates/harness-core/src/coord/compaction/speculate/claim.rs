//! Claiming an armed speculative draft for a real pass.
use super::*;
use harness_providers::NativeCompactionLane;

fn lane_name(lane: NativeCompactionLane) -> &'static str {
    match lane {
        NativeCompactionLane::OpenAiV2 => "openai_v2",
        NativeCompactionLane::OpenAiV1 => "openai_v1",
        NativeCompactionLane::Anthropic => "anthropic",
    }
}

/// An armed result is thrown away when the real walk starts with a method that needs no
/// model summary (archive) or has no method at all.
pub(super) fn first_method_discards(first: Option<CompactionMethod>) -> bool {
    matches!(first, None | Some(CompactionMethod::Archive))
}

/// The snapshot is still the start of the live context and no compaction came after it.
pub(super) fn snapshot_applies(prefix: &[u64], seqs: &[u64], compaction_at: &[usize]) -> bool {
    seqs.starts_with(prefix) && !compaction_at.iter().any(|index| *index >= prefix.len())
}

/// Whether a result that the context outgrew still leaves enough room. `projected` is the
/// rebuilt context; `pending` counts messages not yet in it. The result must land in the
/// recovery band on a known window, below the current local size and below the trigger.
pub(super) fn grown_fits(
    projected: u32,
    pending: u32,
    threshold: u32,
    window_known: bool,
    local: u32,
    trigger: Option<u32>,
) -> bool {
    let total = projected.saturating_add(pending);
    if window_known && total > policy::recovery_target(threshold) {
        return false;
    }
    if local > 0 && total >= local.saturating_add(pending) {
        return false;
    }
    trigger
        .filter(|trigger| *trigger > 0)
        .is_none_or(|trigger| total < trigger)
}

impl Worker {
    /// The preserved remote result can still serve the live model and method settings.
    fn remote_reusable(&self, context: &Context, draft: &Draft) -> bool {
        if draft.state.method != CompactionMethod::Remote {
            return true;
        }
        if !self.compaction.remote_enabled {
            return false;
        }
        let Some(meta) = &draft.state.native else {
            return self.compaction.remote_endpoint.is_some();
        };
        let live = self.request(context);
        self.provider
            .native_compaction_lanes(&live, &self.native_lane_settings(&live))
            .into_iter()
            .any(|lane| lane_name(lane) == meta.lane)
    }

    /// Takes the agent's slot for a real pass. A running pass is aborted; an armed draft is
    /// returned when it still applies to this context and the walk does not start with a
    /// local archive. `trigger` is the occupancy that triggered the pass and `pending_tokens`
    /// the size of messages not yet in the context.
    pub(in crate::coord::compaction) async fn claim_speculation(
        &self,
        context: &Context,
        measure: &Measure,
        first: Option<CompactionMethod>,
        trigger: Option<u32>,
        pending_tokens: u32,
    ) -> Result<Option<Draft>, CoordinatorError> {
        let agent = self.agent_id();
        let model = self.turn.model.clone();
        let active_model = self.turn.target.is_some();
        let async_enabled = self.compaction.async_enabled;
        let seqs: Vec<u64> = context.entries.iter().map(|e| e.seq).collect();
        let compaction_at: Vec<usize> = context
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.compaction.is_some())
            .map(|(index, _)| index)
            .collect();
        let claimed = self
            .handle
            .call(move |s| {
                let Some(slot) = s.speculation.remove(&agent) else {
                    return Ok(None);
                };
                slot.cancel.cancel();
                s.refresh_speculation_status(&agent);
                let vetoable = s
                    .config
                    .hook_runtime_config
                    .hooks
                    .lifecycle
                    .iter()
                    .any(|h| h.event == Hook::CompactionRequested);
                let usable = async_enabled
                    && !vetoable
                    && active_model
                    && slot.model == model
                    && snapshot_applies(&slot.prefix, &seqs, &compaction_at);
                let last = slot.prefix.last().copied();
                Ok(slot
                    .armed
                    .filter(|_| usable)
                    .map(|draft| (draft, slot.prefix.len(), last)))
            })
            .await?;
        let Some((mut draft, snapshot_len, last_seq)) = claimed else {
            return Ok(None);
        };
        if first_method_discards(first) || !self.remote_reusable(context, &draft) {
            return Ok(None);
        }
        let grown = context.entries.len() > snapshot_len;
        if grown {
            let signed = draft
                .state
                .native
                .as_ref()
                .is_some_and(|meta| meta.lane.starts_with("anthropic"));
            if signed {
                // The signed prefix is readable as is; the new suffix stays outside it and
                // replay keeps it live.
                draft.replay_through_seq = draft.replay_through_seq.or(last_seq);
            } else {
                let projected = self.measure(&self.replacement(context, &draft))?;
                if !grown_fits(
                    projected.tokens,
                    pending_tokens,
                    measure.threshold,
                    measure.window_known,
                    measure.local,
                    trigger,
                ) {
                    return Ok(None);
                }
            }
        }
        draft.state.tokens_before = trigger
            .filter(|trigger| *trigger > 0)
            .unwrap_or(measure.tokens);
        Ok(Some(*draft))
    }
}
