use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionRequestedEvent,
    ProviderRequestStartedEvent, RunFailedEvent, RunStartedEvent, ToolCallFinishedEvent,
    ToolCallRequestedEvent, ToolCallStatus, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::app::{AppState, UiIntent};
use harness_tui::render_test::render_to_string;
use harness_tui::{ui, FrameLayoutPlan};
use ratatui::layout::Rect;
use serde_json::json;

#[test]
fn permission_modal_renders_typed_edit_title_diff_and_truthful_always_stage() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(event(
        1,
        Some("tc_edit_permission"),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_edit_preview".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tc_edit_permission".to_string()),
            summary: json!({
                "path": "src/main.rs",
                "diff": "--- src/main.rs\n+++ src/main.rs\n@@\n-old\n+new",
                "selectors": ["workspace:src/main.rs", "digest:abc123"]
            })
            .to_string(),
            request_digest: "digest-edit-preview".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    let rendered = render_text(&app, 120, 34);

    assert!(rendered.contains("Permission required"));
    assert!(rendered.contains("Edit src/main.rs"), "{rendered}");
    assert!(rendered.contains("Diff preview"), "{rendered}");
    assert!(rendered.contains("-old"), "{rendered}");
    assert!(rendered.contains("+new"), "{rendered}");

    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Enter));
    let rendered = render_text(&app, 120, 34);

    assert!(rendered.contains("Allow always"), "{rendered}");
    assert!(rendered.contains("run-scoped durable grant"), "{rendered}");
    assert!(rendered.contains("workspace:src/main.rs"), "{rendered}");
    assert!(rendered.contains("digest:abc123"), "{rendered}");
}

#[test]
fn show_last_error_overlay_renders_recovery_details_and_live_resubmit() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| intents.lock().expect("lock intents").push(intent))
    };
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/g012_error_live")),
        false,
        Some(sink),
    );
    for event in failed_turn_events() {
        app.ingest_event(event);
    }

    app.handle_key(ctrl('p'));
    let palette = render_text(&app, 120, 34);
    assert!(palette.contains("Suggested"), "{palette}");
    assert!(palette.contains("Show last error"), "{palette}");

    for ch in "error".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    let rendered = render_text(&app, 120, 34);

    assert!(rendered.contains("Error details"), "{rendered}");
    assert!(rendered.contains("rate_limited"), "{rendered}");
    assert!(
        rendered.contains("Wait for the provider rate limit"),
        "{rendered}"
    );
    assert!(rendered.contains("req_failed_turn"), "{rendered}");
    assert!(rendered.contains("Resubmit last prompt"), "{rendered}");

    app.handle_key(key(KeyCode::Enter));
    assert!(
        intents
            .lock()
            .expect("lock intents")
            .iter()
            .any(|intent| matches!(intent, UiIntent::SubmitPrompt { text, .. } if text == "Retry the failed request")),
        "expected a normal SubmitPrompt resubmit intent"
    );
}

#[test]
fn replay_error_overlay_is_read_only_without_resubmit_action() {
    let mut app = AppState::new_replay(
        PathBuf::from("/tmp/g012_error_replay"),
        failed_turn_events(),
    );

    app.handle_key(ctrl('p'));
    for ch in "error".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    let rendered = render_text(&app, 120, 34);

    assert!(rendered.contains("Error details"), "{rendered}");
    assert!(rendered.contains("Replay read-only"), "{rendered}");
    assert!(!rendered.contains("Resubmit last prompt"), "{rendered}");
}

#[test]
fn leader_g_renders_message_timeline_and_child_session_dialog_lists_children() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/g012_lineage_parent")), false, None);
    for event in timeline_and_child_events() {
        app.ingest_event(event);
    }

    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('g')));
    let timeline = render_text(&app, 130, 36);
    assert!(timeline.contains("Message timeline"), "{timeline}");
    assert!(timeline.contains("First prompt"), "{timeline}");
    assert!(timeline.contains("Second prompt"), "{timeline}");
    assert!(timeline.contains("stable cutoff"), "{timeline}");

    app.handle_key(key(KeyCode::Esc));
    app.handle_key(ctrl('p'));
    for ch in "child".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    let children = render_text(&app, 130, 36);
    assert!(children.contains("Child sessions"), "{children}");
    assert!(children.contains("agent_child_alpha"), "{children}");
    assert!(children.contains("req_child_alpha"), "{children}");
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

fn failed_turn_events() -> Vec<EventEnvelopeV1> {
    vec![
        event(
            1,
            Some("req_failed_turn"),
            EventV1::RunStarted(RunStartedEvent {
                run_name: "g012-error".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        event(
            2,
            Some("req_failed_turn"),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_failed_turn".to_string(),
                text: "Retry the failed request".to_string(),
            }),
        ),
        event(
            3,
            Some("req_failed_turn"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "provider_req_failed_turn".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5.5".to_string(),
                prompt_summary: "Retry the failed request".to_string(),
                request_digest: "digest-provider-failed-turn".to_string(),
                metadata: None,
            }),
        ),
        event(
            4,
            Some("req_failed_turn"),
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
            Some("req_first"),
            EventV1::RunStarted(RunStartedEvent {
                run_name: "g012-lineage".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        event(
            2,
            Some("req_first"),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_first".to_string(),
                text: "First prompt".to_string(),
            }),
        ),
        event(
            3,
            Some("req_first"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_first_provider".to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "First prompt".to_string(),
                request_digest: "digest-req-first".to_string(),
                metadata: None,
            }),
        ),
        event(
            4,
            Some("req_first"),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_child_alpha".to_string(),
                tool_id: "task".to_string(),
                args_summary: "spawn child".to_string(),
                args_digest: "digest-child-alpha".to_string(),
                metadata: None,
            }),
        ),
        event(
            5,
            Some("req_first"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_child_alpha".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("child spawned".to_string()),
                output_digest: Some("digest-child-alpha-output".to_string()),
                output_json: Some(json!({
                    "child_session_id": "agent_child_alpha",
                    "child_request_id": "req_child_alpha"
                })),
                metadata: None,
            }),
        ),
        event(
            6,
            Some("req_second"),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_second".to_string(),
                text: "Second prompt".to_string(),
            }),
        ),
    ]
}

fn event(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_g012_{seq:04}"),
        seq,
        run_id: "run_g012".to_string(),
        mono_ms: seq,
        ts: Some(format!("2026-06-13T12:{:02}:00Z", seq.min(59))),
        actor: EventActor::new(ActorKind::System, Some("g012-test".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_g012".to_string()),
        payload,
    }
}
