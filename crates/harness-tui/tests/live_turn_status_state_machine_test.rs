use std::sync::Arc;
use std::time::Duration;

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestStartedEvent,
    ProviderStreamDeltaEvent, TaskCancelledEvent, TaskScheduleState, TaskScheduledEvent,
    TaskTerminalScope, ToolCallRequestedEvent, ToolCallStartedEvent, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_tui::app::AppState;
use harness_tui::layout::FrameLayoutPlan;
use harness_tui::render_test::render_to_buffer;
use harness_tui::ui;
use ratatui::layout::Rect;

const HEIGHT: u16 = 30;
const WIDE_WIDTH: u16 = 100;

fn envelope(seq: u64, request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-live-turn-status-{seq:04}"),
        seq,
        run_id: "run_live_turn_status".into(),
        mono_ms: seq.saturating_mul(1_000),
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("live-turn-status-test".to_string())),
        correlation_id: Some(request_id.to_string()),
        causation_id: None,
        stream_key: Some("run:run_live_turn_status".to_string()),
        payload,
    }
}

fn active_app() -> AppState {
    let request_id = "req_live_turn_status";
    let mut app = AppState::new_live(None, false, Some(Arc::new(|_| {})));
    app.ingest_event(envelope(
        1,
        request_id,
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "Show the unified status row".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        request_id,
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_live_turn_status".into(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:mock:model-status".to_string()),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        request_id,
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-status".to_string(),
            prompt_summary: "Show the unified status row".to_string(),
            request_digest: "digest-live-turn-status".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        4,
        request_id,
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: "Unified response".to_string(),
        }),
    ));
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(4_200));
    app
}

fn status_text(app: &AppState, width: u16) -> Option<String> {
    let area = Rect::new(0, 0, width, HEIGHT);
    let status = FrameLayoutPlan::for_app(app, area).status?;
    let buffer = render_to_buffer(app, area, |app, frame, _area| {
        ui::render_app(frame, app);
    });
    let row_start = usize::from(status.y) * usize::from(width) + usize::from(status.x);
    Some(
        buffer.content[row_start..row_start + usize::from(status.width)]
            .iter()
            .map(|cell| cell.symbol())
            .collect(),
    )
}

fn start_running_tool(app: &mut AppState, seq: u64, tool_id: &str, args_summary: &str) {
    app.ingest_event(envelope(
        seq,
        "req_live_turn_status",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: format!("tool_{seq}").into(),
            tool_id: tool_id.to_string(),
            args_summary: args_summary.to_string(),
            args_digest: format!("digest-tool-{seq}"),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        seq.saturating_add(1),
        "req_live_turn_status",
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: format!("tool_{seq}").into(),
        }),
    ));
}

#[test]
fn baseline_stream_renders_one_row_with_phase_total_and_stop() {
    // arrange
    // Given: a fixed-time active response with queued follow-up input.
    let mut app = active_app();
    app.queued_prompt_count = 2;

    // When: the live shell renders at its standard width.
    let row = status_text(&app, WIDE_WIDTH).expect("active status row");

    // act
    // Then: active-turn facts share the single status row, while queued prompts remain
    // absent because responding is not a sendable wait in the reference.
    // assert
    assert!(row.contains("Responding…"), "status row: {row:?}");
    assert!(row.matches("4.2s").count() >= 2, "status row: {row:?}");
    assert!(!row.contains("queued 2"), "status row: {row:?}");
    assert!(row.contains("[stop]"), "status row: {row:?}");
}

#[test]
fn sendable_wait_advertises_queued_prompt_promotion() {
    // arrange
    let mut app = active_app();
    app.queued_prompt_count = 1;
    start_running_tool(
        &mut app,
        5,
        "background_output",
        r#"{"task_id":"bg_1","block":true}"#,
    );
    app.queued_prompt_count = 1;

    // act
    let row = status_text(&app, WIDE_WIDTH).expect("sendable-wait status row");

    // assert
    assert!(
        row.contains("· 1 queued — Enter to send now"),
        "status row: {row:?}"
    );
    assert!(
        row.contains("waiting · 1 queued — Enter to send now"),
        "status row: {row:?}"
    );
    assert!(
        matches!(row.trim_start().chars().next(), Some('○' | '◎' | '◉')),
        "status row: {row:?}"
    );
    assert!(!row.contains("0.0s"), "status row: {row:?}");
    assert!(!row.contains("4.2s"), "status row: {row:?}");
    assert!(!row.contains("[stop]"), "status row: {row:?}");
}

#[test]
fn send_now_hint_is_hidden_while_the_composer_has_a_draft() {
    // arrange
    let mut app = active_app();
    start_running_tool(
        &mut app,
        5,
        "background_output",
        r#"{"task_id":"bg_1","block":true}"#,
    );
    app.queued_prompt_count = 1;
    app.handle_paste("draft in progress");

    // act
    let row = status_text(&app, WIDE_WIDTH).expect("sendable-wait status row");

    // assert
    assert!(!row.contains("Enter to send now"), "status row: {row:?}");
}

#[test]
fn instant_background_poll_does_not_advertise_queued_prompt_promotion() {
    // arrange
    // Given: a non-blocking background status poll with queued follow-up input.
    let mut app = active_app();
    start_running_tool(
        &mut app,
        5,
        "background_output",
        r#"{"task_id":"bg_1","block":false}"#,
    );
    app.queued_prompt_count = 1;

    // When: the live status row is rendered.
    let row = status_text(&app, WIDE_WIDTH).expect("background poll status row");

    // act
    // Then: the instant poll is not advertised as an interruptible parked wait.
    // assert
    assert!(!row.contains("Enter to send now"), "status row: {row:?}");
}

#[test]
fn agent_spawn_wait_advertises_queued_prompt_promotion() {
    // arrange
    // Given: a foreground agent.spawn wait with queued follow-up input.
    let mut app = active_app();
    start_running_tool(
        &mut app,
        5,
        "agent.spawn",
        r#"{"description":"inspect the workspace"}"#,
    );
    app.queued_prompt_count = 1;

    // When: the live status row is rendered.
    let row = status_text(&app, WIDE_WIDTH).expect("agent spawn status row");

    // act
    // Then: agent.spawn shares the same send-now contract as task waits.
    // assert
    assert!(
        row.contains("Waiting on subagent… 0.0s · 1 queued — Enter to send now"),
        "status row: {row:?}"
    );
    assert!(row.contains("4.2s"), "status row: {row:?}");
    assert!(row.contains("[stop]"), "status row: {row:?}");
}

#[test]
fn baseline_cancel_releases_the_status_row_after_turn_cancellation() {
    // arrange
    // Given: an active turn with a coordinator-owned interrupt task.
    let mut app = active_app();

    // When: the turn task reaches its terminal cancellation event.
    app.ingest_event(envelope(
        5,
        "req_live_turn_status",
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: "task_live_turn_status".into(),
            reason: "cancelled by operator".to_string(),
            task_scope: Some(TaskTerminalScope::AgentTurn),
        }),
    ));

    // act
    // Then: cancellation remains transcript/runtime truth without reserving a blank live row.
    // assert
    assert!(
        FrameLayoutPlan::for_app(&app, Rect::new(0, 0, WIDE_WIDTH, HEIGHT))
            .status
            .is_none()
    );
}

#[test]
fn baseline_fail_releases_the_status_row_after_provider_failure() {
    // arrange
    // Given: an active response whose provider reports a terminal runtime failure.
    let mut app = active_app();

    // When: the failure banner becomes the authoritative runtime state.
    app.set_status_banner(Some("provider stream error".to_string()));

    // act
    // Then: the failure surface owns the error and the live row returns to zero height.
    // assert
    assert!(
        FrameLayoutPlan::for_app(&app, Rect::new(0, 0, WIDE_WIDTH, HEIGHT))
            .status
            .is_none()
    );
}

#[test]
fn baseline_recover_labels_the_active_row_as_recovering() {
    // arrange
    // Given: an active turn whose event stream entered replay recovery.
    let mut app = active_app();
    app.set_status_banner(Some("live stream lagged; replaying".to_string()));

    // When: the degraded active turn renders.
    let row = status_text(&app, WIDE_WIDTH).expect("recovering status row");

    // act
    // Then: the row reports recovery rather than stale response activity.
    // assert
    assert!(
        row.contains("Recovering live state…"),
        "status row: {row:?}"
    );
}

#[test]
fn narrow_width_keeps_the_activity_and_stop_before_optional_metadata() {
    // arrange
    // Given: the same active turn with optional queued-input metadata.
    let mut app = active_app();
    app.queued_prompt_count = 2;

    // When: the shell renders in a 24-column viewport.
    let row = status_text(&app, 24).expect("narrow active status row");

    // act
    // Then: required activity and stop affordance survive while lower-priority facts disappear.
    // assert
    assert!(row.contains("Responding…"), "status row: {row:?}");
    assert!(row.contains("[stop]"), "status row: {row:?}");
    assert!(!row.contains("queued 2"), "status row: {row:?}");
}
