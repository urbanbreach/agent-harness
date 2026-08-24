use harness_providers::ProviderOutputCapDisposition;
use serde::{Deserialize, Serialize};

use super::{BudgetStatus, RequestBudget, RequestBudgetComponents};

/// Redacted request-budget values safe for durable event and runtime metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestBudgetSnapshot {
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
    pub const fn snapshot(&self) -> RequestBudgetSnapshot {
        RequestBudgetSnapshot {
            status: self.status,
            requested_output_tokens: self.requested_output_tokens,
            reserved_output_tokens: self.reserved_output_tokens,
            maximum_input_tokens: self.maximum_input_tokens,
            safety_margin_tokens: self.safety_margin_tokens,
            compaction_threshold_tokens: self.compaction_threshold_tokens,
            components: self.components,
            occupied_input_tokens: self.occupied_input_tokens,
            remaining_input_tokens: self.remaining_input_tokens,
            requires_compaction: self.requires_compaction,
            output_cap_disposition: self.output_cap_disposition,
        }
    }
}

impl From<RequestBudget> for RequestBudgetSnapshot {
    fn from(budget: RequestBudget) -> Self {
        budget.snapshot()
    }
}
