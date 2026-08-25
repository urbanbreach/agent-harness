use crate::context_budget::RequestBudgetSnapshot;
use crate::coord::compaction::{
    estimate_text_tokens, estimate_typed_entries_tokens, ActivePathCompactionSnapshot,
    TypedCutPointPlan,
};
use crate::event::EventEnvelopeV1;

#[path = "budget/complete_request.rs"]
mod complete_request;
#[path = "budget/usage_candidates.rs"]
mod usage_candidates;

use complete_request::{
    plan_complete_request, resolve_usage_anchor, CompactionHistoryTokens,
    CompleteRequestComponents, CurrentRequestModel, UsageAnchorResolution,
};
pub(super) use complete_request::{CompleteRequestBudget, CompleteRequestBudgetError};
use usage_candidates::{
    event_usage_candidates, latest_request_budget, latest_request_start, snapshot_usage_candidates,
};

#[derive(Debug, Clone, Copy)]
pub(super) struct CompactionBudget {
    request: Option<RequestBudgetSnapshot>,
    usage_anchor: UsageAnchorResolution,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct CompactionBudgetPlanInput<'a> {
    pub(super) snapshot: &'a ActivePathCompactionSnapshot,
    pub(super) cut: &'a TypedCutPointPlan,
    pub(super) keep_recent_tokens: u32,
}

impl CompactionBudget {
    pub(super) fn resolve(
        prepared: Option<RequestBudgetSnapshot>,
        events: &[EventEnvelopeV1],
        agent_id: &str,
    ) -> Self {
        let request = prepared.or_else(|| latest_request_budget(events, agent_id));
        let current = latest_request_start(events, agent_id);
        let usage_anchor = current.map_or(UsageAnchorResolution::Missing, |current| {
            let candidates = event_usage_candidates(events, agent_id);
            resolve_usage_anchor(
                &candidates,
                CurrentRequestModel {
                    provider_id: &current.provider_id,
                    model_id: &current.model_id,
                },
            )
        });
        Self {
            request,
            usage_anchor,
        }
    }

    pub(super) fn resolve_for_snapshot(
        request: RequestBudgetSnapshot,
        events: &[EventEnvelopeV1],
        snapshot: &ActivePathCompactionSnapshot,
    ) -> Self {
        let candidates = snapshot_usage_candidates(events, snapshot);
        let usage_anchor = resolve_usage_anchor(
            &candidates,
            CurrentRequestModel {
                provider_id: &snapshot.current_model.provider_id,
                model_id: &snapshot.current_model.model_id,
            },
        );
        Self {
            request: Some(request),
            usage_anchor,
        }
    }

    pub(super) fn requires_compaction(self) -> bool {
        self.request
            .and_then(|snapshot| snapshot.requires_compaction)
            == Some(true)
    }

    pub(super) fn request_snapshot(self) -> Option<RequestBudgetSnapshot> {
        self.request
    }

    pub(super) fn history_allowance(self, keep_recent_tokens: u32, reserve_summary: bool) -> u32 {
        let Some(snapshot) = self.request else {
            return keep_recent_tokens;
        };
        let Some(threshold) = snapshot.compaction_threshold_tokens else {
            return keep_recent_tokens;
        };
        let components = snapshot.components;
        let non_history_tokens = [
            components.system_tokens,
            components.tools_tokens,
            components.attachments_tokens,
            components.framing_tokens,
            components.pending_prompt_tokens,
        ]
        .into_iter()
        .fold(0_u32, u32::saturating_add);
        let requested_summary_reserve = snapshot
            .reserved_output_tokens
            .or(snapshot.requested_output_tokens)
            .unwrap_or(snapshot.safety_margin_tokens);
        let available_tokens = threshold.saturating_sub(non_history_tokens);
        if !reserve_summary {
            return keep_recent_tokens.min(available_tokens);
        }
        let summary_reserve = requested_summary_reserve.min(available_tokens.saturating_sub(1));
        keep_recent_tokens.min(available_tokens.saturating_sub(summary_reserve))
    }

    pub(super) fn complete_request_plan(
        self,
        input: CompactionBudgetPlanInput<'_>,
    ) -> Result<CompleteRequestBudget, CompleteRequestBudgetError> {
        let Some(request) = self.request else {
            return Err(CompleteRequestBudgetError::NoSummaryAllowance);
        };
        let (anchored_tokens, anchor_includes_prior_summary, trailing_entries) =
            match self.usage_anchor {
                UsageAnchorResolution::Valid(anchor) => (
                    anchor.anchored_history_tokens,
                    anchor.includes_prior_summary,
                    input
                        .snapshot
                        .entries
                        .get(anchor.through_index.saturating_add(1)..)
                        .unwrap_or(input.snapshot.entries.as_slice()),
                ),
                UsageAnchorResolution::Missing | UsageAnchorResolution::Invalid(_) => {
                    (0, false, input.snapshot.entries.as_slice())
                }
            };
        let prior_summary_tokens = input
            .snapshot
            .prior_active_summary
            .as_ref()
            .map_or(0, |summary| estimate_text_tokens(&summary.summary));
        plan_complete_request(
            CompleteRequestComponents {
                system_tokens: request.components.system_tokens,
                tools_tokens: request.components.tools_tokens,
                attachments_tokens: request.components.attachments_tokens,
                framing_tokens: request.components.framing_tokens,
                pending_prompt_tokens: request.components.pending_prompt_tokens,
                requested_completion_tokens: request
                    .reserved_output_tokens
                    .or(request.requested_output_tokens)
                    .unwrap_or(request.safety_margin_tokens),
                compaction_threshold_tokens: request.compaction_threshold_tokens,
            },
            CompactionHistoryTokens {
                anchored_tokens,
                trailing_tokens: estimate_typed_entries_tokens(trailing_entries),
                prior_summary_tokens,
                anchor_includes_prior_summary,
                retained_tokens: input.cut.retained_tokens,
            },
            input.keep_recent_tokens,
        )
    }
}
