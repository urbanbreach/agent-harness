#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "integration parity tests use fail-fast assertions"
)]

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_tui::app::{AppState, LaunchMetadata};
use harness_tui::render_test::render_to_string;
use harness_tui::ui;
use ratatui::layout::Rect;

const WIDTH: u16 = 120;
const HEIGHT: u16 = 40;

fn envelope(seq: u64, request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-assistant-stream-{seq:04}"),
        seq,
        run_id: "run_assistant_stream".into(),
        mono_ms: seq,
        ts: Some("2026-08-09T12:00:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("assistant-stream".to_string())),
        correlation_id: Some(request_id.to_string()),
        causation_id: None,
        stream_key: Some("run:run_assistant_stream".to_string()),
        payload,
    }
}

fn streaming_app(request_id: &str) -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-stream").with_mode_label("Demo"),
    );
    app.ingest_event(envelope(
        1,
        request_id,
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "stream markdown".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        request_id,
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-stream".to_string(),
            prompt_summary: "stream markdown".to_string(),
            request_digest: "digest-assistant-stream".to_string(),
            metadata: None,
        }),
    ));
    app
}

fn render(app: &AppState) -> String {
    render_to_string(app, Rect::new(0, 0, WIDTH, HEIGHT), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn finish(app: &mut AppState, request_id: &str, seq: u64) {
    app.ingest_event(envelope(
        seq,
        request_id,
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-assistant-finished".to_string()),
            usage: None,
            metadata: None,
        }),
    ));
}

#[test]
fn rich_mermaid_interpretation_waits_for_stream_completion() {
    let request_id = "req_stream_mermaid";
    let mut app = streaming_app(request_id);
    app.ingest_event(envelope(
        3,
        request_id,
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: "```mermaid\ngraph TD\n  A[Start] --> B[Done]\n```".to_string(),
        }),
    ));

    let streaming = render(&app);

    assert!(
        streaming.contains("graph TD"),
        "streaming markdown must remain source-shaped until settlement\n{streaming}"
    );
    assert!(
        !streaming.contains('▼'),
        "finished-only diagram interpretation must not run mid-stream\n{streaming}"
    );

    finish(&mut app, request_id, 4);
    let settled = render(&app);

    assert!(
        settled.contains("Start") && settled.contains("Done") && settled.contains('▼'),
        "completion must finalize the diagram in place\n{settled}"
    );
    assert!(
        !settled.contains("graph TD"),
        "settled diagram must replace source syntax\n{settled}"
    );
}

#[test]
fn open_fence_tail_keeps_its_code_row_when_completion_closes_it() {
    let request_id = "req_stream_open_fence";
    let mut app = streaming_app(request_id);
    app.ingest_event(envelope(
        3,
        request_id,
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: "Stable prefix.\n\n```rust\nfn streamed".to_string(),
        }),
    ));
    let streaming = render(&app);
    let streaming_row = streaming
        .lines()
        .position(|line| line.contains("fn streamed"))
        .expect("streaming code row");

    app.ingest_event(envelope(
        4,
        request_id,
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: "_probe() {}\n```".to_string(),
        }),
    ));
    finish(&mut app, request_id, 5);
    let settled = render(&app);
    let settled_row = settled
        .lines()
        .position(|line| line.contains("fn streamed_probe()"))
        .expect("settled code row");

    assert_eq!(
        streaming_row, settled_row,
        "closing an already-visible fence must not shift its code row\nstreaming:\n{streaming}\nsettled:\n{settled}"
    );
}
