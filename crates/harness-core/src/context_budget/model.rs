use harness_providers::{ProviderOutputCapDisposition, ProviderRequestCost};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::ResolvedModelLimits;

#[derive(Debug, Clone, Copy)]
pub struct RequestBudgetInput<'a> {
    pub model_limits: &'a ResolvedModelLimits,
    pub request_cost: ProviderRequestCost,
    pub requested_output_tokens: Option<u32>,
    pub safety_margin_tokens: u32,
    pub estimated_token_triggers: bool,
    pub fallback_input_tokens: u32,
    pub output_cap_disposition: ProviderOutputCapDisposition,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestBudgetComponents {
    pub system_tokens: u32,
    pub tools_tokens: u32,
    pub history_tokens: u32,
    pub attachments_tokens: u32,
    pub framing_tokens: u32,
    pub pending_prompt_tokens: u32,
}

impl RequestBudgetComponents {
    pub(super) fn checked_occupied(self) -> Result<u32, RequestBudgetError> {
        [
            self.system_tokens,
            self.tools_tokens,
            self.history_tokens,
            self.attachments_tokens,
            self.framing_tokens,
            self.pending_prompt_tokens,
        ]
        .into_iter()
        .try_fold(0_u32, |total, component| {
            total
                .checked_add(component)
                .ok_or(RequestBudgetError::ComponentArithmeticOverflow)
        })
    }
}

impl From<ProviderRequestCost> for RequestBudgetComponents {
    fn from(cost: ProviderRequestCost) -> Self {
        Self {
            system_tokens: cost.system_tokens,
            tools_tokens: cost.tools_tokens,
            history_tokens: cost.history_tokens,
            attachments_tokens: cost.attachments_tokens,
            framing_tokens: cost.framing_tokens,
            pending_prompt_tokens: cost.pending_prompt_tokens,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetStatus {
    Estimated,
    ConservativeFallback,
    UnknownLimits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RequestBudgetError {
    #[error("model limits must provide context and output together, or neither")]
    PartialModelLimits,
    #[error("model context window must be greater than zero")]
    ZeroContextWindow,
    #[error("model maximum output must be greater than zero")]
    ZeroMaximumOutput,
    #[error(
        "reserved output {reserved_output_tokens} exceeds context window {context_window_tokens}"
    )]
    OutputReservationExceedsWindow {
        reserved_output_tokens: u32,
        context_window_tokens: u32,
    },
    #[error(
        "maximum input {maximum_input_tokens} has no usable capacity after safety margin {safety_margin_tokens}"
    )]
    NoUsableInputBudget {
        maximum_input_tokens: u32,
        safety_margin_tokens: u32,
    },
    #[error("request component token arithmetic overflowed")]
    ComponentArithmeticOverflow,
}
