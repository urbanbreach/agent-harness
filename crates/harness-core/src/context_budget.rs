use crate::config::ResolvedModelLimits;
use harness_providers::{ProviderOutputCapDisposition, ProviderRequestCost};
use serde::{Deserialize, Serialize};

pub type RequestBudgetComponents = ProviderRequestCost;
pub type RequestBudgetSnapshot = RequestBudget;

pub struct RequestBudgetInput<'a> {
    pub model_limits: &'a ResolvedModelLimits,
    pub request_cost: ProviderRequestCost,
    pub requested_output_tokens: Option<u32>,
    pub safety_margin_tokens: u32,
    pub estimated_token_triggers: bool,
    pub fallback_input_tokens: u32,
    pub output_cap_disposition: ProviderOutputCapDisposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetStatus {
    Estimated,
    ConservativeFallback,
    UnknownLimits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestBudget {
    pub status: BudgetStatus,
    pub requested_output_tokens: Option<u32>,
    pub reserved_output_tokens: Option<u32>,
    pub maximum_input_tokens: Option<u32>,
    pub safety_margin_tokens: u32,
    pub compaction_threshold_tokens: Option<u32>,
    pub components: RequestBudgetComponents,
    pub occupied_input_tokens: u32,
    pub remaining_input_tokens: Option<u32>,
    pub requires_compaction: Option<bool>,
    pub output_cap_disposition: ProviderOutputCapDisposition,
}
impl RequestBudget {
    pub fn snapshot(&self) -> RequestBudgetSnapshot {
        *self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RequestBudgetError {
    #[error("model limits must include context and output together")]
    PartialModelLimits,
    #[error("context window must be positive")]
    ZeroContextWindow,
    #[error("output limit must be positive")]
    ZeroMaximumOutput,
    #[error("output reservation exceeds context window")]
    OutputReservationExceedsWindow {
        reserved_output_tokens: u32,
        context_window_tokens: u32,
    },
    #[error("input budget has no space after its safety margin")]
    NoUsableInputBudget {
        maximum_input_tokens: u32,
        safety_margin_tokens: u32,
    },
    #[error("request cost overflow")]
    ComponentArithmeticOverflow,
}

pub fn compute_request_budget(
    input: RequestBudgetInput<'_>,
) -> Result<RequestBudget, RequestBudgetError> {
    let occupied = input
        .request_cost
        .total_input_tokens()
        .map_err(|_| RequestBudgetError::ComponentArithmeticOverflow)?;
    let limits = input.model_limits;
    let (status, maximum, reserved, requested) = match (
        limits.context_window_tokens(),
        limits.max_input_tokens(),
        limits.max_output_tokens(),
    ) {
        (Some(0), _, _) => return Err(RequestBudgetError::ZeroContextWindow),
        (_, _, Some(0)) => return Err(RequestBudgetError::ZeroMaximumOutput),
        (Some(context), physical_input, Some(output)) => {
            let requested = input.requested_output_tokens.unwrap_or(output);
            if requested == 0 {
                return Err(RequestBudgetError::ZeroMaximumOutput);
            }
            let reserved = requested.min(output);
            let available = context.checked_sub(reserved).ok_or(
                RequestBudgetError::OutputReservationExceedsWindow {
                    reserved_output_tokens: reserved,
                    context_window_tokens: context,
                },
            )?;
            (
                BudgetStatus::Estimated,
                Some(physical_input.unwrap_or(available).min(available)),
                Some(reserved),
                Some(requested),
            )
        }
        (None, None, None) if input.estimated_token_triggers && input.fallback_input_tokens > 0 => {
            (
                BudgetStatus::ConservativeFallback,
                Some(input.fallback_input_tokens),
                None,
                input.requested_output_tokens,
            )
        }
        (None, None, None) => (
            BudgetStatus::UnknownLimits,
            None,
            None,
            input.requested_output_tokens,
        ),
        _ => return Err(RequestBudgetError::PartialModelLimits),
    };
    let threshold = maximum
        .map(|n| {
            n.checked_sub(input.safety_margin_tokens)
                .filter(|n| *n > 0)
                .ok_or(RequestBudgetError::NoUsableInputBudget {
                    maximum_input_tokens: n,
                    safety_margin_tokens: input.safety_margin_tokens,
                })
        })
        .transpose()?;
    Ok(RequestBudget {
        status,
        requested_output_tokens: requested,
        reserved_output_tokens: reserved,
        maximum_input_tokens: maximum,
        safety_margin_tokens: input.safety_margin_tokens,
        compaction_threshold_tokens: threshold,
        components: input.request_cost,
        occupied_input_tokens: occupied,
        remaining_input_tokens: threshold.map(|n| n.saturating_sub(occupied)),
        requires_compaction: threshold.map(|n| occupied >= n),
        output_cap_disposition: input.output_cap_disposition,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_reserves_output_and_leaves_unknown_capacity_unknown(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let known = ResolvedModelLimits::compatibility_mirror(Some(10_000), Some(8000), Some(3000));
        let unknown = ResolvedModelLimits::default();
        for (limits, fallback, expected) in [
            (&known, false, Some(7900)),
            (&unknown, false, None),
            (&unknown, true, Some(3996)),
        ] {
            let budget = compute_request_budget(RequestBudgetInput {
                model_limits: limits,
                request_cost: ProviderRequestCost {
                    history_tokens: 500,
                    ..Default::default()
                },
                requested_output_tokens: Some(2000),
                safety_margin_tokens: 100,
                estimated_token_triggers: fallback,
                fallback_input_tokens: 4096,
                output_cap_disposition: ProviderOutputCapDisposition::Emitted(2000),
            })?;
            assert_eq!(budget.compaction_threshold_tokens, expected);
            assert_eq!(budget.remaining_input_tokens, expected.map(|n| n - 500));
            assert_eq!(budget.requires_compaction, expected.map(|_| false));
        }
        known.validate("fixture")?;
        unknown.validate("fixture")?;
        for values in [
            (Some(0), None, Some(1)),
            (Some(100), None, None),
            (Some(100), Some(101), Some(20)),
        ] {
            assert!(
                ResolvedModelLimits::compatibility_mirror(values.0, values.1, values.2)
                    .validate("fixture")
                    .is_err()
            );
        }
        Ok(())
    }
}
