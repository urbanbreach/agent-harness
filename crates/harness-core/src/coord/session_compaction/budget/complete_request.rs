use harness_providers::CompletionUsage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestTerminalStatus {
    Completed,
    Aborted,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AnchorBudgetComponents {
    pub(crate) system_tokens: u32,
    pub(crate) tools_tokens: u32,
    pub(crate) attachments_tokens: u32,
    pub(crate) framing_tokens: u32,
}

impl AnchorBudgetComponents {
    fn repeated_input_tokens(self) -> u32 {
        [
            self.system_tokens,
            self.tools_tokens,
            self.attachments_tokens,
            self.framing_tokens,
        ]
        .into_iter()
        .fold(0, u32::saturating_add)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct StartedRequestMetadata<'a> {
    pub(crate) request_id: &'a str,
    pub(crate) provider_id: &'a str,
    pub(crate) model_id: &'a str,
    pub(crate) budget: Option<AnchorBudgetComponents>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct UsageCandidate<'a> {
    pub(crate) terminal_status: RequestTerminalStatus,
    pub(crate) request_id: &'a str,
    pub(crate) provider_id: &'a str,
    pub(crate) model_id: &'a str,
    pub(crate) semantic_usage: Option<&'a CompletionUsage>,
    pub(crate) finished_usage: Option<&'a CompletionUsage>,
    pub(crate) started: Option<StartedRequestMetadata<'a>>,
    pub(crate) through_index: usize,
    pub(crate) includes_prior_summary: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CurrentRequestModel<'a> {
    pub(crate) provider_id: &'a str,
    pub(crate) model_id: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UsageAnchor {
    pub(crate) through_index: usize,
    pub(crate) usage_total_tokens: u32,
    pub(crate) anchored_history_tokens: u32,
    pub(crate) includes_prior_summary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UsageAnchorInvalidReason {
    MissingRequestMetadata,
    RequestMetadataMismatch,
    ProviderModelMismatch,
    MissingBudgetMetadata,
    UsageMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UsageAnchorResolution {
    Valid(UsageAnchor),
    Missing,
    Invalid(UsageAnchorInvalidReason),
}

pub(crate) fn resolve_usage_anchor(
    candidates: &[UsageCandidate<'_>],
    current: CurrentRequestModel<'_>,
) -> UsageAnchorResolution {
    let mut newest_invalid = None;
    for candidate in candidates.iter().rev() {
        if candidate.terminal_status != RequestTerminalStatus::Completed {
            continue;
        }
        let Some(started) = candidate.started else {
            newest_invalid.get_or_insert(UsageAnchorInvalidReason::MissingRequestMetadata);
            continue;
        };
        if candidate.request_id != started.request_id {
            newest_invalid.get_or_insert(UsageAnchorInvalidReason::RequestMetadataMismatch);
            continue;
        }
        let matching_started_model =
            candidate.provider_id == started.provider_id && candidate.model_id == started.model_id;
        let matching_current_model =
            candidate.provider_id == current.provider_id && candidate.model_id == current.model_id;
        if !matching_started_model || !matching_current_model {
            newest_invalid.get_or_insert(UsageAnchorInvalidReason::ProviderModelMismatch);
            continue;
        }
        let Some(budget) = started.budget else {
            newest_invalid.get_or_insert(UsageAnchorInvalidReason::MissingBudgetMetadata);
            continue;
        };
        let (Some(semantic_usage), Some(finished_usage)) =
            (candidate.semantic_usage, candidate.finished_usage)
        else {
            newest_invalid.get_or_insert(UsageAnchorInvalidReason::UsageMismatch);
            continue;
        };
        if semantic_usage.prompt_tokens != finished_usage.prompt_tokens
            || semantic_usage.completion_tokens != finished_usage.completion_tokens
            || semantic_usage.total_tokens != finished_usage.total_tokens
        {
            newest_invalid.get_or_insert(UsageAnchorInvalidReason::UsageMismatch);
            continue;
        }
        return UsageAnchorResolution::Valid(UsageAnchor {
            through_index: candidate.through_index,
            usage_total_tokens: semantic_usage.total_tokens,
            anchored_history_tokens: semantic_usage
                .total_tokens
                .saturating_sub(budget.repeated_input_tokens()),
            includes_prior_summary: candidate.includes_prior_summary,
        });
    }
    newest_invalid.map_or(
        UsageAnchorResolution::Missing,
        UsageAnchorResolution::Invalid,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompleteRequestComponents {
    pub(crate) system_tokens: u32,
    pub(crate) tools_tokens: u32,
    pub(crate) attachments_tokens: u32,
    pub(crate) framing_tokens: u32,
    pub(crate) pending_prompt_tokens: u32,
    pub(crate) requested_completion_tokens: u32,
    pub(crate) compaction_threshold_tokens: Option<u32>,
}

impl CompleteRequestComponents {
    fn fixed_input_tokens(self) -> u32 {
        [
            self.system_tokens,
            self.tools_tokens,
            self.attachments_tokens,
            self.framing_tokens,
            self.pending_prompt_tokens,
        ]
        .into_iter()
        .fold(0, u32::saturating_add)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompactionHistoryTokens {
    pub(crate) anchored_tokens: u32,
    pub(crate) trailing_tokens: u32,
    pub(crate) prior_summary_tokens: u32,
    pub(crate) anchor_includes_prior_summary: bool,
    pub(crate) retained_tokens: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompleteRequestBudget {
    pub(crate) fixed_input_tokens: u32,
    pub(crate) history_tokens_before: u32,
    pub(crate) pre_input_tokens: u32,
    pub(crate) pre_total_tokens: u32,
    pub(crate) retained_history_tokens: u32,
    pub(crate) summary_allowance_tokens: u32,
    pub(crate) post_input_tokens: u32,
    pub(crate) post_total_tokens: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompleteRequestBudgetError {
    RetainedHistoryExceedsKeep,
    NoSummaryAllowance,
}

pub(crate) fn plan_complete_request(
    components: CompleteRequestComponents,
    history: CompactionHistoryTokens,
    keep_recent_tokens: u32,
) -> Result<CompleteRequestBudget, CompleteRequestBudgetError> {
    if history.retained_tokens > keep_recent_tokens {
        return Err(CompleteRequestBudgetError::RetainedHistoryExceedsKeep);
    }
    let fixed_input_tokens = components.fixed_input_tokens();
    let anchored_tokens = if history.anchor_includes_prior_summary {
        history
            .anchored_tokens
            .saturating_sub(history.prior_summary_tokens)
    } else {
        history.anchored_tokens
    };
    let history_tokens_before = [
        anchored_tokens,
        history.trailing_tokens,
        history.prior_summary_tokens,
    ]
    .into_iter()
    .fold(0, u32::saturating_add);
    let pre_input_tokens = fixed_input_tokens.saturating_add(history_tokens_before);
    let pre_total_tokens = pre_input_tokens.saturating_add(components.requested_completion_tokens);
    let occupied_after_cut = fixed_input_tokens.saturating_add(history.retained_tokens);
    let summary_allowance_tokens = components.compaction_threshold_tokens.map_or_else(
        || {
            history_tokens_before
                .saturating_sub(history.retained_tokens)
                .saturating_sub(1)
                .min(components.requested_completion_tokens)
        },
        |threshold| {
            threshold
                .saturating_sub(occupied_after_cut)
                .min(components.requested_completion_tokens)
        },
    );
    if summary_allowance_tokens == 0 {
        return Err(CompleteRequestBudgetError::NoSummaryAllowance);
    }
    let post_input_tokens = occupied_after_cut.saturating_add(summary_allowance_tokens);
    let post_total_tokens =
        post_input_tokens.saturating_add(components.requested_completion_tokens);
    Ok(CompleteRequestBudget {
        fixed_input_tokens,
        history_tokens_before,
        pre_input_tokens,
        pre_total_tokens,
        retained_history_tokens: history.retained_tokens,
        summary_allowance_tokens,
        post_input_tokens,
        post_total_tokens,
    })
}
