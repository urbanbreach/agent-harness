#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "runtime event integration tests use fail-fast assertions"
)]

use harness_core::event::{
    ActorKind, AssistantMessageFinishedEvent, EventActor, EventEnvelopeV1, EventV1,
    LiveEventEnvelope, LiveEventV1, ProviderRequestStartedEvent, RuntimeEvent,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_core::session::{AssistantPart, AssistantToolCall};
use harness_tui::app::AppState;
use harness_tui::render_test::render_to_string;
use harness_tui::ui;
use ratatui::layout::Rect;

fn durable(seq: u64, payload: EventV1) -> RuntimeEvent {
    RuntimeEvent::Durable(Box::new(EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-durable-{seq}"),
        seq,
        run_id: "run-typed-runtime".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some("agent-1".to_string())),
        correlation_id: Some("turn-1".to_string()),
        causation_id: None,
        stream_key: Some("agent:agent-1".to_string()),
        payload,
    }))
}

fn live(event_id: &str, payload: LiveEventV1) -> RuntimeEvent {
    RuntimeEvent::Live(Box::new(LiveEventEnvelope {
        event_id: event_id.to_string(),
        run_id: "run-typed-runtime".into(),
        mono_ms: 3,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some("agent-1".to_string())),
        correlation_id: Some("turn-1".to_string()),
        causation_id: None,
        stream_key: Some("agent:agent-1".to_string()),
        payload,
    }))
}

fn render(app: &AppState) -> String {
    render_to_string(app, Rect::new(0, 0, 120, 40), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

#[test]
fn typed_live_fragments_render_then_final_commit_settles_them() {
    // Given: a live turn with its durable request barrier.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_runtime_event(durable(
        1,
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "turn-1".into(),
            text: "question".to_string(),
        }),
    ));
    app.ingest_runtime_event(durable(
        2,
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "provider-1".into(),
            provider_id: "mock".to_string(),
            model_id: "model".to_string(),
            prompt_summary: "question".to_string(),
            request_digest: "request-digest".to_string(),
            metadata: None,
        }),
    ));

    // When: typed live fragments arrive without durable sequence numbers.
    app.ingest_runtime_event(live(
        "live-reasoning",
        LiveEventV1::ProviderReasoningDelta {
            request_id: "provider-1".into(),
            delta: "draft reasoning".to_string(),
        },
    ));
    app.ingest_runtime_event(live(
        "live-text",
        LiveEventV1::ProviderTextDelta {
            request_id: "provider-1".into(),
            delta: "draft answer".to_string(),
        },
    ));
    app.ingest_runtime_event(live(
        "live-tool",
        LiveEventV1::ProviderToolInputDelta {
            request_id: "provider-1".into(),
            tool_call_id: "tool-1".into(),
            delta: "{\"path\":\"draft\"}".to_string(),
        },
    ));

    // Then: live content is visible but is not added to durable event history.
    let transient = render(&app);
    assert!(transient.contains("draft answer"), "{transient}");
    assert!(transient.contains("draft"), "{transient}");
    assert_eq!(app.selected_event().map(|event| event.seq), Some(2));

    // When: the durable assistant commit arrives with canonical content.
    app.ingest_runtime_event(durable(
        3,
        EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
            request_id: "provider-1".into(),
            tool_call_count: 1,
            parts: vec![
                AssistantPart::Reasoning {
                    text: "final reasoning".to_string(),
                },
                AssistantPart::Text {
                    text: "final answer".to_string(),
                },
                AssistantPart::ToolCall(AssistantToolCall {
                    tool_call_id: "tool-1".into(),
                    provider_tool_call_id: None,
                    tool_id: "read".to_string(),
                    args_summary: "{\"path\":\"final\"}".to_string(),
                    args_digest: "args-digest".to_string(),
                    provider_call_id: None,
                }),
            ],
            provenance: None,
            assistant_message: None,
        }),
    ));

    // Then: canonical content replaces every transient fragment in place.
    let settled = render(&app);
    assert!(settled.contains("final answer"), "{settled}");
    assert!(settled.contains("final"), "{settled}");
    assert!(!settled.contains("draft answer"), "{settled}");
    assert_eq!(settled.matches("Reading 1 file").count(), 1, "{settled}");
    assert_eq!(app.selected_event().map(|event| event.seq), Some(3));
}
