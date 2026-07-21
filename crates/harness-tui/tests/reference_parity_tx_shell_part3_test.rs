//! SHELL-STREAM streaming-state structural owners (split out of
//! `reference_parity_tx_shell_test.rs` by the 800-line file-focus budget).
//!
//! Contract: `docs/grok-build-tui-implementation-prompt.md` §10 SHELL-STREAM
//! row + Wave 4 Packet 4.2 parity evidence.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast asserts"
)]

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestStartedEvent,
    ProviderStreamDeltaEvent, TaskCancelledEvent, TaskTerminalScope, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_tui::app::{AppState, Focus, LaunchMetadata, RuntimeStateKind};
use harness_tui::render_test::render_to_string;
use harness_tui::ui;
use ratatui::layout::Rect;

const W: u16 = 120;
const H: u16 = 40;

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-tx-shell-{seq:04}"),
        seq,
        run_id: "run_tx_shell_parity".into(),
        mono_ms: seq,
        ts: Some("2026-03-19T05:54:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("tx-shell-parity".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_tx_shell_parity".to_string()),
        payload,
    }
}

fn live_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-tx").with_mode_label("Demo"),
    );
    app
}

fn render(app: &AppState) -> String {
    render_to_string(app, Rect::new(0, 0, W, H), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

/// SHELL-STREAM incremental rendering: every stream delta projects into the
/// transcript body as it arrives, the runtime keeps streaming state with a
/// stream indicator exposed, and no overlay opens while streaming.
#[test]
fn shell_stream_renders_each_delta_incrementally_and_stays_streaming() {
    // arrange — user turn + first delta
    let mut app = live_app();
    let request_id = "req_stream_incremental";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "incremental stream probe".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: "incremental stream probe".to_string(),
            request_digest: "digest-stream-inc".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: "alpha-token".to_string(),
        }),
    ));

    // act — first render mid-stream
    let first = render(&app);

    // assert — first delta visible; runtime streaming; no overlays
    assert!(
        first.contains("alpha-token"),
        "SHELL-STREAM: first delta must project\n{first}"
    );
    assert!(
        matches!(
            app.runtime_state().kind,
            RuntimeStateKind::Streaming | RuntimeStateKind::Sending
        ),
        "SHELL-STREAM: runtime must stay streaming mid-stream"
    );
    assert!(
        app.overlay_stack().top().is_none(),
        "SHELL-STREAM: streaming must not open overlays"
    );

    // act — more deltas accumulate
    app.ingest_event(envelope(
        4,
        Some(request_id),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: " beta-token".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        5,
        Some(request_id),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: " gamma-token".to_string(),
        }),
    ));
    let full = render(&app);

    // assert — every delta renders incrementally; state still streaming
    assert!(
        full.contains("alpha-token") && full.contains("beta-token") && full.contains("gamma-token"),
        "SHELL-STREAM: all deltas must render incrementally\n{full}"
    );
    assert!(
        matches!(
            app.runtime_state().kind,
            RuntimeStateKind::Streaming | RuntimeStateKind::Sending
        ),
        "SHELL-STREAM: runtime must stay streaming after later deltas"
    );
    assert!(
        full.contains('❯'),
        "SHELL-STREAM: composer chrome retained while streaming\n{full}"
    );
}

/// SHELL-STREAM recovery: cancelling an in-flight stream returns the shell to
/// idle-ready state — runtime leaves the streaming state, partial stream text
/// stays in the transcript, the composer regains focus, no overlays remain.
#[test]
fn shell_stream_cancellation_returns_to_idle_ready_shell() {
    // arrange — an actively streaming shell
    let mut app = live_app();
    let request_id = "req_stream_cancel";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "stream cancel probe".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: "stream cancel probe".to_string(),
            request_digest: "digest-stream-cancel".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: "partial-before-cancel".to_string(),
        }),
    ));
    assert!(
        matches!(
            app.runtime_state().kind,
            RuntimeStateKind::Streaming | RuntimeStateKind::Sending
        ),
        "precondition: shell must be streaming before cancel"
    );

    // act — cancel the streaming turn
    app.ingest_event(envelope(
        4,
        Some(request_id),
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: "task_stream_cancel".into(),
            reason: "user interrupted streaming turn".to_string(),
            task_scope: Some(TaskTerminalScope::AgentTurn),
        }),
    ));
    let rendered = render(&app);

    // assert — idle-ready shell recovery
    assert!(
        !matches!(
            app.runtime_state().kind,
            RuntimeStateKind::Streaming | RuntimeStateKind::Sending
        ),
        "SHELL-STREAM cancel: runtime must leave the streaming state, got {:?}",
        app.runtime_state().kind
    );
    assert!(
        rendered.contains("partial-before-cancel"),
        "SHELL-STREAM cancel: partial stream must remain in the transcript\n{rendered}"
    );
    assert_eq!(
        app.focus,
        Focus::Prompt,
        "SHELL-STREAM cancel: composer must regain focus"
    );
    assert!(
        app.overlay_stack().top().is_none(),
        "SHELL-STREAM cancel: no overlay may stay open"
    );
    assert!(
        rendered.contains('❯'),
        "SHELL-STREAM cancel: composer chrome retained after cancel\n{rendered}"
    );
}
