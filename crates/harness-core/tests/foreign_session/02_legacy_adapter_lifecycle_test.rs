use super::*;
use harness_core::session::legacy::{LegacyAdapterError, LegacyEventLogAdapter};

#[test]
fn legacy_adapter_rejects_out_of_order_provider_lifecycle() {
    // arrange
    let started = sample_envelope(
        1,
        "legacy-run",
        EventV1::ProviderRequestStarted(harness_core::event::ProviderRequestStartedEvent {
            request_id: "provider-1".into(),
            provider_id: "mock".to_string(),
            model_id: "model-1".to_string(),
            prompt_summary: "redacted".to_string(),
            request_digest: "digest-request".to_string(),
            metadata: None,
        }),
    );
    let finished = sample_envelope(
        2,
        "legacy-run",
        EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
            request_id: "provider-1".into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-output".to_string()),
            usage: None,
            metadata: None,
        }),
    );
    let late_delta = sample_envelope(
        3,
        "legacy-run",
        EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
            request_id: "provider-1".into(),
            delta: "late".to_string(),
        }),
    );
    let premature_assistant = sample_envelope(
        2,
        "legacy-run",
        EventV1::AssistantMessageFinished(
            harness_core::event::AssistantMessageFinishedEvent {
                request_id: "provider-1".into(),
                tool_call_count: 0,
                assistant_message: None,
            },
        ),
    );

    // act
    let late_delta_result =
        LegacyEventLogAdapter::new().project(&[started.clone(), finished, late_delta]);
    let premature_assistant_result =
        LegacyEventLogAdapter::new().project(&[started, premature_assistant]);

    // assert
    assert_eq!(
        late_delta_result,
        Err(LegacyAdapterError::InvalidIdentityRelationship {
            event_id: "evt-3".to_string(),
        })
    );
    assert_eq!(
        premature_assistant_result,
        Err(LegacyAdapterError::InvalidIdentityRelationship {
            event_id: "evt-2".to_string(),
        })
    );
}
