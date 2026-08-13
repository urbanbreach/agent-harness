use std::path::PathBuf;

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestStartedEvent,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::app::AppState;

pub(super) fn envelope(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-motion-{seq}"),
        seq,
        run_id: "run-motion".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("motion-owner".into())),
        correlation_id: Some("req-motion".into()),
        causation_id: None,
        stream_key: Some("run:run-motion".into()),
        payload,
    }
}

pub(super) fn streaming_app() -> AppState {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/run-motion")), false, None);
    app.ingest_event(envelope(
        1,
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req-motion".into(),
            text: "stream".into(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req-motion".into(),
            provider_id: "mock".into(),
            model_id: "mock".into(),
            prompt_summary: "stream".into(),
            request_digest: "digest-motion".into(),
            metadata: None,
        }),
    ));
    app
}
