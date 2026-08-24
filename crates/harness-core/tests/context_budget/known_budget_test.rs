use harness_core::context_budget::{
    compute_request_budget, BudgetStatus, RequestBudget, RequestBudgetError, RequestBudgetInput,
};
use harness_core::UnwrapOrAbort;
use harness_providers::ProviderOutputCapDisposition;
use serde_json::json;

use super::common::{budget_input, components, known_limits};

#[test]
fn known_default_output_and_physical_input_cap_are_exact() {
    // arrange
    let limits = known_limits(1_000, Some(700), 200);
    let input = RequestBudgetInput {
        model_limits: &limits,
        request_cost: components([10, 10, 10, 10, 10, 10]),
        requested_output_tokens: None,
        safety_margin_tokens: 100,
        estimated_token_triggers: false,
        fallback_input_tokens: 0,
        output_cap_disposition: ProviderOutputCapDisposition::Emitted(200),
    };

    // act
    let budget = compute_request_budget(input).unwrap_or_abort();

    // assert
    assert_eq!(
        budget,
        RequestBudget {
            status: BudgetStatus::Estimated,
            requested_output_tokens: Some(200),
            reserved_output_tokens: Some(200),
            maximum_input_tokens: Some(700),
            safety_margin_tokens: 100,
            compaction_threshold_tokens: Some(600),
            components: components([10, 10, 10, 10, 10, 10]).into(),
            occupied_input_tokens: 60,
            remaining_input_tokens: Some(540),
            requires_compaction: Some(false),
            output_cap_disposition: ProviderOutputCapDisposition::Emitted(200),
        }
    );
}

#[test]
fn known_requested_output_is_capped_by_model_maximum() {
    // arrange
    let limits = known_limits(1_000, None, 200);
    let input = RequestBudgetInput {
        requested_output_tokens: Some(500),
        model_limits: &limits,
        ..budget_input(&limits)
    };

    // act
    let budget = compute_request_budget(input).unwrap_or_abort();

    // assert
    assert_eq!(budget.requested_output_tokens, Some(500));
    assert_eq!(budget.reserved_output_tokens, Some(200));
    assert_eq!(budget.maximum_input_tokens, Some(800));
}

#[test]
fn known_missing_physical_maximum_uses_context_minus_output() {
    // arrange
    let limits = known_limits(1_000, None, 200);

    // act
    let budget = compute_request_budget(budget_input(&limits)).unwrap_or_abort();

    // assert
    assert_eq!(budget.maximum_input_tokens, Some(800));
    assert_eq!(budget.compaction_threshold_tokens, Some(700));
}

#[test]
fn boundary_requires_compaction_at_equality() {
    // arrange
    let limits = known_limits(1_000, Some(800), 200);
    let input = RequestBudgetInput {
        request_cost: components([100, 100, 100, 100, 100, 200]),
        model_limits: &limits,
        ..budget_input(&limits)
    };

    // act
    let budget = compute_request_budget(input).unwrap_or_abort();

    // assert
    assert_eq!(budget.occupied_input_tokens, 700);
    assert_eq!(budget.remaining_input_tokens, Some(0));
    assert_eq!(budget.requires_compaction, Some(true));
}

#[test]
fn component_accounting_counts_each_named_component_once() {
    // arrange
    let limits = known_limits(10_000, None, 1_000);
    let cases = [
        [11, 0, 0, 0, 0, 0],
        [0, 12, 0, 0, 0, 0],
        [0, 0, 13, 0, 0, 0],
        [0, 0, 0, 14, 0, 0],
        [0, 0, 0, 0, 15, 0],
        [0, 0, 0, 0, 0, 16],
    ];

    // act
    let budgets = cases.map(|values| {
        compute_request_budget(RequestBudgetInput {
            request_cost: components(values),
            model_limits: &limits,
            ..budget_input(&limits)
        })
        .unwrap_or_abort()
    });

    // assert
    assert_eq!(
        budgets.map(|budget| budget.occupied_input_tokens),
        [11, 12, 13, 14, 15, 16]
    );
}

#[test]
fn component_overflow_returns_typed_error() {
    // arrange
    let limits = known_limits(u32::MAX, None, 1);
    let input = RequestBudgetInput {
        request_cost: components([u32::MAX, 1, 0, 0, 0, 0]),
        model_limits: &limits,
        ..budget_input(&limits)
    };

    // act
    let result = compute_request_budget(input);

    // assert
    assert_eq!(result, Err(RequestBudgetError::ComponentArithmeticOverflow));
}

#[test]
fn known_snapshot_serialization_is_stable_and_redacted() {
    // arrange
    let limits = known_limits(1_000, Some(800), 200);
    let budget = compute_request_budget(RequestBudgetInput {
        request_cost: components([1, 2, 3, 4, 5, 6]),
        output_cap_disposition: ProviderOutputCapDisposition::Emitted(200),
        model_limits: &limits,
        ..budget_input(&limits)
    })
    .unwrap_or_abort();

    // act
    let value = serde_json::to_value(budget.snapshot()).unwrap_or_abort();
    let serialized = serde_json::to_string(&value).unwrap_or_abort();

    // assert
    assert_eq!(
        value,
        json!({
            "status": "estimated",
            "requested_output_tokens": 200,
            "reserved_output_tokens": 200,
            "maximum_input_tokens": 800,
            "safety_margin_tokens": 100,
            "compaction_threshold_tokens": 700,
            "components": {
                "system_tokens": 1,
                "tools_tokens": 2,
                "history_tokens": 3,
                "attachments_tokens": 4,
                "framing_tokens": 5,
                "pending_prompt_tokens": 6
            },
            "occupied_input_tokens": 21,
            "remaining_input_tokens": 679,
            "requires_compaction": false,
            "output_cap_disposition": { "emitted": 200 }
        })
    );
    for forbidden in [
        "model_limits",
        "context_window",
        "max_input",
        "max_output",
        "prompt",
        "schema",
        "content",
        "body",
        "data_url",
        "data-url",
        "secret",
    ] {
        assert!(
            !contains_json_key(&value, forbidden),
            "found forbidden key {forbidden}"
        );
    }
    assert!(!serialized.contains("super-secret-sentinel"));
}

fn contains_json_key(value: &serde_json::Value, forbidden: &str) -> bool {
    match value {
        serde_json::Value::Object(object) => {
            object.contains_key(forbidden)
                || object
                    .values()
                    .any(|nested| contains_json_key(nested, forbidden))
        }
        serde_json::Value::Array(array) => array
            .iter()
            .any(|nested| contains_json_key(nested, forbidden)),
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => false,
    }
}
