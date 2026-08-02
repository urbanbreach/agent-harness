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

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, RunFailedEvent, RunFinishedEvent,
    TaskCancelledEvent, TaskTerminalScope, UserMessageSubmittedEvent, SCHEMA_VERSION,
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

/// SHELL-CANCEL mid-stream: cancelling after streamed deltas preserves the
/// partial assistant response and projects the cancel state distinctly.
#[test]
fn shell_cancel_mid_stream_preserves_partial_response() {
    // arrange — streaming turn with an already-projected delta
    let mut app = live_app();
    let request_id = "req_cancel_mid";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "mid-stream cancel probe".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".into(),
            model_id: "model-tx".into(),
            prompt_summary: "mid-stream cancel probe".into(),
            request_digest: "digest-cancel-mid".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: "partial-cancel-body-before-stop".to_string(),
        }),
    ));
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Streaming);

    // act
    app.ingest_event(envelope(
        4,
        Some(request_id),
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: "task_cancel_mid".into(),
            reason: "user interrupted mid-stream".to_string(),
            task_scope: Some(TaskTerminalScope::AgentTurn),
        }),
    ));
    let rendered = render(&app);

    // assert — Cancelled state, partial body kept, cancel chrome, not fail chrome
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Cancelled);
    assert!(
        rendered.contains("partial-cancel-body-before-stop"),
        "SHELL-CANCEL: partial streamed body must survive the cancel\n{rendered}"
    );
    assert!(
        rendered.contains("Turn cancelled by user"),
        "SHELL-CANCEL: cancel chrome required\n{rendered}"
    );
    assert!(
        !rendered.contains("Retry failed:"),
        "SHELL-CANCEL: must not paint fail chrome\n{rendered}"
    );
}

/// SHELL-FAIL mid-stream: a run failure after streamed deltas keeps the
/// partial body, projects the Failure state, and retains the bordered composer.
#[test]
fn shell_fail_mid_stream_keeps_partial_body_with_failure_state() {
    // arrange — streaming turn that fails after one delta
    let mut app = live_app();
    let request_id = "req_fail_mid";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "mid-stream fail probe".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".into(),
            model_id: "model-tx".into(),
            prompt_summary: "mid-stream fail probe".into(),
            request_digest: "digest-fail-mid".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: "partial-fail-body-before-error".to_string(),
        }),
    ));

    // act
    app.ingest_event(envelope(
        4,
        Some(request_id),
        EventV1::RunFailed(RunFailedEvent {
            error: "provider exploded mid-response".to_string(),
        }),
    ));
    let rendered = render(&app);

    // assert — Failure state, partial body kept, cancel chrome absent
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Failure);
    assert!(
        rendered.contains("partial-fail-body-before-error"),
        "SHELL-FAIL: partial streamed body must survive the failure\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "SHELL-FAIL: bordered composer retained under failure\n{rendered}"
    );
    assert!(
        !rendered.contains("Turn cancelled by user"),
        "SHELL-FAIL: must not reuse cancel chrome\n{rendered}"
    );
}

/// SHELL-RECOVER: after a failure the shell accepts a fresh turn and the
/// runtime leaves the Failure state (recovery path returns to streaming).
#[test]
fn shell_recover_accepts_new_turn_after_failure() {
    // arrange — a failed turn
    let mut app = live_app();
    let failed_id = "req_recover_fail";
    app.ingest_event(envelope(
        1,
        Some(failed_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: failed_id.into(),
            text: "turn that fails".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(failed_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: failed_id.into(),
            provider_id: "mock".into(),
            model_id: "model-tx".into(),
            prompt_summary: "turn that fails".into(),
            request_digest: "digest-recover-fail".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(failed_id),
        EventV1::RunFailed(RunFailedEvent {
            error: "recovery probe failure".to_string(),
        }),
    ));
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Failure);

    // act — user submits a retry turn
    let retry_id = "req_recover_retry";
    app.ingest_event(envelope(
        4,
        Some(retry_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: retry_id.into(),
            text: "retry after failure".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        5,
        Some(retry_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: retry_id.into(),
            provider_id: "mock".into(),
            model_id: "model-tx".into(),
            prompt_summary: "retry after failure".into(),
            request_digest: "digest-recover-retry".to_string(),
            metadata: None,
        }),
    ));
    let rendered = render(&app);

    // assert — runtime recovered into an active turn; both turns visible
    assert!(
        matches!(
            app.runtime_state().kind,
            RuntimeStateKind::Sending | RuntimeStateKind::Streaming
        ),
        "SHELL-RECOVER: runtime must leave Failure for the retry turn; got {:?}",
        app.runtime_state().kind
    );
    assert!(
        rendered.contains("turn that fails"),
        "SHELL-RECOVER: failed turn retained\n{rendered}"
    );
    assert!(
        rendered.contains("retry after failure"),
        "SHELL-RECOVER: retry turn visible\n{rendered}"
    );
}

/// SHELL-COMPLETE: a finished turn (finish_reason=stop + RunFinished) leaves
/// the streaming/sending runtime states — the shell is turn-complete.
#[test]
fn shell_complete_finish_leaves_streaming_state() {
    // arrange
    let mut app = live_app();
    let request_id = "req_complete_finish";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "completion probe".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".into(),
            model_id: "model-tx".into(),
            prompt_summary: "completion probe".into(),
            request_digest: "digest-complete".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: "final completed body".to_string(),
        }),
    ));

    // act — finish the provider request and the run
    app.ingest_event(envelope(
        4,
        Some(request_id),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-complete-out".to_string()),
            usage: None,
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        5,
        Some(request_id),
        EventV1::RunFinished(RunFinishedEvent {
            summary: "completed".to_string(),
        }),
    ));
    let rendered = render(&app);

    // assert — runtime left the streaming/sending states; body + composer intact
    assert!(
        !matches!(
            app.runtime_state().kind,
            RuntimeStateKind::Streaming | RuntimeStateKind::Sending
        ),
        "SHELL-COMPLETE: runtime must leave streaming states after stop; got {:?}",
        app.runtime_state().kind
    );
    assert!(
        rendered.contains("final completed body"),
        "SHELL-COMPLETE: final body rendered\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "SHELL-COMPLETE: composer retained\n{rendered}"
    );
}

/// SHELL-SCROLL bounds: repeated PageUp accumulates scroll without follow,
/// Home pins to the top and End restores the bottom with follow mode on.
#[test]
fn shell_scroll_home_and_end_keys_bound_transcript_follow() {
    // arrange — three completed turns so the transcript is scrollable
    let mut app = live_app();
    for (seq, rid, text) in [
        (1u64, "req_bound_a", "First bounded turn"),
        (2u64, "req_bound_b", "Second bounded turn"),
        (3u64, "req_bound_c", "Third bounded turn"),
    ] {
        app.ingest_event(envelope(
            seq * 10,
            Some(rid),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: rid.into(),
                text: text.into(),
            }),
        ));
        app.ingest_event(envelope(
            seq * 10 + 1,
            Some(rid),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: rid.into(),
                provider_id: "mock".into(),
                model_id: "model-tx".into(),
                prompt_summary: text.into(),
                request_digest: format!("digest-{rid}"),
                metadata: None,
            }),
        ));
        app.ingest_event(envelope(
            seq * 10 + 2,
            Some(rid),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: rid.into(),
                delta: format!("Assistant body line for {rid}\n").repeat(14),
            }),
        ));
        app.ingest_event(envelope(
            seq * 10 + 3,
            Some(rid),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: rid.into(),
                finish_reason: "stop".to_string(),
                output_digest: Some(format!("out-{rid}")),
                usage: None,
                metadata: None,
            }),
        ));
    }
    app.focus = Focus::Details;
    let _ = render(&app); // populate the layout max-scroll cache before scrolling
    let initial = app.transcript_interaction_snapshot();
    assert!(initial.follow_mode, "SHELL-SCROLL: starts in follow mode");

    // act — PageUp twice (accumulates), Home (top), End (bottom)
    app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
    let after_first_page_up = app.transcript_interaction_snapshot();
    app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
    let after_second_page_up = app.transcript_interaction_snapshot();
    app.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    let after_home = app.transcript_interaction_snapshot();
    app.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    let after_end = app.transcript_interaction_snapshot();

    // assert — scroll accumulates without follow; Home tops; End restores bottom+follow
    assert!(
        after_first_page_up.scroll > 0 && !after_first_page_up.follow_mode,
        "SHELL-SCROLL: PageUp scrolls away from follow; got {}",
        after_first_page_up.scroll
    );
    assert!(
        after_second_page_up.scroll >= after_first_page_up.scroll,
        "SHELL-SCROLL: repeated PageUp must not lose scroll ({}, then {})",
        after_first_page_up.scroll,
        after_second_page_up.scroll
    );
    assert!(
        after_home.scroll >= after_second_page_up.scroll && !after_home.follow_mode,
        "SHELL-SCROLL: Home pins at or above the current top; got {}",
        after_home.scroll
    );
    assert_eq!(
        after_end.scroll, 0,
        "SHELL-SCROLL: End returns to the bottom"
    );
    assert!(
        after_end.follow_mode,
        "SHELL-SCROLL: End restores follow mode"
    );
}
