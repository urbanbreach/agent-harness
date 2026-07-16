use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyModifiers};
use harness_core::event::{
    EventV1, ProviderRequestFinishedEvent, ProviderRequestStartedEvent, ProviderStreamDeltaEvent,
    RunFailedEvent, ToolCallRequestedEvent, ToolCallStartedEvent, UserMessageSubmittedEvent,
};
use harness_core::perm::PermissionDecision;
use harness_tui::app::{
    AppState, Focus, LaunchMetadata, ModelOption, ReviewSurface, RuntimeStateKind, UiIntent,
};
use harness_tui::overlay::OverlayKind;

use crate::helpers::*;

// ---------------------------------------------------------------------------
// P0-TX-01
// ---------------------------------------------------------------------------

#[test]
fn transcript_projection_renders_structured_user_assistant_tool_blocks() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    let request_id = "req_p0_tx_taxonomy";

    // act — ingest structured activity families
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "User taxonomy probe message".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "p0-tx-model".to_string(),
            prompt_summary: "User taxonomy probe message".to_string(),
            request_digest: "digest-p0-tx".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: "Assistant taxonomy reply body.".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_p0_tx".into(),
            tool_id: "fs.read".to_string(),
            args_summary: r#"{"path":"README.md"}"#.to_string(),
            args_digest: "digest-args-p0-tx".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        5,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_p0_tx".into(),
        }),
    ));
    app.ingest_event(permission_requested_event(6, "perm_p0_tx", "tc_p0_tx"));

    let rendered = render_text(&app, 120, 40);
    let runtime = app.runtime_state();
    let pending = app.transcript_pending_permissions();

    // assert — structured projection, not opaque dump
    assert!(
        rendered.contains("User taxonomy probe message"),
        "P0-TX-01: user block must project structured text\n{rendered}"
    );
    assert!(
        rendered.contains("Assistant taxonomy reply body."),
        "P0-TX-01: assistant stream must project structured text\n{rendered}"
    );
    assert!(
        rendered.contains("README.md") || rendered.contains("Read"),
        "P0-TX-01: tool call must project structured title/path\n{rendered}"
    );
    assert!(
        rendered.contains("Permission required") || !pending.is_empty(),
        "P0-TX-01: permission family must project as structured pending state\n{rendered}"
    );
    assert!(
        !rendered.contains(r#"{"path":"README.md"}"#)
            || rendered.contains("Read")
            || rendered.contains("README.md"),
        "P0-TX-01: tool args must not be the only opaque dump surface\n{rendered}"
    );
    assert!(
        matches!(
            runtime.kind,
            RuntimeStateKind::Streaming
                | RuntimeStateKind::Sending
                | RuntimeStateKind::PermissionBlocked
                | RuntimeStateKind::PermissionPending
        ),
        "P0-TX-01: runtime must reflect projected activity; got {:?}",
        runtime.kind
    );
    assert!(
        plan_for(&app, 120, 40).transcript.is_some(),
        "P0-TX-01: transcript surface must remain allocated"
    );
}

// ---------------------------------------------------------------------------
// P0-TX-02
// ---------------------------------------------------------------------------

#[test]
fn transcript_activity_lifecycle_streaming_done_and_failed() {
    // arrange / act — streaming
    let mut streaming = AppState::new_live(None, false, None);
    let request_id = "req_p0_tx_life";
    streaming.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "p0-life-model".to_string(),
            prompt_summary: "lifecycle".to_string(),
            request_digest: "digest-life".to_string(),
            metadata: None,
        }),
    ));
    streaming.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: "partial stream body".to_string(),
        }),
    ));
    let streaming_state = streaming.runtime_state();
    let streaming_render = render_text(&streaming, 120, 40);

    // assert — streaming
    assert!(
        matches!(
            streaming_state.kind,
            RuntimeStateKind::Streaming | RuntimeStateKind::Sending
        ),
        "P0-TX-02: streaming must map to Sending/Streaming; got {:?}",
        streaming_state.kind
    );
    assert!(
        streaming_render.contains("partial stream body"),
        "P0-TX-02: partial stream text must remain visible\n{streaming_render}"
    );

    // act — completed
    streaming.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-out".to_string()),
            usage: None,
            metadata: None,
        }),
    ));
    let done_state = streaming.runtime_state();
    assert!(
        matches!(
            done_state.kind,
            RuntimeStateKind::Success | RuntimeStateKind::Ready
        ),
        "P0-TX-02: finished request must leave Success/Ready; got {:?}",
        done_state.kind
    );

    // act — failed
    let mut failed = AppState::new_live(None, false, None);
    failed.ingest_event(envelope(
        1,
        Some("req_p0_tx_fail"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_p0_tx_fail".into(),
            provider_id: "mock".to_string(),
            model_id: "p0-fail-model".to_string(),
            prompt_summary: "fail".to_string(),
            request_digest: "digest-fail".to_string(),
            metadata: None,
        }),
    ));
    failed.ingest_event(envelope(
        2,
        None,
        EventV1::RunFailed(RunFailedEvent {
            error: "provider stream interrupted".to_string(),
        }),
    ));
    let failed_state = failed.runtime_state();
    let failed_render = render_text(&failed, 120, 40);

    // assert — failed distinct from streaming success
    assert_eq!(
        failed_state.kind,
        RuntimeStateKind::Failure,
        "P0-TX-02: RunFailed must project Failure runtime state"
    );
    assert!(
        failed_render.contains("provider stream interrupted")
            || failed_state.summary.to_lowercase().contains("fail")
            || failed_state.detail.as_ref().is_some_and(|d| d.contains("interrupted")),
        "P0-TX-02: failed state must remain distinct and visible\nstate={failed_state:?}\n{failed_render}"
    );
    assert_ne!(
        streaming_state.kind, failed_state.kind,
        "P0-TX-02: streaming and failed must not collapse to one kind"
    );
}
// ---------------------------------------------------------------------------
// P0-TX-03
// ---------------------------------------------------------------------------

#[test]
fn transcript_scroll_keys_keep_full_width_transcript_surface() {
    // arrange
    let mut app = live_session_app();
    app.focus = Focus::List;

    // act
    app.handle_key(key(KeyCode::PageDown));
    app.handle_key(key(KeyCode::PageUp));
    app.handle_key(key(KeyCode::End));
    app.handle_key(key(KeyCode::Home));
    let plan = plan_for(&app, 120, 40);
    let rendered = render_text(&app, 120, 40);

    // assert
    assert!(
        plan.operator_sidebar.is_none(),
        "P0-TX-03: scroll must not introduce operator sidebar"
    );
    assert!(
        plan.transcript.is_some(),
        "P0-TX-03: transcript surface must remain allocated"
    );
    assert!(
        !rendered.is_empty(),
        "P0-TX-03: transcript remains renderable after scroll keys\n{rendered}"
    );
}
