use super::{
    BudgetStatus, RequestBudget, RequestBudgetComponents, RequestBudgetError, RequestBudgetInput,
};

pub fn compute_request_budget(
    input: RequestBudgetInput<'_>,
) -> Result<RequestBudget, RequestBudgetError> {
    let components = RequestBudgetComponents::from(input.request_cost);
    let occupied_input_tokens = components.checked_occupied()?;
    let limits = input.model_limits;

    match (
        limits.context_window_tokens(),
        limits.max_input_tokens(),
        limits.max_output_tokens(),
    ) {
        (None, None, None) => {
            if input.estimated_token_triggers && input.fallback_input_tokens > 0 {
                let compaction_threshold_tokens =
                    usable_threshold(input.fallback_input_tokens, input.safety_margin_tokens)?;
                return Ok(RequestBudget {
                    status: BudgetStatus::ConservativeFallback,
                    requested_output_tokens: input.requested_output_tokens,
                    reserved_output_tokens: None,
                    maximum_input_tokens: Some(input.fallback_input_tokens),
                    safety_margin_tokens: input.safety_margin_tokens,
                    compaction_threshold_tokens: Some(compaction_threshold_tokens),
                    components,
                    occupied_input_tokens,
                    remaining_input_tokens: Some(
                        compaction_threshold_tokens.saturating_sub(occupied_input_tokens),
                    ),
                    requires_compaction: Some(occupied_input_tokens >= compaction_threshold_tokens),
                    output_cap_disposition: input.output_cap_disposition,
                });
            }

            Ok(RequestBudget {
                status: BudgetStatus::UnknownLimits,
                requested_output_tokens: input.requested_output_tokens,
                reserved_output_tokens: None,
                maximum_input_tokens: None,
                safety_margin_tokens: input.safety_margin_tokens,
                compaction_threshold_tokens: None,
                components,
                occupied_input_tokens,
                remaining_input_tokens: None,
                requires_compaction: None,
                output_cap_disposition: input.output_cap_disposition,
            })
        }
        (Some(context_window_tokens), physical_maximum_input, Some(model_maximum_output)) => {
            if context_window_tokens == 0 {
                return Err(RequestBudgetError::ZeroContextWindow);
            }
            if model_maximum_output == 0 {
                return Err(RequestBudgetError::ZeroMaximumOutput);
            }

            let requested_output_tokens = input
                .requested_output_tokens
                .unwrap_or(model_maximum_output);
            let reserved_output_tokens = requested_output_tokens.min(model_maximum_output);
            let context_available_for_input = context_window_tokens
                .checked_sub(reserved_output_tokens)
                .ok_or(RequestBudgetError::OutputReservationExceedsWindow {
                    reserved_output_tokens,
                    context_window_tokens,
                })?;
            let maximum_input_tokens = physical_maximum_input
                .unwrap_or(context_available_for_input)
                .min(context_available_for_input);
            let compaction_threshold_tokens =
                usable_threshold(maximum_input_tokens, input.safety_margin_tokens)?;

            Ok(RequestBudget {
                status: BudgetStatus::Estimated,
                requested_output_tokens: Some(requested_output_tokens),
                reserved_output_tokens: Some(reserved_output_tokens),
                maximum_input_tokens: Some(maximum_input_tokens),
                safety_margin_tokens: input.safety_margin_tokens,
                compaction_threshold_tokens: Some(compaction_threshold_tokens),
                components,
                occupied_input_tokens,
                remaining_input_tokens: Some(
                    compaction_threshold_tokens.saturating_sub(occupied_input_tokens),
                ),
                requires_compaction: Some(occupied_input_tokens >= compaction_threshold_tokens),
                output_cap_disposition: input.output_cap_disposition,
            })
        }
        (None | Some(_), None | Some(_), None | Some(_)) => {
            Err(RequestBudgetError::PartialModelLimits)
        }
    }
}

fn usable_threshold(
    maximum_input_tokens: u32,
    safety_margin_tokens: u32,
) -> Result<u32, RequestBudgetError> {
    match maximum_input_tokens.checked_sub(safety_margin_tokens) {
        Some(threshold @ 1..) => Ok(threshold),
        None | Some(0) => Err(RequestBudgetError::NoUsableInputBudget {
            maximum_input_tokens,
            safety_margin_tokens,
        }),
    }
}
