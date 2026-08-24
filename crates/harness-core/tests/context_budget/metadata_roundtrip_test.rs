use harness_core::context_budget::compute_request_budget;
use harness_core::event::ProviderRequestStartedMetadata;
use harness_core::proj::RecordedRuntimeContext;
use harness_core::UnwrapOrAbort;
use serde_json::json;

use super::common::{budget_input, known_limits};

#[test]
fn known_metadata_containers_roundtrip_optional_redacted_snapshot() {
    // arrange
    let limits = known_limits(1_000, Some(800), 200);
    let snapshot = compute_request_budget(budget_input(&limits))
        .unwrap_or_abort()
        .snapshot();
    let request_metadata = ProviderRequestStartedMetadata {
        context_budget: Some(snapshot),
        ..Default::default()
    };
    let runtime_context = RecordedRuntimeContext {
        last_request_budget: Some(snapshot),
        ..Default::default()
    };

    // act
    let request_value = serde_json::to_value(&request_metadata).unwrap_or_abort();
    let runtime_value = serde_json::to_value(&runtime_context).unwrap_or_abort();
    let empty_request: ProviderRequestStartedMetadata =
        serde_json::from_value(json!({})).unwrap_or_abort();
    let empty_runtime: RecordedRuntimeContext = serde_json::from_value(
        serde_json::to_value(RecordedRuntimeContext::default()).unwrap_or_abort(),
    )
    .unwrap_or_abort();

    // assert
    assert_eq!(
        request_value.get("context_budget"),
        runtime_value.get("last_request_budget")
    );
    assert_eq!(empty_request.context_budget, None);
    assert_eq!(empty_runtime.last_request_budget, None);
}
