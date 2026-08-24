use harness_core::config::{ModelLimitProvenance, ResolvedModelLimits};
use harness_core::context_budget::{
    compute_request_budget, BudgetStatus, RequestBudgetError, RequestBudgetInput,
};
use harness_core::UnwrapOrAbort;
use harness_providers::ProviderOutputCapDisposition;

use super::common::{budget_input, components, known_limits};

#[test]
fn unknown_limits_preserve_components_without_capacity_claims() {
    // arrange
    let limits = ResolvedModelLimits::default();
    let input = RequestBudgetInput {
        requested_output_tokens: Some(64),
        request_cost: components([1, 2, 3, 4, 5, 6]),
        model_limits: &limits,
        ..budget_input(&limits)
    };

    // act
    let budget = compute_request_budget(input).unwrap_or_abort();

    // assert
    assert_eq!(budget.status, BudgetStatus::UnknownLimits);
    assert_eq!(budget.requested_output_tokens, Some(64));
    assert_eq!(budget.reserved_output_tokens, None);
    assert_eq!(budget.maximum_input_tokens, None);
    assert_eq!(budget.compaction_threshold_tokens, None);
    assert_eq!(budget.occupied_input_tokens, 21);
    assert_eq!(budget.remaining_input_tokens, None);
    assert_eq!(budget.requires_compaction, None);
}

#[test]
fn unknown_provider_default_does_not_invent_output_reservation() {
    // arrange
    let limits = ResolvedModelLimits::default();
    let input = RequestBudgetInput {
        output_cap_disposition: ProviderOutputCapDisposition::ProviderDefaulted(4_096),
        model_limits: &limits,
        ..budget_input(&limits)
    };

    // act
    let budget = compute_request_budget(input).unwrap_or_abort();

    // assert
    assert_eq!(budget.status, BudgetStatus::UnknownLimits);
    assert_eq!(budget.requested_output_tokens, None);
    assert_eq!(budget.reserved_output_tokens, None);
    assert_eq!(
        budget.output_cap_disposition,
        ProviderOutputCapDisposition::ProviderDefaulted(4_096)
    );
}

#[test]
fn conservative_fallback_is_labeled_and_can_require_compaction() {
    // arrange
    let limits = ResolvedModelLimits::default();
    let input = RequestBudgetInput {
        request_cost: components([100, 100, 100, 100, 0, 0]),
        estimated_token_triggers: true,
        fallback_input_tokens: 500,
        model_limits: &limits,
        ..budget_input(&limits)
    };

    // act
    let budget = compute_request_budget(input).unwrap_or_abort();

    // assert
    assert_eq!(budget.status, BudgetStatus::ConservativeFallback);
    assert_eq!(budget.reserved_output_tokens, None);
    assert_eq!(budget.maximum_input_tokens, Some(500));
    assert_eq!(budget.compaction_threshold_tokens, Some(400));
    assert_eq!(budget.remaining_input_tokens, Some(0));
    assert_eq!(budget.requires_compaction, Some(true));
}

#[test]
fn conservative_zero_fallback_remains_unknown() {
    // arrange
    let limits = ResolvedModelLimits::default();
    let input = RequestBudgetInput {
        estimated_token_triggers: true,
        fallback_input_tokens: 0,
        model_limits: &limits,
        ..budget_input(&limits)
    };

    // act
    let budget = compute_request_budget(input).unwrap_or_abort();

    // assert
    assert_eq!(budget.status, BudgetStatus::UnknownLimits);
    assert_eq!(budget.maximum_input_tokens, None);
    assert_eq!(budget.requires_compaction, None);
}

#[test]
fn conservative_exhausted_budget_returns_typed_error() {
    // arrange
    let limits = ResolvedModelLimits::default();
    let input = RequestBudgetInput {
        safety_margin_tokens: 500,
        estimated_token_triggers: true,
        fallback_input_tokens: 500,
        model_limits: &limits,
        ..budget_input(&limits)
    };

    // act
    let result = compute_request_budget(input);

    // assert
    assert_eq!(
        result,
        Err(RequestBudgetError::NoUsableInputBudget {
            maximum_input_tokens: 500,
            safety_margin_tokens: 500,
        })
    );
}

#[test]
fn malformed_partial_limits_return_typed_error() {
    // arrange
    let limits = ResolvedModelLimits::from_values(
        Some(1_000),
        None,
        None,
        ModelLimitProvenance::explicit("test"),
    );

    // act
    let result = compute_request_budget(budget_input(&limits));

    // assert
    assert_eq!(result, Err(RequestBudgetError::PartialModelLimits));
}

#[test]
fn malformed_zero_context_returns_typed_error() {
    // arrange
    let limits = known_limits(0, None, 1);

    // act
    let result = compute_request_budget(budget_input(&limits));

    // assert
    assert_eq!(result, Err(RequestBudgetError::ZeroContextWindow));
}

#[test]
fn malformed_zero_output_returns_typed_error() {
    // arrange
    let limits = known_limits(100, None, 0);

    // act
    let result = compute_request_budget(budget_input(&limits));

    // assert
    assert_eq!(result, Err(RequestBudgetError::ZeroMaximumOutput));
}

#[test]
fn malformed_zero_physical_input_has_no_usable_budget() {
    // arrange
    let limits = known_limits(100, Some(0), 10);

    // act
    let result = compute_request_budget(budget_input(&limits));

    // assert
    assert_eq!(
        result,
        Err(RequestBudgetError::NoUsableInputBudget {
            maximum_input_tokens: 0,
            safety_margin_tokens: 100,
        })
    );
}

#[test]
fn underflow_output_reservation_returns_typed_error() {
    // arrange
    let limits = known_limits(100, None, 101);

    // act
    let result = compute_request_budget(budget_input(&limits));

    // assert
    assert_eq!(
        result,
        Err(RequestBudgetError::OutputReservationExceedsWindow {
            reserved_output_tokens: 101,
            context_window_tokens: 100,
        })
    );
}
