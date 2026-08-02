use harness_core::event::{ActorKind, EventActor, EventV1, RunStartedEvent, SCHEMA_VERSION};
use harness_core::store::EventEnvelopeWithoutSeqV1;

pub fn run_started_draft(
    run_id: &str,
    workspace_root: &str,
    marker: u64,
) -> EventEnvelopeWithoutSeqV1 {
    EventEnvelopeWithoutSeqV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{marker:04}"),
        run_id: run_id.to_string().into(),
        mono_ms: marker,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload: EventV1::RunStarted(RunStartedEvent {
            run_name: format!("run-{marker}").into(),
            workspace_root: workspace_root.to_string(),
        }),
    }
}
