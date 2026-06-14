use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionDecision, PermissionRequestedEvent,
    ProviderRequestStartedEvent, RunFailedEvent, RunStartedEvent, ToolCallFinishedEvent,
    ToolCallRequestedEvent, ToolCallStatus, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::app::AppState;
use harness_tui::render_test::render_to_string;
use harness_tui::{ui, FrameLayoutPlan};
use ratatui::layout::Rect;
use serde_json::json;

#[test]
fn g012_visual_capture_surfaces_render_at_required_widths() {
    for width in [159, 100] {
        let capture = g012_visual_capture(width, if width == 159 { 40 } else { 30 });

        assert!(capture.contains("Permission required"), "{capture}");
        assert!(capture.contains("Diff preview"), "{capture}");
        assert!(capture.contains("+new"), "{capture}");
        assert!(capture.contains("Error details"), "{capture}");
        assert!(capture.contains("Resubmit last prompt"), "{capture}");
        assert!(capture.contains("Replay read-only"), "{capture}");
        assert!(capture.contains("Message timeline"), "{capture}");
        assert!(capture.contains("Child sessions"), "{capture}");

        if let Ok(dir) = std::env::var("G012_VISUAL_CAPTURE_DIR") {
            let path = Path::new(&dir).join(format!("g012-visual-{width}.txt"));
            std::fs::create_dir_all(&dir).expect("create visual capture dir");
            std::fs::write(path, capture).expect("write visual capture");
        }
    }
}

fn g012_visual_capture(width: u16, height: u16) -> String {
    let mut permission = AppState::new_live(None, false, None);
    permission.ingest_event(permission_event());

    let mut live_error =
        AppState::new_live(Some(PathBuf::from("/tmp/g012_visual_live")), false, None);
    for event in failed_turn_events() {
        live_error.ingest_event(event);
    }
    open_palette_command(&mut live_error, "error");

    let mut replay_error = AppState::new_replay(
        PathBuf::from("/tmp/g012_visual_replay"),
        failed_turn_events(),
    );
    open_palette_command(&mut replay_error, "error");

    let mut timeline =
        AppState::new_live(Some(PathBuf::from("/tmp/g012_visual_parent")), false, None);
    for event in timeline_and_child_events() {
        timeline.ingest_event(event);
    }
    timeline.handle_key(ctrl('x'));
    timeline.handle_key(key(KeyCode::Char('g')));

    let mut children =
        AppState::new_live(Some(PathBuf::from("/tmp/g012_visual_parent")), false, None);
    for event in timeline_and_child_events() {
        children.ingest_event(event);
    }
    open_palette_command(&mut children, "child");

    [
        ("permission", render_text(&permission, width, height)),
        ("error-live", render_text(&live_error, width, height)),
        ("error-replay", render_text(&replay_error, width, height)),
        ("timeline", render_text(&timeline, width, height)),
        ("children", render_text(&children, width, height)),
    ]
    .into_iter()
    .map(|(label, text)| format!("== {label} ==\n{text}"))
    .collect::<Vec<_>>()
    .join("\n")
}

fn open_palette_command(app: &mut AppState, filter: &str) {
    app.handle_key(ctrl('p'));
    for ch in filter.chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
}

fn render_text(app: &AppState, width: u16, height: u16) -> String {
    render_to_string(app, Rect::new(0, 0, width, height), |app, frame, _area| {
        let _plan = FrameLayoutPlan::for_app(app, frame.area());
        ui::render_app(frame, app)
    })
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(ch: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL)
}

fn permission_event() -> EventEnvelopeV1 {
    event(
        1,
        Some("tc_g012_visual_permission"),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_g012_visual".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tc_g012_visual_permission".to_string()),
            summary: json!({
                "path": "src/main.rs",
                "diff": "--- src/main.rs\n+++ src/main.rs\n@@\n-old\n+new",
                "selectors": ["workspace:src/main.rs"]
            })
            .to_string(),
            request_digest: "digest-g012-visual-permission".to_string(),
            timeout_ms: 30_000,
            default_decision: PermissionDecision::Deny,
        }),
    )
}

fn failed_turn_events() -> Vec<EventEnvelopeV1> {
    vec![
        event(
            1,
            Some("req_g012_visual_failed"),
            EventV1::RunStarted(RunStartedEvent {
                run_name: "g012-visual-error".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        event(
            2,
            Some("req_g012_visual_failed"),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_g012_visual_failed".to_string(),
                text: "Retry visual failure".to_string(),
            }),
        ),
        event(
            3,
            Some("req_g012_visual_failed"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "provider_req_g012_visual_failed".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5.5".to_string(),
                prompt_summary: "Retry visual failure".to_string(),
                request_digest: "digest-g012-visual-provider".to_string(),
                metadata: None,
            }),
        ),
        event(
            4,
            Some("req_g012_visual_failed"),
            EventV1::RunFailed(RunFailedEvent {
                error: "rate_limited: provider request failed with status 429".to_string(),
            }),
        ),
    ]
}

fn timeline_and_child_events() -> Vec<EventEnvelopeV1> {
    vec![
        event(
            1,
            Some("req_visual_first"),
            EventV1::RunStarted(RunStartedEvent {
                run_name: "g012-visual-lineage".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        event(
            2,
            Some("req_visual_first"),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_visual_first".to_string(),
                text: "Visual first prompt".to_string(),
            }),
        ),
        event(
            3,
            Some("req_visual_first"),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_visual_child".to_string(),
                tool_id: "task".to_string(),
                args_summary: "spawn visual child".to_string(),
                args_digest: "digest-g012-visual-child-args".to_string(),
                metadata: None,
            }),
        ),
        event(
            4,
            Some("req_visual_first"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_visual_child".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("visual child spawned".to_string()),
                output_digest: Some("digest-g012-visual-child-output".to_string()),
                output_json: Some(json!({
                    "child_session_id": "agent_visual_child",
                    "child_request_id": "req_visual_child"
                })),
                metadata: None,
            }),
        ),
        event(
            5,
            Some("req_visual_second"),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_visual_second".to_string(),
                text: "Visual second prompt".to_string(),
            }),
        ),
    ]
}

fn event(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_g012_visual_{seq:04}"),
        seq,
        run_id: "run_g012_visual".to_string(),
        mono_ms: seq,
        ts: Some(format!("2026-06-14T13:{:02}:00Z", seq.min(59))),
        actor: EventActor::new(ActorKind::System, Some("g012-visual-test".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_g012_visual".to_string()),
        payload,
    }
}
