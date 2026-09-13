use std::sync::Arc;
use std::time::Duration;

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, LiveEventEnvelope,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, RuntimeEvent, TaskCancelledEvent,
    TaskScheduleState, TaskScheduledEvent, TaskTerminalScope, ToolCallRequestedEvent,
    ToolCallStartedEvent, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::app::AppState;
use harness_tui::layout::FrameLayoutPlan;
use harness_tui::render_test::render_to_buffer;
use harness_tui::ui;
use ratatui::layout::Rect;

const HEIGHT: u16 = 30;
const WIDE_WIDTH: u16 = 100;

#[test]
fn footer_tracks_current_provider_phase_across_tools_and_multiple_responses() {
    use serde_json::json;

    // Earlier answer/reasoning text remains in the transcript across provider requests.
    // Both normalized live streams and legacy replay fragments must describe the current phase.
    for live in [false, true] {
        let mut app = active_app();
        app.freeze_animation_clock();
        let mut seq = 4;
        let mut tick = 4;
        let mut apply = |kind: &str, data: serde_json::Value, label: &str, elapsed: &str| {
            tick += 1;
            let fragment = kind.ends_with("_delta");
            let payload = json!({"event_type": kind, "data": data});
            if fragment && live {
                let event: LiveEventEnvelope = serde_json::from_value(json!({
                    "event_id": format!("live-{tick}"), "run_id": "run_live_turn_status",
                    "mono_ms": tick * 1000, "actor": {"kind": "worker"},
                    "correlation_id": "req_live_turn_status", "payload": payload,
                }))
                .expect("valid live fixture");
                app.ingest_runtime_event(RuntimeEvent::Live(Box::new(event)));
            } else {
                seq += 1;
                let mut payload = payload;
                if kind == "provider_text_delta" {
                    payload["event_type"] = json!("provider_stream_delta");
                }
                let mut event = envelope(
                    seq,
                    "req_live_turn_status",
                    serde_json::from_value(payload).expect("valid durable fixture"),
                );
                event.mono_ms = tick * 1000;
                app.ingest_event(event);
                assert_eq!(app.canonical_projection_error(), None);
            }
            app.advance_wall_clock_for_motion_evidence(Duration::from_millis(500));
            let row = status_text(&app, WIDE_WIDTH).expect("active footer");
            assert!(
                row.contains(&format!("{label} {elapsed}")),
                "live={live}, event={kind}: {row}"
            );
        };
        let start = |id| {
            json!({"request_id": id, "provider_id": "mock", "model_id": "model-status",
            "prompt_summary": "Synthetic status transition", "request_digest": "synthetic"})
        };
        apply(
            "provider_request_started",
            start("provider-2"),
            "Waiting for response…",
            "0.5s",
        );
        apply(
            "provider_reasoning_delta",
            json!({"request_id":"provider-2","delta":"**Checking**"}),
            "Thinking…",
            "0.5s",
        );
        apply(
            "provider_reasoning_delta",
            json!({"request_id":"provider-2","delta":"\n\n**Verifying**"}),
            "Thinking…",
            "1.0s",
        );
        if live {
            apply(
                "provider_tool_input_delta",
                json!({"request_id":"provider-2","tool_call_id":"tool-current","delta":"{"}),
                "Preparing tool call…",
                "0.5s",
            );
            apply(
                "provider_tool_input_delta",
                json!({"request_id":"provider-2","tool_call_id":"tool-current","delta":"}"}),
                "Preparing tool call…",
                "1.0s",
            );
        }
        apply(
            "tool_call_requested",
            json!({"tool_call_id":"tool-current","tool_id":"read","args_summary":"{}","args_digest":"synthetic"}),
            "Waiting for response…",
            "0.5s",
        );
        apply(
            "tool_call_started",
            json!({"tool_call_id":"tool-current"}),
            "Run read",
            "0.5s",
        );
        apply(
            "provider_request_finished",
            json!({"request_id":"provider-2","finish_reason":"tool_calls"}),
            "Run read",
            "1.0s",
        );
        apply(
            "assistant_message_finished",
            json!({"request_id":"provider-2","parts":[
            {"kind":"reasoning","text":"**Checking**\n\n**Verifying**"},
            {"kind":"tool_call","tool_call_id":"tool-current","tool_id":"read","args_summary":"{}","args_digest":"synthetic"}
        ],"tool_call_count":1}),
            "Run read",
            "1.5s",
        );
        apply(
            "tool_call_finished",
            json!({"tool_call_id":"tool-current","status":"succeeded","output_summary":"Synthetic result","output_digest":"synthetic"}),
            "Waiting for response…",
            "0.5s",
        );
        apply(
            "provider_request_started",
            start("provider-3"),
            "Waiting for response…",
            "1.0s",
        );
        apply(
            "provider_reasoning_delta",
            json!({"request_id":"provider-3","delta":""}),
            "Waiting for response…",
            "1.5s",
        );
        apply(
            "provider_reasoning_delta",
            json!({"request_id":"provider-3","delta":"**Reviewing**"}),
            "Thinking…",
            "0.5s",
        );
        apply(
            "provider_text_delta",
            json!({"request_id":"provider-3","delta":"The check is complete."}),
            "Responding…",
            "0.5s",
        );
        apply(
            "provider_text_delta",
            json!({"request_id":"provider-3","delta":" Verified."}),
            "Responding…",
            "1.0s",
        );
        apply(
            "provider_request_finished",
            json!({"request_id":"provider-3","finish_reason":"stop"}),
            "Waiting for response…",
            "0.5s",
        );
    }
}

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
fn question_wait_hides_phase_timer_but_keeps_turn_timer() {
    // Given: an active turn is blocked on a question tool.
    let mut app = active_app();
    start_running_tool(&mut app, 5, "question", r#"{"questions":[]}"#);

    // When: the live status row is rendered.
    let row = status_text(&app, WIDE_WIDTH).expect("question status row");

    // Then: Grok suppresses the answer-pressure phase timer while retaining total turn time.
    assert!(row.contains("Waiting on answers"), "status row: {row:?}");
    assert_eq!(row.matches("4.2s").count(), 1, "status row: {row:?}");
    assert!(row.contains("[stop]"), "status row: {row:?}");
}

#[test]
fn disconnected_status_requires_reopen_and_hides_live_controls() {
    // Given: an active transcript and local draft when the live event stream closes.
    let mut app = active_app();
    app.handle_paste("preserved draft");
    app.set_status_banner(Some("live event stream disconnected".to_string()));

    // When: the disconnected live status is rendered.
    let row = status_text(&app, WIDE_WIDTH).expect("disconnected status row");

    // Then: the copy is truthful, sending is paused, and stale live controls are unavailable.
    assert!(row.contains("Connection lost"), "status row: {row:?}");
    assert!(row.contains("reopen required"), "status row: {row:?}");
    assert!(
        !row.to_ascii_lowercase().contains("reconnecting"),
        "status row: {row:?}"
    );
    assert!(!row.contains("[stop]"), "status row: {row:?}");
    assert!(!row.contains("[send to bg]"), "status row: {row:?}");
    assert_eq!(app.composer.prompt_buffer, "preserved draft");
    assert!(app.composer_disabled());
}

#[test]
fn narrow_width_preserves_grok_timers_and_stop_before_optional_metadata() {
    // arrange
    // Given: the same active turn with optional queued-input metadata.
    let mut app = active_app();
    app.queued_prompt_count = 2;

    // When: the shell renders in a 24-column viewport.
    let row = status_text(&app, 24).expect("narrow active status row");

    // act
    // Then: both non-truncating timers and stop survive while only the label yields.
    // assert
    assert_eq!(row.matches("4.2s").count(), 2, "status row: {row:?}");
    assert!(row.contains("[stop]"), "status row: {row:?}");
    assert!(!row.contains("queued 2"), "status row: {row:?}");
}
