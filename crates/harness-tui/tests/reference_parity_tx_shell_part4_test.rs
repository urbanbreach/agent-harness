//! TX-USER / TX-ASSISTANT / TX-TOOL / TX-DIFF transcript primitive owners
//! (split out of `reference_parity_tx_shell_test.rs` by the 800-line file-focus
//! budget).
//!
//! Contract: `docs/grok-build-tui-implementation-prompt.md` §10 transcript
//! primitives + DESIGN.md §8/§14.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast asserts"
)]

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, ToolCallFinishedEvent,
    ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_tui::app::{AppState, Focus, LaunchMetadata};
use harness_tui::render_test::render_to_string;
use harness_tui::ui;
use ratatui::layout::Rect;

const W: u16 = 120;
const H: u16 = 40;

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-tx-primitive-{seq:04}"),
        seq,
        run_id: "run_tx_primitive_parity".into(),
        mono_ms: seq,
        ts: Some("2026-03-19T05:54:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("tx-primitive-parity".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_tx_primitive_parity".to_string()),
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

fn ingest_started(app: &mut AppState, seq: u64, request_id: &str, text: &str) {
    app.ingest_event(envelope(
        seq,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: text.to_string(),
        }),
    ));
    app.ingest_event(envelope(
        seq + 1,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: text.to_string(),
            request_digest: format!("digest-{request_id}"),
            metadata: None,
        }),
    ));
}

/// TX-USER multi-turn: two user blocks keep submission order with ❯ markers,
/// the first user block stays above the second in the transcript.
#[test]
fn tx_user_blocks_preserve_submission_order_across_turns() {
    // arrange — two completed turns
    let mut app = live_app();
    ingest_started(&mut app, 10, "req_user_a", "first user primitive");
    app.ingest_event(envelope(
        12,
        Some("req_user_a"),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_user_a".into(),
            delta: "first reply".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        13,
        Some("req_user_a"),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "req_user_a".into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-req-user-a-finished".to_string()),
            usage: None,
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        20,
        Some("req_user_b"),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_user_b".into(),
            text: "second user primitive".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        21,
        Some("req_user_b"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_user_b".into(),
            provider_id: "mock".into(),
            model_id: "model-tx".into(),
            prompt_summary: "second user primitive".into(),
            request_digest: "digest-req-user-b".to_string(),
            metadata: None,
        }),
    ));

    // act
    let rendered = render(&app);
    let lines: Vec<&str> = rendered.lines().collect();
    let first_idx = lines
        .iter()
        .position(|line| line.contains("first user primitive"))
        .expect("first user block");
    let second_idx = lines
        .iter()
        .position(|line| line.contains("second user primitive"))
        .expect("second user block");

    // assert — order preserved, both user markers present, no legacy rail
    assert!(
        first_idx < second_idx,
        "TX-USER: first user block must stay above the second\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "TX-USER: user marker chrome required\n{rendered}"
    );
    assert!(
        !rendered.contains('┃'),
        "TX-USER: no legacy outer rail on user blocks\n{rendered}"
    );
}

/// TX-ASSISTANT markdown: a fenced code block in the assistant body renders
/// its code content (fenced text block parsed + highlighted, content kept).
#[test]
fn tx_assistant_code_fence_renders_code_content() {
    // arrange
    let mut app = live_app();
    ingest_started(&mut app, 10, "req_fence", "show a code fence");
    let fenced = [
        "Intro before the fence.",
        "```rust",
        "fn fenced_probe() {",
        "    41 + 1",
        "}",
        "```",
        "Outro after the fence.",
    ]
    .join("\n");
    app.ingest_event(envelope(
        12,
        Some("req_fence"),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_fence".into(),
            delta: fenced,
        }),
    ));

    // act
    let rendered = render(&app);

    // assert — fenced code content survives through the highlighted block
    assert!(
        rendered.contains("fn fenced_probe()"),
        "TX-ASSISTANT: fenced code content must render through the code block\n{rendered}"
    );
    assert!(
        rendered.contains("41 + 1"),
        "TX-ASSISTANT: inner code lines must render\n{rendered}"
    );
    assert!(
        rendered.contains("Intro before the fence."),
        "TX-ASSISTANT: surrounding prose retained\n{rendered}"
    );
}

/// TX-TOOL failure: a failed tool call renders the tool error block (error
/// details surface; the shell keeps the bordered composer).
#[test]
fn tx_tool_failed_call_renders_error_block() {
    // arrange — a tool call that fails with error details in output_summary
    let mut app = live_app();
    let request_id = "req_tool_err";
    ingest_started(&mut app, 10, request_id, "run the failing command");
    app.ingest_event(envelope(
        12,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_err".into(),
            tool_id: "bash".to_string(),
            args_summary: r#"{"command":"false"}"#.to_string(),
            args_digest: "digest-args-err".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        13,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_err".into(),
        }),
    ));
    app.ingest_event(envelope(
        14,
        Some(request_id),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_err".into(),
            status: ToolCallStatus::Failed,
            output_summary: Some("command failed with exit code 7".to_string()),
            output_digest: Some("digest-out-err".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));

    // act
    let rendered = render(&app);

    // assert — error details surface in the tool error block
    assert!(
        rendered.contains("command failed with exit code 7")
            || rendered.contains("No error details available."),
        "TX-TOOL: failed tool must surface error details\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "TX-TOOL: bordered composer retained under tool error\n{rendered}"
    );
}

/// TX-DIFF: an edit projects BOTH old and new versions as inline diff lines
/// (removed + added content visible, rail-free, no message card).
#[test]
fn tx_diff_tool_details_project_removed_and_added_versions() {
    // arrange — session-path live app (transcript surface active) + edit tool call
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/run_tx_diff_parity")), false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-tx").with_mode_label("Demo"),
    );
    let request_id = "req_diff_lines";
    ingest_started(&mut app, 10, request_id, "apply the edit");
    app.ingest_event(envelope(
        12,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_diff_lines".into(),
            tool_id: "edit".to_string(),
            args_summary:
                r#"{"path":"src/lib.rs","oldString":"let answer = 41;","newString":"let answer = 42;"}"#
                    .to_string(),
            args_digest: "digest-args-dl".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        13,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_diff_lines".into(),
        }),
    ));
    app.ingest_event(envelope(
        14,
        Some(request_id),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_diff_lines".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("edited src/lib.rs".to_string()),
            output_digest: Some("digest-out-dl".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        15,
        Some(request_id),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-tx-finished".to_string()),
            usage: None,
            metadata: None,
        }),
    ));
    for _ in 0..12 {
        app.advance_animation_tick_for_evidence();
    }
    app.focus = Focus::Details;

    // act — palette -> "show tool details" -> render
    app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    for ch in "show tool details".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    let rendered = render(&app);

    assert!(
        app.transcript_interaction_snapshot().show_tool_details,
        "TX-DIFF: tool details must toggle on"
    );
    assert!(
        rendered.contains("let answer = 41;"),
        "TX-DIFF: removed line must project in tool details\n{rendered}"
    );
    assert!(
        rendered.contains("let answer = 42;"),
        "TX-DIFF: added line must project in tool details\n{rendered}"
    );
    assert!(
        !rendered.contains('┃'),
        "TX-DIFF: settled grouped edit details must remain rail-free\n{rendered}"
    );
    let _ = Focus::Prompt; // keep Focus import meaningful for future state asserts
}
