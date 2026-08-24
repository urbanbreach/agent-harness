use harness_core::config::{ModelLimitProvenance, ResolvedModelLimits};
use harness_core::context_budget::{compute_request_budget, RequestBudgetInput};
use harness_core::UnwrapOrAbort;
use harness_providers::{ProviderOutputCapDisposition, ProviderRequestCost};
use harness_tui::app::LaunchMetadata;
use serde_json::json;

#[test]
fn context_budget_snapshot_machine_artifact() {
    // arrange: the same canonical input used by the cross-surface core contract.
    let limits = ResolvedModelLimits::from_values(
        Some(128_000),
        Some(96_000),
        Some(4_096),
        ModelLimitProvenance::explicit("budget equality test"),
    );
    let snapshot = compute_request_budget(RequestBudgetInput {
        model_limits: &limits,
        request_cost: ProviderRequestCost {
            system_tokens: 10,
            tools_tokens: 20,
            history_tokens: 30,
            attachments_tokens: 0,
            framing_tokens: 5,
            pending_prompt_tokens: 15,
        },
        requested_output_tokens: None,
        safety_margin_tokens: 16_384,
        estimated_token_triggers: false,
        fallback_input_tokens: 0,
        output_cap_disposition: ProviderOutputCapDisposition::Emitted(4_096),
    })
    .unwrap_or_abort()
    .snapshot();

    // act: the TUI launch/resume surface receives the persisted request snapshot.
    let tui_snapshot = LaunchMetadata::new("default", "provider", Some(String::from("model")))
        .with_last_request_budget(snapshot)
        .last_request_budget()
        .unwrap_or_abort();

    // assert: emit a normalized machine-readable value for the five-way jq proof.
    assert_eq!(tui_snapshot, snapshot);
    println!(
        "G003_BUDGET_COMPONENTS_TUI={}",
        serde_json::to_string(&json!({ "tui": tui_snapshot })).unwrap_or_abort()
    );
}
