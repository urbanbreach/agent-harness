use harness_core::config::{ModelLimitProvenance, ResolvedModelLimits};
use harness_core::context_budget::RequestBudgetInput;
use harness_providers::{ProviderOutputCapDisposition, ProviderRequestCost};

pub(super) fn known_limits(
    context: u32,
    maximum_input: Option<u32>,
    maximum_output: u32,
) -> ResolvedModelLimits {
    ResolvedModelLimits::from_values(
        Some(context),
        maximum_input,
        Some(maximum_output),
        ModelLimitProvenance::explicit("test"),
    )
}

pub(super) fn budget_input(limits: &ResolvedModelLimits) -> RequestBudgetInput<'_> {
    RequestBudgetInput {
        model_limits: limits,
        request_cost: ProviderRequestCost::default(),
        requested_output_tokens: None,
        safety_margin_tokens: 100,
        estimated_token_triggers: false,
        fallback_input_tokens: 0,
        output_cap_disposition: ProviderOutputCapDisposition::UnspecifiedUnknownLimit,
    }
}

pub(super) fn components(values: [u32; 6]) -> ProviderRequestCost {
    ProviderRequestCost {
        system_tokens: values[0],
        tools_tokens: values[1],
        history_tokens: values[2],
        attachments_tokens: values[3],
        framing_tokens: values[4],
        pending_prompt_tokens: values[5],
    }
}
